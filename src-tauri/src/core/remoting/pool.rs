//! Held sessions, one per open environment.
//!
//! The pool owns every [`SessionWorker`]: it starts one lazily on first use,
//! reuses it for every later request, replaces it when the session is lost,
//! closes it when it has been idle, and tears everything down on exit.
//!
//! Three rules shape the design:
//!
//! * **Never retry an authentication failure.** A stale password retried in a
//!   loop locks the shared account out for everyone. After an `AuthFailed`
//!   the entry is *latched*: every automatic request fails immediately with
//!   the same error until the user explicitly reconnects ([`SessionPool::open`])
//!   or changes the account ([`SessionPool::clear_auth_latches`]).
//! * **Reconnect only what is safe to repeat.** When a session dies mid-call
//!   (VPN drop, sleep, host restart) the worker is replaced and the request is
//!   repeated once — but only if the caller marked it [`Retry::Idempotent`].
//!   A mutation is never silently run twice.
//! * **Stay inside the server's quota.** WinRM limits shells per user, and the
//!   environments use a shared account, so every WinGUI user on a host counts
//!   as the same user. The pool caps this client's sessions per host and
//!   closes idle ones.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

use super::worker::{SessionInfo, SessionWorker, WorkerFactory};
use super::RemoteTarget;
use crate::core::error::{codes, AppError, AppResult};

/// Whether a request may be repeated on a fresh session after the old one
/// was lost mid-call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retry {
    /// Reads: listing, stat, status. Safe to run again.
    Idempotent,
    /// Anything that changes the host. Fails with `SessionLost` instead.
    Never,
}

#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// A session unused for this long is closed (and reopened on demand).
    pub idle_timeout: Duration,
    /// Most sessions this client holds to one host at a time.
    pub max_sessions_per_host: usize,
}

impl Default for PoolConfig {
    fn default() -> Self {
        PoolConfig {
            idle_timeout: Duration::from_secs(10 * 60),
            max_sessions_per_host: 4,
        }
    }
}

#[derive(Default)]
struct Slot {
    worker: Option<Arc<dyn SessionWorker>>,
    /// Lower-cased host the worker is connected to.
    host: String,
    /// Lower-cased user the worker authenticated as.
    user: String,
    last_used: Option<Instant>,
    /// Set after an authentication failure (with the time it happened); see
    /// the module docs.
    auth_latch: Option<(Instant, AppError)>,
}

struct Entry {
    slot: Mutex<Slot>,
}

pub struct SessionPool {
    factory: Arc<dyn WorkerFactory>,
    config: PoolConfig,
    entries: Mutex<HashMap<String, Arc<Entry>>>,
    /// host -> session keys with a live worker. Kept separately from the
    /// slots so the per-host cap never needs two slot locks at once.
    live: Mutex<HashMap<String, HashSet<String>>>,
}

impl SessionPool {
    pub fn new(factory: Arc<dyn WorkerFactory>, config: PoolConfig) -> Self {
        SessionPool {
            factory,
            config,
            entries: Mutex::new(HashMap::new()),
            live: Mutex::new(HashMap::new()),
        }
    }

    fn entry(&self, key: &str) -> Arc<Entry> {
        self.entries
            .lock()
            .unwrap()
            .entry(key.to_string())
            .or_insert_with(|| {
                Arc::new(Entry {
                    slot: Mutex::new(Slot::default()),
                })
            })
            .clone()
    }

    /// Explicitly (re)connect: the user pressed Connect or Retry. Clears an
    /// authentication latch and returns what the remote end reported.
    pub fn open(&self, key: &str, target: &RemoteTarget) -> AppResult<SessionInfo> {
        let entry = self.entry(key);
        Ok(self.acquire(&entry, key, target, Some(Instant::now()))?.info())
    }

