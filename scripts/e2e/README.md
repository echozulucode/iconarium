# End-to-end tests (real app, Linux)

`run.mjs` launches the **real** Iconarium binary (Rust backend + WebKitGTK WebView)
through [`tauri-driver`](https://v2.tauri.app/develop/tests/webdriver/) and drives it with
WebdriverIO over W3C WebDriver. Windows (WebView2) is the product target, but WebDriver for
Tauri only works on Linux/WebKitGTK, so this suite runs on Linux under a virtual X display.

## One-time setup

```sh
sudo apt-get install -y xvfb webkit2gtk-driver xclip   # WebKitWebDriver + clipboard probe
cargo install tauri-driver --locked
npm install                                           # webdriverio is a devDependency
```

## Build the app with the frontend embedded

```sh
npm run build
cargo build --release -p iconarium --features tauri/custom-protocol
# (or: cargo build -p iconarium --features tauri/custom-protocol  → target/debug)
```

Without `tauri/custom-protocol` the binary tries to load the Vite dev server instead of `dist/`.
Rebuild the binary after every `npm run build` — the frontend is compiled into it.

## Run

```sh
xvfb-run -a -s "-screen 0 1280x900x24" node scripts/e2e/run.mjs
```

Options: `--bin <path>` (default `target/release/…`, else `target/debug/…`), `--home <dir>`
(isolated HOME, default `/tmp/e2e-home`, wiped each run), `--work <dir>` (scratch copy of
dataset A, default `/tmp/e2e-work`), `--skip 9` (skip the 50k dataset), `--only 1,2,5`
(no automatic dependencies: 3a–7 expect dataset A to be open, so include 2), `--drag`
(also call `start_drag_assets`), `--json <file>` (machine-readable results).

Exit code is non-zero if any scenario fails. Screenshots go to `docs/screenshots/e2e-*.png`
(`e2e-fail-<id>.png` on failure). The app's own log is under
`$HOME/.local/share/com.echozed.iconarium/logs/` of the isolated HOME.

The datasets must exist (`scripts/gen-datasets.sh`, see `crates/svg-core/examples/gen_dataset.rs`).

## Scenarios

| id | what is verified |
|---|---|
| 1 | cold start with an empty HOME shows the "Open an SVG library" empty state |
| 2 | `open_library` on a scratch copy of dataset A: cards appear progressively, visible `<img>` thumbnails load (`naturalWidth > 0`), status reaches **Indexed**, footer count = files on disk; times to first card / first thumbnail / Indexed |
| 3a | filename token, path glob, `re:` regex (UI count = backend count), bad regex shows the inline error (no toast); probe for the known anchored-regex core issue |
| 3b | dataset D content search `"motor controller"` during indexing shows match-explanation lines quoting the text |
| 4 | click / Shift+click / Ctrl+click / Ctrl+A / Esc with status-bar count and `aria-selected`; context menu via Shift+F10 |
| 5 | viewer via Enter and double-click; zoom in, Fit; Select-mode drag creates a region; viewer Ctrl+C puts `image/svg+xml` + `PNG` on the X11 clipboard with `viewBox="0 0 w h"`; `copy_region` png2x size; gallery Ctrl+C = exact source bytes; Ctrl+Shift+C = absolute path |
| 6 | restart with the same HOME restores the library, cached gallery visible from the page's navigation start, catalog loaded from the index (log), Indexed |
| 7 | watcher on the scratch library: add 3, modify 1 (fingerprint changes), delete 1, burst of 600 into an existing folder (one full reconcile); totals/search update within seconds |
| 8 | dataset E: no crash, every MANIFEST row lands in the expected state, limit/parse-error cards render, over-limit asset cannot be opened in the viewer (View disabled, Enter ignored), its `svgfile://` URL errors cleanly, copy SVG is refused with a message, Copy Path still works |
| 9 | dataset C (50k): first card/thumbnail, search latency while indexing, all files discovered, scroll to 25/50/90/10 % with time until every visible thumbnail is loaded, main-thread timer gaps |
| 10 | (`--drag`) `start_drag_assets` returns without hanging on Linux |

## Harness notes / limitations

* Native folder picker and save dialogs can't be automated: libraries are opened with
  `window.__TAURI_INTERNALS__.invoke('open_library', { path })`. The UI follows via the
  `catalog://changed {reason: "load"}` event.
* Search text is set through the input's value setter + `input` event (React's onChange path):
  WebKitWebDriver key synthesis under Xvfb drops Shift for some symbols (`:` → `;`, `^` → `6`).
* WebKitWebDriver loses the release of a right-button click; subsequent left drags then arrive
  as chorded `pointermove`s without `pointerdown`. Scenario 4 therefore opens the context menu
  with Shift+F10, and the one real right-click happens in scenario 8, after all drags.
* WebKitGTK screenshots under Xvfb can lag the DOM by a few frames; `shot()` waits 600 ms.
* Drag-and-drop into Office/Explorer and Office clipboard paste are Windows-only validations
  (see the Windows validation checklist); this suite only checks the X11 clipboard targets.
* Timings depend heavily on the machine (the reference run used 2 cores, software rendering,
  one preview worker). Treat them as regressions signals, not product numbers.
