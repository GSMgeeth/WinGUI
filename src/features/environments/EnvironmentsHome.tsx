import { useMemo, useState } from "react";
import * as api from "../../api";
import type { Category, Connection, Label } from "../../api";
import ClassBadge from "../../components/ClassBadge";
import Dialog from "../../components/Dialog";
import Icon from "../../components/Icon";
import { useConnections } from "../../stores/connections";
import { useSessions } from "../../stores/sessions";
import { useUi } from "../../stores/ui";
import ImportCard from "../settings/ImportCard";
import ConnectionDialog from "./ConnectionDialog";
import GroupsDialog from "./GroupsDialog";

/** What the navigation tree has selected. `null` ids mean "the ones without". */
type Filter =
  | { kind: "all" }
  | { kind: "pinned" }
  | { kind: "category"; categoryId: string | null }
  | { kind: "label"; categoryId: string | null; labelId: string | null };

const NONE = "__none__";
const sameFilter = (a: Filter, b: Filter) => JSON.stringify(a) === JSON.stringify(b);

/** A dangling group id (its group was deleted) counts as "none". */
function resolver(labels: Label[], categories: Category[]) {
  const labelIds = new Set(labels.map((l) => l.id));
  const categoryIds = new Set(categories.map((c) => c.id));
  return {
    labelOf: (c: Connection) => (c.labelId && labelIds.has(c.labelId) ? c.labelId : null),
    categoryOf: (c: Connection) => (c.categoryId && categoryIds.has(c.categoryId) ? c.categoryId : null),
  };
}

