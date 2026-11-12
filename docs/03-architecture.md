# 3. Solution architecture

[← Plan index](../README.md)

## 3.1 Overview

```
 USER'S WINDOWS 11 LAPTOP                                         ENVIRONMENT (headless)
┌───────────────────────────────────────────────────────────┐    ┌──────────────────────┐
│ WinGUI.exe (Tauri 2)                                      │    │ Windows Server       │
│                                                           │    │ (no GUI needed)      │
│  ┌─────────────────────────────────────────────────────┐  │    │                      │
│  │ React UI in WebView2                                │  │    │  WinRM service 5985  │
│  │ Environments · Files · Services · Logs · Transfers  │  │    │        ▲             │
│  └───────────────┬─────────────────────────────────────┘  │    │        │             │
│        invoke()  │  ▲ channels (events, progress)         │    │  wsmprovhost.exe     │
│  ┌───────────────▼──┴──────────────────────────────────┐  │    │  (one per PSSession) │
│  │ Tauri commands (thin)          CLI (thin, Step 6)   │  │    │        │             │
│  └───────────────┬─────────────────────────────────────┘  │    │   file system,       │
│  ┌───────────────▼─────────────────────────────────────┐  │    │   services, JBoss    │
│  │ core::feature::FeatureRegistry                      │  │    └────────▲─────────────┘
│  │ resolve connection → credentials → ENV-CLASS GUARD  │  │             │
│  │ → layout → dispatch → journal                       │  │             │
│  └───────────────┬─────────────────────────────────────┘  │             │
│  ┌───────────────▼─────────────────────────────────────┐  │             │
│  │ core::features::*                                   │  │             │
│  │ fs · transfer · services · processes · overview     │  │             │
│  │ deploy · auto-deploy · restart · logs · status ·    │  │             │
│  │ properties                                          │  │             │
│  └───────────────┬─────────────────────────────────────┘  │             │
│  ┌───────────────▼─────────────────────────────────────┐  │   PowerShell Remoting
│  │ core::remoting  — the only transport owner          │  │   (Negotiate: Kerberos,
│  │ SessionPool ─► worker powershell.exe (persistent) ──┼──┼──► NTLM fallback),
│  │ PsRunner    ─► one-shot / streaming powershell.exe ─┼──┼──► encrypted, over VPN
│  └─────────────────────────────────────────────────────┘  │
│                                                           │
│  JSON stores · SQLite journal · Windows Credential Manager│
└───────────────────────────────────────────────────────────┘
```

The shape is Remote Desktop's (thin front ends → one registry → feature plugins → one
transport owner), with three additions: a **persistent session pool**, a **journal**, and
the new **fs / transfer / services** feature families. The file-explorer UI and the
request/response + event protocol style come from Aeris — without its on-server agent.

## 3.2 Layers and responsibilities

| Layer | Owns | Must not |
|---|---|---|
| React UI (`src/`) | Rendering, view state, user intent | Decide what is allowed; build paths for the backend; hold secrets |
| Tauri commands (`src-tauri/src/commands/`) | Argument translation, adapting channels to core callbacks | Contain business logic, PowerShell, or safety checks |
| `core::feature` | The one invocation path: resolve → guard → layout → dispatch → journal | Be bypassed — nothing calls a feature directly |
| `core::features::*` | One capability each: its script body, argument/result schema, result parsing | Open sessions, read credentials, classify environments |
| `core::remoting` | Sessions, workers, process spawning, timeouts, cancellation, cleanup | Know what a "JAR" or a "service" is |
| `core::{storage, model, credentials, journal}` | Persistence | Reach the network |

Rule carried over from Remote Desktop: **`core/` has zero Tauri imports**, so the GUI, the
CLI and any later front end share it unchanged.

## 3.3 Transport

### 3.3.1 Why a persistent worker

Remote Desktop starts one `powershell.exe` per action; each creates a `PSSession`, does
its work and tears it down. That is right for deploy and restart, but opening a session
costs seconds, and an explorer makes a remote call on almost every click. WinGUI keeps a
session open per environment and reuses it.

### 3.3.2 Session worker

A **worker** is a hidden local `powershell.exe` (absolute `System32` path, `-NoProfile
-NonInteractive`, no window) that:

1. receives its script over **stdin** from a tiny bootstrap, so nothing is written to
   disk and the 32 k command-line limit never applies;
2. reads the host, user and password from **environment variables** set on that child
   only (never arguments), opens one `PSSession` with `-Authentication Negotiate`;