    /// Run one operation on the environment's session, connecting first if
    /// there is none.
    pub fn request(
        &self,
        key: &str,
        target: &RemoteTarget,
        op: &str,
        args: Value,
        timeout: Duration,
        retry: Retry,
    ) -> AppResult<Value> {
        let entry = self.entry(key);
        let mut retried = false;
        loop {
            let worker = self.acquire(&entry, key, target, None)?;
            match worker.request(op, args.clone(), timeout) {
                Ok(v) => {
                    entry.slot.lock().unwrap().last_used = Some(Instant::now());
                    return Ok(v);
                }
                Err(e) => {
                    let dead = e.is(codes::SESSION_LOST) || e.is(codes::TIMEOUT) || !worker.is_alive();
                    if dead {
                        self.discard(&entry, key, &worker);
                    }
                    if e.is(codes::SESSION_LOST) && retry == Retry::Idempotent && !retried {
                        retried = true;
                        continue;
                    }
                    return Err(e);
                }
            }
        }
    }

    /// The live session's details, if there is one.
    pub fn status(&self, key: &str) -> Option<SessionInfo> {
        let entry = self.entries.lock().unwrap().get(key).cloned()?;
        let slot = entry.slot.lock().unwrap();
        slot.worker
            .as_ref()
            .filter(|w| w.is_alive())
            .map(|w| w.info())
    }

    /// Close one environment's session (the tab was closed).
    pub fn close(&self, key: &str) {
        let entry = self.entries.lock().unwrap().remove(key);
        if let Some(entry) = entry {
            let mut slot = entry.slot.lock().unwrap();
            self.release(&mut slot, key);
        }
    }

    /// Close everything (app exit).
    pub fn close_all(&self) {
        let entries: Vec<(String, Arc<Entry>)> = self.entries.lock().unwrap().drain().collect();
        for (key, entry) in entries {
            let mut slot = entry.slot.lock().unwrap();
            self.release(&mut slot, &key);
        }
    }

    /// Close everything and wait (up to `max` in total) for the workers to
    /// remove their remote sessions. For app exit, where a background
    /// clean-up thread would be cut off with the process.
    pub fn shutdown(&self, max: Duration) {
        let deadline = Instant::now() + max;
        let entries: Vec<(String, Arc<Entry>)> = self.entries.lock().unwrap().drain().collect();
        for (key, entry) in entries {
            let mut slot = entry.slot.lock().unwrap();
            if let Some(worker) = slot.worker.take() {
                worker.close_and_wait(deadline.saturating_duration_since(Instant::now()));
            }
            self.release(&mut slot, &key);
        }
    }

    /// Close sessions that have been idle longer than the configured timeout.
    /// Returns how many were closed. Called periodically by the front end.
    pub fn reap_idle(&self) -> usize {
        let entries: Vec<(String, Arc<Entry>)> = self
            .entries
            .lock()
            .unwrap()
            .iter()
            .map(|(k, e)| (k.clone(), e.clone()))
            .collect();
        let mut closed = 0;
        for (key, entry) in entries {
            // A slot that is busy connecting is, by definition, not idle.
            let Ok(mut slot) = entry.slot.try_lock() else {
                continue;
            };
            let idle = slot
                .last_used
                .is_some_and(|t| t.elapsed() >= self.config.idle_timeout);
            if slot.worker.is_some() && idle {
                self.release(&mut slot, &key);
                closed += 1;
            }
        }
        closed
    }

    /// Forget every authentication latch (the account was changed, so the
    /// next attempt uses a different credential).
    pub fn clear_auth_latches(&self) {
        let entries: Vec<Arc<Entry>> = self.entries.lock().unwrap().values().cloned().collect();
        for entry in entries {
            entry.slot.lock().unwrap().auth_latch = None;
        }
    }

    /// Number of live sessions this client holds (for diagnostics and tests).
    pub fn live_sessions(&self) -> usize {
        self.live.lock().unwrap().values().map(|s| s.len()).sum()
    }