export default function EnvironmentsHome() {
  const { connections, labels, categories, loaded, error, refresh } = useConnections();
  const openEnv = useUi((s) => s.openEnv);
  const closeEnv = useUi((s) => s.closeEnv);
  const toast = useUi((s) => s.toast);
  const disconnect = useSessions((s) => s.disconnect);

  const [filter, setFilter] = useState<Filter>({ kind: "all" });
  const [query, setQuery] = useState("");
  const [editing, setEditing] = useState<Connection | "new" | null>(null);
  const [groups, setGroups] = useState<"label" | "category" | null>(null);
  const [deleting, setDeleting] = useState<Connection | null>(null);

  const { labelOf, categoryOf } = useMemo(() => resolver(labels, categories), [labels, categories]);

  const visible = useMemo(() => {
    const q = query.trim().toLowerCase();
    return connections
      .filter((c) => {
        if (q && !`${c.label} ${c.computerName}`.toLowerCase().includes(q)) return false;
        switch (filter.kind) {
          case "all":
            return true;
          case "pinned":
            return c.pinned;
          case "category":
            return categoryOf(c) === filter.categoryId;
          case "label":
            return categoryOf(c) === filter.categoryId && labelOf(c) === filter.labelId;
        }
      })
      .sort((a, b) => Number(b.pinned) - Number(a.pinned) || a.label.localeCompare(b.label));
  }, [connections, filter, query, labelOf, categoryOf]);

  const act = async (work: () => Promise<unknown>) => {
    try {
      await work();
      await refresh();
    } catch (e) {
      toast("error", api.toAppError(e).message);
    }
  };

  const remove = (c: Connection) =>
    act(async () => {
      await api.deleteConnection(c.id);
      closeEnv(c.id);
      await disconnect(c.id);
      setDeleting(null);
    });

  if (!loaded) {
    return <div className="flex flex-1 items-center justify-center opacity-60">Loading…</div>;
  }

  return (
    <div className="flex min-h-0 flex-1">
      {/* Navigation tree: category → label */}
      <aside className="flex w-56 shrink-0 flex-col border-r border-base-300 bg-base-100">
        <nav className="min-h-0 flex-1 overflow-y-auto p-2" aria-label="Environment groups">
          <TreeItem
            label="All environments"
            count={connections.length}
            active={filter.kind === "all"}
            onClick={() => setFilter({ kind: "all" })}
          />
          <TreeItem
            label="Pinned"
            icon="pin"
            count={connections.filter((c) => c.pinned).length}
            active={filter.kind === "pinned"}
            onClick={() => setFilter({ kind: "pinned" })}
          />
          <div className="my-2 border-t border-base-300" />
          {[...categories.map((c) => ({ id: c.id as string | null, name: c.name })), { id: null, name: "Uncategorized" }].map(
            (cat) => {
              const inCat = connections.filter((c) => categoryOf(c) === cat.id);
              if (cat.id === null && inCat.length === 0) return null;
              const catFilter: Filter = { kind: "category", categoryId: cat.id };
              const usedLabels = labels.filter((l) => inCat.some((c) => labelOf(c) === l.id));
              const unlabelled = inCat.filter((c) => labelOf(c) === null).length;
              return (
                <div key={cat.id ?? NONE}>
                  <TreeItem
                    label={cat.name}
                    icon="folder"
                    count={inCat.length}
                    active={sameFilter(filter, catFilter)}
                    onClick={() => setFilter(catFilter)}
                  />
                  {usedLabels.map((l) => {
                    const f: Filter = { kind: "label", categoryId: cat.id, labelId: l.id };
                    return (
                      <TreeItem
                        key={l.id}
                        indent
                        label={l.name}
                        dot={l.color}
                        count={inCat.filter((c) => labelOf(c) === l.id).length}
                        active={sameFilter(filter, f)}
                        onClick={() => setFilter(f)}
                      />
                    );
                  })}
                  {usedLabels.length > 0 && unlabelled > 0 && (
                    <TreeItem
                      indent
                      label="No label"
                      count={unlabelled}
                      active={sameFilter(filter, { kind: "label", categoryId: cat.id, labelId: null })}
                      onClick={() => setFilter({ kind: "label", categoryId: cat.id, labelId: null })}
                    />
                  )}
                </div>
              );
            },
          )}
        </nav>
        <div className="flex gap-1 border-t border-base-300 p-2">
          <button className="btn btn-ghost btn-xs flex-1" onClick={() => setGroups("category")}>
            <Icon name="folder" size={14} /> Categories
          </button>
          <button className="btn btn-ghost btn-xs flex-1" onClick={() => setGroups("label")}>
            <Icon name="tag" size={14} /> Labels
          </button>
        </div>
      </aside>

      {/* Tiles */}
      <section className="flex min-w-0 flex-1 flex-col">
        <header className="flex items-center gap-2 border-b border-base-300 bg-base-100 px-4 py-2">
          <h1 className="mr-auto text-base font-semibold">Environments</h1>
          <label className="input input-bordered input-sm flex w-64 items-center gap-2">
            <Icon name="search" size={14} className="opacity-60" />
            <input
              className="grow"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search by name or computer"
              aria-label="Search environments"
            />
          </label>
          <button className="btn btn-sm btn-primary" onClick={() => setEditing("new")}>
            <Icon name="plus" /> Add environment
          </button>
        </header>

        <div className="min-h-0 flex-1 overflow-y-auto p-4">
          {error && (
            <div role="alert" className="alert alert-error alert-soft mb-4 text-sm">
              {error.message}
            </div>
          )}

          {connections.length === 0 ? (
            <div className="mx-auto mt-10 flex max-w-lg flex-col gap-4 rounded-box border border-base-300 bg-base-100 p-6">
              <h2 className="text-lg font-semibold">No environments yet</h2>
              <p className="text-sm opacity-70">
                Add an environment by its computer name, or bring over what you already saved in the
                Remote Desktop app.
              </p>
              <ImportCard compact />
              <div>
                <button className="btn btn-sm" onClick={() => setEditing("new")}>
                  <Icon name="plus" /> Add environment
                </button>
              </div>
            </div>
          ) : visible.length === 0 ? (
            <p className="mt-10 text-center text-sm opacity-60">Nothing matches.</p>
          ) : (
            <ul className="grid grid-cols-[repeat(auto-fill,minmax(17rem,1fr))] gap-3">
              {visible.map((c) => (
                <Tile
                  key={c.id}
                  conn={c}
                  label={labels.find((l) => l.id === labelOf(c)) ?? null}
                  onOpen={() => openEnv(c.id)}
                  onEdit={() => setEditing(c)}
                  onPin={() => act(() => api.setConnectionPinned(c.id, !c.pinned))}
                  onDelete={() => setDeleting(c)}
                />
              ))}
            </ul>
          )}
        </div>
      </section>

      {editing && (
        <ConnectionDialog
          existing={editing === "new" ? null : editing}
          labels={labels}
          categories={categories}
          onClose={() => setEditing(null)}
          onSaved={async () => {
            setEditing(null);
            await refresh();
          }}
        />
      )}
      {groups && (
        <GroupsDialog
          kind={groups}
          items={groups === "label" ? labels : categories}
          onClose={() => setGroups(null)}
          onChanged={refresh}
        />
      )}
      {deleting && (
        <Dialog
          title="Remove environment"
          onClose={() => setDeleting(null)}
          width="26rem"
          footer={
            <>
              <button className="btn btn-sm btn-ghost" onClick={() => setDeleting(null)}>
                Cancel
              </button>
              <button className="btn btn-sm btn-error" onClick={() => remove(deleting)}>
                Remove
              </button>
            </>
          }
        >
          <p className="text-sm">
            Remove <strong>{deleting.label}</strong> (<span className="mono">{deleting.computerName}</span>) from
            WinGUI? Nothing on the environment itself is touched.
          </p>
        </Dialog>
      )}
    </div>
  );
}

