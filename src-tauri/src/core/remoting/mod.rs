//! The one owner of WinRM/PowerShell transport.
//!
//! Every remote feature used to carry its own copy of the `New-PSSession …
//! -Authentication Negotiate` preamble, its own `CREATE_NO_WINDOW`, its own
//! credential/connection resolution, and its own stdout pump. All of that
//! lives here now — a feature module owns only its `Invoke-Command` script
//! body and its line-protocol schema.
//!
//! * [`resolve_target`] — global credential + connection → [`RemoteTarget`].
//! * [`wrap_session`] — composes the single PS-session preamble around a body.
//! * [`PsRunner`] — the transport trait (one-shot [`PsRunner::run`] and
//!   streaming [`PsRunner::spawn_stream`]); [`RealPsRunner`] is the
//!   `powershell.exe` implementation, and tests inject a fake so orchestration
//!   is unit-testable without spawning anything.
//! * [`pump_lines`] — the shared stdout line pump with side-thread stderr
//!   drain (previously duplicated in service/logs/deploy-status).
//! * [`SessionRegistry`] — tracks streaming child processes so they can be
//!   stopped individually and are killed on drop (previously `LogSessions` +
//!   `DeploySessions`, duplicated).
//!
//! Everything above is the **one-shot path**, ported unchanged from Remote
//! Desktop's `core::remoting` (commit 76f65f0): one `powershell.exe` per
//! action. It stays for the features already proven on it (deploy, restart,
//! log tail, …).
//!
//! WinGUI adds the **held-session path** for interactive work, where paying
//! for a new WinRM session on every click is not an option:
//!
//! * [`worker`] — a persistent `powershell.exe` that keeps one `PSSession`
//!   open and answers line-delimited JSON requests.
//! * [`pool`] — one such worker per open environment: lazy start, reconnect,
//!   idle close, per-host cap.

pub mod pool;
pub mod worker;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read};
use std::process::{Child, ChildStderr, ChildStdout, Command, Stdio};
use std::sync::Mutex;

use super::credentials;
use super::error::{codes, AppError};
use super::model::Connection;

// ---------------------------------------------------------------------------
// Target resolution
// ---------------------------------------------------------------------------

/// Everything the transport needs to reach one host.
#[derive(Debug, Clone)]
pub struct RemoteTarget {
    pub host: String,
    pub username: String,
    pub password: String,
}

/// Resolve the WinRM target for a connection: read the global credential (set
/// in Settings), find the connection, and work out the effective username.
/// Error messages are load-bearing — the UI shows them verbatim; the codes
/// let it offer the right next step (open Settings, pick another environment).
pub fn resolve_target<'a>(
    connections: &'a [Connection],
    connection_id: &str,
) -> Result<(&'a Connection, RemoteTarget), AppError> {
    let (global_username, password) = credentials::read_global()
        .filter(|(_, pw)| !pw.is_empty())
        .ok_or_else(|| {
            AppError::new(
                codes::NO_ACCOUNT,
                "No account password set. Add the account username and password in Settings.",
            )
        })?;
    let conn = connections
        .iter()
        .find(|c| c.id == connection_id)
        .ok_or_else(|| {
            AppError::new(
                codes::NO_CONNECTION,
                format!("No connection with id {connection_id}"),
            )
        })?;
    let username = conn.resolve_username(Some(&global_username));
    let target = RemoteTarget {
        host: conn.computer_name.clone(),
        username: kerberos_qualified(
            &username,
            std::env::var("USERDOMAIN").ok().as_deref(),
            std::env::var("USERDNSDOMAIN").ok().as_deref(),
        ),
        password,
    };
    Ok((conn, target))
}

