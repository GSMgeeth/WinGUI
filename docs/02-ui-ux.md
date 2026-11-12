# 2. UI / UX requirements

[← Plan index](../README.md)

## 2.1 Principles

1. **It should feel like Windows Explorer.** Users are replacing a habit, not learning a
   tool. Same layout, same gestures, same shortcuts.
2. **Always show where you are and what you may do.** Host name, environment class and
   read-only state are visible at all times; a public server never looks like a dev box.
3. **Nothing blocks.** Every remote call is asynchronous with visible progress and a
   cancel. Long jobs go to the task/transfer panel and keep running when you switch tabs.
4. **Safe by default.** Destructive actions confirm, overwrites back up, and anything not
   allowed on this environment is disabled with the reason — the backend decides, the UI
   reflects.
5. **One window.** Tabs for environments, a menu for tools, a panel for background work.

## 2.2 Window layout

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ WinGUI                                                            –  □  ✕     │
├────┬─────────────────────────────────────────────────────────────────────────┤
│    │ ⌂ Environments │ L-abc123-SD ● ✕ │ S-15172-EA-7 🔒 ✕ │  +               │  environment tabs
│ ⌂  ├─────────────────────────────────────────────────────────────────────────┤
│ ⇅  │ L-abc123-SD   [Private server]              Environment tools ▾   ⟳  ⋯   │  environment header
│ ☰  │ Files │ Services │ Processes │ Logs │ Overview                            │  section tabs
│ ⚙  ├─────────────────────────────────────────────────────────────────────────┤
│    │                                                                          │
│    │                       (section content)                                  │
│    │                                                                          │
├────┴─────────────────────────────────────────────────────────────────────────┤
│ ● Connected as cambio.se\user · 2 transfers · 1 task running        ▴ Tasks  │  status / task bar
└──────────────────────────────────────────────────────────────────────────────┘
```

| Region | Content |
|---|---|
| Nav rail (left) | Environments (home), Transfers, Activity (journal), Settings — Settings shows a warning dot when the account or WinRM needs attention |
| Environment tabs | One tab per open environment with a state dot (connected / connecting / disconnected) and a lock icon when read-only |
| Environment header | Host name, class badge, **Environment tools** menu, reconnect, overflow (edit environment, copy host name, open RDP session) |
| Section tabs | Files · Services · Processes · Logs · Overview |
| Task bar | Connection identity, running transfers/tasks summary; expands into the task panel |

## 2.3 Home — Environments

Ported from Remote Desktop's main view.

- Left: category → label tree with counts; "Pinned" and "All" at the top.
- Main: tiles (or a compact list toggle) showing label, computer name, class badge and a
  reachability dot. Search box filters as you type.
- Tile primary action: **Open** (opens/activates the environment tab on Files).
- Tile menu: Environment tools (same menu as the header), Open RDP session, Pin, Edit,
  Delete.
- First run: an empty state with two actions — **Import from Remote Desktop** and
  **Add environment** — followed by a short setup checklist (account, WinRM).

## 2.4 Files — the Explorer view

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ ←  →  ↑  ⟳ │ This PC › D: › Cambio › Production › instance      │ 🔍 Search   │
│ ⬇ Download  ⬆ Upload  ＋ New folder  ✎ Rename  🗑 Delete  ⋯        ☰ View ▾   │
├───────────────────────┬──────────────────────────────────────┬───────────────┤
│ ★ Quick access        │ Name              Modified     Type   Size │ Preview  │
│   Server root         │ 📁 Cosmic        12 Nov 09:14  Folder      │          │
│   JBoss deployments   │ 📁 standalone    12 Nov 09:14  Folder      │ (text    │
│   Modules             │ 📄 run.conf      03 Oct 16:02  CONF   2 KB │  preview │
│   Logs                │ 📄 wrapper.log   13 Nov 08:51  LOG   14 MB │  or      │
│   environment.props   │                                            │  props)  │
│   Billing exports     │                                            │          │
│ 💻 This PC            │                                            │          │
│   C:  ▓▓▓▓▓░░ 41 GB   │                                            │          │
│   D:  ▓▓░░░░░ 180 GB  │                                            │          │
│ ▸ D:\Cambio           │                                            │          │
├───────────────────────┴──────────────────────────────────────┴───────────────┤
│ 4 items · 1 selected (14 MB)                              D: 180 GB free      │
└──────────────────────────────────────────────────────────────────────────────┘
```

