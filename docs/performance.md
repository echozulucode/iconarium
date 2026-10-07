# Performance and stress results (plan §33–34, WP-20)

This page records what the headless core (`crates/svg-core`) achieves on the deterministic datasets from plan §33, measured against the plan §34 targets. It also lists the problems the pathological set exposed.

**Short version:** discovery, cold start, search and cache hits beat every target by 1–3 orders of magnitude, even at 50,000 files. Rendering text-heavy drawings misses: thumbnails take about 115 ms each, and region copy takes about 330 ms.

The pathological set found three robustness bugs: deep nesting aborted the process, and large `<text>` made rendering effectively unbounded. All three are now **fixed** by new limits and covered by regression tests; see [Problems found](#problems-found). Dataset E now runs end to end on the default 2 MiB worker stack with no workarounds. The slowest ready file renders in 1.3 s.

## How to reproduce

```sh
scripts/gen-datasets.sh all --verify          # → ./datasets/{A..E} (gitignored, ~670 MB), checks byte-identical regeneration
cargo run -p svg-core --release --example bench_pipeline -- datasets/C
cargo run -p svg-core --release --example bench_pipeline -- datasets/E     # default 2 MiB worker stacks
cargo test -p svg-core --test pathological                    # 9 tests; also run with --release
```

| Tool | Purpose |
|---|---|
| `examples/gen_dataset.rs` | Deterministic generator. splitmix64, per-file seeds and no libm transcendental functions, so output is byte-identical for a seed on every platform. `gen_dataset hash <dir>` prints a BLAKE3 tree hash. It also writes `E/MANIFEST.tsv` with the expected outcome of every pathological case. |
| `examples/bench_pipeline.rs` | Runs the real on-disk pipeline the way the Tauri shell uses svg-core. Worker threads get a 2 MiB stack, like the app's preview workers. |
| `examples/bench_search.rs` | The earlier synthetic in-memory search benchmark, unchanged. |
| `tests/pathological.rs` | Generates dataset E at reduced sizes with matching reduced `Limits`. Every analyze and render call must be clean and finish in under 5 s on a fresh worker thread. |

### Datasets (default seed `0x5EED2026`)

| Set | Visible SVGs | Content | Size | Gen time |
|---|---:|---|---:|---:|
| A | 1,000 | Varied icons: shapes, gradients, strokes, transforms, nested groups, 3% larger illustrations, about 6% with `<text>`. Folder trees 1–6 deep with 40–500 files per folder (`electrical/…`, `network/switches/…`, `ui/icons/24/…`, `Legacy Assets/Old Style/…`). Hidden `.git/`, `.cache/`, `.backup/` and `.name.svg` entries, plus decoys (`.svgz`, `.svg.bak`, `.png`, `README.md`). | 2 MB | 0.1–0.2 s |
| B | 10,000 | As A | 17 MB | 0.5–2.5 s |
| C | 50,000 | As A (381 directories) | 83 MB | 1.8–9 s |
| D | 10,000 | Engineering diagrams: 10–40 labelled boxes such as PLC, Motor Controller, Supervisory Server and Pump; orthogonal connectors with link labels (Ethernet, Modbus TCP, 4-20 mA…); title, desc, legend, notes and title block; ids and classes. About 2.2 KB of extracted text per file. | 115 MB | 0.3–3 s |
| E | 68 (+7 skipped) | Pathological cases (see [Dataset E](#dataset-e--pathological-inputs)) | 247 MB | 2.6 s |

Generation time varies with page-cache state. All sets together take under 20 s on 2 cores.

## Environment

- 2-vCPU cloud VM (Intel Xeon @ 2.8 GHz), 8 GB RAM, Linux 6.18, virtio disk, rustc 1.97, `--release` (thin LTO, `codegen-units = 1`).
- The CPU was shared with a concurrent Tauri build, and the VM periodically drops the page cache (`idle-reclaim: drop_caches`). I/O-bound stages varied up to about 3x between runs. For example, analysis of C measured 705 ms in one run and 2,410 ms in another. The tables show the later, slower run.
- 335 system fonts, but **no Arial, Helvetica or Times New Roman**. This matters for text rendering; see problem 4.
- **Windows/NVMe numbers will differ.** More cores scale analysis, thumbnails and region rendering roughly linearly, since they are embarrassingly parallel. NTFS directory walks are typically slower than ext4 per entry, and Windows Defender scanning can dominate first reads. Cache-hit and search figures are CPU/memory-bound and should carry over.
- These figures cover the core library only. WebView, IPC serialization, image decode and React rendering are not included.

## Targets (plan §34) vs measured

Primary rows use C (50k icons). D (10k text-heavy diagrams) is shown where it behaves differently.

| Operation | Target | Measured | |
|---|---|---|---|
| Application shell visible | <500 ms | Not measured here (UI) | — |
| Cached gallery visible (core: `Database::open` + `load_catalog` + `Catalog::extend` + list + 200 summaries) | <500 ms | **C 136 ms**, D 156 ms, B 30 ms, A 3 ms | ✅ |
| First uncached assets (scan until first batch, then analyze + render 40 tiles, 2 threads) | <1 s | **C: first tile 8.5 ms, 40 tiles 92 ms** · D: first tile 88 ms, **40 tiles 2.4 s** | ✅ icons / ⚠️ diagrams |
| Search filename/path (worst p95 across token, 2-token, name glob, path glob, regex, exclusion) | <50 ms | **C 6.8 ms** (path glob) · D 18.8 ms (regex over 22 MB of text) | ✅ |
| Search full content (worst p95: content token, phrase, 3 tokens) | <100 ms | **D 8.2 ms** · C 3.0 ms | ✅ |
| Grid scrolling | ~60 FPS | Not measured here (frontend; see `scripts/scroll-perf.mjs`) | — |
| Normal viewer open (core: read + `prepare_for_viewer`, p95) | <200 ms | C 0.03 ms · D 0.15 ms · E 2.2 ms (max 48 ms on the 24.9 MB file) | ✅ |
| Thumbnail cache hit (path + read PNG, p95) | <30 ms | 0.009–0.012 ms (warm page cache) | ✅ |
| Region-copy initiation (`render_region_png`, 50% region, 2x, white bg, p50 over the 10 largest files) | <200 ms typical | A 58 ms · B 73 ms · **C 260 ms** (max 1.09 s) · **D 328 ms** (max 382 ms) | ⚠️ |
| Noticeable UI stalls | none >50 ms | All core work runs off the UI thread. Worker latency per thumbnail: icons p95 7–10 ms, max 58–84 ms; diagrams p95 169 ms, max 246 ms; pathological max 1.3 s (24.9 MB file). Large text used to take minutes; it is now rejected (problem 3, fixed) | ✅ UI / ⚠️ workers |

Notes on the ⚠️ rows:

- **Text-heavy thumbnails are about 40x slower than icons**: p50 115 ms against 3 ms. Removing all `<text>` from a D diagram drops its thumbnail from 113 ms to 3 ms. Naming an installed family (`DejaVu Sans`) instead of `Arial, Helvetica, sans-serif` drops it to 43 ms (problem 4). With 2 workers, a full screen of 40 uncached diagram tiles therefore takes about 2.4 s. The first tile still appears after 88 ms. An 8-core desktop with Arial installed should land around 0.3–0.5 s per screen.
- **Region copy**: at 0.25x the same region costs 120 ms on D (parse + crop + text layout). The remaining ~200 ms at 2x is rasterization and PNG encoding of a ~1600×1000 px image. On C, the "largest 10 files" are synthetic 80–400-element illustrations with gradients and opacity, which is a deliberately heavy case. Ordinary icons (A/B) come in at 58–73 ms.

## Per-dataset details

### Discovery and index

| Set | Files | First batch (250) | Scan + per-batch DB insert | of which inserts | Pure re-scan | Reconcile unchanged |
|---|---:|---:|---:|---:|---:|---:|
| A | 1,000 | 2.4 ms | 12 ms | 7 ms | 2.5 ms | 3.3 ms |
| B | 10,000 | 2.2 ms | 168 ms | — | 43 ms | 60 ms |
| C | 50,000 | 2.0 ms | 847 ms (59k files/s) | 442 ms¹ | 126 ms (396k files/s) | 193 ms |
| D | 10,000 | 2.6 ms | 198 ms | 86 ms¹ | 29 ms | 41 ms |
| E | 68 | 0.3 ms | 0.8 ms | 0.4 ms | 0.3 ms | 0.3 ms |

¹ From the first benchmark run.

Hidden directories and files, `.svgz`, `.svg.bak`, `.png` and `README.md` were all skipped in every set: discovered count equals the visible count exactly. Upper-case `.SVG` files were discovered.

### Analysis (read + `analyze`, 2 threads)

| Set | Throughput | p50 | p95 | max | `save_analyses` (256/txn) | Extracted text |
|---|---:|---:|---:|---:|---:|---:|
| A | 21k files/s | 0.02 ms | 0.12 ms | 1.4 ms | 6 ms | 0.0 MB |
| B | 20k files/s | — | — | 2.2 ms | 125 ms | 0.2 MB |
| C | 21k files/s (71k warm) | 0.017 ms | 0.043 ms | 4.1 ms | 1.37 s | 1.0 MB |
| D | 7.8k files/s (12k warm) | 0.14 ms | 0.28 ms | 8.5 ms | 0.32 s | 22.2 MB |
| E | — | 0.05 ms | 16 ms | 93 ms (24.9 MB file) | 0.5 ms | 0.0 MB |

All A–D files are `ready`. E produces `ready` 46, `parse_error` 14 and `limit_exceeded` 8, matching `MANIFEST.tsv`. Peak benchmark RSS was 387 MB on C and 319 MB on D. The benchmark keeps every analysis in memory, so this is not the app's RSS.

### Cold start (median of 3)

| Set | open | load_catalog | Catalog::extend | first page | total |
|---|---:|---:|---:|---:|---:|
| C | 0.4 ms | 90 ms | 43 ms | 13 ms | **136 ms** |
| D | 0.4 ms | 45 ms | 100 ms | 4 ms | **156 ms** |

### Search (in-memory catalog, `search(q, None)`, 20 runs)

| Query | C hits | C median / p95 | D hits | D median / p95 |
|---|---:|---:|---:|---:|
| `pump` | 3,190 | 3.0 / 3.5 ms | 8,801 | 2.9 / 3.5 ms |
| `motor controller` | 22 | 2.4 / 2.6 ms | 7,036 | 6.1 / 6.6 ms |
| `*switch*.svg` | 987 | 4.4 / 5.3 ms | 0 | 1.3 / 2.3 ms |
| `network/*/*.svg` | 1,102 | 6.3 / 6.8 ms | 394 | 2.0 / 2.1 ms |
| `re:pump.*(station\|valve)` | 454 | 4.4 / 4.7 ms | 6,534 | 17.7 / 18.8 ms |
| `valve -legacy` | 3,567 | 2.7 / 3.2 ms | 8,754 | 4.5 / 5.0 ms |
| `supervisory` (content) | 0 | 2.1 / 2.3 ms | 8,759 | 2.5 / 2.7 ms |
| `"motor controller"` | 8 | 2.9 / 3.0 ms | 7,036 | 2.4 / 3.0 ms |
| `redundant ring ethernet` | 4 | 2.4 / 2.6 ms | 9,074 | 7.8 / 8.2 ms |

Fetching the first 100 summaries with match explanations adds under 1.5 ms. On E it adds 7–12 ms, because `explain` scans 1 MB of extracted text on the massive-text files.

### Thumbnails (256 px, 500-file sample, 2 threads)

| Set | Throughput | render p50 | p95 | max | cache hit p95 |
|---|---:|---:|---:|---:|---:|
| A | 424/s | 3.0 ms | 9.2 ms | 68 ms | 0.008 ms |
| B | 405/s | 3.1 ms | 9.5 ms | 58 ms | 0.012 ms |
| C | 441/s | 3.1 ms | 7.2 ms | 84 ms | 0.009 ms |
| D | **17/s** | **115 ms** | **169 ms** | 246 ms | 0.011 ms |
| E (all 46 ready files, 2 MiB stacks) | 32/s | 3.6 ms | 242 ms | 1.3 s | 0.041 ms |

The slowest icon renders, 50–84 ms, are icons that contain `<text>`. Font database load is 7–9 ms, a one-off cost at startup.

## Dataset E — pathological inputs

Each case is checked by `tests/pathological.rs` at reduced size with matching limits. The full-size files in `datasets/E` were benchmarked in release with the default 2 MiB worker stacks and nothing excluded. Render times below are release, full size. Rows marked "was:" give the behaviour before the fixes.

| Case | analyze → state | `render_thumbnail` | Notes |
|---|---|---|---|
| zero-byte, unclosed tags, truncated, random bytes, plain text, `<html>` root, xhtml-wrapped svg, two roots, undeclared prefix, gzip bytes named `.svg`, invalid UTF-8, UTF-16 without BOM | `parse_error` with a clear message | clean `Err`, <1 ms | |
| `.svgz`, `.svg.bak`, `.git/…`, `.cache/…`, `.hidden-file.svg`, a file named `.svg`, `.png` | not discovered | — | |
| just under limit (24.9 MB of paths) | `ready`, 55 ms | ok, **1.25–1.4 s** | Worst ready case under default limits |
| just over limit (26 MB) / 100 MB (streamed) | `limit_exceeded` (app skips the read) | `Err` (limit) | |
| node bomb (150k elements) | `limit_exceeded` | `Err`, 5 ms | |
| 5,000 nested `<g>` | `limit_exceeded` ("Elements nested too deeply") | `Err` (limit) | Problem 1, fixed. Was: aborts on 2 MiB |
| 1,000 nested `<g>` | `limit_exceeded` | `Err` (limit) | Problem 2, fixed. Was: render aborts on 2 MiB |
| 250 nested `<g>` (just under `max_nesting_depth` = 256) | `ready` | ok, 2 MiB stack | |
| one path, 1M coordinates (5.7 MB) | `ready`, 13–16 ms | ok, 235–264 ms | |
| PNG data URI 16×16 / corrupt JPEG data URI | `ready` | ok | Undecodable image skipped |
| PNG data URI, 60 MB decoded (84 MB file) | `limit_exceeded` | `Err` | Hits the file-size limit first (observation 6) |
| `http://`, `file:///`, relative image, `<use href="other.svg#x">`, CSS `@import` | `ready`, `has_external_refs` | ok; no network access (usvg never fetches `http:`) | The app passes the SVG's folder as `resources_dir` |
| `<script>`, `on*` handlers, `javascript:` href, `foreignObject` | `ready`, `has_scripts` | ok | |
| SMIL animate/set/animateTransform/animateMotion | `ready` | ok (static) | |
| huge blur + morphology filter (4000×4000) | `ready` | ok, **0.8–1.0 s** | |
| pattern with 0.05-unit tiles; dasharray 0.01 (~240k dashes) | `ready` | ok, 2 ms / 26 ms | |
| self/mutually recursive `<use>`; `<use>` fan-out of 10⁶ | `ready` | `Err` ("nodes limit reached"), 0.2 ms / 216 ms | Observation 5 |
| 5 MB in one `<text>` node | `limit_exceeded` ("Text run too long"), 10 ms | `Err` (limit) | Problem 3, fixed. Was: OOM-killed at 6 GB RSS |
| 5 MB over 21k `<text>` elements | `limit_exceeded` ("Too much text"), 26 ms | `Err` (limit) | Problem 3, fixed. Was: 33 s, 2.7 GB RSS |
| 9,000 characters in one `<text>` (under the 10,000 limit) | `ready` | ok, 68 ms | |
| Illustrator-style DTD entities (in `xmlns`, `style`, text) | `ready`; entities expanded and searchable | ok | |
| billion laughs | `parse_error` (roxmltree entity limit), <5 ms | `Err` | |
| XXE `SYSTEM "file:///etc/passwd"` | `parse_error` (unknown entity) | `Err` | Never resolved |
| UTF-16 LE/BE with BOM, UTF-8 BOM, ISO-8859-1 declared | `ready`, text decoded correctly | ok | |
| no viewBox and no size / `%` size / viewBox `-500 300 1000 800` / 1,000,000 px canvas / empty `<svg/>` / no `xmlns` | `ready` | ok | Crop on the non-zero origin is pixel-checked in the test |
| `width="0" height="0"`; negative viewBox width | `ready` | `Err` ("invalid size") / ok | |
| Arabic (RTL), Hebrew, CJK, Hangul, emoji with ZWJ, NFC and NFD `café`, zalgo, U+202E override, zero-width characters, Greek directory, spaces and `#[]&'~$;%` in names, a 225-byte name, `.SVG` | `ready` | ok | Discovered, indexed and searchable; one NFC/NFD caveat (observation 7) |

## Problems found

Problems 1–3 were reported from this work and **fixed in svg-core by the lead**. The `#[ignore]`d reproductions became normal regression tests in `tests/pathological.rs`. Each test spawns a thread with an explicit 2 MiB stack and asserts a clean `LimitExceeded` / `CoreError::Limit` within 5 s, from every entry point:

- `analyze`
- `prepare_for_viewer`
- `render_thumbnail`
- `render_png`
- `render_region_png`
- `crop_svg`

The tests pass in both debug and release. Problem 4 and the observations remain open.

### 1. Process abort: deep nesting overflowed the XML parser's stack — FIXED

- **Was:** `svg/parser.rs::parse_xml` → `roxmltree::Document::parse_with_options`. roxmltree's recursive-descent tokenizer (`parse_element` ↔ `parse_content`) uses about 0.7 KB of stack per level. 5,000 nested `<g>` (`datasets/E/limits/nested-groups-5000.svg`) made every entry point SIGABRT with "thread has overflowed its stack" on a 2 MiB thread. Release builds aborted somewhere between 3,000 and 3,500 levels. `catch_unwind` cannot catch a stack overflow, so one such file killed the app.
- **Fix:** `parse_xml(text, max_nodes, max_depth)` runs a linear pre-scan (`parser::element_depth`) before any recursive code. Over-deep documents are `LimitExceeded`: "Elements nested too deeply…". The new `Limits::max_nesting_depth` defaults to 256. crop and normalize use `HARD_MAX_DEPTH` = 1024.
- **Regression test:** `deep_nesting_is_limit_exceeded_on_a_2mib_stack`, covering 257 / 600 / 1,000 / 1,020 / 3,500 / 5,000 / 100,000 levels, each with and without a viewBox.

### 2. Process abort: moderate nesting overflowed usvg's converter — FIXED

- **Was:** `svg/renderer.rs::tree_from_text` → `usvg::Tree::from_xmltree`, in the recursive `converter::convert_group` → `convert_element` → `convert_children` chain. 600–1,020 nested `<g>` sits below usvg's own 1,024 guard. It aborted render calls on a 2 MiB stack in release from about 600 levels. `analyze` aborted too, via the `usvg_canvas_size` fallback, when the root had no viewBox or size.
- **Fix:** the same depth limit (256) rejects these documents before usvg runs.
- **Regression tests:**
  - `deep_nesting_is_limit_exceeded_on_a_2mib_stack`.
  - `nesting_at_the_limit_renders_on_a_2mib_stack`: 254 nested groups still analyze and render on 2 MiB in release.

### 3. Unbounded time and memory rendering large `<text>` — FIXED

- **Was:** `check_complexity` ignored text volume, and usvg's `text::layout::layout_text` is super-linear in the characters of one text chunk. In release:
  - one `<text>` of 50 KB → 1.0 s; 200 KB → 14.6 s; the 5 MB dataset file → OOM-killed at 6 GB RSS
  - 5 MB over 21k `<text>` elements → 33 s and 2.7 GB for one thumbnail

  These files analyzed as `ready`.
- **Fix:** `Complexity::render_text_chars` is now collected. `max_text_node_chars` counts only `<text>` content. `check_complexity` returns `LimitExceeded` when one run exceeds `max_text_node_chars` (10,000) or the total exceeds `max_render_text_chars` (200,000). Dataset E's two massive-text files now come back in 10–26 ms, against minutes and OOM before.
- **Regression tests:**
  - `massive_text_is_limit_exceeded_quickly`: a 200 KB run, a 10,001-character run, 1,000 × 250 characters and 20,000 × 250 characters.
  - `text_at_the_limits_renders_in_bounded_time`: a 10,000-character run and 100 × 1,000 characters still render.
- **Side effect (resolved):** a `<text>` run over 10,000 characters makes the file `limit_exceeded` (not rendered), but its text is still extracted (truncated to the caps) and stays searchable.

### 4. Text-heavy rendering cost (performance, environment-dependent)

Diagram thumbnails cost about 115 ms and are dominated by text layout: with the text removed they take 3 ms. fontdb's generic families default to MS fonts (`sans-serif` → Arial, `serif` → Times New Roman). On this Linux VM those fonts are absent, so `Arial, Helvetica, sans-serif` falls through to the default serif face. Naming an installed font cuts the cost to 43 ms.

Windows has Arial, so the shipping target should be closer to the 43 ms figure. Calling `fontdb.set_sans_serif_family(...)` and friends with installed families on non-Windows platforms would help. Thumbnails of text-heavy drawings remain about 15x more expensive than icons in any case, so schedule them as viewport-first, which the app's queue already does.

### Observations (behaviour worth a decision, not crashes)

5. **Ready but not renderable.** Some files analyze as `ready` while `render_thumbnail` returns `Err`:
   - any self- or mutually-recursive `<use>`: usvg rejects the whole document, where browsers drop only the bad `<use>`
   - `<use>` fan-out bombs
   - zero-size documents

   The gallery will show an error tile for an asset in the `ready` state.
6. **The embedded-raster limit is unreachable with default limits.** A base64 data URI decoding to more than 50 MB needs a file of more than 66 MB, which trips the 25 MB file limit first. `max_embedded_raster_bytes` only matters if `max_file_bytes` is raised. The test exercises it with reduced limits.
7. **No Unicode normalization in search.** Querying `café` (NFC) finds `café-nfc.svg` but not `café-nfd.svg`, and the reverse is also true. This matters mostly for files that came from macOS (NFD file names). Case-insensitive matching across scripts works, for example `ΕΛΛΗΝΙΚΆ` finds `Ελληνικά`.
8. **Renders cannot be cancelled.** usvg and resvg expose no cancellation hook, so a runaway render holds its worker until it finishes or the process dies. Up-front limits, as in the problem 3 fix, are the only defence. The slowest ready file under default limits now takes 1.3 s (the 24.9 MB file).
