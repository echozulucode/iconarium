---
type: status
updated: 2026-10-23
current_phase: "F — Release engineering"
blockers:
  - "No Windows machine in the build environment: Windows build, Office paste/drag and WebView2 behaviour unverified"
next_actions:
  - "Push the CI fix and confirm the run is green on windows-latest"
  - "Run the Release dry run workflow; install the artifact on a real machine"
  - "On Windows: `just dev`, then `just build-unsigned` and install the NSIS output"
  - "Work through docs/windows-validation.md and record results (issues 16, 17)"
  - "Release dry run → tag v0.1.0 → publish; then v0.1.1 to prove the updater end to end"
---

# Status Log

## Session: 2026-10-24: CLI and MCP server implemented

**Actions taken:**
- **Built:**
  - `iconarium-cli`: commands plus an MCP server, with 6 read-only tools and opt-in `export_icons`/`tag_icons`.
  - `iconarium-catalog`: multi-library search, tags, `iconarium.yaml`, contact sheets, export.
  - svg-core reader APIs: query-only open, stats, scored search, match descriptions, pixmap rendering, shared `paths`.
- **Installer:** bundles the CLI and adds it to PATH (`tauri.bundle.conf.json`, NSIS hooks). New `just cli`, `install-cli`, `mcp-inspect` recipes.
- **Docs:** `docs/mcp.md` (setup and reference), `docs/library-metadata.md` (format spec), README section, Windows validation §E.
- **Verified on Linux:**
  - 153/153 workspace tests; clippy `-D warnings` and fmt clean.
  - MCP integration tests on 2025-11-25 and 2026-07-28.
  - A real index of 4,187 Excalidraw icons: search ~23 ms, 12-icon contact sheet 64 ms.
  - `tauri build --no-bundle` with the bundle config (CLI staged as `externalBin`).
- **Review:** an independent code review found 8 bugs, all fixed (details in `docs/mcp-cli-plan.md`).

**Outcome:** Implementation complete except the headless `index` command and the app's "Set up MCP…" menu. **Still needed on Windows:** run `just ci`, `just build-unsigned`, and validation §E (NSIS hooks, PATH script, MCP clients). **Manual step:** add `args: --config src-tauri/tauri.bundle.conf.json` to the tauri-action step in `.github/workflows/release.yml` (workflow files are protected from remote edits).

---

## Session: 2026-10-23 (b): CLI and MCP server plan

**Actions taken:**
- Wrote `docs/mcp-cli-plan.md`. It proposes one Rust binary, `iconarium-cli`, with subcommands plus `mcp` (stdio), reading the app's index in query-only mode. Tools: search, preview (contact sheet), details, libraries, tags, folders; export and tagging are opt-in later. Phases M0–M4.
- Spiked rmcp 3.5.1: typed tools, `Json<T>` structured output, image content, resource links and annotations all work over stdio. Prototyped a 24-icon contact sheet (~600 image tokens).
- An independent review against the code corrected:
  - render stacks (16 MiB)
  - `Json<T>` content shape
  - filename-match explanation
  - library identity
  - refresh churn from thumbnail commits
  - log-prefix collision
  - updater lock on a running CLI

**Outcome:** Plan ready for Eric's review. Open questions are in §14 (tag storage, analysis of non-active libraries, scope, PATH, embeddings).

---

## Session: 2026-10-23 — Excalidraw library support (standalone converter)

**Actions taken:**
- Evaluated `.excalidrawlib` support (`docs/excalidraw-feasibility.md`). In-app support is feasible but costly, so Eric chose a standalone converter.
- Built `tools/excalidraw-to-svg` (Node, no browser). It runs Excalidraw's exporter, outlines text with Excalidraw's fonts, adds `<title>`/`<desc>` for search, names files `<index>-<name>.svg`, accepts optional `--meta` sidecar names, and skips unchanged libraries on re-runs. 19 tests.
- Added `just excalidraw` and `just test-excalidraw`. `tools/ensure-deps.mjs` now takes an optional package folder.
- Validation:
  - Public catalog (232 libraries): 4,187/4,187 items converted with 0 errors; all parse and render with resvg 0.48, within the default limits.
  - Eric's mirror (116 libraries): 1,556/1,556 items, including 51 image-based brand logos, every item named via `vision.json`. Output is in `datasets/excalidraw/` (gitignored).