**Navigation pane**

- *Quick access*: built-in COSMIC shortcuts resolved from the environment's layout, then
  user pins. A shortcut whose folder does not exist on this host is shown greyed with a
  tooltip, not hidden.
- *This PC*: every accessible drive with a usage bar and free space; turns amber/red
  when space is low.
- *Tree*: lazily loaded folders; expanding shows a spinner on that node only.

**Address bar**

- Breadcrumbs by default; click empty space or press **Ctrl+L** to edit as text; paste a
  full path and press Enter. Invalid or inaccessible paths show an inline error and keep
  the current folder.
- Back/forward keep per-tab history; the last folder per environment is remembered.

**File list**

- Details view by default (Name, Date modified, Type, Size); optional tiles view.
  Column widths, sort and view mode persist.
- Folders first, then files, sorted by the active column. Sizes are human-readable with
  the exact byte count in the tooltip. Dates are shown in the laptop's local time with
  the remote time zone in Properties.
- Hidden/system items are dimmed when shown. Inaccessible folders show a lock overlay.
- Large folders stream in: the list is usable immediately and a subtle "Loading… 12,000
  items" indicator sits in the status bar.
- File-type icons by extension; JAR/WAR/EAR, logs, properties and XML get distinct icons
  since those are what users look for.

**Context menu** (also the toolbar and keyboard)

| Target | Items |
|---|---|
| File | Open locally · Preview · Download… · Copy · Copy path · Tail in log viewer · Compare with local file… · Edit · Rename · Delete · Properties |
| Folder | Open · Open in new tab · Download… · Copy · Copy path · Pin to Quick access · Compress to ZIP · Rename · Delete · Properties |
| Empty space | Paste · Upload files… · Upload folder… · New folder · Refresh · Properties |
| JAR in a module folder | adds **Replace with local JAR…** (the deploy flow with backup) |

Mutating items are disabled, not removed, on read-only environments, with the reason in
the tooltip.

**Preview pane** (toggle, off by default): text preview of the first N KB with line
count and encoding; for anything else, the properties summary.

## 2.5 Copying to the local machine

Three routes to the same result; all feed the transfer queue.

| Route | Flow |
|---|---|
| **Copy → paste in Explorer** | Select → **Ctrl+C** (or *Copy*). A toast shows staging progress, then "Ready to paste — 14 files". **Ctrl+V** in any Windows Explorer window pastes them. This mirrors the built-in Remote Desktop habit |
| **Download…** | Select → *Download…* → native folder picker (defaults to the last used folder) → queue |
| **Open locally** | Double-click a file → downloads to the temp area → opens with the default app (read-only copy; clearly labelled) |

Upload is the reverse: drop files from Windows Explorer onto the file list (a drop
overlay names the target folder), or use *Upload…*. Dropping onto a folder row uploads
into that folder.

**Transfer panel**

- One row per transfer: direction, name, source → destination, progress bar, speed, ETA,
  pause / cancel / retry; folder transfers expand to show per-file state.
- Header: overall progress and "Clear finished". Completed downloads offer **Show in
  folder**.
- Conflicts open a single dialog with *Skip / Overwrite / Keep both* and "Do this for all".
- Failures stay in the list with the reason and a Retry; nothing fails silently.

## 2.6 Environment tools menu

The "separate menu" holding the Remote Desktop features. Available from the environment
header and from each home tile.

```
Environment tools ▾
  Deploy JAR…                     Ctrl+D
  Auto-detect & deploy… (beta)
  ─────────────────────────────
  Restart JBoss…
  Check deployment status
  View log (ix3.log)              Ctrl+G
  ─────────────────────────────
  Apply property set…
  ─────────────────────────────
  Open RDP session
  ─────────────────────────────
  Manage modules…
  Manage property sets…
```

- Dialogs keep the Remote Desktop wording and flow so existing users recognise them.
- Restart shows live phases (connecting → stopping → starting → running) and then starts
  the deployment watch automatically; progress appears as a bar in the environment header.
- Items not allowed for this environment class are disabled with the reason.
- The log viewer opens in the **Logs** section (and can pop out into its own window):
  follow/pause, filter, highlight levels, search, wrap toggle, copy selection, download
  the whole file, and a picker for rotated archives.

## 2.7 Services and Processes

**Services**

