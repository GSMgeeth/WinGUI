import { useEffect, useState } from "react";
import * as api from "./api";
import Icon, { type IconName } from "./components/Icon";
import Toasts from "./components/Toasts";
import EnvironmentsHome from "./features/environments/EnvironmentsHome";
import EnvironmentView from "./features/session/EnvironmentView";
import SettingsView from "./features/settings/SettingsView";
import { useConnections } from "./stores/connections";
import { useSessions } from "./stores/sessions";
import { useUi } from "./stores/ui";

/** The shell: navigation rail, environment tabs, the active view, status bar. */
export default function App() {
  const refresh = useConnections((s) => s.refresh);
  const connections = useConnections((s) => s.connections);
  const view = useUi((s) => s.view);
  const openIds = useUi((s) => s.openIds);
  const show = useUi((s) => s.show);
  const closeEnv = useUi((s) => s.closeEnv);
  const sessions = useSessions((s) => s.byId);
  const disconnect = useSessions((s) => s.disconnect);

  // The account is needed for everything; flag Settings when it is missing.
  const [accountMissing, setAccountMissing] = useState(false);
  useEffect(() => {
    void refresh();
  }, [refresh]);
  useEffect(() => {
    api
      .getAccount()
      .then((a) => setAccountMissing(!a.hasPassword))
      .catch(() => setAccountMissing(false));
  }, [view]);

  const tabs = openIds
    .map((id) => connections.find((c) => c.id === id))
    .filter((c): c is NonNullable<typeof c> => !!c);
  const active = view.kind === "env" ? tabs.find((c) => c.id === view.id) : undefined;

  const close = (id: string) => {
    closeEnv(id);
    void disconnect(id);
  };

  const connected = Object.values(sessions).filter((s) => s.status === "connected").length;

  return (
    <div className="flex h-full bg-base-200 text-base-content">
      <nav className="flex w-14 shrink-0 flex-col items-center gap-1 border-r border-base-300 bg-base-100 py-2" aria-label="Main">
        <RailButton icon="home" label="Environments" active={view.kind === "home"} onClick={() => show({ kind: "home" })} />
        <span className="flex-1" />
        <RailButton
          icon="settings"
          label="Settings"
          active={view.kind === "settings"}
          onClick={() => show({ kind: "settings" })}
          attention={accountMissing ? "No account password is set" : undefined}
        />
      </nav>

      <div className="flex min-w-0 flex-1 flex-col">
        {tabs.length > 0 && (
          <div className="flex items-end gap-1 overflow-x-auto border-b border-base-300 bg-base-100 px-2 pt-1.5" role="tablist" aria-label="Open environments">
            {tabs.map((c) => {
              const status = sessions[c.id]?.status ?? "idle";
              const isActive = active?.id === c.id;
              const dot =
                status === "connected" ? "bg-success" : status === "error" ? "bg-error" : status === "connecting" ? "bg-warning animate-pulse" : "bg-base-300";
              return (
                <div
                  key={c.id}
                  className={`flex max-w-56 items-center gap-1.5 rounded-t-field border border-b-0 px-2 py-1 text-sm ${
                    isActive ? "border-base-300 bg-base-200 font-medium" : "border-transparent hover:bg-base-200/60"
                  }`}
                >
                  <button
                    role="tab"
                    aria-selected={isActive}
                    className="flex min-w-0 items-center gap-1.5"
                    onClick={() => show({ kind: "env", id: c.id })}
                    title={`${c.label} — ${c.computerName} (${status})`}
                  >
                    <span className={`h-2 w-2 shrink-0 rounded-full ${dot}`} />
                    {c.readOnly && <Icon name="lock" size={12} className="opacity-70" />}
                    <span className="truncate">{c.label}</span>
                  </button>
                  <button
                    className="btn btn-ghost btn-xs btn-square h-5 min-h-0 w-5"
                    onClick={() => close(c.id)}
                    aria-label={`Close ${c.label}`}
                    title="Close (ends the session)"
                  >
                    <Icon name="close" size={12} />
                  </button>
                </div>
              );
            })}
          </div>
        )}

        <main className="flex min-h-0 flex-1 flex-col">
          {view.kind === "settings" ? (
            <SettingsView />
          ) : active ? (
            <EnvironmentView key={active.id} conn={active} />
          ) : (
            <EnvironmentsHome />
          )}
        </main>

        <footer className="flex items-center gap-3 border-t border-base-300 bg-base-100 px-3 py-1 text-xs opacity-80">
          <span>
            {connected === 0 ? "No open sessions" : `${connected} open session${connected === 1 ? "" : "s"}`}
          </span>
          {accountMissing && (
            <button className="link flex items-center gap-1" onClick={() => show({ kind: "settings" })}>
              <Icon name="alert" size={12} /> No account password set
            </button>
          )}
          <span className="ml-auto">WinGUI 0.1 · Step 1</span>
        </footer>
      </div>

      <Toasts />
    </div>
  );
}

function RailButton({
  icon,
  label,
  active,
  onClick,
  attention,
}: {
  icon: IconName;
  label: string;
  active: boolean;
  onClick: () => void;
  attention?: string;
}) {
  return (
    <button
      className={`relative flex h-10 w-10 items-center justify-center rounded-field ${
        active ? "bg-primary/15 text-primary" : "hover:bg-base-200"
      }`}
      onClick={onClick}
      aria-label={attention ? `${label} — ${attention}` : label}
      aria-current={active ? "page" : undefined}
      title={attention ? `${label} — ${attention}` : label}
    >
      <Icon name={icon} size={20} />
      {attention && <span className="absolute right-1.5 top-1.5 h-2 w-2 rounded-full bg-warning" />}
    </button>
  );
}
