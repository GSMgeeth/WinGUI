# WinGUI

> **Status:** Step 1 of 6 (foundation and transport) is built; its network measurements
> are still to be taken on real environments — see
> [Decision 001](docs/decisions/001-transport.md). Plan written 2026-11-13.
> **Working name:** WinGUI. App identifier: `com.curiouserlabs.wingui`.

## Running it

Prerequisites: Windows 11, Rust (stable), Node.js 22.12 or newer, Visual Studio Build
Tools with the *Desktop development with C++* workload.

```sh
npm install
npm run tauri dev        # the desktop app, with hot reload
```

| Command | What it does |
|---|---|
| `npm run tauri dev` | Runs the desktop app |
| `npm run dev` | UI only, in a browser at `http://localhost:1420`, served by an in-memory mock backend (`src/api/mock.ts`) — for working on screens without an environment |
| `npm run typecheck` | Type-checks the frontend |
| `cargo test --lib` (in `src-tauri/`) | Backend tests, including ones that start a real PowerShell worker locally |
| `npm run tauri build` | Builds the per-user installer |
| `.\scripts\Spike-Transport.ps1 -ComputerName <host>` | Measures the transport against a real private environment (on the VPN) |

Set `WINGUI_CONFIG_DIR` to a scratch folder to run a dev build without touching your real
saved environments in `%APPDATA%\com.curiouserlabs.wingui`.

**What works today:** saving and grouping environments, the account and WinRM setup,
import from Remote Desktop, opening an environment over a held WinRM session, and
listing its drives. Files, transfers, services and the Environment tools menu are
Steps 2–5.

---

# The plan

WinGUI is a Windows desktop app (Rust + Tauri 2 + React) that gives Cambio developers
and testers everything they currently open a Remote Desktop session to a **server-side**
environment for — browsing drives, copying files back to their laptop, swapping JBoss
JARs, restarting JBoss, reading logs, managing COSMIC/Cambio services — **without the
server needing a GUI**. That lets the AD environment's server VMs move to a headless
(Server Core–style) image, which is where the cost saving comes from.

Client-side machines keep their GUI (COSMIC is a desktop app), so WinGUI still offers a
one-click RDP launch for those.

## Documents

| # | Document | Contents |
|---|---|---|
| 1 | [Requirements](docs/01-requirements.md) | Scope, users, functional requirements (incl. Remote Desktop parity matrix and proposed extras), non-functional requirements |
| 2 | [UI / UX](docs/02-ui-ux.md) | Layout, the Explorer view, the Environment tools menu, transfers, services, safety UX, keyboard map, states |
| 3 | [Architecture](docs/03-architecture.md) | Solution architecture, transport design, safety model, folder structure, data storage and management |
| 4 | [Runtime environment](docs/04-runtime-environment.md) | Windows 11 client concerns, headless server concerns, network/auth, packaging |
| 5 | [Execution plan](docs/05-execution-plan.md) | Six steps with scope, deliverables and exit criteria; risks |

## The plan in one page

**What it is.** One window. Pick an environment → it opens as a tab with
**Files · Services · Processes · Logs · Overview**, plus an **Environment tools** menu
holding every Remote Desktop feature except workspace management (deploy JAR,
auto-detect & deploy, restart JBoss, deployment status, property sets, log tail, RDP
launch).

**How it reaches servers.** Agentless, over **WinRM / PowerShell Remoting** with the
same account and auth path the Remote Desktop app uses today. Nothing is installed on
the environments. The new piece is a **persistent session worker** — one hidden
`powershell.exe` per open environment that keeps a `PSSession` alive and answers
requests over stdin/stdout — because a file explorer cannot afford to open a new WinRM
session for every click.

**What it reuses.**

| From | Reused |
|---|---|
| **Remote Desktop** (`CuriouserLabs/Remote-Desktop`) | The Tauri-free `core/` design, the feature registry, the environment-class safety guard, `EnvLayout`, `JsonStore`, Credential Manager storage, WinRM setup, and the deploy / auto-deploy / restart / logs / deploy-status / properties features — ported as code, not re-invented |
| **Aeris** (`CuriouserLabs/Aeris`) | The file-explorer UI decomposition (toolbar, breadcrumbs, list view, context menu, status bar, transfer progress), Zustand stores, the request/response + streaming protocol shape, chunked transfers, the operation journal idea. **Not** its on-server agent |