- Table: status dot, Name, Display name, Status, Startup type, Log on as, PID, Memory.
- Filter chips: **COSMIC / Cambio** (default) · All · Running · Stopped; search box;
  auto-refresh indicator with a pause toggle.
- Row actions: Start · Stop · Restart (enabled only when allowed), Open file location,
  Details. Transitional states (Starting…/Stopping…) animate in place with a bounded wait
  and a clear timeout message.
- Details drawer: description, executable path, dependencies and dependents, process
  uptime and memory, last 20 related event-log entries (when FR-F4 exists).
- Stopping or restarting asks for confirmation naming the service and host; services
  outside the Cambio filter are view-only unless the user opted in under Settings;
  protected services show a shield and can never be stopped.

**Processes**

- Table: Name, PID, CPU %, Memory, Started, User, Service (if any), Command line
  (truncated, full in tooltip). Default sort by memory; `java.exe` rows are easy to spot.
- *End process* with confirmation, on environments that allow it.

## 2.8 Safety UX

| Situation | Behaviour |
|---|---|
| Environment class | Badge in the tab and header: **Private server**, **Private client**, **Public server · read-only**, **CCP · read-only**, **Unclassified · read-only**. Read-only environments get an amber header strip |
| Disallowed action | Disabled, with a tooltip stating the rule ("Public servers cannot be changed") |
| Delete | Confirmation lists item count and total size; recursive delete above the threshold requires typing the folder name |
| Overwrite on upload | Conflict dialog; "Back up existing file" is ticked by default |
| Stop / restart service, end process | Confirmation naming the target and host |
| Unclassified host | A dismissible explanation of the naming convention (`-SD`, `-CL`, `-CP`, `-EA`) and why it is read-only |
| After any mutation | Toast with the outcome and, where one exists, the backup path; an entry in Activity |

## 2.9 Keyboard map

| Key | Action |
|---|---|
| Enter / double-click | Open folder, or open file locally |
| Backspace, Alt+↑ | Up one level |
| Alt+← / Alt+→ | Back / forward |
| F5 | Refresh |
| Ctrl+L, Alt+D | Edit address |
| Ctrl+F, Ctrl+E | Search / filter |
| ↑ ↓ Home End PgUp PgDn | Move selection |
| Shift / Ctrl + click or arrows | Extend / toggle selection |
| Ctrl+A | Select all |
| Type letters | Jump to the first matching name |
| Ctrl+C | Copy (stage for paste in Windows Explorer) |
| Ctrl+V | Paste (remote copy, or upload of local files on the clipboard) |
| Ctrl+Shift+N | New folder |
| F2 | Rename |
| Delete | Delete (with confirmation) |
| Alt+Enter | Properties |
| Ctrl+T / Ctrl+W | New / close explorer tab |
| Ctrl+Tab | Next environment tab |
| Esc | Cancel edit / close dialog / clear selection |

## 2.10 States every view must handle

| State | Presentation |
|---|---|
| Connecting | Skeleton rows + "Connecting to L-abc123-SD…" with Cancel |
| Not on VPN / host unreachable | Full-pane message with the probable cause and **Retry**; no stack trace |
| Authentication failed | Message plus "Attempted as `cambio.se\user`" and a link to account settings |
| WinRM not configured | Message with a button that opens the setup in Settings |
| Session dropped mid-use | Non-modal banner "Reconnecting…"; the view keeps its last data, greyed |
| Access denied on a folder | Inline message in the list; navigation history intact |
| Empty folder | "This folder is empty" (with Upload / New folder when allowed) |
| File in use | Explains that JBoss holds the file and offers the staged-swap path or "stop service first" |
| Long operation | Moves to the task panel after ~1 s rather than holding a modal |

## 2.11 Visual design

- Windows 11 feel: Segoe UI Variable, Fluent-style icons, 4 px spacing grid, compact
  density by default (rows ~28 px) with a comfortable option.
- Light and dark themes following the system setting; colour is never the only signal
  (badges carry text and an icon).
- Minimum window 960 × 600; panes resizable and collapsible; layout remembered per user.
- Motion is brief and functional (progress, state change) and honours reduced-motion.
- Component stack: React 19 + TypeScript, Zustand for state, a virtualised list for the
  file and service tables, Tailwind CSS 4 with daisyUI 5 so the Aeris file-explorer
  components can be adapted rather than rewritten; Remote Desktop's dialogs are ported
  and restyled onto the same tokens.
