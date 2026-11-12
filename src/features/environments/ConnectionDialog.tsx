import { useState, type FormEvent } from "react";
import * as api from "../../api";
import type { Category, Connection, ConnectionInput, Label } from "../../api";
import Dialog from "../../components/Dialog";

const blankToNull = (s: string) => (s.trim() === "" ? null : s.trim());

/** Add or edit an environment. Validation lives in the backend; this only collects. */
export default function ConnectionDialog({
  existing,
  labels,
  categories,
  onClose,
  onSaved,
}: {
  existing: Connection | null;
  labels: Label[];
  categories: Category[];
  onClose: () => void;
  onSaved: (saved: Connection) => void;
}) {
  const [label, setLabel] = useState(existing?.label ?? "");
  const [computerName, setComputerName] = useState(existing?.computerName ?? "");
  const [username, setUsername] = useState(existing?.username ?? "");
  const [serviceName, setServiceName] = useState(existing?.serviceName ?? "");
  const [serverRoot, setServerRoot] = useState(existing?.serverRoot ?? "");
  const [clientRoot, setClientRoot] = useState(existing?.clientRoot ?? "");
  const [labelId, setLabelId] = useState(existing?.labelId ?? "");
  const [categoryId, setCategoryId] = useState(existing?.categoryId ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    const input: ConnectionInput = {
      label,
      computerName,
      username: blankToNull(username),
      serviceName: blankToNull(serviceName),
      serverRoot: blankToNull(serverRoot),
      clientRoot: blankToNull(clientRoot),
      labelId: blankToNull(labelId),
      categoryId: blankToNull(categoryId),
    };
    try {
      onSaved(existing ? await api.updateConnection(existing.id, input) : await api.addConnection(input));
    } catch (err) {
      setError(api.toAppError(err).message);
      setBusy(false);
    }
  };

  const field = "input input-bordered input-sm w-full";

  return (
    <Dialog
      title={existing ? "Edit environment" : "Add environment"}
      onClose={onClose}
      footer={
        <>
          <button className="btn btn-sm btn-ghost" onClick={onClose} type="button">
            Cancel
          </button>
          <button className="btn btn-sm btn-primary" form="connection-form" disabled={busy}>
            {busy ? "Saving…" : "Save"}
          </button>
        </>
      }
    >
      <form id="connection-form" className="flex flex-col gap-3" onSubmit={submit}>
        <label className="flex flex-col gap-1">
          <span className="text-xs font-medium">Computer name</span>
          <input
            className={`${field} mono`}
            value={computerName}
            onChange={(e) => setComputerName(e.target.value)}
            placeholder="L-abc123-SD"
            spellCheck={false}
            required
          />
          <span className="text-xs opacity-60">
            The suffix decides what is allowed: -SD and -CL are private and can be changed; anything
            else is read-only.
          </span>
        </label>

        <label className="flex flex-col gap-1">
          <span className="text-xs font-medium">Name (optional)</span>
          <input
            className={field}
            value={label}
            onChange={(e) => setLabel(e.target.value)}
            placeholder="Shown on the tile; defaults to the computer name"
          />
        </label>

        <div className="grid grid-cols-2 gap-3">
          <label className="flex flex-col gap-1">
            <span className="text-xs font-medium">Category</span>
            <select
              className="select select-bordered select-sm w-full"
              value={categoryId}
              onChange={(e) => setCategoryId(e.target.value)}
            >
              <option value="">None</option>
              {categories.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name}
                </option>
              ))}
            </select>
          </label>
          <label className="flex flex-col gap-1">
            <span className="text-xs font-medium">Label</span>
            <select
              className="select select-bordered select-sm w-full"
              value={labelId}
              onChange={(e) => setLabelId(e.target.value)}
            >
              <option value="">None</option>
              {labels.map((l) => (
                <option key={l.id} value={l.id}>
                  {l.name}
                </option>
              ))}
            </select>
          </label>
        </div>

        <details className="rounded-field border border-base-300 px-3 py-2" open={!!(username || serviceName || serverRoot || clientRoot)}>
          <summary className="cursor-pointer text-xs font-medium">Advanced</summary>
          <div className="mt-3 flex flex-col gap-3">
            <label className="flex flex-col gap-1">
              <span className="text-xs font-medium">Username override</span>
              <input
                className={field}
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                placeholder="Blank = the account from Settings"
                spellCheck={false}
              />
            </label>
            <label className="flex flex-col gap-1">
              <span className="text-xs font-medium">JBoss service name</span>
              <input
                className={`${field} mono`}
                value={serviceName}
                onChange={(e) => setServiceName(e.target.value)}
                placeholder="Production_Cosmic"
                spellCheck={false}
              />
            </label>
            <label className="flex flex-col gap-1">
              <span className="text-xs font-medium">Server root</span>
              <input
                className={`${field} mono`}
                value={serverRoot}
                onChange={(e) => setServerRoot(e.target.value)}
                placeholder="D:\Cambio\Production"
                spellCheck={false}
              />
            </label>
            <label className="flex flex-col gap-1">
              <span className="text-xs font-medium">Client root</span>
              <input
                className={`${field} mono`}
                value={clientRoot}
                onChange={(e) => setClientRoot(e.target.value)}
                placeholder="D:\CosmicClient\Client\Application"
                spellCheck={false}
              />
            </label>
          </div>
        </details>

        {error && (
          <div role="alert" className="alert alert-error alert-soft py-2 text-sm">
            {error}
          </div>
        )}
      </form>
    </Dialog>
  );
}
