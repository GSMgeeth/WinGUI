import { useUi } from "../stores/ui";

const TONE = { info: "alert-info", success: "alert-success", error: "alert-error" } as const;

export default function Toasts() {
  const toasts = useUi((s) => s.toasts);
  const dismiss = useUi((s) => s.dismissToast);
  if (toasts.length === 0) return null;
  return (
    <div className="toast toast-end toast-bottom z-50 mb-8" role="status" aria-live="polite">
      {toasts.map((t) => (
        <button
          key={t.id}
          className={`alert ${TONE[t.tone]} max-w-md cursor-pointer whitespace-pre-wrap py-2 text-left text-sm`}
          onClick={() => dismiss(t.id)}
          title="Dismiss"
        >
          {t.text}
        </button>
      ))}
    </div>
  );
}
