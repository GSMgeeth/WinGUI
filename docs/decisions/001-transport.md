# Decision 001 — Transport: held WinRM sessions through a PowerShell worker

[← Plan index](../../README.md) · Step 1 of the [execution plan](../05-execution-plan.md)

**Status:** mechanism built and proven locally; **network numbers still to be measured**
against real environments (see "What is still open"). Written 2026-11-13.

## Decision

WinGUI reaches environments over WinRM using one persistent, hidden
`powershell.exe` per open environment (the *session worker*), which holds a single
`PSSession` and answers line-delimited JSON requests over stdin/stdout. A pool owns the
workers. The one-shot path from Remote Desktop (a `powershell.exe` per action) is kept
for the features already proven on it.

## What has been established

All of this was run on a Windows 11 laptop with Windows PowerShell 5.1. None of it
involved a WinRM hop — WinRM is not enabled on the development machine.

| Question | Finding | Evidence |
|---|---|---|
| Can a hidden PowerShell 5.1 process take its script over stdin, then serve requests on the same pipe? | Yes. With `-InputFormat None` the PowerShell host leaves stdin alone; the process starts in ~0.2 s and exits by itself when stdin closes | Probe during development; `worker::tests` |
| Do non-ASCII names survive? | Yes, when the worker wraps the raw std streams as UTF-8 instead of using `[Console]::In/Out` (whose encoding follows the console code page) | Probe round-tripped `åäö` and CJK text |
| Is anything written to disk, or passed on the command line? | No. The script travels over stdin; host, user and password are environment variables of the child only, and the password variable is removed once the credential object exists | `worker.rs`, `worker.ps1` |
| Can request data become script? | No. Operations are a fixed table; the request selects one by name and its values travel as data | `worker::tests::request_text_is_data_never_script` |
| Does a failed or unknown operation kill the session? | No — it is reported with a code and the worker keeps serving | `worker::tests::unknown_operation_is_an_error_not_a_crash` |
| Is a rejected password ever re-sent automatically? | No. An authentication failure latches the entry until the user reconnects or changes the account; a concurrent attempt cannot override a latch set after it started | `pool::tests::an_auth_failure_latches_until_the_user_reconnects` and neighbours |
| Is a mutation ever silently repeated after a dropped session? | No. Only requests marked idempotent are retried, once | `pool::tests::a_mutation_is_never_repeated_after_a_lost_session` |
| Are sessions bounded and cleaned up? | Capped per host (default 4), closed after 10 minutes idle, closed politely on tab close and app exit (so the server's quota is released), and tied to a Job Object so they die if the app crashes | `pool::tests`; desktop app launched and closed with no process left behind |
| Does the whole stack work inside the real desktop app? | Yes for everything that needs no server: the window starts, commands answer over Tauri IPC, the guard refuses a public client before any credential is read, stores are written atomically with a schema version | Driven over WebView2's DevTools protocol against a scratch config folder |

## What is still open

These need a laptop on the VPN and real environments. Run
[`scripts/Spike-Transport.ps1`](../../scripts/Spike-Transport.ps1) from a PowerShell
prompt; it writes `001-transport-spike-<host>.md` next to this file.

```powershell
.\scripts\Spike-Transport.ps1 -ComputerName L-abc123-SD
```

Run it against **a private server with a GUI** and **a headless one**, then fill in:

| Measure | Target (docs/01-requirements.md) | GUI server | Headless server | Verdict |
|---|---|---|---|---|
| Session open, first | NFR-P1: ≤ 5 s | | | |
| Warm request, median / p95 | — (drives NFR-P2) | | | |
| List 1,000 entries | NFR-P2: ≤ 500 ms | | | |
| List 10,000 / 50,000 entries | first page ≤ 1 s | | | |
| Large file, chunked 1 MB / 4 MB | NFR-P5: publish the rate | | | |
| Large file, `Copy-Item -FromSession` | comparison | | | |
| Small files, remote zip | comparison | | | |
| Small files, `Copy-Item -FromSession` | comparison | | | |
| SMB reachable? Large / small files | decides the optional SMB path (D6) | | | |
| WinRM quotas (`MaxShellsPerUser`, `MaxConcurrentUsers`, …) | per-host cap of 4 must fit | | | |
| Language mode on the target | must be `FullLanguage` | | | |

Also to check by hand in the app, since no test can:

1. Open a private and a public environment: drives appear; the class badge and allowed
   list match the environment.
2. Enter a wrong password: one failure, an "environment rejected the account" message,
   and no further attempts until **Try again**.
3. Drop the VPN with a tab open, press refresh on the drives: a clear "could not be
   reached" state; reconnect the VPN and **Reconnect** works.
4. Close the app with tabs open: no `powershell.exe` left on the laptop, and no
   `wsmprovhost.exe` left for the account on the server.

## Decision gate

If session open or warm latency is far outside target on the real network, or held
sessions prove unreliable, stop before Step 2 and reassess (SMB-first for file access, or
an agent). If only transfer throughput is poor, proceed and plan the SMB strategy for
Step 6.

## Known limits of the current design

- **No in-band cancel.** A request that times out kills the worker; the next request
  starts a fresh session. Long operations (listing pages, transfers) will need
  cooperative cancellation when they are added in Steps 2 and 3.
- **Requests are serialised per environment.** One worker serves one request at a time;
  transfer workers (Step 3) keep long copies from blocking browsing.
- **Connect-error classification is best-effort.** It matches WinRM error codes first and
  English phrases second; on a non-English Windows an unrecognised failure falls back to
  the generic "could not connect" with the full message shown.
