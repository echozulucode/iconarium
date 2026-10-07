import { create } from "zustand";

export type ToastKind = "success" | "error" | "info";

export interface Toast {
  id: number;
  kind: ToastKind;
  message: string;
  detail?: string;
}

export interface AssetMenuState {
  kind: "asset";
  x: number;
  y: number;
  /** IDs the menu acts on (the selection, ordered). */
  ids: number[];
  primary: number;
}

interface UiState {
  toasts: Toast[];
  menu: AssetMenuState | null;
  propertiesId: number | null;
  /** Number of open popovers (menus/dialogs) that own the keyboard. */
  popovers: number;

  pushToast(kind: ToastKind, message: string, detail?: string): void;
  dismissToast(id: number): void;
  openAssetMenu(m: Omit<AssetMenuState, "kind">): void;
  closeMenu(): void;
  showProperties(id: number): void;
  hideProperties(): void;
  popoverOpened(): void;
  popoverClosed(): void;
}

let toastSeq = 0;
const timers = new Map<number, ReturnType<typeof setTimeout>>();

export const useUi = create<UiState>((set, get) => ({
  toasts: [],
  menu: null,
  propertiesId: null,
  popovers: 0,

  pushToast(kind, message, detail) {
    const id = ++toastSeq;
    set((s) => ({ toasts: [...s.toasts.slice(-3), { id, kind, message, detail }] }));
    timers.set(
      id,
      setTimeout(() => get().dismissToast(id), kind === "error" ? 5200 : 2400),
    );
  },
  dismissToast(id) {
    const t = timers.get(id);
    if (t) clearTimeout(t);
    timers.delete(id);
    set((s) => ({ toasts: s.toasts.filter((x) => x.id !== id) }));
  },
  openAssetMenu(m) {
    set({ menu: { kind: "asset", ...m } });
  },
  closeMenu() {
    set({ menu: null });
  },
  showProperties(id) {
    set({ propertiesId: id, menu: null });
  },
  hideProperties() {
    set({ propertiesId: null });
  },
  popoverOpened() {
    set((s) => ({ popovers: s.popovers + 1 }));
  },
  popoverClosed() {
    set((s) => ({ popovers: Math.max(0, s.popovers - 1) }));
  },
}));

export const toast = {
  success: (m: string, d?: string) => useUi.getState().pushToast("success", m, d),
  error: (m: string, d?: string) => useUi.getState().pushToast("error", m, d),
  info: (m: string, d?: string) => useUi.getState().pushToast("info", m, d),
};
