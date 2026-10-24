# excalidraw-to-svg

Converts Excalidraw libraries (`.excalidrawlib`) and scenes (`.excalidraw`) into folders of
standalone SVG files. You can then browse, search and copy them in Iconarium like any other
SVG library, or use them anywhere else.

```powershell
just excalidraw C:\Users\me\Downloads\excalidraw-libraries --out datasets\excalidraw
```

Without just: `npm ci` in this folder, then `node tools/excalidraw-to-svg/cli.mjs <inputs> --out <folder>`.

## What you get

```text
datasets/excalidraw/
  gcp/gcp-icons/000-google-cloud-platform-logo.svg
  gcp/gcp-icons/001-layered-stacked-steps-icon.svg
  network/network-elements/000-router.svg
  ...
  .excalidraw-to-svg.json          manifest for incremental runs (hidden)
```

- **One folder per library.** The folder layout under each input folder is mirrored.
- **Files are named `<index>-<item-name>.svg`.** The index is the item's 0-based position in the library, matching Excalidraw catalogues (`#0`) and sidecar keys. An unnamed item takes its label from `--meta`, then from its first text element, and otherwise gets the bare index.
- **Each file has a `<title>` and `<desc>`** holding the item name, source library, keywords and visible text, so Iconarium's search finds items by what they say even after text becomes outlines.
- **Rendering is Excalidraw's own exporter** (`@excalidraw/utils`), so hand-drawn strokes, hachure fills, arrows, frames and embedded images look exactly as they do in Excalidraw. Output is deterministic.

## Options

| Option | Default | Meaning |
|---|---|---|
| `-o, --out <folder>` | required | Output folder |
| `--text outline` | ✓ | Text becomes paths, using Excalidraw's fonts. Renders identically in Iconarium, Office, Figma and browsers, and needs no fonts installed |
| `--text live` | | Text stays text, with font subsets embedded the way Excalidraw's own export does. Browsers render it exactly; Office, Inkscape and Figma ignore embedded fonts and substitute installed ones |
| `--theme light\|dark` | light | Excalidraw's dark-mode colour transform |
| `--padding <px>` | 10 | Space around each item |
| `--background <css color>` | transparent | e.g. `white` |
| `--hide-frames` | off | Skip frame outlines and labels |
| `--scenes` | off | Also convert `.excalidraw` files found in folders. Scene files named on the command line always convert, to `<name>.svg` |
| `--meta <file.json>` | | Names, summaries and keywords for items (see below) |
| `--force` | | Reconvert everything |
| `--dry-run` | | Show what would change; write nothing |
| `-q, --quiet` | | Print only errors and the summary |

The exit code is 0 on success, 1 if any library or item failed (the rest still convert), and 2 for usage errors.

## Incremental runs

Re-running is cheap. A library is reconverted only when its bytes, the options, the tool
version or its sidecar metadata change. When an item is renamed or removed, its old file is
deleted. When a library file disappears from an input folder, its output folder is removed.
Only files listed in the manifest are ever deleted, so anything else you put in the output
folder is left alone.

## Sidecar metadata (`--meta`)

Many published library items have no names. A JSON object can supply them:

```json
{
  "gcp__gcp-icons__3": { "summary": "Bank building with security shield", "kw": ["bank", "security"] },
  "gcp-icons__4":      { "name": "Password shield", "keywords": "security, password" }
}
```

Keys are `<folder>__<sub-folder>__<library>__<index>` (the path relative to the input
folder) or just `<library>__<index>`. Values may have `name`, `summary` (or
`description`), and `kw`/`keywords`/`tags`. The name, or failing that the summary, becomes
the file name and `<title>`; keywords go into `<desc>`. The `tools/vision.json` in the
andreasbros/excalidraw library mirror uses this format.

## How it works and its limits

- It runs in plain Node, with no browser. `exportScene` from `@excalidraw/utils` draws into
  a [linkedom](https://github.com/WebReflection/linkedom) DOM, and
  [resvg-js](https://github.com/yisibl/resvg-js) turns text into paths.
- Fonts are the TTFs inside `@excalidraw/utils`, loaded in place. Nothing is copied or
  modified. Excalidraw's family names are mapped to the TTFs' own names
  (`lib/fonts.mjs`, guarded by `test/fonts.test.mjs`).
- System fonts are loaded only for characters the Excalidraw fonts lack, such as emoji or
  Hebrew/Arabic/Thai text. CJK uses Excalifont's own glyphs only when the item uses
  Excalifont; otherwise it uses system fonts.
- Cosmetic differences from Excalidraw in the browser: emoji come out monochrome, and
  programming ligatures such as `<-` drawn as an arrow aren't applied.
- In outline mode, files are larger than Excalidraw's own export when they carry a lot of
  text (about 2× typical, more for text-heavy canvases), and text can't be edited after
  pasting. Use `--text live` when that matters.
- `@excalidraw/utils` is pinned exactly (`0.1.7`). Its export API changed between 0.1.5
  and 0.1.6, so upgrade deliberately and re-run the tests.

## Tests

```powershell
just test-excalidraw          # or: npm test (in this folder)
```
