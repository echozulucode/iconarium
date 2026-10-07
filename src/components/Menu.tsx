import { Check } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { useUi } from "../stores/uiStore";

export type MenuEntry =
  | {
      type?: "item";
      id: string;
      label: string;
      icon?: ReactNode;
      shortcut?: string;
      secondary?: string;
      disabled?: boolean;
      danger?: boolean;
      /** Shows a check mark column (checkbox / radio semantics). */
      checked?: boolean;
      role?: "menuitem" | "menuitemcheckbox" | "menuitemradio";
      /** Keep the menu open after selecting (toggles). */
      keepOpen?: boolean;
      onSelect: () => void;
    }
  | { type: "separator"; id: string }
  | { type: "heading"; id: string; label: string };

export interface MenuProps {
  /** Viewport point (context menu) or anchor rect (dropdown). */
  at: { x: number; y: number } | DOMRect;
  items: MenuEntry[];
  onClose: () => void;
  minWidth?: number;
  align?: "start" | "end";
  label?: string;
}

const MARGIN = 8;

type Item = Extract<MenuEntry, { onSelect: () => void }>;
const isItem = (e: MenuEntry): e is Item => e.type === undefined || e.type === "item";

/** Popup menu rendered in a portal, clamped to the window, fully keyboard accessible. */
export function Menu({ at, items, onClose, minWidth = 220, align = "start", label }: MenuProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; top: number; origin: string } | null>(null);
  const enabled = useMemo(() => items.map((e, i) => (isItem(e) && !e.disabled ? i : -1)).filter((i) => i >= 0), [items]);
  const [active, setActive] = useState<number>(-1);
  const restoreFocus = useRef<Element | null>(null);

  // Register as a popover so global keyboard handlers stand down.
  useEffect(() => {
    restoreFocus.current = document.activeElement;
    useUi.getState().popoverOpened();
    return () => {
      useUi.getState().popoverClosed();
      const el = restoreFocus.current;
      if (el instanceof HTMLElement && document.contains(el)) el.focus({ preventScroll: true });
    };
  }, []);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const w = el.offsetWidth;
    const h = el.offsetHeight;
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    let left: number;
    let top: number;
    let origin = "top left";
    if (at instanceof DOMRect) {
      left = align === "end" ? at.right - w : at.left;
      top = at.bottom + 6;
      if (top + h > vh - MARGIN && at.top - 6 - h > MARGIN) {
        top = at.top - 6 - h;
        origin = "bottom left";
      }
      if (align === "end") origin = origin.replace("left", "right");
    } else {
      left = at.x;
      top = at.y;
      if (left + w > vw - MARGIN) {
        left = Math.max(MARGIN, at.x - w);
        origin = origin.replace("left", "right");
      }
      if (top + h > vh - MARGIN) {
        top = Math.max(MARGIN, at.y - h);
        origin = origin.replace("top", "bottom");
      }
    }
    left = Math.min(Math.max(MARGIN, left), Math.max(MARGIN, vw - w - MARGIN));
    top = Math.min(Math.max(MARGIN, top), Math.max(MARGIN, vh - h - MARGIN));
    setPos({ left, top, origin });
  }, [at, align]);

  const positioned = pos !== null;
  useEffect(() => {
    if (positioned) ref.current?.focus({ preventScroll: true });
  }, [positioned]);

  useEffect(() => {
    const onDown = (e: PointerEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    const onBlur = () => onClose();
    const onResize = () => onClose();
    window.addEventListener("pointerdown", onDown, true);
    window.addEventListener("blur", onBlur);
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("blur", onBlur);
      window.removeEventListener("resize", onResize);
    };
  }, [onClose]);

  const activate = (i: number) => {
    const e = items[i];
    if (!e || !isItem(e) || e.disabled) return;
    if (!e.keepOpen) onClose();
    e.onSelect();
  };

  const move = (dir: 1 | -1) => {
    if (enabled.length === 0) return;
    const pos = enabled.indexOf(active);
    const next = pos < 0 ? (dir > 0 ? 0 : enabled.length - 1) : (pos + dir + enabled.length) % enabled.length;
    setActive(enabled[next]);
  };

  // The open menu owns the keyboard (capture phase), wherever focus currently is.
  const onKeyDown = (e: KeyboardEvent) => {
    e.stopPropagation();
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        move(1);
        break;
      case "ArrowUp":
        e.preventDefault();
        move(-1);
        break;
      case "Home":
        e.preventDefault();
        if (enabled.length) setActive(enabled[0]);
        break;
      case "End":
        e.preventDefault();
        if (enabled.length) setActive(enabled[enabled.length - 1]);
        break;
      case "Enter":
      case " ":
        e.preventDefault();
        if (active >= 0) activate(active);
        break;
      case "Escape":
      case "Tab":
        e.preventDefault();
        onClose();
        break;
      default:
        if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) {
          const ch = e.key.toLowerCase();
          const start = enabled.indexOf(active);
          for (let k = 1; k <= enabled.length; k++) {
            const idx = enabled[(start + k + enabled.length) % enabled.length];
            const it = items[idx];
            if (isItem(it) && it.label.toLowerCase().startsWith(ch)) {
              setActive(idx);
              break;
            }
          }
        }
    }
  };

  const keyRef = useRef(onKeyDown);
  keyRef.current = onKeyDown;
  useEffect(() => {
    const h = (e: KeyboardEvent) => keyRef.current(e);
    window.addEventListener("keydown", h, true);
    return () => window.removeEventListener("keydown", h, true);
  }, []);

  const hasChecks = items.some((e) => isItem(e) && e.checked !== undefined);
  const hasIcons = items.some((e) => isItem(e) && e.icon);

  return createPortal(
    <div
      ref={ref}
      role="menu"
      aria-label={label}
      tabIndex={-1}
      onContextMenu={(e) => e.preventDefault()}
      className="pop-in fixed z-[60] max-h-[calc(100vh-16px)] overflow-y-auto rounded-[10px] border border-line bg-elevated p-1 text-[13px] text-fg shadow-[var(--shadow-float)] outline-none scroll-quiet"
      style={{
        left: pos?.left ?? -9999,
        top: pos?.top ?? -9999,
        minWidth,
        transformOrigin: pos?.origin,
        visibility: pos ? "visible" : "hidden",
      }}
    >
      {items.map((e, i) => {
        if (e.type === "separator") return <div key={e.id} role="separator" className="mx-2 my-1 h-px bg-line" />;
        if (e.type === "heading")
          return (
            <div key={e.id} className="px-2.5 pt-1.5 pb-1 text-[11.5px] font-medium text-fg-subtle">
              {e.label}
            </div>
          );
        const isActive = i === active;
        return (
          <div
            key={e.id}
            role={e.role ?? (e.checked !== undefined ? "menuitemcheckbox" : "menuitem")}
            aria-checked={e.checked !== undefined ? e.checked : undefined}
            aria-disabled={e.disabled || undefined}
            data-active={isActive || undefined}
            onPointerMove={() => !e.disabled && active !== i && setActive(i)}
            onPointerLeave={() => setActive(-1)}
            onClick={() => activate(i)}
            className={[
              "flex min-h-[30px] cursor-default items-center gap-2.5 rounded-[6px] px-2.5 py-1 transition-colors duration-75",
              e.disabled ? "text-fg-subtle" : e.danger ? "text-danger" : "",
              isActive ? (e.danger ? "bg-danger-soft" : "bg-accent-soft") : "",
            ].join(" ")}
          >
            {hasChecks && (
              <span className="flex w-4 shrink-0 justify-center text-accent">
                {e.checked ? <Check size={14} strokeWidth={2.4} /> : null}
              </span>
            )}
            {hasIcons && <span className={`flex w-4 shrink-0 justify-center ${e.disabled ? "opacity-50" : "text-fg-muted"}`}>{e.icon}</span>}
            <span className="min-w-0 flex-1">
              <span className="block truncate">{e.label}</span>
              {e.secondary && <span className="block truncate text-[11.5px] text-fg-subtle">{e.secondary}</span>}
            </span>
            {e.shortcut && <span className="ml-4 shrink-0 text-[11.5px] text-fg-subtle tnum">{e.shortcut}</span>}
          </div>
        );
      })}
    </div>,
    document.body,
  );
}
