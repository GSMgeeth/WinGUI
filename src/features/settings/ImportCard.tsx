import { useEffect, useState } from "react";
import * as api from "../../api";
import type { ImportPreview } from "../../api";
import Icon from "../../components/Icon";
import { useConnections } from "../../stores/connections";
import { useUi } from "../../stores/ui";

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/**
 * Offer to copy Remote Desktop's saved data. Shown on the empty home screen
 * and in Settings. Renders nothing when that app has no data on this machine.
 */
export default function ImportCard({ compact = false }: { compact?: boolean }) {
  const [preview, setPreview] = useState<ImportPreview | null>(null);
  const [includeAccount, setIncludeAccount] = useState(true);
  const [busy, setBusy] = useState(false);
  const refresh = useConnections((s) => s.refresh);
  const toast = useUi((s) => s.toast);

  useEffect(() => {
    api.importPreview().then(setPreview).catch(() => setPreview(null));
  }, []);

  if (!preview?.available) {
    return compact ? null : (
      <p className="text-sm opacity-60">Remote Desktop has no saved data on this machine.</p>
    );
  }

  const run = async () => {
    setBusy(true);
    try {
      const s = await api.importRemoteDesktop(includeAccount && preview.hasAccount);
      await refresh();
      const parts = [
        plural(s.connectionsAdded, "environment"),
        plural(s.labelsAdded, "label"),
        plural(s.categoriesAdded, "category", "categories"),
        plural(s.modulesAdded, "module"),
        plural(s.propertySetsAdded, "property set"),
      ];
      const extra = [
        s.connectionsSkipped > 0 ? `${s.connectionsSkipped} already present or invalid` : "",
        s.accountImported ? "account copied" : "",
      ].filter(Boolean);
      toast("success", `Imported ${parts.join(", ")}.${extra.length ? ` (${extra.join("; ")})` : ""}`);
    } catch (e) {
      toast("error", api.toAppError(e).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-3">
      <p className="text-sm">
        Found {plural(preview.connections, "environment")}, {plural(preview.labels, "label")},{" "}
        {plural(preview.categories, "category", "categories")}, {plural(preview.modules, "module")} and{" "}
        {plural(preview.propertySets, "property set")} saved by the Remote Desktop app.
      </p>
      <p className="text-xs opacity-70">
        Importing copies them. Nothing in Remote Desktop is changed, and anything already in WinGUI is
        kept.
      </p>
      {preview.hasAccount && (
        <label className="flex cursor-pointer items-center gap-2 text-sm">
          <input
            type="checkbox"
            className="checkbox checkbox-sm"
            checked={includeAccount}
            onChange={(e) => setIncludeAccount(e.target.checked)}
          />
          <span>
            Also copy the account
            {preview.accountUsername && (
              <>
                {" "}
                (<span className="mono">{preview.accountUsername}</span>)
              </>
            )}{" "}
            and its password, unless one is already set here
          </span>
        </label>
      )}
      <div>
        <button className="btn btn-sm btn-primary" onClick={run} disabled={busy}>
          <Icon name="import" />
          {busy ? "Importing…" : "Import from Remote Desktop"}
        </button>
      </div>
    </div>
  );
}
