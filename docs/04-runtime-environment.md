# 4. Runtime environment concerns

[← Plan index](../README.md)

WinGUI runs where Remote Desktop runs today: users' **Windows 11 laptops**, on the
corporate **VPN**, talking **WinRM** to Cambio environments. Most concerns below are
already handled by Remote Desktop's code and are carried over; the ones marked **new**
arise from holding sessions open, browsing arbitrary folders and moving large files.

Items marked *verify* are assumptions about defaults or behaviour that Step 1 must
confirm on a real environment — they are not measurements.

## 4.1 The user's laptop

| Concern | What to do |
|---|---|
| **WebView2** | Preinstalled on Windows 11. The installer still declares the dependency (bootstrapper) for the rare stripped image |
| **Windows PowerShell 5.1 only** | Use `powershell.exe` from `System32` by absolute path. Do not assume PowerShell 7. Avoid 7-only syntax and cmdlets |
| **Encoding** (new) | PowerShell 5.1 writes console output in the OEM code page by default. The worker forces UTF-8 in and out, otherwise names with å/ä/ö arrive corrupted |
| **Execution policy** | Scripts are passed inline / over stdin with `-ExecutionPolicy Bypass`; no `.ps1` files need to be trusted |
| **AppLocker / WDAC / Constrained Language Mode** | If a laptop enforces Constrained Language Mode, .NET calls in the scripts fail. Remote Desktop already depends on Full Language mode, so this is an existing constraint — add an explicit check to the Doctor with a clear message |
| **Endpoint protection** | A GUI app spawning hidden PowerShell with long scripts can attract EDR attention. Keep scripts readable (no encoded commands), keep one stable signed executable, and brief IT security before rollout |
| **No admin rights** | Install per user. Elevation (UAC) is needed only for the one-time WinRM client setup (`TrustedHosts`, WinRM service) |
| **WinRM client config drift** | Group Policy can reset `TrustedHosts` or stop the service. Detect on start and on connection failure; surface it in Settings with a one-click fix. Re-run after adding environments (it merges) |
| **Domain-joined vs Entra-ID-joined** | Keep the `NETBIOS\user` → `dns.domain\user` rewrite so Kerberos can find a KDC from Entra-joined laptops; otherwise Negotiate falls back to NTLM and fails for hosts not in `TrustedHosts` (error `0x8009030e`) |
| **VPN** | All features need it. Distinguish "not on VPN / name does not resolve" from "host down" from "auth failed" in error messages |
| **Sleep, lid close, VPN reconnect** (new) | Held sessions die silently. Detect on next use, reconnect with backoff, and resume or cleanly fail in-flight transfers |
| **Child-process hygiene** (new) | Each worker is a `powershell.exe` using tens of MB. Cap workers, close idle ones, and tie all children to a Job Object so a crash never leaves orphans |
| **Local disk** (new) | Check free space before large downloads; put `.partial` files beside the target (same volume); sweep the staging folder |
| **Local paths** (new) | Remote names can be longer than, or illegal in, the local target (reserved names, trailing dots, > 260 chars). Use long-path-aware APIs and report unmappable names per file |
| **Antivirus on downloads** | Real-time scanning slows many-small-file copies and may quarantine JARs; surface the failure clearly rather than "transfer failed" |
| **Clipboard** (new) | The file-drop clipboard format needs files that already exist locally, so "copy" stages first. Clear stale staged data when the clipboard is replaced or the app exits |
| **Display** | Honour per-monitor DPI, light/dark and reduced-motion; test at 100 %, 150 % and 200 % |
| **Single instance** | A second launch focuses the first window; the CLI may run alongside and must write stores safely |
| **April 2026 `.rdp` consent dialog** | Keep the signing flow (self-signed publisher certificate provisioned on first run, or a corporate CA certificate on managed machines) so RDP launch stays one click |

## 4.2 The environments (headless servers)