3. loops: read one JSON request line from stdin → run the named operation → write JSON
   lines to stdout.

```
request   →  {"id":"7","op":"fs.list","args":{"path":"D:\\Cambio","page":0}}
events    ←  {"id":"7","kind":"event","data":{…}}          (0..n, e.g. listing pages, progress)
result    ←  {"id":"7","kind":"result","data":{…}}         (exactly one)
error     ←  {"id":"7","kind":"error","code":"AccessDenied","message":"…"}
cancel    →  {"id":"7","op":"$cancel"}
```

Design rules for worker operations:

- **Operations are a fixed table** of named functions inside the worker script. The
  request selects one by name; it can never supply script text. Arguments travel as
  data via `-ArgumentList`. This is what keeps a hostile file name from becoming code.
- **Remote script blocks return pre-serialised strings**, not object collections.
  Returning thousands of objects through PowerShell's object serialisation is slow;
  one compact string per page is not.
- **UTF-8 everywhere**: the worker sets console input/output encoding to UTF-8 so
  Swedish characters in file names survive Windows PowerShell 5.1's default code page.
- **Errors are classified** in the worker into stable codes (`NotFound`, `AccessDenied`,
  `InUse`, `PathTooLong`, `SessionLost`, `Timeout`, `Unknown`) so the UI can respond
  specifically.
- **Long paths and odd names**: every path uses `-LiteralPath` / .NET APIs; no wildcard
  expansion of user paths.

### 3.3.3 Session pool

Per open environment, `SessionPool` manages:

| Worker | Count | Used for | Lifetime |
|---|---|---|---|
| Interactive | 1 | Listing, stat, search, services, processes, small mutations | While the tab is open; closed after 10 min idle, reopened on demand |
| Transfer | 0–2 (configurable) | Downloads/uploads, so a long copy never blocks browsing | Started on demand, closed when the queue drains |
| Streaming | as needed | Log tail, deployment watch (ported one-process-per-stream model) | Until stopped |

Pool responsibilities: lazy start, health check, **reconnect with backoff** when the
session is lost, per-request timeouts, cancellation, request queueing with fairness, a
hard **cap on sessions per host** (default 4) so a shared account does not exhaust the
server's per-user WinRM shell quota, and guaranteed teardown (`Remove-PSSession` in
`finally`, child kill on drop, and a Windows **Job Object** so children die if the app
crashes).

### 3.3.4 One-shot path (kept)

`PsRunner::run` / `spawn_stream` and `wrap_session` are ported as-is for the features
that are already proven with them (deploy, auto-deploy, restart, properties, log tail,
deployment watch). They can migrate onto pooled sessions later, one at a time, behind
their existing tests — not as part of the port.

### 3.3.5 Transfers

Driven by a transfer worker; the bytes never pass through Rust or the webview.

**Download** (remote → local)

1. Stat the source (size, mtime); for folders, enumerate to a manifest.
2. Loop: remote reads a chunk at an offset with a shared read handle and returns it; the
   local worker appends it to `<target>.partial`; a progress event is emitted.
3. Verify size (and SHA-256 when requested, computed on both sides); rename to the final
   name; journal the transfer.
4. **Resume**: if a `.partial` exists and the source size/mtime are unchanged, continue
   from its length.
5. **Many small files**: above a threshold, zip on the remote into its temp folder using
   the .NET zip classes, transfer the single archive, expand locally with entry-path
   validation, delete the remote temp archive in `finally`.

**Upload** (local → remote) reuses the proven deploy sequence: guard that the target is
not a directory → optional timestamped `.bak` → stage to a temp name **beside** the
target → verify → rename into place, tolerating a lock only on the final swap.

**Clipboard copy**: download into the local staging folder, then place the resulting
paths on the Windows clipboard as a file-drop list so Windows Explorer can paste them.

**Strategy seam**: transfers go through a `TransferStrategy` trait. `WinRmChunked` is
the default and always available. Step 1 benchmarks it against `Copy-Item -FromSession`
and against SMB (`\\host\D$`); an `Smb` strategy is added in Step 6 only if the numbers
justify it and port 445 is reachable (Q5). Selection would be automatic with fallback.

## 3.4 Feature families

