import { useEffect, useRef, type ReactNode } from "react";
import Icon from "./Icon";

/** A modal dialog: Escape and the close button dismiss it; focus starts inside. */
export default function Dialog({
  title,
  onClose,
  children,
  footer,
  width = "32rem",
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
  width?: string;
}) {
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    // Put focus on the first field (or the dialog itself) so typing works at once.
    const first = box.current?.querySelector<HTMLElement>("input, select, textarea, button.btn-primary");
    (first ?? box.current)?.focus();
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/40 p-4"
      onMouseDown={(e) => e.target === e.currentTarget && onClose()}
    >
      <div
        ref={box}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        tabIndex={-1}
        style={{ width }}
        className="flex max-h-full max-w-full flex-col rounded-box border border-base-300 bg-base-100 shadow-xl outline-none"
      >
        <header className="flex items-center justify-between border-b border-base-300 px-4 py-3">
          <h2 className="text-base font-semibold">{title}</h2>
          <button className="btn btn-ghost btn-xs btn-square" onClick={onClose} aria-label="Close">
            <Icon name="close" />
          </button>
        </header>
        <div className="min-h-0 flex-1 overflow-y-auto px-4 py-4">{children}</div>
        {footer && (
          <footer className="flex items-center justify-end gap-2 border-t border-base-300 px-4 py-3">
            {footer}
          </footer>
        )}
      </div>
    </div>
  );
}
