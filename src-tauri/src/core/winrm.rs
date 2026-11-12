//! One-time WinRM client setup so deploy/restart/logs can reach hosts.
//!
//! Those features use PowerShell Remoting (WinRM). Because the target servers
//! are ephemeral and not Kerberos-reachable by short name, WinRM falls back to
//! NTLM, which needs the local WinRM service running and the target in the
//! client's `TrustedHosts` list. On managed laptops that config is often missing
//! or reset by Group Policy. This module reports the current state and applies
//! the fix (behind a UAC elevation prompt).
//!
//! **TrustedHosts is an allow-list, not a formality.** An entry tells the WinRM
//! client it may authenticate to that host with NTLM *without* being able to
//! verify the server's identity, so the account's credentials are handed to
//! whatever machine answers to the name. A `*` entry therefore means any host
//! anyone can steer a name lookup to (DNS/LLMNR/NBNS spoofing, a typo, a
//! malicious `connections.json`) can harvest — or relay — the shared company
//! account. Setup instead writes the **saved connections' computer names**,
//! merged with any entries already configured, so the blast radius is the hosts
//! the user actually uses. Re-running setup after adding connections extends the
//! list.

#[cfg(windows)]
use std::process::Command;

use serde::Serialize;

#[cfg(windows)]
use super::exec;
#[cfg(windows)]
use super::model::load_connections;

/// Current WinRM client configuration, for the Settings UI.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WinrmStatus {
    /// Whether the local WinRM service is running.
    pub service_running: bool,
    /// The client's TrustedHosts value (empty when unset).
    pub trusted_hosts: String,
    /// Service running *and* TrustedHosts non-empty — deploy/restart can connect.
    pub configured: bool,
    /// TrustedHosts contains a `*` wildcard: every host is trusted for NTLM, so
    /// the account credential can be captured by any host that answers to a
    /// name. Surfaced so Settings can warn and offer to narrow it.
    pub wildcard: bool,
}

/// PowerShell (non-elevated) that prints the current WinRM client state as two
/// `KEY=value` lines. Reading TrustedHosts needs the service up, so a stopped
/// service simply yields an empty value — i.e. "not configured".
#[cfg_attr(not(windows), allow(dead_code))]
const STATUS_SCRIPT: &str = r#"
$svc = (Get-Service -Name WinRM -ErrorAction SilentlyContinue).Status
Write-Output ("SERVICE=" + $svc)
try {
    $th = (Get-Item -Path WSMan:\localhost\Client\TrustedHosts -ErrorAction Stop).Value
    Write-Output ("TRUSTEDHOSTS=" + $th)
} catch {
    Write-Output "TRUSTEDHOSTS="
}
"#;

/// Non-elevated launcher that elevates the temp setup script and waits for it,
/// surfacing its exit code. Reads the script path from `RD_SCRIPT`.
#[cfg_attr(not(windows), allow(dead_code))]
const ELEVATE_SCRIPT: &str = r#"
$ErrorActionPreference = 'Stop'
$p = Start-Process powershell.exe -Verb RunAs -Wait -PassThru -WindowStyle Hidden -ArgumentList @('-NoProfile','-ExecutionPolicy','Bypass','-File',$env:RD_SCRIPT)
exit $p.ExitCode
"#;

/// Whether a host entry is a plain host name/IP we are willing to write into
/// TrustedHosts: letters, digits, `.`, `-`, `_` only. This both rejects the `*`
/// wildcard (and partial wildcards like `*.corp`) and guarantees the value can
/// be embedded in the generated PowerShell literal without escaping concerns.
fn is_plain_host(entry: &str) -> bool {
    !entry.is_empty()
        && entry
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
}

/// Whether a TrustedHosts value grants blanket trust (any `*` entry).
pub fn is_wildcard(trusted_hosts: &str) -> bool {
    trusted_hosts.split(',').any(|e| e.trim().contains('*'))
}

/// The TrustedHosts value setup should write: every saved connection's computer
/// name, plus the specific hosts already trusted, deduplicated
/// case-insensitively and in a stable order (existing entries first). Wildcards
/// and anything that isn't a plain host name are dropped — narrowing a `*` is
/// the point of this function. Returns `None` when nothing valid remains.
pub fn desired_trusted_hosts(existing: &str, connections: &[String]) -> Option<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |raw: &str| {
        let e = raw.trim();
        if !is_plain_host(e) {
            return;
        }
        if out.iter().any(|k: &String| k.eq_ignore_ascii_case(e)) {
            return;
        }
        out.push(e.to_string());
    };
    for e in existing.split(',') {
        push(e);
    }
    for c in connections {
        push(c);
    }
    if out.is_empty() {
        None
    } else {
        Some(out.join(","))
    }
}

/// The elevated setup script for a concrete host list. Only values that passed
/// [`is_plain_host`] reach the literal, so it cannot carry quotes or newlines.
#[cfg_attr(not(windows), allow(dead_code))]
fn setup_script(trusted_hosts: &str) -> String {
    format!(
        r#"
$ErrorActionPreference = 'Stop'
Set-Service -Name WinRM -StartupType Automatic
Start-Service -Name WinRM
$hosts = '{trusted_hosts}'
try {{
    Set-Item -Path WSMan:\localhost\Client\TrustedHosts -Value $hosts -Force
}} catch {{
    # Client config not initialised yet — bring it up, then set TrustedHosts.
    winrm quickconfig -quiet | Out-Null
    Set-Item -Path WSMan:\localhost\Client\TrustedHosts -Value $hosts -Force
}}
"#
    )
}