| Family | Feature ids (indicative) | Capability | Notes |
|---|---|---|---|
| File browse | `fs.drives`, `fs.list`, `fs.stat`, `fs.tree`, `fs.search`, `fs.preview`, `fs.hash` | `FsBrowse` | `fs.list` streams pages; `fs.search` is bounded and cancellable |
| File transfer | `transfer.download`, `transfer.stage-clipboard` | `FsDownload` | Queue lives in the core, not the UI |
| File write | `transfer.upload`, `fs.mkdir`, `fs.rename`, `fs.copy`, `fs.move`, `fs.delete`, `fs.zip`, `fs.unzip`, `fs.write-text` | `FsWrite` | Every one journaled; overwrites back up |
| Services | `services.list`, `services.detail` | `ServiceView` | CIM query, joined with process info |
| | `services.control` (start/stop/restart) | `ServiceControl` | Bounded waits; allow-list + protected list enforced in the core |
| Processes | `processes.list` / `processes.kill` | `ProcessView` / `ProcessKill` | |
| Overview | `overview.snapshot` | `FsBrowse` | Drives, JBoss service, last deployment state |
| Ported | `deploy`, `auto-deploy`, `restart`, `apply-properties`, `log-tail`, `deploy-status` | existing | Scripts byte-identical at port time |
| Local | `rdp.connect`, `winrm.setup`, `import.remote-desktop` | — | Act on the laptop; no guard |

Adding a feature keeps Remote Desktop's recipe: new module in `core/features/` → register
in `build_registry()` → thin command → UI.

## 3.5 Safety model

The existing rule is extended, not replaced. Classification is unchanged: the host is
reduced to its short name, then its suffix decides the class; anything unrecognised is
**Unclassified** and read-only.

| Capability | Mutating | Private server `-SD`/`-EA` | Private client `-CL` | CCP `-CP`, `-CP-n` | Public server `-EA-n`/`-SD-n` | Public client `-CL-n` | Unclassified |
|---|---|---|---|---|---|---|---|
| FsBrowse | | ✓ | ✓ | ✓ | ✓ | ✗ ¹ | ✓ |
| FsDownload | | ✓ | ✓ | ✓ | ✓ ² | ✗ ¹ | ✓ |
| FsWrite | ● | ✓ | ✓ | ✗ | ✗ | ✗ | ✗ |
| ServiceView | | ✓ | ✓ | ✓ | ✓ | ✗ ¹ | ✓ |
| ServiceControl | ● | ✓ | ✓ | ✗ | ✗ | ✗ | ✗ |
| ProcessView | | ✓ | ✓ | ✓ | ✓ | ✗ ¹ | ✓ |
| ProcessKill | ● | ✓ | ✓ | ✗ | ✗ | ✗ | ✗ |
| DeployJar | ● | ✓ | ✓ | ✗ | ✗ | ✗ | ✗ |
| RestartJboss | ● | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ |
| EditProperties | ● | ✓ | ✗ | ✗ | ✗ | ✗ | ✗ |
| Logs / DeployStatus | | ✓ | ✗ | ✓ | ✓ | ✗ | ✓ |

The last four rows are Remote Desktop's current table, unchanged.
¹ Mirrors today's "public client → nothing"; open question Q3.
² Open question Q4.

Further invariants (existing ones kept, new ones marked ★):

| Invariant | Where |
|---|---|
| The guard runs in `FeatureRegistry` for every remote call; the UI renders a backend-computed feature set | `core::feature`, `core::env_class` |
| Credentials go only to saved hosts; `TrustedHosts` is never `*` | `core::winrm` |
| Programs are launched by absolute path | `core::exec` |
| Secrets travel in child-process environment variables, never arguments | `core::remoting` |
| Values in a generated `.rdp` carry no line breaks | `core::model`, `launcher` |
| ★ Worker operations are a fixed table; request data is never evaluated as script | `core::remoting::worker` |
| ★ Remote paths are validated: absolute drive-letter paths only, no device or UNC paths, no control characters; destinations of copy/move/extract stay inside the chosen target | `core::features::fs::path` |
| ★ Service control is limited to the Cambio allow-list (plus explicit opt-in) and never touches protected services such as WinRM | `core::features::services` |
| ★ Every mutating call and every transfer is written to the journal, success or failure | `core::journal` |
| ★ Deletes are refused for drive roots and for the configured server/client root itself | `core::features::fs` |
| Archive entries cannot escape their extraction root | `core::features::fs::archive` |
| Webview CSP: `script-src 'self'`, no remote origins | `tauri.conf.json` |

## 3.6 Frontend architecture

- **`src/api/<domain>.ts`** — typed `invoke` wrappers, one file per domain; the only place
  that knows command names.
