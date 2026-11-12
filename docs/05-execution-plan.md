# 5. Execution plan

[← Plan index](../README.md)

Six steps. Each ends in something a user can run, and each has exit criteria checked
against a **real environment over VPN** — unit tests alone cannot prove WinRM behaviour.

```
 1 Foundation      2 Explorer        3 Transfers &      4 Services,       5 Environment     6 Hardening,
   & transport  ─►   (read-only)  ─►   file operations ─► processes &   ─►  tools menu    ─►  packaging
   spike                                                   overview          (RD parity)       & rollout
                                                          └──── 4 and 5 are independent; either order or in parallel ────┘
```

| Step | Relative size | First point at which… |
|---|---|---|
| 1 | M | …the riskiest assumption (fast, reliable held sessions on a headless server) is proven or disproven |
| 2 | L | …users can look around a server without RDP |
| 3 | L | …users can copy files out and replace files — the core of the RDP replacement |
| 4 | M | …users can manage services without `services.msc` |
| 5 | M | …WinGUI covers everything Remote Desktop does for an environment |
| 6 | M | …it is safe to hand to everyone and switch servers to headless |

---

## Step 1 — Foundation and transport spike

> **Progress (2026-11-13):** build items 1–6 are done and tested locally. Item 7 (the
> spike) has its script and a report template but **no real measurements yet**, so the
> exit criteria that need an environment, and the decision gate, are still open — see
> [Decision 001](decisions/001-transport.md).

**Goal:** a running app that can open an environment and list its drives through a
persistent session, with measured numbers behind the transport decisions.

**Build**

1. Scaffold Tauri 2 + React 19 + TypeScript + Vite; identifier, CSP, least-privilege
   capabilities, single-instance guard; CI running `cargo test` and type-check.
2. Port from Remote Desktop, with their tests: `storage`, `model`, `credentials`, `exec`,
   `account`, `winrm`, `env_class`, `env_layout`, `remoting` (one-shot path), `feature`.
   Add atomic writes and `schemaVersion` to `JsonStore`.
