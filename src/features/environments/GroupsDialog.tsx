import { useState } from "react";
import * as api from "../../api";
import Dialog from "../../components/Dialog";
import Icon from "../../components/Icon";

interface Group {
  id: string;
  name: string;
  color?: string;
}

const DEFAULT_COLOR = "#3b82f6";

/**
 * Manage labels or categories: add, rename, recolour (labels), reorder,
 * delete. The two are the same shape, so one dialog serves both.
 */
export default function GroupsDialog({
  kind,
  items,
  onClose,
  onChanged,
}: {
  kind: "label" | "category";
  items: Group[];
  onClose: () => void;
  onChanged: () => Promise<void>;
}) {
  const colored = kind === "label";
  const [name, setName] = useState("");
  const [color, setColor] = useState(DEFAULT_COLOR);
  const [error, setError] = useState<string | null>(null);

  const run = async (work: () => Promise<unknown>) => {
    setError(null);
    try {
      await work();
      await onChanged();
    } catch (e) {
      setError(api.toAppError(e).message);
    }
  };

  const add = () =>
    run(async () => {
      if (colored) await api.addLabel({ name, color });
      else await api.addCategory({ name });
      setName("");
    });

  const rename = (g: Group, next: string) => {
    if (next.trim() === g.name) return;
    return run(() =>
      colored ? api.updateLabel(g.id, { name: next, color: g.color ?? DEFAULT_COLOR }) : api.updateCategory(g.id, { name: next }),
    );
  };

  const recolor = (g: Group, next: string) => run(() => api.updateLabel(g.id, { name: g.name, color: next }));

  const remove = (g: Group) => run(() => (colored ? api.deleteLabel(g.id) : api.deleteCategory(g.id)));

  const move = (index: number, delta: number) => {
    const ids = items.map((i) => i.id);
    const to = index + delta;
    if (to < 0 || to >= ids.length) return;
    [ids[index], ids[to]] = [ids[to], ids[index]];
    return run(() => (colored ? api.reorderLabels(ids) : api.reorderCategories(ids)));
  };

  const title = colored ? "Labels" : "Categories";

  return (
    <Dialog
      title={title}
      onClose={onClose}
      width="28rem"
      footer={
        <button className="btn btn-sm" onClick={onClose}>
          Done
        </button>
      }
    >
      <p className="mb-3 text-xs opacity-70">
        {colored
          ? "Labels group environments and give them a colour. Deleting a label keeps its environments."
          : "Categories are a second way to group environments. Deleting a category keeps its environments."}
      </p>

      <ul className="mb-3 flex flex-col gap-1">
        {items.length === 0 && <li className="py-2 text-sm opacity-60">None yet.</li>}
        {items.map((g, i) => (
          <li key={g.id} className="flex items-center gap-1">
            {colored && (
              <input
                type="color"
                className="h-7 w-7 shrink-0 cursor-pointer rounded border border-base-300 bg-transparent p-0.5"
                value={g.color ?? DEFAULT_COLOR}
                onChange={(e) => recolor(g, e.target.value)}
                aria-label={`Colour of ${g.name}`}
              />
            )}
            <input
              className="input input-bordered input-sm min-w-0 flex-1"
              defaultValue={g.name}
              key={g.name}
              onBlur={(e) => rename(g, e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
              aria-label={`Name of ${g.name}`}
            />
            <button className="btn btn-ghost btn-xs btn-square" disabled={i === 0} onClick={() => move(i, -1)} aria-label={`Move ${g.name} up`}>
              <Icon name="up" />
            </button>
            <button
              className="btn btn-ghost btn-xs btn-square"
              disabled={i === items.length - 1}
              onClick={() => move(i, 1)}
              aria-label={`Move ${g.name} down`}
            >
              <Icon name="down" />
            </button>
            <button className="btn btn-ghost btn-xs btn-square text-error" onClick={() => remove(g)} aria-label={`Delete ${g.name}`}>
              <Icon name="trash" />
            </button>
          </li>
        ))}
      </ul>

      <form
        className="flex items-center gap-1 border-t border-base-300 pt-3"
        onSubmit={(e) => {
          e.preventDefault();
          void add();
        }}
      >
        {colored && (
          <input
            type="color"
            className="h-7 w-7 shrink-0 cursor-pointer rounded border border-base-300 bg-transparent p-0.5"
            value={color}
            onChange={(e) => setColor(e.target.value)}
            aria-label="Colour of the new label"
          />
        )}
        <input
          className="input input-bordered input-sm min-w-0 flex-1"
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder={colored ? "New label" : "New category"}
        />
        <button className="btn btn-sm btn-primary" disabled={!name.trim()}>
          Add
        </button>
      </form>

      {error && (
        <div role="alert" className="alert alert-error alert-soft mt-3 py-2 text-sm">
          {error}
        </div>
      )}
    </Dialog>
  );
}
