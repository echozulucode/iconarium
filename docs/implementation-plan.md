# SVG Library Browser — Phased Implementation Plan

Source: `docs/plan.md` (product plan). This document turns that plan into an executable build sequence, fixes the technical decisions it left open, defines the contracts between subsystems, and records how the work is split across a team of parallel agents.

Status legend: ☐ planned · ◐ in progress · ☑ done · ⚠ needs Windows/Office manual validation

---

## 1. Ground rules carried from the product plan

| Rule | How it is enforced in the build |
|---|---|
| UI never waits for the filesystem | Scanner, parser, hasher, renderer all run on Rust worker threads; the UI only receives batches/events and pulls pages of data. |
| Rust owns traversal/parsing/hashing/search/rendering | React never walks directories or parses SVG. React displays SVG only through `<img>` (no script execution) served by Rust URI protocols. |
| Everything expensive is incremental, bounded, prioritized, cached | Single priority work queue (P0–P3), bounded worker pool, persistent SQLite index, on-disk thumbnail cache keyed by fingerprint + renderer version + size. |
| Limits are configuration, not constants | `Limits` struct in `svg-core::config`, loaded from `settings.json`, passed to every stage. |
| Selections are in SVG user units | Viewer converts pointer → SVG coordinates via the document `viewBox`; crop/export consumes only SVG units. |
| Copy/export preserves transparency | White background only via explicit "with White Background" commands. Viewer background is presentation-only. |

---

## 2. Technical decisions (open questions in plan.md, now closed)

| Area | Decision | Rationale |
|---|---|---|
| Rust layout | Cargo workspace: `crates/svg-core` (pure library, no Tauri) + `src-tauri` (thin app shell) | Core logic is unit-testable and benchmarkable headless; Tauri layer stays small. Module tree mirrors plan §38. |
| Tauri version | Tauri **2.x** stable (not 3.0-alpha) | Production stability; all needed plugins exist for v2. |
| SVG parsing / extraction | `roxmltree` (with node limit) | Fast, read-only, gives byte ranges (needed for text-preserving crop). |
| Rendering | `resvg`/`usvg`/`tiny-skia`, single shared `fontdb` loaded once | Pure Rust, deterministic, no WebView involvement. |
| Region crop (SVG) | **Text-preserving wrap**, not re-serialization: new root `<svg width/height viewBox="0 0 w h">` → `<clipPath>` rect → `<g transform="translate(-x,-y)">` → original root children copied byte-for-byte; prolog/DOCTYPE kept; original root attributes (namespaces, style, fonts) carried over except geometry. Output physical size scales with the source's unit-per-px ratio. | Preserves text, gradients, patterns, symbols, defs, clip paths, CSS. usvg re-serialization would outline text. Explicit clip guarantees "no outside content" even in receivers that ignore viewport overflow. |
| Region crop (PNG) | Render the cropped SVG with resvg at 1x/2x; transparent unless white requested | One code path → SVG and PNG are pixel-consistent. |
| Hashing | BLAKE3 computed when file bytes are already read for metadata (background), never during discovery | Discovery uses path/size/mtime only. |
| Fast fingerprint | `blake3(relative_path ‖ size ‖ mtime_ns)` truncated to 16 hex | Cheap change detection + thumbnail cache key. |
| Database | `rusqlite` (bundled SQLite), WAL mode, versioned migrations | Plan §10 tables: `libraries, assets, svg_metadata, svg_text, thumbnails, settings`. |
| Search engine | In-memory catalog (loaded from SQLite at startup) with pre-lowercased fields; token-AND, glob (`globset`), `re:` regex; weighted ranking per plan §17 | 50k × substring scans run in single-digit ms; avoids FTS tokenizer mismatches for paths/IDs. FTS5 kept as a later option. |
| Result transport | `search` returns the full ordered ID list (binary `u32` LE via `tauri::ipc::Response`); UI fetches summaries only for visible IDs (`get_assets`) | 50k IDs ≈ 200 KB; selection/range logic stays trivial in the UI. |
| Thumbnails to UI | Custom async URI protocol `thumb://localhost/{id}?v={fingerprint}`; cache hit → file bytes; miss → enqueue at P0 and respond when rendered | Only mounted `<img>` elements request thumbnails, so the browser's own lazy loading is the P0 signal. |
| Viewer source | Protocol `svgfile://localhost/{id}` serving the raw SVG (rejected above hard limit) rendered via `<img>` | Scripts in SVG never execute; WebView rasterizes vectors crisply at zoom. |
| Prioritization | `set_viewport(visible_ids, nearby_ids)` command reprioritizes queued work (P0 visible, P1 near, P2 current results, P3 rest) | Plan §9 priority classes. |
| Filesystem watcher | `notify` 8 + `notify-debouncer-full` (500 ms); ≤200 changed paths → per-path reconcile, more → full reconcile scan | Coalesces git-checkout bursts into one pass (plan §31). |
| Clipboard | `clipboard-rs`: bitmap (CF_DIB) + registered `PNG` + registered `image/svg+xml`; optional `text/plain` SVG markup behind a setting | Office reads `image/svg+xml`; `PNG` keeps alpha; DIB for Paint. Text format is off by default because some receivers prefer text over graphics (validate in WP-02). |
| Native drag | `drag` crate (CrabNebula, OLE `DoDragDrop` on Windows) invoked on the main thread, `DragMode::Copy`, `CF_HDROP` file list | Multi-file drag + copy semantics; never moves sources. |
| Explorer / open | `tauri-plugin-opener` (`reveal_items_in_dir`, `open_path`) | Multi-select reveal in a single Explorer window per folder. |
| Dialogs | `tauri-plugin-dialog` (folder pick, save-as) | — |
| Frontend | React 19, Vite, TypeScript (strict), Tailwind v4, Zustand, `@tanstack/react-virtual`, `lucide-react` | Plan §6/§39. |
| UI dev without Rust | `src/api/` has a typed `Backend` interface with two implementations: Tauri and an in-browser **mock** (generated SVGs) | Frontend can be developed, screenshot-tested and demoed in a plain browser. |
| Logging | `tracing` + daily rolling file in the app log dir + stderr | Plan §6. |
| Temp files | `<app cache>/tmp/`; cropped drag files named `{original}-crop.svg` in per-drag subfolders; purged at startup and after 10 min | Never assume the drop target finished reading when the drag returns (plan §28). |