| Concern | What to do |
|---|---|
| **WinRM must be on** | Windows Server enables PowerShell Remoting by default, but confirm the headless image and Jenkins provisioning keep it, with the firewall rule for 5985 on the profile the VPN lands in (*verify*, Q7) |
| **Account rights** | Remoting, `D:\` access and service control need the account to be a local administrator on the environment (as today). A non-admin account would connect but fail on most operations — report this specifically |
| **Server Core differences** | No Explorer, no `services.msc`, no shell dialogs — irrelevant to WinGUI, which uses .NET file APIs and CIM. Avoid any cmdlet that depends on the Desktop Experience feature; test every operation on the headless image (*verify*) |
| **Per-user shell quotas** (new) | WinRM limits shells per user and concurrent users (defaults in the order of 30 shells per user and 10 concurrent users — *verify*). With a **shared account**, every WinGUI and Remote Desktop user on one environment counts as the same user. Hence the cap of 4 sessions per host, idle close, and strict cleanup. On quota errors, say so and name the fix |
| **Shell memory and idle limits** (new) | Each remote shell has a memory ceiling and an idle timeout (*verify*). Stream large results in pages, never build huge arrays remotely, and treat an expired session as a normal reconnect |
| **Double hop** (new) | Inside a WinRM session the account cannot authenticate onward, so mapped drives and UNC shares on *other* servers are not reachable. The explorer therefore lists local drives only; say so if a user types a UNC path |
| **Locked files** | A running JBoss holds module JARs open. Keep the staged-then-swap upload and surface "in use" as its own error with the options (retry swap, stop the service first) |
| **Disk space** | Backups and temp archives consume space on small environment disks. Show free space, warn before large remote zips, offer backup clean-up (FR-F3) |
| **Paths differ per environment** | Never hardcode `D:\Cambio\Production`; derive from `EnvLayout` with per-environment overrides. Quick-access shortcuts that do not exist on a host are greyed, not errors |
| **Access denied inside listings** | System folders will refuse enumeration. One inaccessible child must not fail the parent listing |
| **Very large folders and files** | Log and export folders can hold tens of thousands of entries and multi-GB files: page listings, chunk transfers, never read whole files into memory on either side |
| **Clocks and time zones** | Return timestamps in UTC and convert for display. Kerberos tolerates only a few minutes of clock skew — include skew in the Doctor |
| **Ephemeral hosts and name reuse** | An environment can be rebuilt under the same name. Treat any session-identity change as a fresh connection and drop cached listings |
| **Restart side effects** | Restarting JBoss or the host kills streams and may drop the session; reconnect and resume watches automatically |
| **Stopping the wrong service** (new) | Stopping WinRM cuts the branch WinGUI sits on, with no GUI to recover. Protected-service list is enforced in the core |
| **Public and CCP environments** | Read-only by rule, enforced in the core. Test the refusal path as carefully as the success path |

## 4.3 Network and authentication

| Concern | What to do |
|---|---|
| **Transport security** | WinRM over HTTP 5985 with Negotiate encrypts the payload at message level (Kerberos or NTLM session security). HTTPS 5986 is not required but should work if an environment offers it — keep the port/scheme a per-environment advanced option |
| **NTLM fallback** | Only for hosts in `TrustedHosts`; that list is the saved environments, never `*`, because an entry means "send credentials without verifying the server" |
| **Name resolution** | Short names depend on VPN DNS suffixes. Classification always uses the short name, so an FQDN cannot dodge the safety rule |
| **Throughput** (new) | WinRM file transfer is base64 inside SOAP — expect it to be far slower than SMB. Measure in Step 1; mitigate with chunk size tuning, remote zip for many small files, and the optional SMB strategy if 445 is open (Q5) |
| **Latency** | Batch remote calls: one call returns a whole listing page; never one call per file |
| **Password changes** | When the shared account's password rotates, every session fails authentication. Detect the auth failure class and link straight to account settings rather than retrying into a lockout |
| **Lockout risk** (new) | Automatic reconnect must **not** retry on authentication failure — only on connectivity loss |

## 4.4 Build, packaging and distribution

| Concern | What to do |
|---|---|
| **Toolchain** | Rust stable, Node.js (match Remote Desktop's pinned version), Visual Studio Build Tools with the C++ workload |
| **Installer** | NSIS per-user installer from `tauri build`; MSI available if IT prefers managed deployment |
| **Code signing** | An unsigned exe triggers SmartScreen and looks worse to EDR. Sign with a company certificate if one is available (Q8) |
| **Updates** | Start with manual installs from the release page; adopt the Tauri updater once signing and a release location are settled. Settings and credentials survive upgrades |
| **Coexistence with Remote Desktop** | Different identifier, config folder and credential target — the two apps can be installed side by side and never write each other's files. Both add to the same `TrustedHosts` list by merging |
| **Size and start-up** | SQLite is compiled in; no runtime dependencies. Keep the bundle lean (tree-shaken icons, no heavyweight editor unless FR-C12 needs it — then load it lazily) |
| **CI** | Platform-neutral core tests on Linux runners (Win32 APIs gated behind `cfg(windows)`, as in Remote Desktop); Windows runner for the build, installer and worker integration tests |
| **Diagnostics** | Local rotating log with secrets redacted and a "copy diagnostics" action; no telemetry |

## 4.5 Pre-rollout checklist for a headless environment

1. WinRM listening on 5985 and reachable from the VPN.
2. The account can open a remoting session and is a local administrator.
3. Every WinGUI operation exercised on the headless image: drives, listing, download,
   upload, rename/delete, services list and control, deploy, restart, log tail,
   deployment watch, property set.
4. Session quotas checked against expected concurrent users of the shared account.
5. JBoss service name and server root recorded for the environment template.
6. Billing-export path confirmed and added to quick access.
7. Fallback agreed: how to reach the machine if WinRM is down (console access via the
   hypervisor), since there is no RDP desktop to fall back to.
