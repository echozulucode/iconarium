# Windows and Office Validation Checklist

These items can only be validated on Windows, with Office, Visio and draw.io installed. They correspond to the plan.md Phase 0 spikes (§5.1–5.3) and to WP-12, WP-13 and WP-17. Everything else (core, UI, integration) is covered by automated tests.

What has been verified so far:
- The full workspace, including all Windows-only code, type-checks for `x86_64-pc-windows-gnu`.
- The app runs end to end on Linux with WebKitGTK.

What still needs checking on Windows:
- WebView2 behavior
- OLE drag
- Office paste behavior

Record results in the "Result" column (✅ / ⚠️ / ❌ plus a note).

## Setup

1. `npm run tauri build`, then install from `target\release\bundle\nsis\`.
2. Open a library. A generated one works: `scripts/gen-datasets.sh A D` (in Git Bash or WSL), then open `datasets\A` and `datasets\D`.
3. Optional: InsideClipboard (NirSoft) or a similar tool to inspect clipboard formats.

## A. Clipboard (plan §2.1, §2.4, §20)

The expected clipboard formats after **Copy SVG**, in this order:
1. `image/svg+xml`
2. `PNG`
3. `CF_BITMAP` (synthesized as CF_DIB/CF_DIBV5)
4. `CF_UNICODETEXT`, only if `clipboardIncludeSvgText` is on

| # | Action | Paste into | Expected | Result |
|---|---|---|---|---|
| A1 | Gallery → Copy SVG (icon) | PowerPoint 365 | Inserted as an **SVG graphic**: Graphics Format tab, "Convert to Shape" available, transparent background | |
| A2 | Same | Word 365 | SVG graphic | |
| A3 | Same | Visio | Vector, not a bitmap | |
| A4 | Same | draw.io desktop / browser | Image or shape is inserted | |
| A5 | Same | Paint | Bitmap on a white background | |
| A6 | Inspect formats | InsideClipboard | `image/svg+xml`, `PNG`, `CF_BITMAP`/`CF_DIB` present | |
| A7 | Copy PNG / Copy PNG with White Background | PowerPoint, Paint | Transparent / white background; small icons are upscaled so the longest side is at least 256 px | |
| A8 | Ctrl+Shift+C / Copy Filename | Notepad | Absolute path / filename; CRLF between multiple items | |
| A9 | Multi-select → Ctrl+C | Explorer folder (Ctrl+V) and Notepad | Files are copied into the folder; Notepad gets the paths | |

If PowerPoint picks the bitmap instead of the SVG, try these settings one at a time:
- `clipboardIncludeBitmapWithSvg: false`
- `clipboardIncludeSvgText: false`. This is the default; turning it on can make some apps paste text.

Settings live in the `settings` table (JSON) and are exposed through `update_settings`. Record which combination works.

## B. Region copy (plan §5.3, §25–§27)

| # | Action | Expected | Result |
|---|---|---|---|
| B1 | Viewer → S → drag a region on a diagram → Ctrl+C → PowerPoint | Only the region is pasted, as a vector, with transparency kept, and no content from outside the region | |
| B2 | In PowerPoint, ungroup / Convert to Shape | Text is still text (editable), not outlines | |
| B3 | Copy Selection as PNG / PNG 2× / PNG with White Background | Correct pixel size (1×, 2×); transparency or white as chosen | |
| B4 | Save Selection as SVG / PNG | Save dialog suggests `<name>-crop.svg`; the file opens correctly in a browser; the source file is unchanged | |
| B5 | Zoom or pan after selecting, then copy | The same region is copied, because selection is held in SVG coordinates | |
| B6 | Change the display scale (100% ↔ 150%) with the viewer open | The region stays on the same content | |

## C. Native drag-out (plan §5.1, §19, §28)

| # | Action | Expected | Result |
|---|---|---|---|
| C1 | Drag one card → PowerPoint | Inserted as an SVG graphic (vector) | |
| C2 | Drag one card → Visio, draw.io desktop, draw.io in a browser | Inserted | |
| C3 | Drag one card → Explorer folder | **Copy** (the "+ Copy" cursor); the source file is not moved | |
| C4 | Select 3 → drag any selected card → PowerPoint / Explorer | All 3 arrive | |
| C5 | Drag with Shift held → Explorer | Still a copy, never a move | |
| C6 | Viewer → region → drag the grip handle → PowerPoint | Cropped SVG is inserted; temp file named `<name>-crop.svg` | |
| C7 | Restart the app | `%LOCALAPPDATA%\com.echozed.iconarium\tmp` is emptied | |

## D. WebView2 and general behavior

| # | Check | Expected | Result |
|---|---|---|---|
| D1 | Cold start with a 50k library already indexed | Gallery visible in under 1 s; status shows "Checking for changes…" then "Indexed" | |
| D2 | Fast scrolling through 50k items | About 60 FPS; thumbnails fill in shortly after scrolling stops; no blank grid | |
| D3 | Thumbnail URLs | `http://thumb.localhost/...` loads, with no CSP errors (DevTools → Console) | |
| D4 | Library on a network share (UNC path) | Opens; scanning is progressive; UI stays responsive | |
| D5 | `git checkout` of another branch inside the library | One reconcile pass; gallery updates within a few seconds | |
| D6 | Delete a folder (including one with a dot in its name) | Its assets disappear from the gallery | |
| D7 | Open `C:\Lib` and later `c:\lib` | Treated as the same library, with no re-index | |
| D8 | Reveal in Explorer with multiple files in one folder | One Explorer window with all of them selected | |
| D9 | High-DPI display (150–200%) | Crisp thumbnails and viewer; selection handles are aligned | |