- Not run: `just bench datasets/excalidraw` (Iconarium's own pipeline over the output); Windows run of the converter itself.

**Outcome:** Converter complete and verified on Linux. Next: run it on Windows and open `datasets/excalidraw` in Iconarium.

---

## Session: 2026-10-07 (c) — Signing set up; first CI run

**Phase:** F — Release engineering

**Actions taken:**
- Eric generated the updater key, committed the public key (6aff95a) and set both Actions secrets. That resolves issue 18.
- The first CI run (37620355058) failed at `cargo fmt --check`: a test was edited after the last format pass (issue 19). Fixed, and the full `just ci` gate was run locally.
- 6aff95a also reverted the `docs/plan.md` tracking sections, because Windows had the file locked and `git commit -am` picked up the stale copy. Restored them; the stale copy was moved to `docs/.plan.md.stale`.

- The second CI run failed on two clippy lints that are new in Rust 1.99 (CI used `stable`; local verification used 1.97). Fixed both and pinned the toolchain to 1.97.0 in `rust-toolchain.toml` and all workflows (issue 20). `just ci` is green on 1.97.0.

**Outcome:** Ready to re-push. Next: confirm a green CI run, then do the release dry run or the first tag.

---

## Session: 2026-10-07 (b) — Iconarium, MIT, NSIS + auto-update, project docs

**Phase:** F — Release engineering

**Actions taken:**
- Renamed the product to **Iconarium** throughout: window title, identifier `com.echozed.iconarium`, crate and binary `iconarium`, log files, docs and E2E scripts. Removed the merged agent worktrees and their branches.
- Added the MIT `LICENSE`, `.gitattributes` (LF), and license/author/repository metadata.
- Set up the release pipeline, mirroring Richochet (markdown-converter):
  - NSIS-only, per-user installer
  - updater and process plugins with their capabilities
  - `createUpdaterArtifacts`, plus an unsigned config for local builds
  - update store and the Updates section in the ⋯ menu, with a dot on the button
  - version sync script, the `just updater-key` helper, and CI/release/dry-run workflows
- Formatted the Rust code and made clippy clean with `-D warnings`. The upgrade to vitest 5 (previous session) cleared the dev-dependency audit findings.
- Set up the project-docs system: `plan.md` frontmatter and tracking sections, `status.md`, `issues.yaml`, `lessons.yaml`, `research.md`, `index.yaml`.

**Outcome:** Phase F is code-complete. Remaining steps need a human: the signing key, the GitHub secrets, and a Windows machine.

---

## Session: 2026-10-07 (a) — justfile

**Actions taken:** Added the `justfile` (`just dev` and friends), README updates, and the vitest 5 upgrade.

**Outcome:** `just dev` verified on Linux (Vite and the Rust app launch). The Windows recipes run under PowerShell 5.1 with `-ExecutionPolicy Bypass`.

---

## Session: 2026-10-06 → 07 — Plan and build (phases A–E)

**Actions taken:**
- Turned the product plan into `docs/implementation-plan.md`: closed the decisions it left open, froze the contracts, assigned work to the team.
- Phase A, scaffold and contracts: done by the lead.
- Phase B, three agents in parallel: the SVG engine, scanner/index/search, and the React UI with its mock backend.
- Phase C, Tauri integration (lead): priority queue, thumbnail/svgfile protocols, scan and reconcile, watcher, clipboard, OLE drag.
- Phase D (agent): deterministic datasets A–E and benchmarks. It found three crash-level robustness bugs, all fixed.
- Phase E: an independent review found 3 High and 4 Medium issues, all fixed. A real-app WebDriver E2E suite under Xvfb passes 11 of 11.
- Delivered to `C:\Projects\20261006-svg`. The Windows target type-checks via `-Zbuild-std`.

**Outcome:** The MVP is complete on Linux. Windows and Office validation is outstanding.