    /// The worker for `key`, connecting if needed. Holds the slot lock while
    /// connecting, so concurrent callers for the same environment wait for
    /// the one connection attempt instead of each starting their own.
    ///
    /// `explicit` is `Some(when the user asked)` for a user-initiated connect,
    /// which overrides an authentication latch — but only one that predates
    /// the request. A latch set *while this call was waiting for the lock*
    /// came from a concurrent attempt with the same credential, and sending
    /// it again would be exactly the repeat the latch exists to prevent.
    fn acquire(
        &self,
        entry: &Entry,
        key: &str,
        target: &RemoteTarget,
        explicit: Option<Instant>,
    ) -> AppResult<Arc<dyn SessionWorker>> {
        let host = target.host.trim().to_lowercase();
        let user = target.username.trim().to_lowercase();
        let mut slot = entry.slot.lock().unwrap();

        // Same environment, same identity, still running: reuse it.
        let same_target = slot.host == host && slot.user == user;
        let reusable = slot
            .worker
            .as_ref()
            .filter(|w| same_target && w.is_alive())
            .cloned();
        if let Some(worker) = reusable {
            slot.last_used = Some(Instant::now());
            return Ok(worker);
        }
        // Dead, or the connection was edited to point somewhere else.
        if slot.worker.is_some() {
            self.release(&mut slot, key);
        }

        if let Some((at, latched)) = &slot.auth_latch {
            let overridden = explicit.is_some_and(|asked| *at < asked);
            if same_target && !overridden {
                return Err(latched.clone());
            }
            // A different host or user, or the user asked again: try afresh.
            slot.auth_latch = None;
        }

        {
            let live = self.live.lock().unwrap();
            let held = live.get(&host).map(|s| s.len()).unwrap_or(0);
            if held >= self.config.max_sessions_per_host {
                return Err(AppError::new(
                    codes::TOO_MANY_SESSIONS,
                    format!(
                        "WinGUI already holds {held} sessions to {} — the most it will open to one \
                         host, to stay inside the server's per-user WinRM limit. Close another tab \
                         to this host first.",
                        target.host
                    ),
                ));
            }
        }

        slot.host = host.clone();
        slot.user = user;
        match self.factory.spawn(target) {
            Ok(worker) => {
                slot.worker = Some(worker.clone());
                slot.last_used = Some(Instant::now());
                self.live
                    .lock()
                    .unwrap()
                    .entry(host)
                    .or_default()
                    .insert(key.to_string());
                Ok(worker)
            }
            Err(e) => {
                if e.is(codes::AUTH_FAILED) {
                    slot.auth_latch = Some((Instant::now(), e.clone()));
                }
                Err(e)
            }
        }
    }

    /// Drop `worker` from the entry if it is still the current one.
    fn discard(&self, entry: &Entry, key: &str, worker: &Arc<dyn SessionWorker>) {
        let mut slot = entry.slot.lock().unwrap();
        let current = slot
            .worker
            .as_ref()
            .is_some_and(|w| Arc::ptr_eq(w, worker));
        if current {
            self.release(&mut slot, key);
        }
    }

    /// Close and forget the slot's worker and its place in the per-host count.
    fn release(&self, slot: &mut Slot, key: &str) {
        if let Some(worker) = slot.worker.take() {
            worker.close();
        }
        let mut live = self.live.lock().unwrap();
        if let Some(keys) = live.get_mut(&slot.host) {
            keys.remove(key);
            if keys.is_empty() {
                live.remove(&slot.host);
            }
        }
    }
}

impl Drop for SessionPool {
    fn drop(&mut self) {
        self.close_all();
    }
}

#[cfg(test)]
pub mod test_support {
    //! A scripted worker and factory, so pool behaviour is testable without
    //! starting a process.

    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::*;

    pub struct FakeWorker {
        pub host: String,
        pub replies: Mutex<VecDeque<AppResult<Value>>>,
        pub alive: AtomicBool,
        pub closed: AtomicBool,
        pub requests: Mutex<Vec<String>>,
    }

    impl SessionWorker for FakeWorker {
        fn info(&self) -> SessionInfo {
            SessionInfo {
                computer_name: self.host.to_uppercase(),
                ..SessionInfo::default()
            }
        }

        fn request(&self, op: &str, _args: Value, _timeout: Duration) -> AppResult<Value> {
            self.requests.lock().unwrap().push(op.to_string());
            let reply = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(Value::String(format!("ok:{op}"))));
            if let Err(e) = &reply {
                if e.is(codes::SESSION_LOST) || e.is(codes::TIMEOUT) {
                    self.alive.store(false, Ordering::SeqCst);
                }
            }
            reply
        }