---

## 3. Interface contracts (frozen before parallel work starts)

### 3.1 Tauri commands (`src/api/backend.ts` ⇄ `src-tauri/src/commands/*`)

| Command | Input | Output |
|---|---|---|
| `get_app_state` | — | `{ library: LibraryInfo \| null, recent: LibraryInfo[], scan: ScanStatus, settings: Settings }` |
| `open_library` | `{ path }` | `LibraryInfo` (starts background scan/reconcile) |
| `pick_and_open_library` | — | `LibraryInfo \| null` |
| `search` | `{ query, limit? }` | binary `u32[]` asset IDs (ranked; path order when query empty) |
| `get_assets` | `{ ids, query? }` | `AssetSummary[]` (`match` explanation when query given) |
| `get_asset_detail` | `{ id }` | `AssetDetail` (dimensions, viewBox, size, state, title/desc, hash) |
| `set_viewport` | `{ visible, nearby }` | — |
| `copy_assets` | `{ ids, format: 'svg' \| 'png' \| 'png_white' \| 'path' \| 'filename' }` | — |
| `copy_region` | `{ id, region, format: 'svg' \| 'png' \| 'png2x' \| 'png_white' }` | — |
| `save_region` | `{ id, region, format: 'svg' \| 'png' }` | saved path or null (shows save dialog) |
| `start_drag_assets` | `{ ids }` | — (native drag, copy effect) |
| `start_drag_region` | `{ id, region }` | — |
| `reveal_assets` / `open_external` | `{ ids }` / `{ id }` | — |
| `update_settings` | `Partial<Settings>` | `Settings` |

### 3.2 Events (Rust → UI)

| Event | Payload | Throttle |
|---|---|---|
| `scan://progress` | `{ phase: 'discovering' \| 'reconciling' \| 'processing' \| 'idle', discovered, processed, total, message }` | ≤ 4/s |
| `catalog://changed` | `{ total, reason: 'scan' \| 'watch' \| 'metadata' }` | ≤ 4/s; UI re-runs current search |
| `thumb://ready` | `{ ids }` | batched |

### 3.3 Core types (Rust `svg-core::model` ⇄ TS `src/api/types.ts`)

`AssetSummary { id, filename, relDir, state, width?, height?, fingerprint, match? }`
`ProcessingState = 'discovered' | 'ready' | 'limit_exceeded' | 'parse_error' | 'missing'`
`Region { x, y, width, height }` (SVG user units)
`MatchInfo { field: 'filename' | 'path' | 'title' | 'text' | 'desc' | 'id_class', snippet }`

