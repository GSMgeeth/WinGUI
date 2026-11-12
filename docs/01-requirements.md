# 1. Requirements

[← Plan index](../README.md)

## 1.1 Goal and success criteria

**Goal.** Remove every reason a developer or tester has to open an interactive Remote
Desktop session to a **server-side** environment, so those VMs can run without a GUI.

**Success looks like:**

- A user can complete the daily server-side tasks — find a file, copy it to the laptop,
  replace JARs, restart JBoss, watch it deploy, read the log, check and manage services —
  from WinGUI alone.
- A pilot group works against a headless environment for two weeks without needing RDP
  to the server.
- No new software is installed on the environments, and no database server on laptops.

## 1.2 Users

| User | Typical tasks |
|---|---|
| Developer | Replace JARs for a server-side change, restart JBoss, tail `ix3.log`, add properties, compare a deployed JAR with a local build |
| Tester / QA | Check services, fetch billing export files, read logs, check free disk space |
| Environment owner | Inspect what is running, clean up, diagnose a failed deployment |

## 1.3 Scope

**In scope:** Windows server-side environments reachable over VPN + WinRM; Windows
client machines for file/JAR operations and RDP launch.

**Out of scope:**

- Workspace management, Jira/Bitbucket integration, cloning and building repos (stays in
  Remote Desktop).
- Rendering a remote GUI / remote application windows.
- Installing an agent or anything else on environments.
- Linux hosts, non-WinRM transports (SSH), provisioning environments.
- Managing Windows itself (updates, firewall, users, registry editing).

## 1.4 Functional requirements