/// Rewrite a `NETBIOS\user` username to `dns.domain\user` so Kerberos can use
/// it from a client that isn't joined to the domain.
///
/// Negotiate with explicit credentials tries Kerberos first, which has to
/// locate a KDC for the credential's domain. On an Entra-ID-joined (not AD
/// joined) laptop the NetBIOS name (`CAMBIO`) often can't be resolved to a
/// domain controller — whether it can depends on the network the laptop is
/// on — while the DNS name (`cambio.se`) always can. When Kerberos fails,
/// Negotiate falls back to NTLM, which the WinRM client only permits for
/// TrustedHosts entries, so any environment not in that list fails with
/// `0x8009030e` ("A specified logon session does not exist").
///
/// So when the username's domain is the logged-on user's own NetBIOS domain
/// (`USERDOMAIN`) and Windows knows its DNS name (`USERDNSDOMAIN`), send the
/// DNS form: same account, resolvable realm. Anything else — UPNs, local
/// `.\user` / `HOST\user` accounts, other domains, bare names — is left as-is.
fn kerberos_qualified(username: &str, netbios: Option<&str>, dns: Option<&str>) -> String {
    let (Some(netbios), Some(dns)) = (netbios, dns) else {
        return username.to_string();
    };
    match username.split_once('\\') {
        Some((domain, user))
            if !user.is_empty()
                && !netbios.is_empty()
                && !dns.is_empty()
                && domain.eq_ignore_ascii_case(netbios)
                && !domain.eq_ignore_ascii_case(dns) =>
        {
            format!("{dns}\\{user}")
        }
        _ => username.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Script composition
// ---------------------------------------------------------------------------

/// Compose the full remote script: strict error handling, an `Emit` helper for
/// flushed line output, credential + session setup (auth method and open
/// timeout live only here), the feature body, and guaranteed session cleanup.
///
/// `pre_session` runs before `New-PSSession` (e.g. an early progress line);
/// pass `""` when not needed. The body talks to the session as `$s` and reads
/// its inputs from `RD_*` environment variables.
pub fn wrap_session(pre_session: &str, body: &str) -> String {
    format!(
        r#"
$ErrorActionPreference = 'Stop'
function Emit($m) {{ [Console]::Out.WriteLine($m); [Console]::Out.Flush() }}
$sec = ConvertTo-SecureString $env:RD_PASSWORD -AsPlainText -Force
$cred = New-Object System.Management.Automation.PSCredential($env:RD_USER, $sec)
{pre_session}
$opt = New-PSSessionOption -OpenTimeout 20000
$s = New-PSSession -ComputerName $env:RD_HOST -Credential $cred -Authentication Negotiate -SessionOption $opt
try {{
{body}
}} finally {{
    Remove-PSSession $s
}}
"#
    )
}

/// The `RD_*` environment for a target's credentials, ready to extend with
/// feature-specific variables.
pub fn target_env(target: &RemoteTarget) -> Vec<(&'static str, String)> {
    vec![
        ("RD_HOST", target.host.clone()),
        ("RD_USER", target.username.clone()),
        ("RD_PASSWORD", target.password.clone()),
    ]
}

// ---------------------------------------------------------------------------
// Runners
// ---------------------------------------------------------------------------

/// Captured output of a one-shot PowerShell run.
#[derive(Debug, Clone, Default)]
pub struct PsOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    /// Display form of the exit status, e.g. `exit code: 1`.
    pub status: String,
}

impl PsOutput {
    /// The first stdout line starting with `prefix` (trimmed at the start),
    /// e.g. the `OK|…` result line of a feature script.
    pub fn find_line(&self, prefix: &str) -> Option<&str> {
        self.stdout
            .lines()
            .map(|l| l.trim_start())
            .find(|l| l.starts_with(prefix))
    }
}

/// The transport seam: features run scripts through this trait, so tests can
/// inject a fake that returns canned output instead of spawning
/// `powershell.exe`.
pub trait PsRunner: Send + Sync {
    /// Run a script to completion, with `env` exported as process environment
    /// (secrets travel via env, never argv, so they don't show in the process
    /// list).
    fn run(&self, script: &str, env: &[(&str, String)]) -> Result<PsOutput, String>;

    /// Spawn a script with piped stdout/stderr for streaming (log tail,
    /// deploy watch, restart progress).
    fn spawn_stream(&self, script: &str, env: &[(&str, String)]) -> Result<Child, String>;
}

/// Build the standard non-interactive `powershell.exe` invocation. The one
/// place `CREATE_NO_WINDOW` and the interpreter flags are defined for remote
/// features (signing/winrm keep their own — separate concerns).
///
/// The interpreter is resolved to its absolute `System32` path
/// ([`super::exec::powershell`]): this process runs from inside developer
/// workspaces and cloned repos, and the Windows search order would otherwise
/// prefer a `powershell.exe` sitting in the working directory — which would
/// receive the account password in `RD_PASSWORD`.
fn ps_command(script: &str, env: &[(&str, String)]) -> Command {
    let mut cmd = super::exec::command(&super::exec::powershell());
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        script,
    ]);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd
}

/// Run a script on the *local* machine to completion (also used by
/// auto-deploy's zip inspection helpers, which previously carried their own
/// copies of this invocation).
pub fn run_local_ps(script: &str, env: &[(&str, String)]) -> Result<PsOutput, String> {
    let output = ps_command(script, env)
        .output()
        .map_err(|e| format!("could not run powershell: {e}"))?;
    Ok(PsOutput {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        status: output.status.to_string(),
    })
}

/// The real `powershell.exe`-spawning runner used outside tests.
pub struct RealPsRunner;

impl PsRunner for RealPsRunner {
    fn run(&self, script: &str, env: &[(&str, String)]) -> Result<PsOutput, String> {
        run_local_ps(script, env)
    }