## E. CLI and MCP server (docs/mcp.md)

Build the installer with `just build-unsigned` (it bundles `iconarium-cli.exe`), install it, and open at least two libraries in the app first.

| # | Check | Expected | Result |
|---|---|---|---|
| E1 | Fresh install, then a **new** PowerShell: `iconarium-cli doctor` | `iconarium-cli.exe` sits next to `Iconarium.exe` in `%LOCALAPPDATA%\Iconarium`; doctor reports the index and your libraries | |
| E2 | PATH is intact: `reg query HKCU\Environment /v Path` before and after install | Same entries plus the Iconarium folder; still `REG_EXPAND_SZ`; entries like `%USERPROFILE%\...` unexpanded. Reinstalling doesn't duplicate the entry | |
| E3 | `iconarium-cli search router`, `--json search router`, `preview <ids> -o s.png`, `export <id> --to out --png` | Results; JSON parses; sheet shows icons with labels (Segoe UI); PNG written | |
| E4 | Claude Desktop config (docs/mcp.md), restart, ask "find me a router icon and show me options" | Tools listed under connectors; search, then a preview image appears in the chat; no errors in `%APPDATA%\Claude\logs\mcp-server-iconarium.log` | |
| E5 | `claude mcp add --transport stdio --scope user iconarium -- "$env:LOCALAPPDATA\Iconarium\iconarium-cli.exe" mcp`, then `/mcp` in Claude Code | `iconarium` connected; tools callable | |
| E6 | With `--allow-write C:\temp\icons --allow-tagging`: export and tag | Files written only under `C:\temp\icons`; `iconarium.yaml` appears in the library root; tag search finds the icon | |
| E7 | Keep Claude Desktop running (server alive), then install a newer build over it | Installer completes (CLI process ended by the hook); server reconnects on next use | |
| E8 | App indexing a large library while the MCP server is used | No errors or slowdowns; newly analyzed files show up in search within a few seconds | |
| E9 | Uninstall | `iconarium-cli.exe` gone; PATH entry removed; other PATH entries untouched | |
| E10 | Library path with spaces and non-ASCII (e.g. `C:\Users\me\Desktop\Ícones Ünicode`) | Search, preview and export work; absPath correct | |
| E11 | Machine where PowerShell scripts are blocked by policy (if available) | Install still succeeds; the installer's details show "could not update PATH"; `iconarium-cli` works by full path | |
