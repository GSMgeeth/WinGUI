import { useEffect } from "react";
import type { AppError, Connection, DriveInfo, EnvFeatures } from "../../api";
import ClassBadge from "../../components/ClassBadge";
import Icon from "../../components/Icon";
import { formatBytes } from "../../format/bytes";
import { useSessions } from "../../stores/sessions";
import { useUi } from "../../stores/ui";

/** What each backend-computed permission means, in the order shown. */
const FEATURE_LABELS: [keyof EnvFeatures, string][] = [
  ["fsBrowse", "Browse files"],
  ["fsDownload", "Copy files out"],
  ["fsWrite", "Change files"],
  ["serviceView", "View services"],
  ["serviceControl", "Control services"],
  ["processView", "View processes"],
  ["processKill", "End processes"],
  ["deployJar", "Deploy JARs"],
  ["restartJboss", "Restart JBoss"],
  ["logs", "View logs"],
  ["deployStatus", "Deployment status"],
  ["editProperties", "Edit properties"],
];

/**
 * One open environment. Step 1 shows the connection state and the drives —
 * enough to prove the held session; Files, Services and the tools menu are
 * built on top of this in the following steps.
 */
export default function EnvironmentView({ conn }: { conn: Connection }) {
  const state = useSessions((s) => s.byId[conn.id]) ?? { status: "idle" as const };
  const connect = useSessions((s) => s.connect);
  const loadDrives = useSessions((s) => s.loadDrives);

  // Connect when the tab first opens. Later attempts are the user's (Retry):
  // nothing here loops on a failure, so a wrong password is sent once.
  useEffect(() => {
    if (conn.openable && state.status === "idle") void connect(conn.id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [conn.id, conn.openable]);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <header className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b border-base-300 bg-base-100 px-4 py-2">
        <Icon name={conn.side === "client" ? "monitor" : "server"} size={18} className="opacity-70" />
        <h1 className="text-base font-semibold">{conn.label}</h1>
        <span className="mono selectable text-xs opacity-70">{conn.computerName}</span>
        <ClassBadge conn={conn} size="md" />
        <span className="ml-auto" />
        {conn.openable && (
          <button
            className="btn btn-sm btn-ghost"
            onClick={() => connect(conn.id)}
            disabled={state.status === "connecting"}
            title="Open a fresh session to this environment"
          >
            <Icon name="refresh" /> Reconnect
          </button>
        )}
      </header>

      {conn.openable && conn.readOnly && (
        <div className="flex items-center gap-2 border-b border-warning/30 bg-warning/10 px-4 py-1.5 text-xs">
          <Icon name="lock" size={14} />
          Read-only: you can look around and copy files out, but nothing on this environment can be changed.
          {conn.envClass === "unclassified" &&
            " Its name does not follow the environment convention (-SD, -CL, -CP, -EA), so it is treated as read-only."}
        </div>
      )}

      <div className="min-h-0 flex-1 overflow-y-auto p-4">
        {!conn.openable ? (
          <Notice icon="lock" title="This environment cannot be opened">
            <p>
              <span className="mono">{conn.computerName}</span> is a {conn.envClassLabel} environment. Shared
              client machines are off-limits to WinGUI.
            </p>
          </Notice>
        ) : state.status === "connecting" || state.status === "idle" ? (
          <div className="mt-16 flex flex-col items-center gap-3 opacity-80" role="status">
            <span className="loading loading-spinner loading-md" />
            <span>
              Connecting to <span className="mono">{conn.computerName}</span>…
            </span>
          </div>
        ) : state.status === "error" ? (
          <ConnectError error={state.error} onRetry={() => connect(conn.id)} />
        ) : (
          <div className="mx-auto flex max-w-4xl flex-col gap-5">
            <p className="text-sm opacity-80">
              <span className="mr-1 inline-block h-2 w-2 rounded-full bg-success align-middle" /> Connected to{" "}
              <span className="mono selectable">{state.info.computerName}</span> as{" "}
              <span className="mono selectable">{state.info.user}</span>
              {state.info.os && <> · {state.info.os}</>}
              {state.info.psVersion && <> · PowerShell {state.info.psVersion}</>}
            </p>

            <section aria-labelledby="drives-heading">
              <div className="mb-2 flex items-center gap-2">
                <h2 id="drives-heading" className="text-sm font-semibold">
                  Drives
                </h2>
                <button
                  className="btn btn-ghost btn-xs btn-square"
                  onClick={() => loadDrives(conn.id)}
                  aria-label="Refresh drives"
                  title="Refresh"
                >
                  <Icon name="refresh" size={14} />
                </button>
              </div>
              {state.drivesError ? (
                <div role="alert" className="alert alert-error alert-soft text-sm">
                  <span className="selectable whitespace-pre-wrap">{state.drivesError.message}</span>
                </div>
              ) : state.drives === null ? (
                <div className="flex gap-3">
                  <div className="skeleton h-20 w-60" />
                  <div className="skeleton h-20 w-60" />
                </div>
              ) : state.drives.length === 0 ? (
                <p className="text-sm opacity-60">No drives reported.</p>
              ) : (
                <ul className="grid grid-cols-[repeat(auto-fill,minmax(15rem,1fr))] gap-3">
                  {state.drives.map((d) => (
                    <Drive key={d.name} drive={d} />
                  ))}
                </ul>
              )}
            </section>

            <section aria-labelledby="allowed-heading">
              <h2 id="allowed-heading" className="mb-2 text-sm font-semibold">
                Allowed on this environment
              </h2>
              <ul className="flex flex-wrap gap-1.5">
                {FEATURE_LABELS.map(([key, text]) => {
                  const on = conn.features[key];
                  return (
                    <li
                      key={key}
                      className={`badge badge-sm gap-1 ${on ? "badge-soft badge-success" : "badge-ghost opacity-60"}`}
                    >
                      <Icon name={on ? "check" : "close"} size={11} />
                      {text}
                      <span className="sr-only">{on ? " (allowed)" : " (not allowed)"}</span>
                    </li>
                  );
                })}
              </ul>
              <p className="mt-2 text-xs opacity-60">
                Decided by the environment's name and enforced by the app's backend. The Files, Services and
                Environment tools views arrive in the next steps of the plan.
              </p>
            </section>
          </div>
        )}
      </div>
    </div>
  );
}

function Drive({ drive }: { drive: DriveInfo }) {
  if (!drive.ready) {
    return (
      <li className="flex items-center gap-3 rounded-box border border-base-300 bg-base-100 p-3 opacity-60">
        <Icon name="drive" size={22} />
        <div>
          <div className="font-medium">{drive.name}</div>
          <div className="text-xs">{drive.driveType === "CDRom" ? "No disc" : "Not ready"}</div>
        </div>
      </li>
    );
  }
  const used = drive.totalBytes - drive.freeBytes;
  const pct = drive.totalBytes > 0 ? Math.round((used / drive.totalBytes) * 100) : 0;
  const freePct = 100 - pct;
  // Low space is called out in words as well as colour.
  const low = freePct < 10;
  const tone = low ? "progress-error" : freePct < 20 ? "progress-warning" : "progress-primary";
  return (
    <li className="flex flex-col gap-1.5 rounded-box border border-base-300 bg-base-100 p-3">
      <div className="flex items-center gap-2">
        <Icon name="drive" size={20} className="opacity-70" />
        <span className="font-medium">
          {drive.label ? `${drive.label} (${drive.name})` : drive.name}
        </span>
        <span className="ml-auto text-xs opacity-60">{drive.fileSystem}</span>
      </div>
      <progress className={`progress ${tone} h-2 w-full`} value={pct} max={100} aria-label={`${drive.name} ${pct}% used`} />
      <div className="flex items-center justify-between text-xs">
        <span title={`${drive.freeBytes.toLocaleString()} bytes free`}>
          {formatBytes(drive.freeBytes)} free of {formatBytes(drive.totalBytes)}
        </span>
        {low && (
          <span className="flex items-center gap-1 font-medium text-error">
            <Icon name="alert" size={12} /> Low space
          </span>
        )}
      </div>
    </li>
  );
}

/** What went wrong, where, as whom — and the one useful next step for each cause. */
function ConnectError({ error, onRetry }: { error: AppError; onRetry: () => void }) {
  const show = useUi((s) => s.show);
  const settings = (
    <button className="btn btn-sm" onClick={() => show({ kind: "settings" })}>
      <Icon name="settings" /> Open Settings
    </button>
  );
  const retry = (
    <button className="btn btn-sm btn-primary" onClick={onRetry}>
      <Icon name="refresh" /> Try again
    </button>
  );

  const known: Record<string, { title: string; hint: string; actions: React.ReactNode }> = {
    NoAccount: {
      title: "No account is set up",
      hint: "Add the account username and password in Settings, then try again.",
      actions: (
        <>
          {settings}
          {retry}
        </>
      ),
    },
    AuthFailed: {
      title: "The environment rejected the account",
      hint: "The password may have changed, or the account may not be an administrator on this machine. WinGUI will not retry by itself — repeated attempts with a wrong password can lock the account.",
      actions: (
        <>
          {settings}
          {retry}
        </>
      ),
    },
    Unreachable: {
      title: "The environment could not be reached",
      hint: "Check that you are on the VPN, that the computer name is right, and that the environment is running.",
      actions: retry,
    },
    NotTrusted: {
      title: "WinRM is not set up for this environment",
      hint: "Run the WinRM setup in Settings so this computer is allowed to authenticate to it, then try again.",
      actions: (
        <>
          {settings}
          {retry}
        </>
      ),
    },
    QuotaExceeded: {
      title: "The environment has no free remote sessions",
      hint: "Too many sessions are open for this account on the server. Close other WinGUI or Remote Desktop sessions to it and try again in a moment.",
      actions: retry,
    },
    TooManySessions: {
      title: "Too many sessions to this host",
      hint: "Close another tab that points at the same computer.",
      actions: retry,
    },
    NotAllowed: {
      title: "Not allowed on this environment",
      hint: "",
      actions: null,
    },
  };
  const k = known[error.code] ?? {
    title: "Could not connect",
    hint: "",
    actions: retry,
  };

  return (
    <Notice icon="alert" title={k.title}>
      {k.hint && <p>{k.hint}</p>}
      <pre className="selectable mt-1 max-h-48 overflow-auto whitespace-pre-wrap rounded-field bg-base-200 p-2 text-xs">
        {error.message}
      </pre>
      {k.actions && <div className="mt-1 flex gap-2">{k.actions}</div>}
    </Notice>
  );
}

function Notice({
  icon,
  title,
  children,
}: {
  icon: "alert" | "lock";
  title: string;
  children: React.ReactNode;
}) {
  return (
    <div
      role="alert"
      className="mx-auto mt-10 flex max-w-xl flex-col gap-2 rounded-box border border-base-300 bg-base-100 p-5 text-sm"
    >
      <h2 className="flex items-center gap-2 text-base font-semibold">
        <Icon name={icon} size={18} /> {title}
      </h2>
      {children}
    </div>
  );
}