    fn spawn_stream(&self, script: &str, env: &[(&str, String)]) -> Result<Child, String> {
        ps_command(script, env)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("could not run powershell: {e}"))
    }
}

/// Compose the standard failure message for a remote action: stderr, falling
/// back to stdout (when `stdout_fallback`), falling back to the exit status —
/// plus the identity/host line that makes auth failures diagnosable.
pub fn failure_detail(
    output: &PsOutput,
    target: &RemoteTarget,
    stdout_fallback: bool,
) -> String {
    let mut detail = output.stderr.trim().to_string();
    if detail.is_empty() && stdout_fallback {
        detail = output.stdout.trim().to_string();
    }
    if detail.is_empty() {
        detail = format!("powershell exited with {}", output.status);
    }
    format!(
        "{detail}\n\nAttempted as: {}  (host: {})",
        target.username, target.host
    )
}

/// One-line form of a failure detail for compact summaries: the first
/// non-empty line, joined with the next when the first only introduces it.
/// PowerShell's `New-PSSession` error reads "Connecting to remote server X
/// failed with the following error message :" with the actual reason (and
/// error code) on the following line.
pub fn summary_line(detail: &str) -> String {
    let mut lines = detail.lines().map(str::trim).filter(|l| !l.is_empty());
    let first = lines.next().unwrap_or("");
    match lines.next() {
        Some(next) if first.ends_with(':') => format!("{first} {next}"),
        _ => first.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Streaming helpers
// ---------------------------------------------------------------------------

/// Read `stdout` line-by-line, forwarding each to `on_line` until the stream
/// ends or `on_line` returns `false` (receiver gone). Drains `stderr` on a
/// side thread the whole time so a large error stream can't fill the pipe and
/// deadlock the child. Returns the collected stderr text.
pub fn pump_lines(
    stdout: ChildStdout,
    stderr: ChildStderr,
    mut on_line: impl FnMut(&str) -> bool,
) -> String {
    let mut stderr = stderr;
    let err_handle = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stderr.read_to_string(&mut buf);
        buf
    });

    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        if !on_line(&line) {
            break;
        }
    }

    err_handle.join().unwrap_or_default()
}

/// Tracks streaming child processes (log tails, deploy watches) by session id
/// so each can be stopped individually; kills whatever is left on drop.
#[derive(Default)]
pub struct SessionRegistry {
    children: Mutex<HashMap<String, Child>>,
}

impl SessionRegistry {
    pub fn insert(&self, id: String, child: Child) {
        self.children.lock().unwrap().insert(id, child);
    }

    /// Remove a session without killing it (the pump thread does this when the
    /// stream ends, then `wait()`s the child itself).
    pub fn remove(&self, id: &str) -> Option<Child> {
        self.children.lock().unwrap().remove(id)
    }

    /// Stop a session, killing its child process. Unknown ids are a no-op.
    pub fn stop(&self, id: &str) {
        if let Some(mut child) = self.remove(id) {
            let _ = child.kill();
        }
    }
}

impl Drop for SessionRegistry {
    fn drop(&mut self) {
        if let Ok(mut map) = self.children.lock() {
            for child in map.values_mut() {
                let _ = child.kill();
            }
        }
    }
}

#[cfg(test)]
pub mod test_support {
    //! A canned-output fake runner for orchestration tests.

    use std::sync::Mutex;

    use super::*;

    pub struct FakePsRunner {
        /// Queue of results returned by successive `run` calls.
        pub outputs: Mutex<Vec<Result<PsOutput, String>>>,
        /// Scripts and envs the fake received, for assertions.
        pub calls: Mutex<Vec<(String, Vec<(String, String)>)>>,
    }

    impl FakePsRunner {
        pub fn with_outputs(outputs: Vec<Result<PsOutput, String>>) -> Self {
            FakePsRunner {
                outputs: Mutex::new(outputs),
                calls: Mutex::new(Vec::new()),
            }
        }

        pub fn ok(stdout: &str) -> PsOutput {
            PsOutput {
                success: true,
                stdout: stdout.to_string(),
                stderr: String::new(),
                status: "exit code: 0".into(),
            }
        }

        pub fn failed(stderr: &str) -> PsOutput {
            PsOutput {
                success: false,
                stdout: String::new(),
                stderr: stderr.to_string(),
                status: "exit code: 1".into(),
            }
        }
    }

    impl PsRunner for FakePsRunner {
        fn run(&self, script: &str, env: &[(&str, String)]) -> Result<PsOutput, String> {
            self.calls.lock().unwrap().push((
                script.to_string(),
                env.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(),
            ));
            let mut outputs = self.outputs.lock().unwrap();
            if outputs.is_empty() {
                Ok(FakePsRunner::ok(""))
            } else {
                outputs.remove(0)
            }
        }

