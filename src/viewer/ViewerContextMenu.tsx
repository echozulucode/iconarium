import { Copy, Download, ExternalLink, Eraser, FileCode2, FolderSearch, GripVertical, Image, Info, Link2, Maximize, Scan, SquareDashed } from "lucide-react";
import type { Region } from "../api/types";
import { copyAssets, copyRegion, openExternal, revealAssets, saveRegion } from "../app/commands";
import { modKey } from "../lib/format";
import { Menu, type MenuEntry } from "../components/Menu";
import { useUi } from "../stores/uiStore";
import { useViewer } from "../stores/viewerStore";

const WhiteSwatch = () => <span className="block h-3 w-3 rounded-[3px] border border-line-strong bg-white" />;

export function regionMenuItems(id: number, region: Region): MenuEntry[] {
  return [
    { id: "svg", label: "Copy Selection as SVG", icon: <FileCode2 size={15} />, shortcut: `${modKey}+C`, onSelect: () => void copyRegion(id, region, "svg") },
    { id: "png", label: "Copy Selection as PNG", icon: <Image size={15} />, onSelect: () => void copyRegion(id, region, "png") },
    { id: "png2x", label: "Copy PNG 2×", icon: <Image size={15} />, onSelect: () => void copyRegion(id, region, "png2x") },
    { id: "pngw", label: "Copy PNG with White Background", icon: <WhiteSwatch />, onSelect: () => void copyRegion(id, region, "png_white") },
    { type: "separator", id: "s1" },
    { id: "savesvg", label: "Save Selection as SVG…", icon: <Download size={15} />, onSelect: () => void saveRegion(id, region, "svg") },
    { id: "savepng", label: "Save Selection as PNG…", icon: <Download size={15} />, onSelect: () => void saveRegion(id, region, "png") },
    { type: "separator", id: "s2" },
    {
      id: "drag",
      label: "Drag Selection",
      secondary: "Use the grip beside the selection",
      icon: <GripVertical size={15} />,
      disabled: true,
      onSelect: () => {},
    },
    { id: "clear", label: "Clear Selection", icon: <Eraser size={15} />, shortcut: "Esc", onSelect: () => useViewer.getState().setRegion(null) },
  ];
}

export function documentMenuItems(id: number, actions: { fit(): void; actual(): void; selectAll(): void }): MenuEntry[] {
  return [
    { id: "svg", label: "Copy SVG", icon: <FileCode2 size={15} />, shortcut: `${modKey}+C`, onSelect: () => void copyAssets([id], "svg") },
    { id: "png", label: "Copy PNG", icon: <Image size={15} />, onSelect: () => void copyAssets([id], "png") },
    { id: "pngw", label: "Copy PNG with White Background", icon: <WhiteSwatch />, onSelect: () => void copyAssets([id], "png_white") },
    { id: "path", label: "Copy Full Path", icon: <Link2 size={15} />, shortcut: `${modKey}+Shift+C`, onSelect: () => void copyAssets([id], "path") },
    { id: "name", label: "Copy Filename", icon: <Copy size={15} />, onSelect: () => void copyAssets([id], "filename") },
    { type: "separator", id: "s1" },
    {
      id: "select",
      label: "Select Region",
      icon: <SquareDashed size={15} />,
      shortcut: "S",
      onSelect: () => useViewer.getState().setMode("select"),
    },
    { id: "selectall", label: "Select Entire Document", icon: <Scan size={15} />, shortcut: `${modKey}+A`, onSelect: actions.selectAll },
    { id: "fit", label: "Fit to Window", icon: <Maximize size={15} />, shortcut: "F", onSelect: actions.fit },
    { id: "actual", label: "Actual Size", icon: <span className="text-[10px] font-bold">1:1</span>, shortcut: "0", onSelect: actions.actual },
    { type: "separator", id: "s2" },
    { id: "reveal", label: "Reveal in Explorer", icon: <FolderSearch size={15} />, onSelect: () => void revealAssets([id]) },
    { id: "open", label: "Open Externally", icon: <ExternalLink size={15} />, onSelect: () => void openExternal(id) },
    { type: "separator", id: "s3" },
    { id: "props", label: "Properties", icon: <Info size={15} />, onSelect: () => useUi.getState().showProperties(id) },
  ];
}

export function ViewerContextMenu({ at, items, onClose }: { at: { x: number; y: number }; items: MenuEntry[]; onClose(): void }) {
  return <Menu at={at} items={items} onClose={onClose} minWidth={252} label="Viewer actions" />;
}
