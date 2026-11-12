import { useEffect, useState, type FormEvent, type ReactNode } from "react";
import * as api from "../../api";
import type { AccountInfo, WinrmStatus } from "../../api";
import Icon from "../../components/Icon";
import { useConnections } from "../../stores/connections";
import { useUi } from "../../stores/ui";
import ImportCard from "./ImportCard";

export default function SettingsView() {
  return (
    <div className="min-h-0 flex-1 overflow-y-auto">
      <div className="mx-auto flex max-w-2xl flex-col gap-4 p-4">
        <h1 className="text-base font-semibold">Settings</h1>
        <AccountCard />
        <WinrmCard />
        <Card title="Import from Remote Desktop" icon="import">
          <ImportCard />
        </Card>
      </div>
    </div>
  );
}

function Card({ title, icon, children }: { title: string; icon: "plug" | "lock" | "import"; children: ReactNode }) {
  return (
    <section className="rounded-box border border-base-300 bg-base-100 p-4">
      <h2 className="mb-3 flex items-center gap-2 text-sm font-semibold">
        <Icon name={icon} /> {title}
      </h2>
      {children}
    </section>
  );
}

function AccountCard() {
  const toast = useUi((s) => s.toast);
  const [account, setAccount] = useState<AccountInfo | null>(null);
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);

  const load = async () => {
    const a = await api.getAccount();
    setAccount(a);
    setUsername(a.username ?? "");
  };
  useEffect(() => {
    load().catch(() => setAccount({ username: null, hasPassword: false }));
  }, []);

  const save = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    try {
      await api.setAccount(username, password === "" ? null : password);
      setPassword("");
      await load();
      toast("success", "Account saved.");
    } catch (err) {
      toast("error", api.toAppError(err).message);
    } finally {
      setBusy(false);
    }
  };

  const clear = async () => {
    try {
      await api.clearAccount();
      setPassword("");
      await load();
      toast("info", "Account removed. Open sessions were closed.");
    } catch (err) {
      toast("error", api.toAppError(err).message);
    }
  };

  return (
    <Card title="Account" icon="lock">
      <p className="mb-3 text-xs opacity-70">
        The account used to reach every environment. The password is kept in Windows Credential Manager,
        never in WinGUI's own files. An environment can override the username in its own settings.
      </p>
      <form className="flex flex-col gap-3" onSubmit={save}>
        <label className="flex flex-col gap-1">
          <span className="text-xs font-medium">Username</span>
          <input
            className="input input-bordered input-sm w-full"
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            placeholder="DOMAIN\user or user@domain — blank uses your Windows account"
            spellCheck={false}
            autoComplete="off"
          />
        </label>
        <label className="flex flex-col gap-1">
          <span className="text-xs font-medium">Password</span>
          <input
            type="password"
            className="input input-bordered input-sm w-full"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            placeholder={account?.hasPassword ? "Stored — leave blank to keep it" : "Required"}
            autoComplete="off"
          />
        </label>
        <div className="flex items-center gap-2">
          <button className="btn btn-sm btn-primary" disabled={busy || (!account?.hasPassword && password === "")}>
            Save
          </button>
          {account?.hasPassword && (
            <button type="button" className="btn btn-sm btn-ghost" onClick={clear}>
              Remove account
            </button>
          )}
          <span className="ml-auto flex items-center gap-1 text-xs opacity-70">
            {account?.hasPassword ? (
              <>
                <Icon name="check" size={14} className="text-success" /> Password stored
              </>
            ) : account ? (
              <>
                <Icon name="alert" size={14} className="text-warning" /> No password stored
              </>
            ) : null}
          </span>
        </div>
      </form>
    </Card>
  );
}

function WinrmCard() {
  const toast = useUi((s) => s.toast);
  const count = useConnections((s) => s.connections.length);
  const [status, setStatus] = useState<WinrmStatus | null>(null);
  const [busy, setBusy] = useState(false);

  const load = () =>
    api
      .winrmStatus()
      .then(setStatus)
      .catch(() => setStatus(null));
  useEffect(() => {
    void load();
  }, []);

  const setup = async () => {
    setBusy(true);
    try {
      toast("success", await api.setupWinrm());
    } catch (err) {
      toast("error", api.toAppError(err).message);
    } finally {
      setBusy(false);
      await load();
    }
  };

  return (
    <Card title="WinRM client" icon="plug">
      <p className="mb-3 text-xs opacity-70">
        WinGUI talks to environments over WinRM. Setup starts the local WinRM service and adds{" "}
        <strong>your saved environments</strong> to this computer's TrustedHosts list — never a wildcard, since a
        trusted host is one the account's credentials may be sent to without verifying who answers. Run it again
        after adding environments. It needs an administrator prompt.
      </p>
      {status === null ? (
        <div className="skeleton h-12 w-full" />
      ) : (
        <dl className="mb-3 grid grid-cols-[8rem_1fr] gap-y-1 text-sm">
          <dt className="opacity-70">Service</dt>
          <dd className="flex items-center gap-1">
            <Icon name={status.serviceRunning ? "check" : "alert"} size={14} className={status.serviceRunning ? "text-success" : "text-warning"} />
            {status.serviceRunning ? "Running" : "Not running"}
          </dd>
          <dt className="opacity-70">Trusted hosts</dt>
          <dd className="mono selectable break-all">{status.trustedHosts || "(none)"}</dd>
        </dl>
      )}
      {status?.wildcard && (
        <div role="alert" className="alert alert-warning alert-soft mb-3 py-2 text-sm">
          <Icon name="alert" />
          TrustedHosts contains a wildcard, so the account's credentials can be sent to any machine that answers
          to a name. Run setup to narrow it to your saved environments.
        </div>
      )}
      <div className="flex items-center gap-2">
        <button className="btn btn-sm" onClick={setup} disabled={busy || count === 0}>
          {busy ? "Waiting for approval…" : status?.configured ? "Run setup again" : "Set up WinRM"}
        </button>
        <button className="btn btn-sm btn-ghost btn-square" onClick={load} aria-label="Refresh WinRM status" title="Refresh">
          <Icon name="refresh" />
        </button>
        {count === 0 && <span className="text-xs opacity-70">Add an environment first.</span>}
      </div>
    </Card>
  );
}