3. Extend `env_class` with the new capabilities and the full truth-table test
   ([Architecture §3.5](03-architecture.md#35-safety-model)).
4. Home view: environments tree and tiles, add/edit/delete, labels, categories, pinning;
   Settings: account and WinRM status/setup.
5. Import from Remote Desktop (files validated through the normalisers; account on
   consent).
6. Session worker + pool: bootstrap over stdin, line protocol, timeouts, cancel,
   reconnect, idle close, per-host cap, Job Object; operations `ping` and `fs.drives`.
7. **Spike and record results** in `docs/decisions/`:
   - session open time; warm request latency;
   - listing 1 k / 10 k / 50 k entries;
   - download throughput for a large file and for 5,000 small files using chunked
     transfer, `Copy-Item -FromSession`, and SMB if reachable;
   - all of the above against a **headless** image;
   - actual WinRM quota values on a provisioned environment.

**Exit criteria**

- Open a private and a public environment; drives and free space appear; the class badge
  and allowed-feature set come from the backend.
- Kill the VPN mid-session: the app reports it and reconnects when the VPN returns,
  without retrying on authentication failures.
- Closing the app (and force-killing it) leaves no `powershell.exe` behind and no open
  remote session.
- The spike report exists and either confirms NFR-P1/P2/P5 or proposes revised targets
  and the chosen transfer strategy.

**Decision gate:** if held sessions are unreliable or listing is far outside target on
the real network, stop and reassess the transport (SMB-first for file access, or an
agent) before building the explorer on it.

---

## Step 2 — File Explorer (read-only)

**Goal:** browse any accessible drive and folder like Windows Explorer.

**Build**

1. Core features under `FsBrowse`: `fs.list` (paged streaming, access-denied tolerant),
   `fs.stat`, `fs.tree`, `fs.search` (bounded, cancellable), `fs.preview`, `fs.hash`;
   remote path validation.
2. Explorer UI (adapting Aeris's file-explorer components): navigation pane with quick
   access, This PC and lazy tree; breadcrumb/editable address bar; back/forward/up;
   virtualised details list with sort; multi-select; context menu; status bar;
   properties dialog; preview pane.
3. Quick access: COSMIC shortcuts from `EnvLayout`, billing-export path from Settings,
   user pins (global and per environment).
4. Environment tabs with independent explorer state; listing cache with revalidation;
   last folder remembered.
5. Full keyboard map; all states in [UI/UX §2.10](02-ui-ux.md#210-states-every-view-must-handle).

**Exit criteria**

- Navigate every drive on a private server, a public server and a client machine.
- A 10,000-entry folder is usable within a second and scrolls smoothly.
- Names with å/ä/ö, spaces, brackets and very long paths list and open correctly.
- An inaccessible folder shows a clear message and navigation still works.
- A public **client** machine is refused by the backend (per the current rule), from the
  UI and from a direct command call.

---

## Step 3 — Transfers and file operations

**Goal:** copy files and folders to the laptop the way users do over RDP today, and
change files on environments that allow it.

**Build**

1. Transfer queue in the core: download of files and folders, progress events, pause,
   cancel, retry, resume from `.partial`, size and SHA-256 verification, conflict
   handling, remote-zip fast path for many small files.
2. Copy → paste in Windows Explorer: stage locally, place a file-drop list on the
   clipboard; staging sweep.
3. "Open locally"; "Compare with local file".
4. Under `FsWrite`: upload (picker and drag-and-drop) with stage-then-swap and optional
   `.bak`; new folder, rename, copy, move, delete with tiered confirmation; zip / unzip
   with entry containment; root-delete refusals.
5. Text editor for small files with diff-before-save and backup (FR-C12).
6. Journal (`journal.db`), Activity view with filters and CSV export; every mutation and
   transfer recorded.
7. Transfer panel merged with the task bar.

**Exit criteria**

- Copy a billing-export folder to the laptop by **Ctrl+C in WinGUI → Ctrl+V in Explorer**
  and by *Download…*; contents match by hash.
- Pull the network cable mid-download of a large file; after reconnect the transfer
  resumes and verifies.
- Replace a JAR that JBoss has open on a private server; a `.bak` exists; the journal
  shows the action with the backup path.
- Upload, rename and delete are refused on a public server from the UI **and** from a
  direct command call, with the same error.
- No truncated file is ever left under its real name after a cancelled or failed transfer.

---

## Step 4 — Services, processes and overview

**Goal:** see and manage what is running without `services.msc` or Task Manager.

**Build**

1. `services.list` / `services.detail` (CIM, joined with process data);
   `services.control` with bounded waits, the Cambio allow-list, the protected list and
   live status events.
2. Services view: filter chips, search, auto-refresh, row actions, details drawer.
3. `processes.list` / `processes.kill`; Processes view.
4. `overview.snapshot` and the Overview section: reachability, JBoss state and uptime,
   drives with low-space warnings, last deployment result.
5. Settings: service patterns, opt-in for non-Cambio service control.

**Exit criteria**

- The default filter shows exactly the COSMIC/Cambio services on a real environment
  (patterns agreed with the team — Q2).
- Stop, start and restart the JBoss service on a private server with correct live
  status; a hung stop reports a timeout instead of waiting forever.
- Attempting to stop WinRM is refused by the backend.
- Service control and process kill are refused on public and CCP environments.

---

## Step 5 — Environment tools menu (Remote Desktop parity)

**Goal:** everything Remote Desktop does for an environment, in one menu.

**Build**

1. Port features with their tests, scripts unchanged: `deploy` (+ modules registry),
   `auto-deploy` (without the build step), `restart`, `deploy-status`, `log-tail`,
   `apply-properties` (+ property-set registry).
2. Port and restyle the dialogs; add the **Environment tools** menu to the environment
   header and home tiles; restart → automatic deployment watch → header progress bar.
3. Logs section: ported viewer generalised to tail any file from the explorer; rotated
   archive picker; pop-out window.
4. RDP launch: `launcher`, `.rdp` signing provisioning, `TERMSRV/<host>` credentials.
5. Explorer integration: **Replace with local JAR…** on JARs; "Tail in log viewer" on
   text files; restart offered from the Services view.
6. Backup manager (FR-F3).

**Exit criteria**

- Remote Desktop's manual smoke checklist items 1–9 pass in WinGUI (connect via RDP,
  deploy JAR, auto-detect deploy for Spider and IX3, restart, tail logs, deployment
  status, apply property set, WinRM setup, disallowed operation on a public host).
- A user who imported their Remote Desktop data can do a full "replace JARs → restart →
  watch deployment → tail log" cycle without touching Remote Desktop or RDP.

---

## Step 6 — Hardening, packaging and rollout

**Goal:** safe to give to everyone, and safe to switch servers to headless.

**Build**

1. CLI over the same registry (`list`, `ls`, `get`, `put`, `services`, `deploy`,
   `restart`, `logs`, `status`, `doctor`, `--json`, `--yes` gate) and, if wanted, the
   agent skills that call it.
2. SMB transfer strategy — **only if** the Step 1 numbers call for it and 445 is open.
3. Security review of the whole surface: path validation, worker operation table, CSP
   and capabilities, credential handling, journal coverage; fix findings.
4. Performance pass against the NFR targets; memory and process-count audit with
   several environments open.
5. Accessibility pass (keyboard-only walkthrough, contrast, screen-reader names);
   DPI and theme checks.
6. Installer (per-user NSIS, MSI if required), code signing, upgrade test preserving
   data; update mechanism decision.
7. Documentation: product README, `ARCHITECTURE.md`, user guide, troubleshooting
   (VPN, WinRM, authentication, quotas), the headless pre-rollout checklist
   ([Runtime §4.5](04-runtime-environment.md#45-pre-rollout-checklist-for-a-headless-environment)).
8. Pilot with a small group on a headless environment; collect feedback; then phased
   rollout and the switch of server images.
9. Remaining "should"/"could" items by pilot demand (event log viewer, folder watch,
   JAR peek, drag-out).

**Exit criteria**

- Pilot users go two weeks on a headless environment without needing RDP to the server.
- No open high-severity security findings.
- Fresh install and upgrade both verified on a standard-issue laptop without admin
  rights (other than the WinRM setup prompt).
- A documented recovery route exists for an environment whose WinRM is down.

---

## Cross-cutting practices (every step)

- Nothing reaches the transport except through `FeatureRegistry`; every new capability
  gets a row in the truth-table test before its feature is written.
- Each feature lands with unit tests against the fake transport and a line in the manual
  smoke checklist.
- Mutations are journaled from the day they exist.
- UI text for errors is reviewed against "what failed, where, as whom, what to try".
- Decisions that change this plan are recorded in `docs/decisions/` with the evidence.

## Risks

| # | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| R1 | WinRM transfer speed is too low for large export folders | Medium | High | Measure in Step 1; chunk tuning; remote zip; SMB strategy; set expectations in the UI with ETA |
| R2 | Shared-account shell quotas are hit when many people use one environment | Medium | High | Per-host session cap, idle close, strict cleanup; measure real quotas; ask IT to raise limits if needed (Q6) |
| R3 | Headless image or provisioning does not keep WinRM reachable | Low | High | Verify in Step 1 on the actual target image; pre-rollout checklist |
| R4 | Held sessions are flaky over VPN (sleep, roaming) | Medium | Medium | Reconnect design from Step 1; never retry on auth failure; cached views stay readable |
| R5 | EDR or policy blocks the hidden PowerShell worker | Low | High | Early conversation with IT security; signed binary; plain readable scripts; Doctor check |
| R6 | A user damages an environment (wrong delete, wrong service) | Medium | Medium | Fail-closed guard, confirmations, protected services, root-delete refusal, backups, journal |
| R7 | No fallback when WinRM breaks on a server with no desktop | Low | High | Protected-service rule; documented console recovery; keep RDP launch for hosts that still have a GUI |
| R8 | Ported code drifts from Remote Desktop | Medium | Low | Port with tests; note the source commit; decide on a shared crate after Step 5 (Q9) |
| R9 | Scope creep from the "could" list | High | Medium | Steps 1–5 are the committed scope; extras only in Step 6, driven by pilot feedback |
| R10 | Sensitive data copied from public environments | Low | Medium | Decide Q4 before Step 3; downloads are journaled either way |

## Inputs needed before Step 1 starts

Answers to Q1–Q7 in the [plan index](../README.md#open-questions) — in particular one
private `-SD` environment, one public environment and one **headless** environment to
test against, and the billing-export path and service names.