### 3.4 svg-core public API (consumed by src-tauri)

```text
svg::analyze(bytes, &Limits) -> Analysis { meta: SvgMeta, text: SvgText, complexity, state }
svg::render_thumbnail(bytes, size, &Limits) -> Result<Png>
svg::crop::crop_svg(bytes, Region) -> Result<String>
svg::render_region_png(bytes, Region, scale, Background) -> Result<Png>
library::scanner::Scanner::run(root, existing: Snapshot, batch_size, cancel, on_batch) -> ScanSummary
index::Database::{open, migrate, upsert_assets, mark_missing, save_analysis, load_catalog, libraries, settings}
search::Catalog::{insert, update, remove, search(query) -> Vec<AssetId>, explain(id, query) -> Option<MatchInfo>}
```

---

## 4. Phases

Phase numbering below is the *execution* order; each row names the plan.md work packages it delivers.

### Phase A — Foundation & contracts (WP-01) — owner: lead

| Deliverable | Acceptance |
|---|---|
| Cargo workspace, `svg-core` + `src-tauri` crates with plan §38 module tree | `cargo check --workspace` passes |
| Vite + React + TS strict + Tailwind v4 + Zustand shell | `npm run build` passes |
| Settings/limits config (`settings.json`, defaults from plan §2.5/§13) | Round-trip test |
| Logging (tracing, rolling file) and error type that serializes to the UI | Errors surface as toast messages |
| Contracts in §3 committed as Rust types + TS types | Both sides compile against them |

### Phase B — Parallel core build (WP-04…11, 14–16, 18) — three agents

| Agent | Scope (WPs) | Owned paths | Acceptance |
|---|---|---|---|
| **B1 SVG engine** | WP-06 metadata/text, plan §13 guardrails, WP-07 renderer, WP-16 crop SVG/PNG | `crates/svg-core/src/svg/**` | Unit tests: dimensions/viewBox/units; title/desc/text/tspan/ids/classes extraction with normalization + caps; limit states for zero-byte, malformed, oversize, node bomb, huge data-URI; thumbnail 256² transparent PNG; crop produces `viewBox="0 0 w h"`, translated content, clip, preserved defs/text; region PNG dimensions at 1x/2x; white background option |
| **B2 Library, index, search** | WP-04 scanner, WP-05 SQLite/change detection, plan §11 hashing, WP-09 + WP-11 search/ranking/explanations | `crates/svg-core/src/{library,index,search,model.rs}` | Scanner emits batches (100–500) progressively and supports cancel; reconcile classifies new/changed/unchanged/deleted by size+mtime; migrations idempotent; catalog round-trips through SQLite; search: AND tokens, glob, `re:`, ranking order of plan §17, match explanations; 50k-asset search < 50 ms (bench) |
| **B3 Frontend** | WP-08 gallery, WP-10 multi-select, WP-14 viewer, WP-15 region selection, WP-18 minimap, WP-21 shell polish, keyboard model §37, context menus §36 | `src/**` | Virtualized responsive grid (S/M/L), only visible cards mounted; selection store with click/Ctrl/Shift/Ctrl+A/Esc/arrows, survives scroll; viewer zoom at pointer, pan, Space-pan, +/−/0/F, fit; region overlay in SVG units with live dimensions and handles; minimap with draggable viewport; context-sensitive Ctrl+C; checkerboard/white/dark backgrounds; works fully against the mock backend in a browser |

### Phase C — Tauri integration (WP-02 productionized as WP-12/13, WP-17, WP-19, WP-27 startup) — owner: lead (+ agent if needed)

| Deliverable | Acceptance |
|---|---|
| App state: DB, catalog, active library, settings | Relaunch shows cached gallery before any filesystem access |
| Priority work queue (P0–P3) + worker pool; metadata & thumbnail jobs; `set_viewport` reprioritization | Visible thumbnails render first while 50k background jobs are pending |
| `thumb://` and `svgfile://` async protocols with on-disk cache | Cache hit served without render; renderer-version bump invalidates |
| Progressive scan → DB → catalog → throttled events | Gallery fills while scanning; search works mid-scan |
| Startup reconcile ("Checking for changes…") | No blocking rescan on launch |
| Watcher with debounce and incremental updates + thumbnail invalidation | Burst of 600 changes → one reconcile pass |
| Clipboard: SVG/PNG/PNG-white/path/filename/paths, region SVG/PNG/2x/white | ⚠ Office paste validation |
| Native drag: single, multi, region (temp file lifecycle) | ⚠ Office/Explorer drop validation |
| Reveal in Explorer / open externally / save region dialogs | — |

