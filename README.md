# Iconarium

A fast Windows desktop app for browsing, searching, previewing, region-cropping, copying and dragging SVG assets from large folder trees (tested to 50,000 files).

Built with Rust and Tauri 2 on the backend, and React 19, TypeScript, Tailwind v4 and Zustand on the frontend. The index is stored in SQLite and rendering uses resvg.

- The product spec, phase status and decisions are in `docs/plan.md`; the session log is `docs/status.md`.
- Open issues and lessons learned are in `docs/issues.yaml` and `docs/lessons.yaml` (`docs/index.yaml` lists every doc).
- The build plan, contracts and deviations are in `docs/implementation-plan.md`.
- Measured performance is in `docs/performance.md`; manual Office and Windows checks are in `docs/windows-validation.md`.

## Install

Download `Iconarium_<version>_x64-setup.exe` from [Releases](https://github.com/echozulucode/iconarium/releases/latest) and run it. It installs for the current user into `%LOCALAPPDATA%\Iconarium`, so there is no administrator prompt. Installed copies check for updates on launch; when one is available a dot appears on the ⋯ settings button, and the Updates section of that menu downloads it and restarts into the new version.

Windows SmartScreen warns on first run because the installer isn't Authenticode-signed: choose **More info → Run anyway**.

## Build on Windows

Prerequisites:
- Rust stable (MSVC toolchain): `rustup default stable-x86_64-pc-windows-msvc`
- Visual Studio 2022 Build Tools with the "Desktop development with C++" workload
- Node.js 20 or newer
- WebView2 Runtime. Windows 11 includes it, and the installer embeds a bootstrapper.
- [just](https://just.systems): `winget install Casey.Just` (or `cargo install just`)

```powershell
just dev             # dev mode: Vite hot reload + Rust backend (installs npm deps on first run)
just build-unsigned  # NSIS installer for local testing (no updater signature)
just build           # signed release installer — needs the updater key (see Releasing)
just                 # list every recipe (test, check, ci, datasets, bench, dev-ui, …)
```

Without just: `npm install`, then `npm run tauri dev` / `npm run tauri build`.

The app works fully offline (the launch update check fails silently without a network). The index and thumbnail cache live in `%LOCALAPPDATA%\com.echozed.iconarium\`. Logs go to `%LOCALAPPDATA%\com.echozed.iconarium\logs\`, with one rolling file per day.

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

## Releasing

Releases follow a tag-driven flow: GitHub Actions builds the NSIS installer, signs it for the updater, and attaches the installer, its `.sig` and `latest.json` to a **draft** GitHub Release. Installed copies poll `releases/latest/download/latest.json`, which ignores drafts, so nobody is offered an update until you publish.

### One-time setup

1. Generate the updater signing key. It goes in `~/.tauri/`, outside the repo, and its public key is written into `src-tauri/tauri.conf.json` for you:

   ```powershell
   just updater-key      # prompts for a passphrase
   git commit -am "Set updater public key"
   ```

2. In GitHub → Settings → Secrets and variables → Actions, add:
   - `TAURI_SIGNING_PRIVATE_KEY`: the **contents** of `~/.tauri/iconarium-updater.key`
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: its passphrase

3. Back up the key and passphrase like a password. Losing them strands every installed copy on its current version.

4. Optional: run the **Release dry run** workflow (Actions → Release dry run → Run workflow), then install the artifact on a real machine before the first tag.

Local signed builds need the same two variables:

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = "$HOME\.tauri\iconarium-updater.key"   # a path is accepted
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = "<passphrase>"
just build
```

### Each release

```powershell
just ci                  # green locally first
just bump patch --tag    # one version in package.json, Cargo.toml and tauri.conf.json; commit + tag
git push; git push origin v0.1.1
# Actions builds → a draft release appears. Install it over an existing copy and smoke-test.
# Edit the notes, then Publish — that is the moment clients see the update.
```

Bump forward and never re-tag a published version: clients that already read `latest.json` would see different bytes under the same version.

## License

[MIT](LICENSE) © 2026 Eric Zimmerman

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