Priority: **M** = must (first usable release), **S** = should, **C** = could / later.
"Mutating" requirements are subject to the environment-class guard in
[Architecture §3.5](03-architecture.md#35-safety-model).

### A. Environments and access (ported from Remote Desktop)

| ID | Requirement | Pri |
|---|---|---|
| FR-A1 | Save, edit, delete environments by computer name; optional friendly label, username override, JBoss service name, server/client root overrides | M |
| FR-A2 | Group by label and category, pin favourites, browse as a category → label tree; search/filter by name | M |
| FR-A3 | One global account (username + password) stored in Windows Credential Manager; per-environment username override | M |
| FR-A4 | Show each environment's class (private server, private client, public server, CCP, unclassified) and the actions it allows, computed by the backend | M |
| FR-A5 | WinRM client status and one-click setup (service + `TrustedHosts` = saved hosts only, never `*`), behind UAC | M |
| FR-A6 | One-time import of connections, labels, categories, modules, property sets, auto-deploy roots and the account from Remote Desktop | M |
| FR-A7 | Open several environments at once as tabs; show per-tab connection state; reconnect automatically after VPN drop or sleep | M |
| FR-A8 | "Doctor" check: account present, WinRM configured, host resolvable, port reachable, authentication succeeds — with a plain-language cause for each failure | S |

### B. File Explorer

| ID | Requirement | Pri |
|---|---|---|
| FR-B1 | List all accessible drives with label, file system, total and free space | M |
| FR-B2 | Browse folders in a details view: name, date modified, type, size; sort by any column; folders first | M |
| FR-B3 | Navigate by double-click, breadcrumb, editable address bar (paste a path), back / forward / up, refresh | M |
| FR-B4 | Folder tree in the navigation pane, lazily expanded | M |
| FR-B5 | Multi-select (Ctrl, Shift, Ctrl+A), context menu, keyboard navigation and type-ahead | M |
| FR-B6 | Handle large folders (tens of thousands of entries) with progressive loading and a virtualised list | M |
| FR-B7 | Quick access: built-in COSMIC shortcuts derived from the environment layout (server root, JBoss deployments, modules, log folder, `environment.properties`, billing exports) plus user-pinned folders, global or per environment | M |
| FR-B8 | Filter the current folder by name; recursive search under a folder with bounded depth/result count and cancel | S |
| FR-B9 | Show hidden/system items toggle; clear indication of inaccessible folders (access denied) without failing the whole listing | M |
| FR-B10 | Properties: full path, size (folder size calculated on demand), created/modified, attributes, SHA-256 on demand | S |
| FR-B11 | Preview pane for text files (first N KB, encoding-aware); "Open locally" downloads to a temp area and opens with the default app | S |
| FR-B12 | Copy full path / copy as UNC to the clipboard | S |
| FR-B13 | Compare a remote file with a local file (size + SHA-256) — "is the deployed JAR my build?" | S |
| FR-B14 | Peek inside JAR/ZIP: list entries, show `MANIFEST.MF` and Maven `pom.properties` version | C |
| FR-B15 | Several explorer tabs per environment | C |

### C. Transfers and file operations

| ID | Requirement | Pri |
|---|---|---|
| FR-C1 | Download files and whole folders from the remote machine to a chosen local folder | M |
| FR-C2 | Copy on the remote side, paste in Windows Explorer: **Ctrl+C** in WinGUI stages the files locally and places them on the Windows clipboard so **Ctrl+V** works in Explorer — the same habit as built-in Remote Desktop | M |
| FR-C3 | Transfer queue with per-item and overall progress, speed, ETA, pause/cancel, retry; survives switching tabs | M |
| FR-C4 | Integrity: size check always, SHA-256 optional per transfer (default on for uploads) | M |
| FR-C5 | Conflict handling on existing targets: skip, overwrite, keep both, apply to all | M |
| FR-C6 | Resume an interrupted download from where it stopped | S |
| FR-C7 | Upload files and folders (file picker and drag-and-drop from Windows Explorer into the window) — **mutating** | M |
| FR-C8 | Uploads onto an existing file are staged beside the target then swapped, with an optional timestamped `.bak` (the proven deploy behaviour) — **mutating** | M |
| FR-C9 | New folder, rename, delete, copy and move within the remote machine — **mutating** | M |
| FR-C10 | Delete requires confirmation naming the item count and total size; recursive deletes above a threshold require typing the folder name — **mutating** | M |
| FR-C11 | Compress to ZIP / extract ZIP on the remote machine — **mutating** | S |
| FR-C12 | Edit small text files (properties, XML, config) in-app with backup on save and a diff before saving — **mutating** | S |
| FR-C13 | Fast path for many small files: zip remotely, transfer one archive, expand locally (automatic above a file-count threshold) | S |
| FR-C14 | Drag files *out* of WinGUI onto the desktop / Explorer | C |

### D. Environment tools menu (Remote Desktop parity)

Everything Remote Desktop offers except workspace management, reachable from one
**Environment tools** menu on each environment and from the home tile.

| ID | Remote Desktop feature | In WinGUI | Pri |
|---|---|---|---|
| FR-D1 | Deploy JAR to a registered module (backup, staged swap, size verify) + module registry | Ported unchanged | M |
| FR-D2 | Auto-detect & deploy (Spider / IX3, beta) from a built module folder | Ported; the "build first" option is dropped because the shared Maven/Gradle command lives in workspace config | M |
| FR-D3 | Restart JBoss with live phase updates | Ported; also offered from the Services view | M |
| FR-D4 | Deployment status watch (JBoss marker files) | Ported; auto-starts after a restart | M |
| FR-D5 | Log viewer tailing `ix3.log` | Ported and generalised: tail **any** text file from the explorer; list and download rotated `ix3.log.<timestamp>` archives | M |
| FR-D6 | Property sets (append-only, backup, skip existing keys) + property-set registry | Ported unchanged | M |
| FR-D7 | Launch an RDP session (signed `.rdp` + `mstsc`), saved `TERMSRV/<host>` credential | Ported; primary use is client machines | M |
| FR-D8 | Background task bar / task panel | Ported and merged with the transfer queue | M |
| FR-D9 | Command line (`list`, `deploy`, `restart`, `logs`, `status`, `doctor`, …) | Ported and extended (`ls`, `get`, `put`, `services`) | S |
| FR-D10 | AI-agent skills that shell out to the CLI (`/deploy`, `/restart-jboss`, `/add-property`) | Ported with the CLI | C |
| — | Workspaces, Jira/Bitbucket, clone index, build, skills sync into workspaces | **Not ported** | — |

### E. Services and processes

| ID | Requirement | Pri |
|---|---|---|
| FR-E1 | List Windows services: name, display name, status, startup type, log-on account, PID, executable path | M |
| FR-E2 | Default filter "COSMIC / Cambio" (configurable name/display-name/path patterns, anything under the server or client root, and the environment's JBoss service); toggle to show all | M |
| FR-E3 | Search, sort, auto-refresh while the view is open | M |
| FR-E4 | Start, stop, restart a service with bounded waits and live status — **mutating**; allowed only for services in the Cambio filter by default | M |
| FR-E5 | Protected services can never be stopped from WinGUI (at minimum WinRM, which is the transport) | M |
| FR-E6 | Service details: description, dependencies, memory and uptime of its process, link to its executable folder in the explorer | S |
| FR-E7 | Process list (name, PID, CPU, memory, start time, command line, owning service); end a process — **mutating** | S |
| FR-E8 | Change a service's startup type — **mutating** | C |

### F. Additional features proposed

| ID | Feature | Why it earns its place | Pri |
|---|---|---|---|
| FR-F1 | **Environment overview**: reachability, JBoss service state and uptime, free space per drive with low-space warning, last deployment result, last restart | Replaces the "log in and look around" habit; disk-full is a common silent failure | S |
| FR-F2 | **Activity journal**: every mutating action and every transfer, with host, time, target, outcome and backup path; filterable; exportable to CSV | Replaces the audit trail an RDP session never had; essential once several people change shared environments | M |
| FR-F3 | **Backup manager**: find the `.bak` files WinGUI/Remote Desktop created, restore one, delete old ones | Backups accumulate on small disks; restoring is the natural undo | S |
| FR-F4 | **Windows Event Log viewer** (Application/System, filtered to errors and to COSMIC/JBoss sources) | The usual next stop when a service will not start | C |
| FR-F5 | **Watch a folder** and notify when a new file appears (e.g. a billing export finishes) | Saves polling the export folder by hand | C |
| FR-F6 | **Remote PowerShell console** on private environments, off by default | Escape hatch for anything the GUI used to do; high-risk, so opt-in and journaled | C |
| FR-F7 | **Clean-up helpers**: clear JBoss `tmp`/`data`, prune old rotated logs, with a size preview | Only if these are real routine tasks — confirm before building | C |

### G. Settings

| ID | Requirement | Pri |
|---|---|---|
| FR-G1 | Account, WinRM setup, default download folder, transfer concurrency, confirmation thresholds | M |
| FR-G2 | COSMIC service patterns, billing-export path, default server/client roots | M |
| FR-G3 | Theme (follow system / light / dark), density, date and size formats | S |
| FR-G4 | Export / import settings (no secrets) to share a team baseline | C |

## 1.5 Non-functional requirements

Numeric targets are goals to **validate in Step 1** against a real environment over VPN;
adjust them there rather than treating them as measured facts.

### Performance

| ID | Requirement |
|---|---|
| NFR-P1 | Opening an environment (first session) completes in ≤ 5 s on a healthy VPN |
| NFR-P2 | Listing a folder of ≤ 1,000 entries on a warm session returns in ≤ 500 ms (p95); larger folders show the first page in ≤ 1 s and keep streaming |
| NFR-P3 | Scrolling and selection stay smooth at 50,000 entries (virtualised rendering) |
| NFR-P4 | App cold start to interactive in ≤ 2 s; no remote call blocks the UI thread |
| NFR-P5 | Transfer throughput is measured and published in Step 1; if WinRM cannot sustain a usable rate for typical billing-export folders, the SMB fast path (D6) is built |
| NFR-P6 | Idle footprint is bounded: at most one interactive worker per open environment, transfer workers only while transferring, idle sessions closed after 10 minutes |

### Reliability

| ID | Requirement |
|---|---|
| NFR-R1 | Every remote operation has a timeout and can be cancelled; transfers use stall detection instead of a fixed timeout |
| NFR-R2 | A dropped session (VPN, sleep, host reboot, JBoss restart) is detected and re-established transparently; the in-flight operation fails with a clear, retryable error |
| NFR-R3 | Downloads write to a `.partial` file and rename on success; uploads stage then swap — an interrupted transfer never leaves a truncated file under the real name |
| NFR-R4 | No orphaned `powershell.exe`: children are killed when the app exits or crashes |
| NFR-R5 | Corrupt or missing config files never prevent start-up; writes are atomic |

### Security

| ID | Requirement |
|---|---|
| NFR-S1 | Passwords live only in Windows Credential Manager (DPAPI), never in JSON, logs, the journal, or process arguments |
| NFR-S2 | The environment-class guard runs in the Rust core for every remote operation; the UI only reflects it. Unknown host names are read-only |
| NFR-S3 | User-supplied values (paths, names, patterns) reach PowerShell only as arguments or environment variables — never by string-building script text |
| NFR-S4 | Credentials are only sent to hosts the user saved; `TrustedHosts` is never `*` |
| NFR-S5 | External programs are started by absolute path (`System32`), never by bare name |
| NFR-S6 | The webview runs under a restrictive CSP with no remote origins; Tauri capabilities are least-privilege |
| NFR-S7 | Archive extraction and remote destinations are contained (no `..` escape) |
| NFR-S8 | Every mutating action and transfer is journaled |
| NFR-S9 | No telemetry; nothing leaves the laptop except WinRM traffic to the chosen environment |

### Compatibility and deployment

| ID | Requirement |
|---|---|
| NFR-C1 | Runs on Windows 11 with the built-in WebView2 runtime and Windows PowerShell 5.1; does not require PowerShell 7, .NET SDKs, Java, 7-Zip or any database |
| NFR-C2 | Works from domain-joined and Entra-ID-joined laptops (the existing `NETBIOS\user` → `dns.domain\user` Kerberos fix is retained) |
| NFR-C3 | Targets Windows Server with or without Desktop Experience (Server Core) with default WinRM over HTTP 5985 |
| NFR-C4 | Per-user installer, no administrator rights to install or run; elevation only for the one-time WinRM client setup |
| NFR-C5 | Single installer; upgrades preserve all settings |

### Usability and accessibility

| ID | Requirement |
|---|---|
| NFR-U1 | Explorer behaves like Windows Explorer for the common gestures and shortcuts ([UI/UX §2.9](02-ui-ux.md#29-keyboard-map)) — no training needed |
| NFR-U2 | Every action is reachable by keyboard; focus is always visible; controls have accessible names; contrast meets WCAG AA in both themes |
| NFR-U3 | Errors say what failed, on which host, as which user, and what to try — never a raw stack trace alone |
| NFR-U4 | Disabled actions explain why (e.g. "Read-only: public server") |
| NFR-U5 | Respects system DPI scaling, light/dark theme and reduced-motion |

### Maintainability and observability

| ID | Requirement |
|---|---|
| NFR-M1 | All business logic sits in a Tauri-free `core/`; GUI commands and the CLI are thin adapters |
| NFR-M2 | The transport sits behind a trait with a fake for tests; registry, guard, parsers, path rules and stores are unit-tested and run on CI without a Windows server |
| NFR-M3 | Adding a remote feature follows one recipe (feature module → register → thin command → UI) |
| NFR-M4 | A local rotating log file with secrets redacted; a "copy diagnostics" action for support |
| NFR-M5 | A manual smoke checklist against one real environment gates each release (WinRM behaviour cannot be proven by unit tests) |