**Where data lives.** JSON files in `%APPDATA%\com.curiouserlabs.wingui\` (same method
as Remote Desktop), secrets in Windows Credential Manager, and one bundled SQLite file
for the activity journal and transfer history. No database server, nothing to install.

**Safety.** The Remote Desktop rule — *only private `-SD` / `-CL` environments may be
changed; everything else is read-only; unknown names fail closed* — is extended to the
new capabilities (file writes, service control, process kill) and enforced in the Rust
core, not in the UI.

## Key decisions

| # | Decision | Why | Alternative rejected |
|---|---|---|---|
| D1 | Agentless WinRM transport | Environments are ephemeral and Jenkins-provisioned; nothing to install, patch or secure on them; auth path already proven by Remote Desktop | Aeris-style agent on each server (extra port, token/TLS management, deployment into every environment) |
| D2 | Persistent session worker + small pool per environment | Explorer needs sub-second listing; opening a session per operation takes seconds | One `powershell.exe` per operation (fine for deploy/restart, too slow for browsing) |
| D3 | Port Remote Desktop's `core/` by copying | Fast, keeps the two apps independently releasable; that core was built to be lifted | Shared crate / git dependency now (couples release cycles before the shape is stable) — revisit after Step 5 |
| D4 | JSON + Credential Manager + one bundled SQLite | Matches Remote Desktop; SQLite only where data is append-only and queried | A database server (explicitly ruled out); JSON for the journal (unbounded growth, no querying) |
| D5 | Safety guard in the core, fail closed | Same guarantee for GUI, CLI and any future front end | UI-only button hiding |
| D6 | Chunked, resumable transfers driven by the worker; optional SMB fast path later | Progress, cancel, resume and hash verification; works wherever WinRM works | `Copy-Item -FromSession` only (no progress/cancel/resume) — kept as the fallback to benchmark against in Step 1 |
| D7 | Single-window tabbed UI | Simple, familiar, keyboard-friendly | Aeris's desktop-in-a-window metaphor (windows, taskbar, start menu) — more to build, no benefit here |

## Assumptions made (please confirm)

1. **"Aries" = `CuriouserLabs/Aeris`.** That is the only close match in the org. It was
   shallow-cloned to a temp folder for reading and is **not** in this workspace.
2. **"Browser-based" = a web UI (React) inside Tauri's WebView2 shell**, exactly like the
   Remote Desktop app — not a site opened in Chrome/Edge. A plain browser tab cannot read
   Credential Manager or start WinRM sessions. The core stays Tauri-free, so a
   localhost-served mode remains possible later if wanted.
3. **Headless servers still have WinRM enabled** and reachable on 5985 over the VPN, as
   today's Remote Desktop deploy/restart/logs features require.
4. **Client-side machines (`-CL`) keep their GUI**; only server-side VMs go headless.
5. **WinGUI is a separate app** from Remote Desktop with its own config folder, and offers
   a one-time import of the existing connections, labels, categories, modules, property
   sets and account.

## Open questions

| # | Question | Affects |
|---|---|---|
| Q1 | What is the billing-export folder path (same on every environment, or per environment)? | Quick-access shortcut defaults |
| Q2 | Which Windows service names/display names count as "COSMIC / Cambio" (e.g. `Production_Cosmic`)? | Default service filter and the allow-list for start/stop |
| Q3 | Should public **client** machines (`-CL-<n>`) stay fully off-limits (today's rule) or allow read-only browse/download? | Capability table |
| Q4 | Is downloading from **public** servers acceptable given billing exports may hold sensitive test data? | Capability table, journal |
| Q5 | Is SMB (port 445, `\\host\D$`) reachable over the VPN? | Whether the optional fast transfer path is worth building |
| Q6 | The Remote Desktop account is described as shared. How many people use the same account against one environment at once? | WinRM per-user shell quotas, session pool caps |
| Q7 | What is the target headless image (Windows Server version, Server Core?), and does Jenkins provisioning keep `Enable-PSRemoting`? | Compatibility testing in Step 1 |
| Q8 | Is a code-signing certificate available for the installer? | SmartScreen warnings, auto-update |
| Q9 | Long term: keep two apps, or fold WinGUI's views into Remote Desktop? | Whether D3 becomes a shared crate |

## References

- Remote Desktop app: `..\Windows Remote Desktop\` — see its `README.md`, `ARCHITECTURE.md`
  and `external-references\MODULE_DEPLOYMENT_FINDINGS.md`.
- Aeris: `https://github.com/CuriouserLabs/Aeris` — `apps/client/src/components/apps/file-explorer/`,
  `apps/agent/src/filesystem.rs`, `docs/PROTOCOL.md`.
