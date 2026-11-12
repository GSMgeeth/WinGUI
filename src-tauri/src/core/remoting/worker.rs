//! The persistent session worker: one hidden `powershell.exe` that keeps a
//! single WinRM `PSSession` open and answers requests over stdin/stdout.
//!
//! Remote Desktop pays for a new WinRM session on every action, which is fine
//! for a deploy and far too slow for an explorer that makes a remote call on
//! almost every click. A worker opens the session once and reuses it.
//!
//! How a worker is started (see `scripts/worker.ps1` for the PowerShell side):
//!
//! 1. `powershell.exe` is launched by its absolute `System32` path with a tiny
//!    **bootstrap** as its `-Command`. `-InputFormat None` stops the PowerShell
//!    host from reading stdin itself, so the pipe is ours alone.
//! 2. The bootstrap reads the real worker script **from stdin** up to a
//!    sentinel line and dot-sources it. Nothing is written to disk and the
//!    32k command-line limit never applies.
//! 3. Host, user and password travel as **environment variables of that child
//!    only** — never arguments, which any local process can read.
//! 4. The worker opens the session and prints one `ready` (or `fatal`) line,
//!    then serves one JSON request per line until stdin closes.
//!
//! Requests are served strictly one at a time; [`PsWorker::request`]
//! serialises callers. There is no in-band cancel: a request that times out
//! kills the process, and the pool starts a fresh one on the next call.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::RemoteTarget;
use crate::core::error::{codes, AppError, AppResult};
use crate::core::exec;

/// The worker script, embedded at build time.
const WORKER_SCRIPT: &str = include_str!("scripts/worker.ps1");

/// Marks the end of the script on stdin; everything after it is requests.
const SCRIPT_END: &str = "#WG-END";

/// The `-Command` bootstrap: wrap the raw std streams as UTF-8, read the
/// worker script from stdin up to [`SCRIPT_END`], and dot-source it.
const BOOTSTRAP: &str = concat!(
    "$ErrorActionPreference='Stop';",
    "$u=New-Object System.Text.UTF8Encoding($false);",
    "$WgIn=New-Object System.IO.StreamReader([Console]::OpenStandardInput(),$u);",
    "$WgOut=New-Object System.IO.StreamWriter([Console]::OpenStandardOutput(),$u);",
    "$WgOut.AutoFlush=$true;",
    "$b=New-Object System.Text.StringBuilder;",
    "while($true){$l=$WgIn.ReadLine();if($null -eq $l -or $l -eq '#WG-END'){break};[void]$b.AppendLine($l)};",
    ". ([scriptblock]::Create($b.ToString()))",
);

/// How long to wait for the session to open. The worker's own
/// `New-PSSession` open timeout is 20 s; the rest covers process start,
/// name resolution and authentication.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(60);

/// How long a closing worker gets to remove its remote session before it is
/// killed.
const GRACEFUL_EXIT: Duration = Duration::from_secs(5);

/// Cap on captured stderr, so a chatty failure can't grow without bound.
const STDERR_CAP: usize = 16 * 1024;

/// What the remote end reported when the session opened.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SessionInfo {
    /// The machine's own name (`%COMPUTERNAME%`) — may differ in case or form
    /// from what the user typed.
    pub computer_name: String,
    /// The identity the session runs as on the server.
    pub user: String,
    /// OS caption, e.g. `Microsoft Windows Server 2022 Standard`.
    pub os: String,
    pub ps_version: String,
    /// Remote clock at connect time (ISO 8601, UTC).
    pub utc_now: String,
}

/// A held session that can answer requests. The trait is the seam that lets
/// the pool be tested with a fake.
pub trait SessionWorker: Send + Sync {
    fn info(&self) -> SessionInfo;
    /// Run one operation and return its `data`. Blocks until the reply
    /// arrives or `timeout` passes.
    fn request(&self, op: &str, args: Value, timeout: Duration) -> AppResult<Value>;
    /// False once the process has exited or been killed.
    fn is_alive(&self) -> bool;
    /// Shut the worker down, removing the remote session if it can. Returns
    /// immediately; the process is given a moment to exit in the background.
    fn close(&self);
    /// Like [`close`](Self::close) but waits (up to `max`) for the worker to
    /// finish removing its remote session. Used at app exit: a session that
    /// is merely abandoned keeps counting against the server's per-user
    /// quota until WinRM's idle timeout reclaims it.
    fn close_and_wait(&self, _max: Duration) {
        self.close();
    }
}