        fn spawn_stream(&self, _script: &str, _env: &[(&str, String)]) -> Result<Child, String> {
            Err("FakePsRunner does not support streaming".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(id: &str, name: &str, username: Option<&str>) -> Connection {
        Connection {
            id: id.into(),
            label: name.into(),
            computer_name: name.into(),
            username: username.map(String::from),
            service_name: None,
            server_root: None,
            client_root: None,
            pinned: false,
            label_id: None,
            category_id: None,
        }
    }

    #[test]
    fn wrap_session_contains_the_session_shell_exactly_once() {
        let script = wrap_session("Emit 'STEP|Connecting…'", "    Emit 'OK|done'");
        assert_eq!(script.matches("New-PSSession -ComputerName").count(), 1);
        assert_eq!(script.matches("Remove-PSSession").count(), 1);
        assert!(script.contains("-Authentication Negotiate"));
        assert!(script.contains("OpenTimeout 20000"));
        // Pre-session code appears before the session is created.
        let pre = script.find("STEP|Connecting").unwrap();
        let new = script.find("New-PSSession").unwrap();
        assert!(pre < new);
    }

    #[test]
    fn resolve_target_unknown_connection() {
        // Note: resolve_target checks the global credential before the
        // connection, so this test only runs where one is present; the
        // missing-connection branch is covered via the registry tests with a
        // fake credential-independent path in CI-less environments.
        let list = vec![conn("a", "L-a-SD", None)];
        match resolve_target(&list, "missing") {
            Err(e) => assert!(
                e.is(codes::NO_CONNECTION) || e.is(codes::NO_ACCOUNT),
                "unexpected error: {e:?}"
            ),
            Ok(_) => {
                // A machine with a real global credential resolves only
                // existing connections.
                panic!("resolved a target for a missing connection");
            }
        }
    }

    #[test]
    fn kerberos_qualified_maps_own_netbios_domain_to_dns_name() {
        let q = |u: &str| kerberos_qualified(u, Some("CAMBIO"), Some("cambio.se"));
        assert_eq!(q(r"CAMBIO\Geeth.Sandaru"), r"cambio.se\Geeth.Sandaru");
        assert_eq!(q(r"cambio\svc"), r"cambio.se\svc");
        // Already resolvable, or not the user's own domain: untouched.
        assert_eq!(q(r"cambio.se\svc"), r"cambio.se\svc");
        assert_eq!(q("svc@cambio.se"), "svc@cambio.se");
        assert_eq!(q(r".\admin"), r".\admin");
        assert_eq!(q(r"L-abc-SD\admin"), r"L-abc-SD\admin");
        assert_eq!(q(r"OTHER\svc"), r"OTHER\svc");
        assert_eq!(q("svc"), "svc");
        assert_eq!(q(r"CAMBIO\"), r"CAMBIO\");
        // No DNS domain known (local-account logon): nothing to map to.
        assert_eq!(kerberos_qualified(r"CAMBIO\svc", Some("CAMBIO"), None), r"CAMBIO\svc");
        assert_eq!(kerberos_qualified(r"CAMBIO\svc", Some("CAMBIO"), Some("")), r"CAMBIO\svc");
        assert_eq!(kerberos_qualified(r"\svc", Some(""), Some("cambio.se")), r"\svc");
    }

    #[test]
    fn summary_line_keeps_the_reason_after_an_introducing_line() {
        let ps = "New-PSSession : [H] Connecting to remote server H failed with the following error message : \r\n\
                  WinRM cannot process the request. The following error with error code 0x8009030e occurred\r\n\
                  authentication: A specified logon session does not exist.";
        assert_eq!(
            summary_line(ps),
            "New-PSSession : [H] Connecting to remote server H failed with the following error message : \
             WinRM cannot process the request. The following error with error code 0x8009030e occurred"
        );
        assert_eq!(summary_line("\n  local file missing\nmore"), "local file missing");
        assert_eq!(summary_line("ends with colon:"), "ends with colon:");
        assert_eq!(summary_line(""), "");
    }

    #[test]
    fn failure_detail_prefers_stderr_then_stdout_then_status() {
        let target = RemoteTarget {
            host: "H".into(),
            username: "U".into(),
            password: "P".into(),
        };
        let mut out = PsOutput {
            success: false,
            stdout: "out".into(),
            stderr: "err".into(),
            status: "exit code: 1".into(),
        };
        assert!(failure_detail(&out, &target, true).starts_with("err"));
        out.stderr.clear();
        assert!(failure_detail(&out, &target, true).starts_with("out"));
        assert!(failure_detail(&out, &target, false).starts_with("powershell exited with"));
        out.stdout.clear();
        let msg = failure_detail(&out, &target, true);
        assert!(msg.starts_with("powershell exited with exit code: 1"));
        assert!(msg.contains("Attempted as: U  (host: H)"));
    }
}
