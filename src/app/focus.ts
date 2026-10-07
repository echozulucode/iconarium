// Shared DOM handles for focus management across components.
export const searchInputRef: { current: HTMLInputElement | null } = { current: null };
export const gridRef: { current: HTMLDivElement | null } = { current: null };

export function focusSearch(selectAll = true): void {
  const el = searchInputRef.current;
  if (!el) return;
  el.focus();
  if (selectAll) el.select();
}

export function focusGrid(): void {
  gridRef.current?.focus({ preventScroll: true });
}

export function isTypingTarget(t: EventTarget | null): boolean {
  if (!(t instanceof HTMLElement)) return false;
  const tag = t.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || t.isContentEditable;
}