/// Starts workers. The production implementation is [`PsWorkerFactory`].
pub trait WorkerFactory: Send + Sync {
    fn spawn(&self, target: &RemoteTarget) -> AppResult<Arc<dyn SessionWorker>>;
}

/// Starts real `powershell.exe` workers.
pub struct PsWorkerFactory;

impl WorkerFactory for PsWorkerFactory {
    fn spawn(&self, target: &RemoteTarget) -> AppResult<Arc<dyn SessionWorker>> {
        Ok(Arc::new(PsWorker::spawn(Mode::Remote(target))?))
    }
}

enum Mode<'a> {
    Remote(&'a RemoteTarget),
    /// Operations run on this machine with no `PSSession`. Exists so the
    /// script and protocol can be tested without a WinRM server; the app
    /// itself can never select it.
    #[cfg(test)]
    Local,
}

/// The writable half plus the reply stream; guarded by one mutex so requests
/// are strictly sequential.
struct Io {
    /// `None` once closed — dropping it is what tells the worker to exit.
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
}

/// A running `powershell.exe` session worker.
pub struct PsWorker {
    info: SessionInfo,
    io: Mutex<Io>,
    child: Arc<Mutex<Child>>,
    alive: Arc<AtomicBool>,
    stderr: Arc<Mutex<String>>,
    next_id: AtomicU64,
}

