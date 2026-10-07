import { Copy, ExternalLink, Eye, FileCode2, FileText, FolderSearch, Hand, Image, Info, Link2, X } from "lucide-react";
import { useMemo } from "react";
import { copyAssets, openExternal, openViewer, revealAssets } from "../app/commands";
import { formatCount, modKey } from "../lib/format";
import { useSelection } from "../stores/selectionStore";
import { summaryCache } from "../stores/summaryCache";
import { useUi } from "../stores/uiStore";
import { isPreviewable } from "./AssetCard";
import { Menu, type MenuEntry } from "./Menu";

export function AssetContextMenu() {
  const menu = useUi((s) => s.menu);
  const close = useUi((s) => s.closeMenu);
  const at = useMemo(() => (menu ? { x: menu.x, y: menu.y } : null), [menu]);
  if (!menu || !at) return null;
  return <Menu at={at} items={buildItems(menu.ids, menu.primary)} onClose={close} minWidth={244} label="Asset actions" />;
}

export function buildItems(ids: number[], primary: number): MenuEntry[] {
  if (ids.length > 1) {
    return [
      { type: "heading", id: "h", label: `${formatCount(ids.length)} items selected` },
      {
        id: "drag",
        label: "Drag Selected",
        secondary: "Drag any selected card into another app",
        icon: <Hand size={15} />,
        disabled: true,
        onSelect: () => {},
      },
      { type: "separator", id: "s0" },
      { id: "paths", label: "Copy Paths", icon: <Link2 size={15} />, shortcut: `${modKey}+C`, onSelect: () => void copyAssets(ids, "path") },
      { id: "names", label: "Copy Filenames", icon: <FileText size={15} />, onSelect: () => void copyAssets(ids, "filename") },
      { type: "separator", id: "s1" },
      { id: "reveal", label: "Reveal in Explorer", icon: <FolderSearch size={15} />, onSelect: () => void revealAssets(ids) },
      { type: "separator", id: "s2" },
      { id: "clear", label: "Clear Selection", icon: <X size={15} />, shortcut: "Esc", onSelect: () => useSelection.getState().clear() },
    ];
  }

  const id = ids[0] ?? primary;
  const s = summaryCache.get(id);
  const previewable = isPreviewable(s);
  return [
    { id: "view", label: "View", icon: <Eye size={15} />, shortcut: "Enter", disabled: !previewable, onSelect: () => openViewer(id) },
    { type: "separator", id: "s0" },
    { id: "svg", label: "Copy SVG", icon: <FileCode2 size={15} />, shortcut: `${modKey}+C`, disabled: !previewable, onSelect: () => void copyAssets([id], "svg") },
    { id: "png", label: "Copy PNG", icon: <Image size={15} />, disabled: !previewable, onSelect: () => void copyAssets([id], "png") },
    {
      id: "pngw",
      label: "Copy PNG with White Background",
      icon: <span className="block h-3 w-3 rounded-[3px] border border-line-strong bg-white" />,
      disabled: !previewable,
      onSelect: () => void copyAssets([id], "png_white"),
    },
    { id: "path", label: "Copy Full Path", icon: <Link2 size={15} />, shortcut: `${modKey}+Shift+C`, onSelect: () => void copyAssets([id], "path") },
    { id: "name", label: "Copy Filename", icon: <Copy size={15} />, onSelect: () => void copyAssets([id], "filename") },
    { type: "separator", id: "s1" },
    { id: "reveal", label: "Reveal in Explorer", icon: <FolderSearch size={15} />, onSelect: () => void revealAssets([id]) },
    { id: "open", label: "Open Externally", icon: <ExternalLink size={15} />, onSelect: () => void openExternal(id) },
    { type: "separator", id: "s2" },
    { id: "props", label: "Properties", icon: <Info size={15} />, shortcut: "Alt+Enter", onSelect: () => useUi.getState().showProperties(id) },
  ];
}
