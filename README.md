# SVG Library Browser

A fast Windows desktop app for browsing, searching, previewing, region-cropping, copying and dragging SVG assets from large folder trees (tested to 50,000 files).

Built with Rust and Tauri 2 on the backend, and React 19, TypeScript, Tailwind v4 and Zustand on the frontend. The index is stored in SQLite and rendering uses resvg.

- The product spec is `docs/plan.md`.
- The phased build plan, decisions and contracts are in `docs/implementation-plan.md`.
- Measured performance is in `docs/performance.md`.
- The manual Office and Windows checks are in `docs/windows-validation.md`.

## Build on Windows

Prerequisites:
- Rust stable (MSVC toolchain): `rustup default stable-x86_64-pc-windows-msvc`
- Visual Studio 2022 Build Tools with the "Desktop development with C++" workload
- Node.js 20 or newer
- WebView2 Runtime. Windows 11 includes it, and the installer embeds a bootstrapper.
- [just](https://just.systems): `winget install Casey.Just` (or `cargo install just`)

```powershell
just dev        # dev mode: Vite hot reload + Rust backend (installs npm deps on first run)
just build      # release build + NSIS/MSI installers in target/release/bundle/
just            # list every recipe (test, check, datasets, bench, dev-ui, …)
```

Without just: `npm install`, then `npm run tauri dev` / `npm run tauri build`.

The app runs fully offline. The index and thumbnail cache live in `%LOCALAPPDATA%\com.svglibrary.browser\`. Logs go to `%LOCALAPPDATA%\com.svglibrary.browser\logs\`, with one rolling file per day.

## Repository layout

```text
crates/svg-core/      headless engine (no Tauri): scanner, SQLite index, search, SVG analysis/render/crop
  examples/           gen_dataset (deterministic test libraries), bench_pipeline, bench_search
  tests/              svg engine, index, scanner, search, pathological inputs
src-tauri/            app shell: commands, priority work queue, thumb:// + svgfile:// protocols,
                      watcher, clipboard (Windows: clipboard-win), native drag (OLE, copy-only)
src/                  React UI: virtualized gallery, search, multi-select, viewer, region selection, minimap
  api/                Backend interface → tauri.ts (real) and mock.ts (in-browser demo backend)
scripts/              gen-datasets.sh, screenshots/smoke (mock UI), e2e/ (real app via WebDriver)
docs/                 plan, implementation plan, performance, Windows validation, screenshots
```

## Develop and test

```sh
just test              # all Rust + frontend tests (core includes pathological inputs)
just check             # tsc + clippy
just dev-ui            # UI in a plain browser against the mock backend (no Rust build)
just datasets          # deterministic test libraries A–E into ./datasets
just bench datasets/C  # core pipeline benchmark on the 50k-file library
```

`scripts/e2e/README.md` covers end-to-end tests of the real app on Linux, using Xvfb and tauri-driver.

## Key behaviors

- The UI never waits for the filesystem. Cached libraries appear right away while changes are reconciled in the background. Discovery streams in batches. Analysis runs at a lower priority than visible work, and background thumbnails run lower still.
- Search covers filename, path, `<title>`, `<desc>`, `<text>`, IDs and classes.
  - `ethernet switch` (AND)
  - `"motor controller"` (phrase)
  - `-legacy` (exclude)
  - `network/*switch*.svg` (glob)
  - `re:^valve-\d+` (regex)
- Region selections are stored in SVG user units. Crops get a canvas that starts at `0,0` and keep text, defs and gradients.
- Copy and export keep transparency unless you choose "with White Background". Ctrl+C depends on context: the selected region, otherwise the whole SVG, or the file paths when several gallery items are selected.
- Files over the complexity limits stay listed and searchable but aren't rendered. The limits are file size, node count, nesting depth, embedded images and text volume. Adjust them in settings (`limits`).