impl PsWorker {
    fn spawn(mode: Mode) -> AppResult<Self> {
        let mut cmd = exec::command(&exec::powershell());
        cmd.args([
            "-NoProfile",
            "-NonInteractive",
            "-InputFormat",
            "None",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            BOOTSTRAP,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

        let who = match &mode {
            Mode::Remote(t) => {
                cmd.env("WG_HOST", &t.host)
                    .env("WG_USER", &t.username)
                    .env("WG_PASSWORD", &t.password)
                    .env_remove("WG_LOCAL");
                format!("\n\nAttempted as: {}  (host: {})", t.username, t.host)
            }
            #[cfg(test)]
            Mode::Local => {
                cmd.env("WG_LOCAL", "1").env_remove("WG_PASSWORD");
                String::new()
            }
        };

        let mut child = cmd.spawn().map_err(|e| {
            AppError::new(codes::WORKER_FAILED, format!("could not start powershell: {e}"))
        })?;
        // Crash safety: the worker must not outlive the app.
        exec::kill_with_parent(&child);

        let mut stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let mut stderr_pipe = child.stderr.take().expect("piped stderr");

        // stdout -> channel of lines. The sender is dropped at EOF, which is
        // how a waiting request learns the process is gone.
        let alive = Arc::new(AtomicBool::new(true));
        let (tx, lines) = mpsc::channel::<String>();
        {
            let alive = alive.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines() {
                    let Ok(line) = line else { break };
                    if tx.send(line).is_err() {
                        break;
                    }
                }
                alive.store(false, Ordering::SeqCst);
            });
        }

        // stderr is drained continuously so a full pipe can't stall the child.
        let stderr = Arc::new(Mutex::new(String::new()));
        {
            let stderr = stderr.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 2048];
                while let Ok(n) = stderr_pipe.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    let mut s = stderr.lock().unwrap();
                    if s.len() < STDERR_CAP {
                        s.push_str(&String::from_utf8_lossy(&buf[..n]));
                    }
                }
            });
        }

        let child = Arc::new(Mutex::new(child));
        let kill = |child: &Arc<Mutex<Child>>| {
            let _ = child.lock().unwrap().kill();
        };

        // Hand over the script.
        let sent = stdin
            .write_all(WORKER_SCRIPT.as_bytes())
            .and_then(|()| stdin.write_all(format!("\n{SCRIPT_END}\n").as_bytes()))
            .and_then(|()| stdin.flush());
        if let Err(e) = sent {
            kill(&child);
            return Err(AppError::new(
                codes::WORKER_FAILED,
                format!("could not send the worker script: {e}"),
            ));
        }

        // Wait for the session to open.
        let info = match lines.recv_timeout(CONNECT_TIMEOUT) {
            Ok(line) => match serde_json::from_str::<Value>(&line) {
                Ok(v) if v["kind"] == "ready" => {
                    serde_json::from_value::<SessionInfo>(v["data"].clone()).unwrap_or_default()
                }
                Ok(v) if v["kind"] == "fatal" => {
                    kill(&child);
                    let code = v["code"].as_str().unwrap_or(codes::CONNECT_FAILED).to_string();
                    let message = v["message"].as_str().unwrap_or("could not connect");
                    return Err(AppError {
                        code,
                        message: format!("{}{who}", message.trim()),
                    });
                }
                _ => {
                    kill(&child);
                    return Err(AppError::new(
                        codes::WORKER_FAILED,
                        format!("unexpected first line from the worker: {line}"),
                    ));
                }
            },
            Err(RecvTimeoutError::Timeout) => {
                kill(&child);
                return Err(AppError::new(
                    codes::UNREACHABLE,
                    format!(
                        "No answer within {} seconds while opening the session.{who}",
                        CONNECT_TIMEOUT.as_secs()
                    ),
                ));
            }
            Err(RecvTimeoutError::Disconnected) => {
                kill(&child);
                // Give the drain thread a moment to collect the reason.
                std::thread::sleep(Duration::from_millis(150));
                let detail = stderr.lock().unwrap().trim().to_string();
                return Err(AppError::new(
                    codes::WORKER_FAILED,
                    if detail.is_empty() {
                        "The session worker exited before it was ready.".to_string()
                    } else {
                        format!("The session worker exited before it was ready:\n{detail}")
                    },
                ));
            }
        };

        Ok(PsWorker {
            info,
            io: Mutex::new(Io {
                stdin: Some(stdin),
                lines,
            }),
            child,
            alive,
            stderr,
            next_id: AtomicU64::new(1),
        })
    }

    /// Start a worker that runs operations on this machine (tests only).
    #[cfg(test)]
    pub fn spawn_local() -> AppResult<Self> {
        Self::spawn(Mode::Local)
    }

    fn kill(&self) {
        self.alive.store(false, Ordering::SeqCst);
        let _ = self.child.lock().unwrap().kill();
    }

    /// Start shutting down. Closing stdin makes the worker's read loop end,
    /// and its `finally` removes the remote session — the polite path, so the
    /// server's per-user shell quota is released promptly. Returns `false`
    /// when a request is in flight (stdin unreachable) and the process was
    /// killed instead.
    fn begin_close(&self) -> bool {
        let polite = match self.io.try_lock() {
            Ok(mut io) => io.stdin.take().is_some(),
            Err(_) => false,
        };
        self.alive.store(false, Ordering::SeqCst);
        if !polite {
            let _ = self.child.lock().unwrap().kill();
        }
        polite
    }

    /// Wait up to `max` for the process to exit by itself, then kill it.
    fn wait_exit(child: &Mutex<Child>, max: Duration) {
        let deadline = Instant::now() + max;
        loop {
            match child.lock().unwrap().try_wait() {
                Ok(Some(_)) | Err(_) => return,
                Ok(None) => {}
            }
            if Instant::now() >= deadline {
                let _ = child.lock().unwrap().kill();
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn lost(&self, what: &str) -> AppError {
        let detail = self.stderr.lock().unwrap().trim().to_string();
        AppError::new(
            codes::SESSION_LOST,
            if detail.is_empty() {
                what.to_string()
            } else {
                format!("{what}\n{detail}")
            },
        )
    }
}

impl SessionWorker for PsWorker {
    fn info(&self) -> SessionInfo {
        self.info.clone()
    }

    fn request(&self, op: &str, args: Value, timeout: Duration) -> AppResult<Value> {
        let mut io = self.io.lock().unwrap();
        if !self.is_alive() {
            return Err(self.lost("The session to the environment has ended."));
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst).to_string();
        let line = serde_json::json!({ "id": id, "op": op, "args": args }).to_string();

        let Some(stdin) = io.stdin.as_mut() else {
            return Err(self.lost("The session to the environment was closed."));
        };
        if stdin
            .write_all(line.as_bytes())
            .and_then(|()| stdin.write_all(b"\n"))
            .and_then(|()| stdin.flush())
            .is_err()
        {
            self.kill();
            return Err(self.lost("The session to the environment has ended."));
        }

        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match io.lines.recv_timeout(remaining) {
                Ok(reply) => {
                    let Ok(v) = serde_json::from_str::<Value>(&reply) else {
                        continue; // not protocol output — ignore
                    };
                    if v["id"].as_str() != Some(id.as_str()) {
                        continue; // a stale reply from an abandoned request
                    }
                    return match v["kind"].as_str() {
                        Some("result") => Ok(v["data"].clone()),
                        _ => {
                            let code = v["code"].as_str().unwrap_or(codes::ERROR).to_string();
                            let message = v["message"]
                                .as_str()
                                .unwrap_or("the operation failed")
                                .trim()
                                .to_string();
                            if code == codes::SESSION_LOST {
                                self.kill();
                            }
                            Err(AppError { code, message })
                        }
                    };
                }
                Err(RecvTimeoutError::Timeout) => {
                    // No in-band cancel: the only way to stop a stuck remote
                    // call is to drop the process (and with it the session).
                    self.kill();
                    return Err(AppError::new(
                        codes::TIMEOUT,
                        format!(
                            "'{op}' did not answer within {} seconds.",
                            timeout.as_secs().max(1)
                        ),
                    ));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    self.alive.store(false, Ordering::SeqCst);
                    return Err(self.lost("The session to the environment was lost."));
                }
            }
        }
    }

    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    fn close(&self) {
        if self.begin_close() {
            let child = self.child.clone();
            std::thread::spawn(move || PsWorker::wait_exit(&child, GRACEFUL_EXIT));
        }
    }

    fn close_and_wait(&self, max: Duration) {
        if self.begin_close() {
            PsWorker::wait_exit(&self.child, max);
        }
    }
}

impl Drop for PsWorker {
    fn drop(&mut self) {
        if self.is_alive() {
            self.close();
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    //! These start a real `powershell.exe` in local mode: they prove the
    //! bootstrap, the script and the line protocol work on Windows PowerShell
    //! 5.1 — everything except the WinRM hop itself.

    use super::*;

    const T: Duration = Duration::from_secs(30);

    #[test]
    fn local_worker_reports_ready_and_lists_drives() {
        let w = PsWorker::spawn_local().expect("worker should start");
        assert!(w.is_alive());
        let info = w.info();
        assert!(!info.computer_name.is_empty(), "{info:?}");
        assert!(info.ps_version.starts_with('5'), "expects Windows PowerShell 5.x: {info:?}");

        let drives = w.request("fs.drives", serde_json::json!({}), T).expect("drives");
        let list = drives.as_array().expect("an array of drives");
        assert!(!list.is_empty());
        let system = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
        let c = list
            .iter()
            .find(|d| d["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(&system)))
            .expect("the system drive should be listed");
        assert_eq!(c["ready"], true);
        assert!(c["totalBytes"].as_u64().unwrap() > 0);
        assert!(c["freeBytes"].as_u64().unwrap() <= c["totalBytes"].as_u64().unwrap());
        w.close();
    }

    #[test]
    fn requests_are_answered_in_order_on_one_session() {
        let w = PsWorker::spawn_local().unwrap();
        for _ in 0..5 {
            let pong = w.request("ping", serde_json::json!({}), T).unwrap();
            assert_eq!(pong["computerName"].as_str(), Some(w.info().computer_name.as_str()));
        }
        w.close();
    }

    #[test]
    fn unknown_operation_is_an_error_not_a_crash() {
        let w = PsWorker::spawn_local().unwrap();
        let err = w
            .request("fs.format-everything", serde_json::json!({}), T)
            .unwrap_err();
        assert!(err.is(codes::UNKNOWN_OP), "{err:?}");
        // The worker survives and keeps serving.
        assert!(w.is_alive());
        assert!(w.request("ping", serde_json::json!({}), T).is_ok());
        w.close();
    }

    #[test]
    fn request_text_is_data_never_script() {
        // An op name or argument that looks like PowerShell must not run.
        let marker = std::env::temp_dir().join(format!("wingui-inject-{}", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        let w = PsWorker::spawn_local().unwrap();
        let payload = format!("'; Set-Content -LiteralPath '{}' -Value x; '", marker.display());
        let _ = w.request(&payload, serde_json::json!({ "path": payload }), T);
        let _ = w.request("fs.drives", serde_json::json!({ "path": payload }), T);
        assert!(!marker.exists(), "request data was executed as script");
        w.close();
    }

    #[test]
    fn closing_ends_the_process() {
        let w = PsWorker::spawn_local().unwrap();
        w.close();
        assert!(!w.is_alive());
        let err = w.request("ping", serde_json::json!({}), T).unwrap_err();
        assert!(err.is(codes::SESSION_LOST), "{err:?}");
        // The process exits on its own once stdin closes.
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if w.child.lock().unwrap().try_wait().unwrap().is_some() {
                break;
            }
            assert!(Instant::now() < deadline, "worker did not exit after close");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
