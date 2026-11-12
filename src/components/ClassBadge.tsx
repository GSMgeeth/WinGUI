import type { Connection } from "../api";
import Icon from "./Icon";

/**
 * The environment's class and whether it can be changed — always visible, so a
 * public server never looks like a dev box. Text plus icon: colour is never
 * the only signal. Everything shown here is computed by the backend.
 */
export default function ClassBadge({ conn, size = "sm" }: { conn: Connection; size?: "sm" | "md" }) {
  const tone = !conn.openable ? "badge-neutral" : conn.readOnly ? "badge-warning" : "badge-success";
  const name = conn.envClassLabel.charAt(0).toUpperCase() + conn.envClassLabel.slice(1);
  const suffix = !conn.openable ? " · not available" : conn.readOnly ? " · read-only" : "";
  const title = !conn.openable
    ? "This kind of environment cannot be opened from WinGUI."
    : conn.readOnly
      ? "You can look and copy files out, but nothing on this environment can be changed."
      : "A private environment: files, services and deployments can be changed.";
  return (
    <span
      className={`badge badge-soft ${tone} ${size === "sm" ? "badge-sm" : ""} gap-1 whitespace-nowrap`}
      title={title}
    >
      {(conn.readOnly || !conn.openable) && <Icon name="lock" size={12} />}
      {name}
      {suffix}
    </span>
  );
}