#[cfg(windows)]
fn powershell(script: &str) -> Command {
    let mut cmd = exec::command(&exec::powershell());
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        script,
    ]);
    cmd
}

#[cfg(windows)]
pub fn read_status() -> WinrmStatus {
    let output = powershell(STATUS_SCRIPT).output();
    let mut service_running = false;
    let mut trusted_hosts = String::new();
    if let Ok(output) = output {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let line = line.trim();
            if let Some(v) = line.strip_prefix("SERVICE=") {
                service_running = v.eq_ignore_ascii_case("Running");
            } else if let Some(v) = line.strip_prefix("TRUSTEDHOSTS=") {
                trusted_hosts = v.trim().to_string();
            }
        }
    }
    let configured = service_running && !trusted_hosts.is_empty();
    let wildcard = is_wildcard(&trusted_hosts);
    WinrmStatus {
        service_running,
        trusted_hosts,
        configured,
        wildcard,
    }
}

#[cfg(windows)]
pub fn run_setup() -> Result<String, String> {
    let current = read_status();
    let names: Vec<String> = load_connections()
        .into_iter()
        .map(|c| c.computer_name)
        .collect();
    let hosts = desired_trusted_hosts(&current.trusted_hosts, &names).ok_or_else(|| {
        "No environments to trust yet. Add the connections you deploy to first — WinRM setup \
         allows exactly those hosts (a wildcard would let any machine that answers to a name \
         collect the account password)."
            .to_string()
    })?;

    // Drop the elevated script into a private, freshly created directory with an
    // unpredictable name. A fixed path in the shared temp dir could be swapped
    // between the write and the elevation, turning this into a local
    // privilege-escalation primitive (we run the file as administrator).
    let dir = std::env::temp_dir().join(format!("rd-winrm-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&dir).map_err(|e| format!("could not create setup dir: {e}"))?;
    let script_path = dir.join("setup.ps1");
    let write = std::fs::write(&script_path, setup_script(&hosts));

    let output = write.and_then(|()| {
        powershell(ELEVATE_SCRIPT)
            .env("RD_SCRIPT", &script_path)
            .output()
    });
    let _ = std::fs::remove_dir_all(&dir);

    let output = output.map_err(|e| format!("could not run WinRM setup: {e}"))?;
    if output.status.success() {
        let status = read_status();
        return Ok(format!(
            "WinRM configured (service running, TrustedHosts = {}).",
            if status.trusted_hosts.is_empty() {
                hosts
            } else {
                status.trusted_hosts
            }
        ));
    }

    let detail = String::from_utf8_lossy(&output.stderr).to_lowercase();
    if detail.contains("cancel") {
        return Err("Setup needs administrator approval — the elevation prompt was dismissed.".into());
    }
    let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(if detail.is_empty() {
        "WinRM setup failed. It may be enforced by Group Policy — ask IT to enable the WinRM client and set TrustedHosts.".into()
    } else {
        detail
    })
}

#[cfg(not(windows))]
pub fn read_status() -> WinrmStatus {
    WinrmStatus {
        service_running: false,
        trusted_hosts: String::new(),
        configured: false,
        wildcard: false,
    }
}

#[cfg(not(windows))]
pub fn run_setup() -> Result<String, String> {
    Err("WinRM setup is only supported on Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_entries_are_detected() {
        assert!(is_wildcard("*"));
        assert!(is_wildcard("L-a-SD,*"));
        assert!(is_wildcard("*.corp.example"));
        assert!(!is_wildcard(""));
        assert!(!is_wildcard("L-a-SD,L-b-SD"));
    }

    #[test]
    fn desired_hosts_narrows_a_wildcard_to_the_saved_connections() {
        let got = desired_trusted_hosts("*", &["L-abc123-SD".into(), "L-def456-CL".into()]);
        assert_eq!(got.as_deref(), Some("L-abc123-SD,L-def456-CL"));
    }

    #[test]
    fn desired_hosts_keeps_existing_entries_and_dedupes() {
        let got = desired_trusted_hosts(
            "other-host, L-ABC123-SD",
            &["l-abc123-sd".into(), "L-new-SD".into()],
        );
        assert_eq!(got.as_deref(), Some("other-host,L-ABC123-SD,L-new-SD"));
    }

    #[test]
    fn desired_hosts_rejects_injection_and_wildcards() {
        // Nothing that could break out of the generated PowerShell literal, and
        // no partial wildcards, may survive.
        let got = desired_trusted_hosts(
            "",
            &[
                "good-host".into(),
                "bad';Invoke-Expression 'x".into(),
                "with space".into(),
                "*.corp".into(),
                "line\nbreak".into(),
            ],
        );
        assert_eq!(got.as_deref(), Some("good-host"));
    }

    #[test]
    fn desired_hosts_is_none_when_nothing_valid_remains() {
        assert!(desired_trusted_hosts("*", &[]).is_none());
        assert!(desired_trusted_hosts("", &["*".into()]).is_none());
    }

    #[test]
    fn setup_script_writes_the_exact_host_list_and_no_wildcard() {
        let script = setup_script("L-a-SD,L-b-SD");
        assert!(script.contains("$hosts = 'L-a-SD,L-b-SD'"), "{script}");
        assert!(!script.contains("-Value '*'"), "{script}");
    }
}