### Phase D — Datasets & hardening (WP-20, plan §33–34) — agent D

| Deliverable | Acceptance |
|---|---|
| `crates/svg-core/examples/gen_dataset.rs` deterministic generator for datasets A (1k), B (10k), C (50k), D (10k text-heavy), E (pathological set) | Reproducible byte-identical output for a seed |
| `crates/svg-core/examples/bench.rs` timing scan, catalog load, search (filename & content), analyze, thumbnail render | Results table recorded in `docs/performance.md` against plan §34 targets |
| Pathological inputs never panic and land in the correct state | Test over dataset E |

### Phase E — Review, polish, delivery (WP-21, WP-22 prep)

| Deliverable | Acceptance |
|---|---|
| Independent code review by an agent that did not write the code | Findings fixed or recorded |
| UI screenshots from the mock backend (gallery, selection, viewer, region, minimap, menus) | Visual check passes in light and dark |
| `README.md` build/run instructions for Windows; `docs/windows-validation.md` checklist for ⚠ items | — |
| Windows release config (`tauri.conf.json` NSIS/MSI bundle, icons, offline WebView2 bootstrapper option) | `npm run tauri build` on Windows produces an installer |

---

## 5. Team plan

```text
Lead (orchestrator)
 ├─ Phase A: scaffold + contracts                     (serial, lead)
 ├─ Phase B: ┌─ B1 SVG engine agent      ─┐
 │           ├─ B2 Library/index/search   ├─ parallel, disjoint file ownership
 │           └─ B3 Frontend agent         ─┘
 ├─ Phase C: Tauri integration                        (lead, after B1+B2 land)
 ├─ Phase D: dataset + bench agent                    (parallel with C)
 └─ Phase E: fresh-eyes review agent → fixes → delivery
```

Coordination rules: agents edit only their owned paths; any contract change is requested from the lead, not made unilaterally; each agent leaves `cargo test -p svg-core` / `npm run build` green for its area.

---

## 6. Environment constraints and what they mean

| Constraint | Consequence |
|---|---|
| Build runs on Linux; Windows target std is not downloadable here | Full Tauri app compiles and links on Linux (WebKitGTK); Windows-only paths come from established crates (`drag`, `clipboard-rs`, `tauri-plugin-opener`) rather than hand-written `cfg(windows)` code. Final Windows build happens on the developer machine. |
| No Office/Visio/draw.io in the build environment | Plan §5.1–5.3 spikes are delivered as working features plus a manual checklist (`docs/windows-validation.md`). The clipboard text-format and format-ordering choices are settings so they can be tuned without code changes. |

---

## 7. MVP coverage (plan §41)

| MVP item | Phase |
|---|---|
| Directory selection, recursive indexing, persistent index | B2, C |
| Fast thumbnails, virtualized grid | B1, B3, C |
| Filename/path search, SVG-text search | B2 |
| Single and multi-selection | B3 |
| Copy path, copy full SVG | C |
| Native SVG drag, multi-file drag | C ⚠ |
| Viewer, zoom/pan, region selection | B3 |
| Copy region as SVG / PNG, transparent background | B1, C ⚠ |
| Basic SVG complexity limits | B1 |
| Also included beyond MVP | Minimap, save region, region drag-out, watcher, startup reconcile |

Deferred (architecture leaves room): favorites, collections, duplicate UI (hash already stored), text highlight in viewer, snippets beyond single-line match, recently/frequently used, multiple simultaneous libraries (schema supports it).

---

## 8. Risks

| Risk | Mitigation |
|---|---|
| PowerPoint picks the bitmap instead of SVG from the clipboard | Format set and order configurable; validate per checklist; fallback = file drag (CF_HDROP) which PowerPoint imports as vector |
| WebView2 rasterization cost at high zoom on large SVGs | Zoom capped (64×) and image sized (not CSS-scaled) so the engine re-rasterizes crisply; files above the hard limit never reach the viewer |
| Font differences between resvg thumbnails and WebView | Shared system fontdb; thumbnails are previews, crop output keeps live text |
| CSS selectors relying on root structure break after crop wrap | Rare in practice; documented; crop never edits elements |
| Huge libraries on network drives | Scanner is cancelable, batches progressively; watcher falls back to periodic reconcile when notify fails |