function TreeItem({
  label,
  count,
  active,
  onClick,
  icon,
  dot,
  indent = false,
}: {
  label: string;
  count: number;
  active: boolean;
  onClick: () => void;
  icon?: "pin" | "folder";
  dot?: string;
  indent?: boolean;
}) {
  return (
    <button
      className={`flex w-full items-center gap-2 rounded-field px-2 py-1 text-left text-sm ${
        active ? "bg-primary/15 font-medium" : "hover:bg-base-200"
      } ${indent ? "pl-7" : ""}`}
      onClick={onClick}
      aria-current={active ? "true" : undefined}
    >
      {icon && <Icon name={icon} size={14} className="opacity-70" />}
      {dot && <span className="h-2.5 w-2.5 shrink-0 rounded-full" style={{ background: dot }} />}
      <span className="min-w-0 flex-1 truncate">{label}</span>
      <span className="text-xs tabular-nums opacity-60">{count}</span>
    </button>
  );
}

function Tile({
  conn,
  label,
  onOpen,
  onEdit,
  onPin,
  onDelete,
}: {
  conn: Connection;
  label: Label | null;
  onOpen: () => void;
  onEdit: () => void;
  onPin: () => void;
  onDelete: () => void;
}) {
  const session = useSessions((s) => s.byId[conn.id]?.status);
  return (
    <li
      className="flex flex-col gap-2 rounded-box border border-base-300 bg-base-100 p-3"
      style={label ? { borderLeft: `3px solid ${label.color}` } : undefined}
    >
      <div className="flex items-start gap-2">
        <Icon name={conn.side === "client" ? "monitor" : "server"} size={18} className="mt-0.5 opacity-70" />
        <div className="min-w-0 flex-1">
          <div className="truncate font-medium" title={conn.label}>
            {conn.label}
          </div>
          <div className="mono selectable truncate text-xs opacity-70" title={conn.computerName}>
            {conn.computerName}
          </div>
        </div>
        <button
          className={`btn btn-ghost btn-xs btn-square ${conn.pinned ? "text-primary" : "opacity-40 hover:opacity-100"}`}
          onClick={onPin}
          aria-label={conn.pinned ? `Unpin ${conn.label}` : `Pin ${conn.label}`}
          aria-pressed={conn.pinned}
          title={conn.pinned ? "Unpin" : "Pin"}
        >
          <Icon name="pin" size={14} />
        </button>
      </div>

      <div className="flex flex-wrap items-center gap-1">
        <ClassBadge conn={conn} />
        {label && <span className="badge badge-ghost badge-sm">{label.name}</span>}
        {session === "connected" && <span className="badge badge-soft badge-info badge-sm">Connected</span>}
      </div>

      <div className="mt-1 flex items-center gap-1">
        <button
          className="btn btn-sm btn-primary flex-1"
          onClick={onOpen}
          disabled={!conn.openable}
          title={conn.openable ? undefined : "This kind of environment cannot be opened from WinGUI."}
        >
          Open
        </button>
        <button className="btn btn-sm btn-ghost btn-square" onClick={onEdit} aria-label={`Edit ${conn.label}`} title="Edit">
          <Icon name="edit" />
        </button>
        <button
          className="btn btn-sm btn-ghost btn-square"
          onClick={onDelete}
          aria-label={`Remove ${conn.label}`}
          title="Remove"
        >
          <Icon name="trash" />
        </button>
      </div>
    </li>
  );
}
