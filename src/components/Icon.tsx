// A small inline icon set (stroke icons on a 20px grid) — no icon-font or
// package dependency, and nothing fetched at runtime (the CSP forbids it).

const PATHS = {
  home: "M3 9.5 10 3l7 6.5M5 8v8h4v-5h2v5h4V8",
  settings:
    "M10 12.5a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5ZM16 10l1.5-1-1.5-2.6-1.7.6a5.6 5.6 0 0 0-1.4-.8L12.5 4h-3l-.4 2.2c-.5.2-1 .5-1.4.8L6 6.4 4.5 9 6 10l-1.5 1L6 13.6l1.7-.6c.4.3.9.6 1.4.8l.4 2.2h3l.4-2.2c.5-.2 1-.5 1.4-.8l1.7.6L17.5 11Z",
  server: "M3.5 4h13v5h-13zM3.5 11h13v5h-13zM6 6.5h.01M6 13.5h.01",
  monitor: "M3 4h14v9H3zM7 16.5h6M10 13v3.5",
  lock: "M5.5 9h9v7h-9zM7.5 9V6.5a2.5 2.5 0 0 1 5 0V9",
  pin: "m12 3 5 5-3 1-2.5 2.5.5 3.5-1 1-3.5-3.5L3.5 16.5 7.5 12.5 4 9l1-1 3.5.5L11 6Z",
  plus: "M10 4v12M4 10h12",
  refresh: "M16 10a6 6 0 1 1-1.8-4.3M16 3.5v3h-3",
  close: "m5 5 10 10M15 5 5 15",
  edit: "m13.5 3.5 3 3L7 16H4v-3Z",
  trash: "M4 6h12M8 6V4h4v2M6 6l.7 10h6.6L14 6",
  drive: "M3 11.5 5 5h10l2 6.5v4H3zM3 11.5h14M14 13.5h.01",
  tag: "M3 3.5h6.5l7 7-6 6-7-7zM6.5 7h.01",
  folder: "M3 5h5l1.5 2H17v8.5H3z",
  chevronRight: "m8 5 5 5-5 5",
  chevronDown: "m5 8 5 5 5-5",
  alert: "M10 3.5 17.5 16h-15zM10 8.5v3.5M10 14.2h.01",
  check: "m4.5 10.5 3.5 3.5 7.5-8",
  more: "M5 10h.01M10 10h.01M15 10h.01",
  up: "m5 12 5-5 5 5",
  down: "m5 8 5 5 5-5",
  import: "M10 3v9M6.5 8.5 10 12l3.5-3.5M4 15.5h12",
  plug: "M7.5 3v4M12.5 3v4M5.5 7h9v2.5a4.5 4.5 0 0 1-9 0zM10 14v3",
  search: "M9 14.5a5.5 5.5 0 1 0 0-11 5.5 5.5 0 0 0 0 11ZM13 13l4 4",
} as const;

export type IconName = keyof typeof PATHS;

export default function Icon({
  name,
  size = 16,
  className = "",
}: {
  name: IconName;
  size?: number;
  className?: string;
}) {
  return (
    <svg
      className={`shrink-0 ${className}`}
      width={size}
      height={size}
      viewBox="0 0 20 20"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d={PATHS[name]} />
    </svg>
  );
}