- **Stores (Zustand)** — `connections`, `sessions` (per-tab state), `explorer` (path,
  history, selection, sort, per tab), `transfers`, `tasks`, `settings`. Explorer state is
  keyed by environment tab so tabs are independent.
- **Streaming** — one hook (`useChannelStream`, ported) owns the start / message / stop /
  unmount lifecycle for every backend stream: listing pages, transfer progress, log
  lines, deployment status, restart phases.
- **Caching** — directory listings are cached per (environment, path) and revalidated on
  focus/refresh, so back/forward is instant; any mutation invalidates the affected folders.
- **Feature folders** — each feature owns its components, hooks and formatting; shared
  primitives live in `components/`.
- **Error boundary** per section so a rendering fault in one view never blanks the window.

## 3.7 Folder structure (high level)

```
WinGUI/
├── README.md                       plan index (becomes the product README)
├── docs/                           this plan; later ARCHITECTURE and user guide
├── package.json  vite.config.ts  tsconfig*.json  index.html
│
├── src/                            React UI
│   ├── main.tsx  App.tsx           shell: nav rail, environment tabs, task bar
│   ├── api/                        typed invoke wrappers per domain
│   ├── stores/                     Zustand stores
│   ├── hooks/                      useChannelStream, useSession, useKeyboardNav
│   ├── features/
│   │   ├── environments/           home: tree, tiles, connection/label/category dialogs
│   │   ├── explorer/               toolbar, address bar, nav pane, file list,
│   │   │                           context menu, preview, properties, dialogs
│   │   ├── transfers/              queue panel, conflict dialog
│   │   ├── services/               table, details drawer
│   │   ├── processes/
│   │   ├── env-tools/              deploy, auto-deploy, restart, status,
│   │   │                           properties, modules  (ported dialogs)
│   │   ├── logs/                   log viewer (ix3.log and any file)
│   │   ├── overview/               environment health
│   │   ├── activity/               journal view
│   │   └── settings/               account, WinRM, paths, patterns, import
│   ├── components/                 shared primitives (table, dialog, toast, badge)
│   ├── format/                     structured result → text, sizes, dates
│   └── styles/                     theme tokens
│
├── src-tauri/
│   ├── Cargo.toml  tauri.conf.json  build.rs  capabilities/  icons/
│   └── src/
│       ├── main.rs  lib.rs         GUI start-up, state, command registration
│       ├── commands/               thin Tauri commands, one module per domain
│       ├── tasks.rs                background task manager (ported, generalised)
│       ├── launcher.rs  signing.rs RDP launch and .rdp signing (ported)
│       ├── cli.rs                  headless front end (Step 6)
│       └── core/                   ── NO Tauri imports ──
│           ├── storage.rs          config dir, JsonStore (atomic writes, versioned)
│           ├── model.rs            Connection, Label, Category, Module, PropertySet, Settings
│           ├── credentials.rs      Windows Credential Manager
│           ├── exec.rs             absolute-path process launching, Job Object
│           ├── account.rs  winrm.rs
│           ├── env_class.rs        classification + capability table
│           ├── env_layout.rs       per-environment paths, quick-access templates
│           ├── feature.rs          traits, FeatureRegistry, guard
│           ├── journal.rs          SQLite activity journal + transfer history
│           ├── import.rs           one-time import from Remote Desktop
│           ├── remoting/
│           │   ├── mod.rs          resolve_target, wrap_session, PsRunner (ported)
│           │   ├── worker.rs       worker process + line protocol
│           │   ├── pool.rs         per-environment pool, reconnect, limits
│           │   └── scripts/        worker and operation scripts, embedded at build time
│           └── features/
│               ├── mod.rs          build_registry()
│               ├── fs/             list, stat, search, preview, mutate, archive, path
│               ├── transfer/       queue, download, upload, verify, strategy
│               ├── services.rs  processes.rs  overview.rs
│               └── deploy.rs  auto_deploy.rs  restart.rs  properties.rs
│                   logs.rs  deploy_status.rs            (ported)
│
├── scripts/                        Setup-RdpSigning.ps1, smoke-test helpers
└── .github/workflows/              CI: cargo test, type-check, build installer
```

## 3.8 Data storage and management

No database server. Three stores, each chosen for what it holds.

### 3.8.1 What lives where