        fn is_alive(&self) -> bool {
            self.alive.load(Ordering::SeqCst)
        }

        fn close(&self) {
            self.alive.store(false, Ordering::SeqCst);
            self.closed.store(true, Ordering::SeqCst);
        }
    }

    /// Hands out [`FakeWorker`]s; each spawn can be scripted to fail, and each
    /// worker can be given a queue of replies.
    #[derive(Default)]
    pub struct FakeFactory {
        pub spawns: AtomicUsize,
        /// Results for successive spawns: `Err` fails the spawn, `Ok(replies)`
        /// starts a worker with those scripted replies. Empty => plain worker.
        pub script: Mutex<VecDeque<AppResult<Vec<AppResult<Value>>>>>,
        pub workers: Mutex<Vec<Arc<FakeWorker>>>,
    }

    impl FakeFactory {
        pub fn spawn_count(&self) -> usize {
            self.spawns.load(Ordering::SeqCst)
        }

        pub fn push(&self, outcome: AppResult<Vec<AppResult<Value>>>) {
            self.script.lock().unwrap().push_back(outcome);
        }
    }

    impl WorkerFactory for FakeFactory {
        fn spawn(&self, target: &RemoteTarget) -> AppResult<Arc<dyn SessionWorker>> {
            self.spawns.fetch_add(1, Ordering::SeqCst);
            let replies = match self.script.lock().unwrap().pop_front() {
                Some(Err(e)) => return Err(e),
                Some(Ok(replies)) => replies,
                None => Vec::new(),
            };
            let worker = Arc::new(FakeWorker {
                host: target.host.clone(),
                replies: Mutex::new(replies.into()),
                alive: AtomicBool::new(true),
                closed: AtomicBool::new(false),
                requests: Mutex::new(Vec::new()),
            });
            self.workers.lock().unwrap().push(worker.clone());
            Ok(worker)
        }
    }

    pub fn target(host: &str) -> RemoteTarget {
        RemoteTarget {
            host: host.into(),
            username: r"cambio.se\tester".into(),
            password: "pw".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use serde_json::json;

    use super::test_support::*;
    use super::*;

    const T: Duration = Duration::from_secs(5);

    fn pool(factory: &Arc<FakeFactory>, config: PoolConfig) -> SessionPool {
        SessionPool::new(factory.clone(), config)
    }

    fn lost() -> AppError {
        AppError::new(codes::SESSION_LOST, "gone")
    }

    #[test]
    fn one_session_is_opened_and_reused() {
        let f = Arc::new(FakeFactory::default());
        let p = pool(&f, PoolConfig::default());
        let t = target("L-a-SD");
        for _ in 0..5 {
            p.request("c1", &t, "fs.drives", json!({}), T, Retry::Idempotent).unwrap();
        }
        assert_eq!(f.spawn_count(), 1);
        assert_eq!(p.live_sessions(), 1);
        assert_eq!(p.status("c1").unwrap().computer_name, "L-A-SD");
        assert!(p.status("other").is_none());
    }

    #[test]
    fn a_lost_session_is_replaced_and_an_idempotent_request_repeated_once() {
        let f = Arc::new(FakeFactory::default());
        f.push(Ok(vec![Err(lost())]));
        let p = pool(&f, PoolConfig::default());
        let got = p
            .request("c1", &target("L-a-SD"), "fs.list", json!({}), T, Retry::Idempotent)
            .unwrap();
        assert_eq!(got, json!("ok:fs.list"));
        assert_eq!(f.spawn_count(), 2);
        assert!(f.workers.lock().unwrap()[0].closed.load(Ordering::SeqCst));
        assert_eq!(p.live_sessions(), 1);
    }

    #[test]
    fn a_second_loss_in_a_row_is_reported_not_retried_forever() {
        let f = Arc::new(FakeFactory::default());
        f.push(Ok(vec![Err(lost())]));
        f.push(Ok(vec![Err(lost())]));
        let p = pool(&f, PoolConfig::default());
        let err = p
            .request("c1", &target("L-a-SD"), "fs.list", json!({}), T, Retry::Idempotent)
            .unwrap_err();
        assert!(err.is(codes::SESSION_LOST));
        assert_eq!(f.spawn_count(), 2);
        assert_eq!(p.live_sessions(), 0);
    }

    #[test]
    fn a_mutation_is_never_repeated_after_a_lost_session() {
        let f = Arc::new(FakeFactory::default());
        f.push(Ok(vec![Err(lost())]));
        let p = pool(&f, PoolConfig::default());
        let err = p
            .request("c1", &target("L-a-SD"), "fs.delete", json!({}), T, Retry::Never)
            .unwrap_err();
        assert!(err.is(codes::SESSION_LOST));
        assert_eq!(f.spawn_count(), 1, "the delete must not be attempted on a new session");
        assert_eq!(f.workers.lock().unwrap()[0].requests.lock().unwrap().len(), 1);
    }

    #[test]
    fn an_ordinary_error_keeps_the_session() {
        let f = Arc::new(FakeFactory::default());
        f.push(Ok(vec![Err(AppError::new(codes::ACCESS_DENIED, "no"))]));
        let p = pool(&f, PoolConfig::default());
        let t = target("L-a-SD");
        let err = p.request("c1", &t, "fs.list", json!({}), T, Retry::Idempotent).unwrap_err();
        assert!(err.is(codes::ACCESS_DENIED));
        p.request("c1", &t, "fs.list", json!({}), T, Retry::Idempotent).unwrap();
        assert_eq!(f.spawn_count(), 1);
    }

    #[test]
    fn a_timeout_drops_the_session_without_retrying() {
        let f = Arc::new(FakeFactory::default());
        f.push(Ok(vec![Err(AppError::new(codes::TIMEOUT, "slow"))]));
        let p = pool(&f, PoolConfig::default());
        let t = target("L-a-SD");
        let err = p.request("c1", &t, "fs.search", json!({}), T, Retry::Idempotent).unwrap_err();
        assert!(err.is(codes::TIMEOUT));
        assert_eq!(f.spawn_count(), 1);
        assert_eq!(p.live_sessions(), 0);
        // The next request simply opens a new session.
        p.request("c1", &t, "ping", json!({}), T, Retry::Idempotent).unwrap();
        assert_eq!(f.spawn_count(), 2);
    }

    #[test]
    fn an_auth_failure_latches_until_the_user_reconnects() {
        let f = Arc::new(FakeFactory::default());
        f.push(Err(AppError::new(codes::AUTH_FAILED, "bad password")));
        let p = pool(&f, PoolConfig::default());
        let t = target("L-a-SD");

        for _ in 0..4 {
            let err = p.request("c1", &t, "fs.drives", json!({}), T, Retry::Idempotent).unwrap_err();
            assert!(err.is(codes::AUTH_FAILED));
        }
        assert_eq!(f.spawn_count(), 1, "a rejected password must not be re-sent automatically");

        // An explicit reconnect tries again (and succeeds this time).
        p.open("c1", &t).unwrap();
        assert_eq!(f.spawn_count(), 2);
        p.request("c1", &t, "fs.drives", json!({}), T, Retry::Idempotent).unwrap();
        assert_eq!(f.spawn_count(), 2);
    }

    #[test]
    fn changing_the_account_clears_the_latch() {
        let f = Arc::new(FakeFactory::default());
        f.push(Err(AppError::new(codes::AUTH_FAILED, "bad password")));
        let p = pool(&f, PoolConfig::default());
        let t = target("L-a-SD");
        assert!(p.request("c1", &t, "ping", json!({}), T, Retry::Idempotent).is_err());
        p.clear_auth_latches();
        assert!(p.request("c1", &t, "ping", json!({}), T, Retry::Idempotent).is_ok());
        assert_eq!(f.spawn_count(), 2);
    }

    #[test]
    fn a_different_user_is_not_blocked_by_someone_elses_latch() {
        let f = Arc::new(FakeFactory::default());
        f.push(Err(AppError::new(codes::AUTH_FAILED, "bad password")));
        let p = pool(&f, PoolConfig::default());
        let mut t = target("L-a-SD");
        assert!(p.request("c1", &t, "ping", json!({}), T, Retry::Idempotent).is_err());
        t.username = r"cambio.se\someone-else".into();
        assert!(p.request("c1", &t, "ping", json!({}), T, Retry::Idempotent).is_ok());
    }

    #[test]
    fn other_connect_failures_do_not_latch() {
        let f = Arc::new(FakeFactory::default());
        f.push(Err(AppError::new(codes::UNREACHABLE, "vpn down")));
        let p = pool(&f, PoolConfig::default());
        let t = target("L-a-SD");
        assert!(p.request("c1", &t, "ping", json!({}), T, Retry::Idempotent).unwrap_err().is(codes::UNREACHABLE));
        assert!(p.request("c1", &t, "ping", json!({}), T, Retry::Idempotent).is_ok());
        assert_eq!(f.spawn_count(), 2);
    }

    #[test]
    fn editing_the_host_reconnects_to_the_new_one() {
        let f = Arc::new(FakeFactory::default());
        let p = pool(&f, PoolConfig::default());
        p.request("c1", &target("L-a-SD"), "ping", json!({}), T, Retry::Idempotent).unwrap();
        p.request("c1", &target("L-b-SD"), "ping", json!({}), T, Retry::Idempotent).unwrap();
        assert_eq!(f.spawn_count(), 2);
        assert!(f.workers.lock().unwrap()[0].closed.load(Ordering::SeqCst));
        assert_eq!(p.status("c1").unwrap().computer_name, "L-B-SD");
        assert_eq!(p.live_sessions(), 1);
    }

    #[test]
    fn sessions_per_host_are_capped() {
        let f = Arc::new(FakeFactory::default());
        let p = pool(
            &f,
            PoolConfig {
                max_sessions_per_host: 2,
                ..PoolConfig::default()
            },
        );
        let t = target("L-a-SD");
        p.open("c1", &t).unwrap();
        // Host comparison ignores case and stray whitespace.
        p.open("c2", &target(" l-A-sd ")).unwrap();
        let err = p.open("c3", &t).unwrap_err();
        assert!(err.is(codes::TOO_MANY_SESSIONS));
        // Another host is unaffected; closing one frees a place.
        p.open("c4", &target("L-other-SD")).unwrap();
        p.close("c1");
        p.open("c3", &t).unwrap();
        assert_eq!(p.live_sessions(), 3);
    }

    #[test]
    fn idle_sessions_are_closed_and_reopened_on_demand() {
        let f = Arc::new(FakeFactory::default());
        let p = pool(
            &f,
            PoolConfig {
                idle_timeout: Duration::from_millis(20),
                ..PoolConfig::default()
            },
        );
        let t = target("L-a-SD");
        p.open("c1", &t).unwrap();
        assert_eq!(p.reap_idle(), 0, "a session just used is not idle");
        std::thread::sleep(Duration::from_millis(40));
        assert_eq!(p.reap_idle(), 1);
        assert!(p.status("c1").is_none());
        assert_eq!(p.live_sessions(), 0);
        assert!(f.workers.lock().unwrap()[0].closed.load(Ordering::SeqCst));

        p.request("c1", &t, "ping", json!({}), T, Retry::Idempotent).unwrap();
        assert_eq!(f.spawn_count(), 2);
    }

    #[test]
    fn close_all_and_drop_close_every_worker() {
        let f = Arc::new(FakeFactory::default());
        {
            let p = pool(&f, PoolConfig::default());
            p.open("c1", &target("L-a-SD")).unwrap();
            p.open("c2", &target("L-b-SD")).unwrap();
            p.close_all();
            assert_eq!(p.live_sessions(), 0);
            p.open("c3", &target("L-c-SD")).unwrap();
        } // dropped here
        for w in f.workers.lock().unwrap().iter() {
            assert!(w.closed.load(Ordering::SeqCst), "{} left open", w.host);
        }
    }
}
