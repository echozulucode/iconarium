// Gallery keyboard model (plan §37). Active only when the viewer, menus and dialogs are closed,
// and never while the user is typing in an input.
import { useEffect, useRef } from "react";
import { copyAssets, galleryCopy, openViewer } from "../app/commands";
import { isTypingTarget } from "../app/focus";
import type { MoveKey } from "../lib/selection";
import { useSelection } from "../stores/selectionStore";
import { summaryCache } from "../stores/summaryCache";
import { useUi } from "../stores/uiStore";
import { useViewer } from "../stores/viewerStore";
import { isPreviewable } from "../components/AssetCard";

const MOVE_KEYS: Record<string, MoveKey> = {
  ArrowLeft: "left",
  ArrowRight: "right",
  ArrowUp: "up",
  ArrowDown: "down",
  Home: "home",
  End: "end",
  PageUp: "pageup",
  PageDown: "pagedown",
};

export interface GalleryKeyboardContext {
  columns: number;
  pageRows: number;
  /** Open the context menu for the focused item (Shift+F10 / Menu key). */
  openMenuForFocus(): void;
}

export function gallerySuspended(): boolean {
  const ui = useUi.getState();
  return useViewer.getState().openId !== null || ui.popovers > 0 || ui.propertiesId !== null || ui.menu !== null;
}

export function useGalleryKeyboard(ctx: GalleryKeyboardContext): void {
  const ref = useRef(ctx);
  ref.current = ctx;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented || gallerySuspended() || isTypingTarget(e.target)) return;
      const ctrl = e.ctrlKey || e.metaKey;
      const sel = useSelection.getState();
      const { columns, pageRows } = ref.current;

      const move = MOVE_KEYS[e.key];
      if (move && !e.altKey) {
        e.preventDefault();
        sel.move(move, columns, pageRows, { shift: e.shiftKey, ctrl: ctrl && !e.shiftKey });
        return;
      }

      if (ctrl && !e.altKey && e.key.toLowerCase() === "a") {
        e.preventDefault();
        sel.selectAll();
        return;
      }
      if (ctrl && e.shiftKey && e.key.toLowerCase() === "c") {
        e.preventDefault();
        void copyAssets(sel.ordered(), "path");
        return;
      }
      if (ctrl && !e.shiftKey && e.key.toLowerCase() === "c") {
        e.preventDefault();
        galleryCopy();
        return;
      }
      if (ctrl && e.key === " ") {
        e.preventDefault();
        sel.toggleFocused();
        return;
      }
      if (e.key === "Escape") {
        if (sel.selected.size > 0) {
          e.preventDefault();
          sel.clear();
        }
        return;
      }
      if (e.key === "Enter") {
        const id = sel.focus ?? sel.ordered()[0];
        if (id == null) return;
        e.preventDefault();
        if (e.altKey) {
          useUi.getState().showProperties(id);
          return;
        }
        if (isPreviewable(summaryCache.get(id))) openViewer(id);
        return;
      }
      if ((e.shiftKey && e.key === "F10") || e.key === "ContextMenu") {
        e.preventDefault();
        ref.current.openMenuForFocus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}