| Data | Store | Location |
|---|---|---|
| Environments, labels, categories | JSON | `%APPDATA%\com.curiouserlabs.wingui\connections.json`, `labels.json`, `categories.json` |
| Modules, property sets, auto-deploy roots | JSON | `modules.json`, `property_sets.json`, `auto_deploy.json` |
| Settings (download folder, concurrency, service patterns, billing-export path, thresholds, theme) | JSON | `settings.json` |
| Quick-access pins (global and per environment) | JSON | `quick_access.json` |
| UI state (window size, column widths, last folder per environment) | JSON | `ui_state.json` — disposable |
| RDP signing thumbprint | JSON | `signing.json` |
| **Account password**, per-host RDP credentials | Windows Credential Manager (DPAPI) | `WinGUI/global` (generic, local-machine persistence); `TERMSRV/<host>` |
| **Activity journal**, transfer history | SQLite, compiled into the exe | `journal.db` in the same config folder |
| Staged files for clipboard / "open locally" | Files | `%LOCALAPPDATA%\com.curiouserlabs.wingui\staging\` |
| Application log | Rotating text | `%LOCALAPPDATA%\com.curiouserlabs.wingui\logs\` |
| In-flight downloads | Files | `<target>.partial` beside the destination |

**Why JSON for configuration:** small, human-readable, diff-able, trivially backed up,
and identical to what Remote Desktop users already have — which makes the import a file
copy plus validation.

**Why one SQLite file for the journal:** it is append-only, grows without bound, and is
queried by host, date, action and outcome. SQLite is compiled into the executable
(`rusqlite` with the `bundled` feature, as Remote Desktop already does for its clone
index), so there is nothing to install. It is best-effort: if the file is missing or
corrupt the app recreates it and carries on.

**Why Credential Manager for secrets:** OS-protected, per-user, and already the
established pattern. Local-machine persistence is kept so the shared account's password
does not roam with the domain profile.

### 3.8.2 Journal schema (indicative)

| Table | Columns |
|---|---|
| `operations` | id, started_utc, duration_ms, host, env_class, acting_user, feature_id, capability, target (path / service / module), summary, outcome (ok / failed / cancelled), error_code, error_text, backup_path |
| `transfers` | id, started_utc, finished_utc, host, direction, remote_path, local_path, bytes, sha256, strategy, outcome, error_text |

Never stored: passwords, file contents, property values.

### 3.8.3 Management rules

| Concern | Rule |
|---|---|
| Source of truth | Disk. Stores are read on demand — no in-memory mirror to drift — so a CLI process sees what the GUI wrote |
| Atomic writes | Write to a temp file in the same folder, then rename over the target |
| Versioning | Each JSON file carries a `schemaVersion`; loaders migrate forward and keep a `.bak` of the pre-migration file |
| Corruption | An unreadable store loads as empty/default and the bad file is preserved as `*.corrupt-<timestamp>` rather than overwritten |
| Concurrency | One GUI instance (single-instance guard); the CLI takes a short file lock when writing |
| Retention | Journal pruned to 90 days / 50,000 rows at start-up (configurable); staging swept at start-up and exit, and anything older than 24 h removed; logs capped by size |
| Import | First run offers to copy from `%APPDATA%\com.curiouserlabs.remote-desktop\`; files are validated through the same normalisers as manual entry; the account is copied only on explicit consent. Remote Desktop's data is never modified |
| Backup / move to a new laptop | Copy the config folder; re-enter the password |
| Uninstall | Removes the program; config and credentials are left unless the user ticks "remove my data" |

### 3.8.4 What is written on the environments

WinGUI keeps no state on servers. The only artefacts are the ones Remote Desktop already
creates, plus short-lived temp files:

- timestamped `*.bak` backups beside replaced files (intentional; managed by FR-F3);
- transient `*.rd-upload-<guid>` staging files, removed on success or failure;
- temporary archives under the remote user's temp folder for folder transfers, removed
  in `finally` and swept by age on the next connection.

## 3.9 Testing strategy

| Level | Covers | Runs |
|---|---|---|
| Unit (Rust) | Capability truth table, guard, path validation, service allow-list, layout derivation, worker protocol framing, result parsers, stores, journal, migration, import | CI, no Windows server needed |
| Orchestration (Rust) | Features against a fake `PsRunner` / fake worker: correct operation, arguments, env injection, error mapping, cleanup | CI |
| Component (TS) | Explorer selection/keyboard model, sort, virtualisation boundaries, transfer state machine | CI |
| Worker integration | The real worker script against **localhost** WinRM on a Windows runner: listing, Unicode names, long paths, chunked transfer round-trip, cancel | CI on Windows where WinRM can be enabled; otherwise a local script |
| Manual smoke | Real environment over VPN, including a headless one and a public one | Release gate |
