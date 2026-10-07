import { CircleAlert, CircleCheck, Info, X } from "lucide-react";
import { useUi } from "../stores/uiStore";

export function Toasts() {
  const toasts = useUi((s) => s.toasts);
  return (
    <div aria-live="polite" className="pointer-events-none fixed right-4 bottom-10 z-[80] flex w-[340px] max-w-[calc(100vw-32px)] flex-col items-end gap-2">
      {toasts.map((t) => (
        <div
          key={t.id}
          role={t.kind === "error" ? "alert" : "status"}
          className="toast-in pointer-events-auto flex w-full items-start gap-2.5 rounded-[10px] border border-line bg-elevated px-3 py-2.5 shadow-[var(--shadow-float)]"
        >
          <span className={`mt-px shrink-0 ${t.kind === "error" ? "text-danger" : t.kind === "success" ? "text-success" : "text-accent"}`}>
            {t.kind === "error" ? <CircleAlert size={16} /> : t.kind === "success" ? <CircleCheck size={16} /> : <Info size={16} />}
          </span>
          <div className="min-w-0 flex-1">
            <div className="text-[13px] font-medium text-fg">{t.message}</div>
            {t.detail && <div className="mt-0.5 break-words text-[12px] text-fg-muted">{t.detail}</div>}
          </div>
          <button
            type="button"
            aria-label="Dismiss"
            onClick={() => useUi.getState().dismissToast(t.id)}
            className="focus-ring -mr-1 flex h-5 w-5 shrink-0 items-center justify-center rounded-[5px] text-fg-subtle transition-colors hover:bg-surface-2 hover:text-fg"
          >
            <X size={13} />
          </button>
        </div>
      ))}
    </div>
  );
}
