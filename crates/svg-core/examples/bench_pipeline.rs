//! End-to-end pipeline benchmark over a real dataset on disk (plan §34, WP-20).
//!
//! ```text
//! cargo run -p svg-core --release --example bench_pipeline -- <dataset_dir> \
//!     [--threads N] [--sample N] [--stack-mib N] [--work DIR] [--per-file] [--no-render SUBSTR]...
//! ```
//!
//! `--work DIR` keeps the SQLite index and thumbnail cache in DIR (default: a temp dir).
//! `--per-file` logs every thumbnail render time to stderr (default for ≤200 files).
//! `--no-render SUBSTR` keeps matching files out of the rendering stages (they are still
//! discovered and analyzed) — useful for isolating slow files.
//!
//! Stages (each mirrors what the Tauri shell does with svg-core):
//!
//! * **0** first screen, uncached: scan until the first batch, analyze + render 40 thumbnails
//! * **a** progressive discovery with per-batch DB inserts; pure re-scan
//! * **b** reconcile of an unchanged tree against the persisted snapshot
//! * **c** parallel analyze (read + `analyze`, oversize files skipped like the app) + `save_analyses`
//! * **d** cold start: `Database::open` + `load_catalog` + `Catalog::extend` + first page
//! * **e** search latency (median / p95 over 20 runs) per query class
//! * **f** 256 px thumbnails for a sample (render + cache write), then cache-hit reads
//! * **g** viewer open (read + `prepare_for_viewer`)
//! * **h** region copy: `crop_svg` + `render_region_png` (2x and 0.25x, white) on the largest drawings
//!
//! Worker threads use a 2 MiB stack by default — the std/tokio default the app's preview
//! workers run on — so stack-depth problems show up here exactly as they would in the app.
//! Results are printed as markdown.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use svg_core::config::{Limits, MB};
use svg_core::index::Database;
use svg_core::library::scanner::{scan, to_absolute, Reconciler, ScanOptions};
use svg_core::model::{
    Analysis, AssetId, AssetRecord, Complexity, ProcessingState, Region, SearchText, SvgMeta,
};
use svg_core::search::Catalog;
use svg_core::svg::{
    analyze, crop_svg, prepare_for_viewer, render_region_png, render_thumbnail, warm_up_fonts,
    Background, RENDERER_VERSION,
};

const THUMB: u32 = 256;

struct Args {
    dir: PathBuf,
    threads: usize,
    sample: usize,
    stack: usize,
    work: Option<PathBuf>,
    per_file: Option<bool>,
    /// Relative-path substrings excluded from rendering stages (still discovered/analyzed).
    no_render: Vec<String>,
}

fn parse_args() -> Args {
    let mut a = Args {
        dir: PathBuf::new(),
        threads: std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(2),
        sample: 500,
        stack: 2 * 1024 * 1024,
        work: None,
        per_file: None,
        no_render: Vec::new(),
    };
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    let num = |i: usize| -> usize {
        raw.get(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| usage())
    };
    while i < raw.len() {
        match raw[i].as_str() {
            "--threads" => {
                a.threads = num(i).max(1);
                i += 2;
            }
            "--sample" => {
                a.sample = num(i).max(1);
                i += 2;
            }
            "--stack-mib" => {
                a.stack = num(i).max(1) * 1024 * 1024;
                i += 2;
            }
            "--work" => {
                a.work = Some(PathBuf::from(raw.get(i + 1).unwrap_or_else(|| usage())));
                i += 2;
            }
            "--no-render" => {
                a.no_render
                    .push(raw.get(i + 1).unwrap_or_else(|| usage()).clone());
                i += 2;
            }
            "--per-file" => {
                a.per_file = Some(true);
                i += 1;
            }
            s if !s.starts_with("--") && a.dir.as_os_str().is_empty() => {
                a.dir = PathBuf::from(s);
                i += 1;
            }
            _ => usage(),
        }
    }
    if a.dir.as_os_str().is_empty() {
        usage();
    }
    a
}

fn usage() -> ! {
    eprintln!("usage: bench_pipeline <dataset_dir> [--threads N] [--sample N] [--stack-mib N] [--work DIR] [--per-file] [--no-render SUBSTR]...");
    std::process::exit(2)
}

// ---------------------------------------------------------------- stats

#[derive(Clone, Copy, Default)]
struct Stats {
    n: usize,
    p50: f64,
    p95: f64,
    max: f64,
    mean: f64,
}

fn stats(v: &[f64]) -> Stats {
    if v.is_empty() {
        return Stats::default();
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let q = |p: f64| s[((s.len() - 1) as f64 * p).round() as usize];
    Stats {
        n: s.len(),
        p50: q(0.5),
        p95: q(0.95),
        max: *s.last().unwrap(),
        mean: s.iter().sum::<f64>() / s.len() as f64,
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

fn time<T>(f: impl FnOnce() -> T) -> (T, f64) {
    let t = Instant::now();
    let v = f();
    (v, ms(t.elapsed()))
}

fn fmt_ms(v: f64) -> String {
    if v >= 10_000.0 {
        format!("{:.1} s", v / 1e3)
    } else if v >= 100.0 {
        format!("{v:.0} ms")
    } else if v >= 1.0 {
        format!("{v:.1} ms")
    } else {
        format!("{v:.3} ms")
    }
}

fn row(label: &str, s: Stats) -> String {
    format!(
        "| {label} | {} | {} | {} | {} | {} |",
        s.n,
        fmt_ms(s.p50),
        fmt_ms(s.p95),
        fmt_ms(s.max),
        fmt_ms(s.mean)
    )
}

/// Run `f(i)` for i in 0..n on `threads` workers with `stack`-sized stacks.
fn parallel<T: Send>(
    n: usize,
    threads: usize,
    stack: usize,
    f: impl Fn(usize) -> T + Sync,
) -> Vec<T> {
    let next = AtomicUsize::new(0);
    let out: Mutex<Vec<(usize, T)>> = Mutex::new(Vec::with_capacity(n));
    std::thread::scope(|s| {
        for k in 0..threads.min(n.max(1)) {
            let (next, out, f) = (&next, &out, &f);
            std::thread::Builder::new()
                .name(format!("bench-{k}"))
                .stack_size(stack)
                .spawn_scoped(s, move || loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= n {
                        break;
                    }
                    let v = f(i);
                    out.lock().unwrap().push((i, v));
                })
                .expect("spawn worker");
        }
    });
    let mut v = out.into_inner().unwrap();
    v.sort_by_key(|(i, _)| *i);
    v.into_iter().map(|(_, t)| t).collect()
}

fn thumb_path(dir: &Path, id: AssetId, fingerprint: &str) -> PathBuf {
    // Same layout as src-tauri/src/preview/cache.rs.
    dir.join(format!("{:02x}", id % 256)).join(format!(
        "{id}-{fingerprint}-r{RENDERER_VERSION}-s{THUMB}.png"
    ))
}

fn oversize(size: u64) -> Analysis {
    Analysis {
        state: ProcessingState::LimitExceeded,
        error: Some("File exceeds rendering limit".into()),
        content_hash: String::new(),
        meta: SvgMeta::default(),
        text: SearchText::default(),
        complexity: Complexity {
            file_size: size,
            ..Default::default()
        },
    }
}

struct Analyzed {
    id: AssetId,
    analysis: Analysis,
    ms: f64,
}

fn main() {
    let args = parse_args();
    let root = std::fs::canonicalize(&args.dir).expect("dataset dir");
    let limits = Limits::default();
    // --work DIR keeps the index and thumbnails for inspection (DIR must not hold an index).
    let tmp = tempfile::tempdir().unwrap();
    let work: PathBuf = match &args.work {
        Some(w) => {
            std::fs::create_dir_all(w).unwrap();
            assert!(
                !w.join("index.sqlite").exists(),
                "{} already holds an index",
                w.display()
            );
            w.clone()
        }
        None => tmp.path().to_path_buf(),
    };
    let db_path = work.join("index.sqlite");
    let thumbs = work.join("thumbs");
    let mut report: Vec<String> = Vec::new();
    let mut targets: Vec<(String, String, String)> = Vec::new(); // (operation, target, measured)
    let say = |s: &str| eprintln!("[bench] {s}");

    println!("# bench_pipeline — {}", root.display());
    println!();
    println!(
        "threads: {} · worker stack: {} MiB · thumbnail: {THUMB}px · sample: {} · limits: default",
        args.threads,
        args.stack / (1024 * 1024),
        args.sample
    );

    // Fonts (the app warms these on a background thread at startup).
    let ((), font_ms) = time(warm_up_fonts);
    say(&format!("font database: {font_ms:.0} ms"));

    // ------------------------------------------------------------ 0. first uncached screen
    let first_screen_ms = {
        let t0 = Instant::now();
        let cancel = AtomicBool::new(false);
        let mut first = Vec::new();
        scan(&root, &ScanOptions::default(), &cancel, |b| {
            if first.is_empty() {
                first = b;
                cancel.store(true, Ordering::Relaxed);
            }
        })
        .unwrap();
        let first_batch = ms(t0.elapsed());
        let tiles: Vec<_> = first
            .into_iter()
            .filter(|f| {
                !args
                    .no_render
                    .iter()
                    .any(|p| f.relative_path.contains(p.as_str()))
            })
            .take(40)
            .collect();
        let root2 = &root;
        let limits2 = &limits;
        let done = parallel(tiles.len(), args.threads, args.stack, |i| {
            let p = to_absolute(root2, &tiles[i].relative_path);
            if tiles[i].file_size <= limits2.max_file_bytes {
                if let Ok(b) = std::fs::read(&p) {
                    let a = analyze(&b, limits2);
                    if a.state == ProcessingState::Ready {
                        let _ = render_thumbnail(&b, THUMB, limits2, p.parent());
                    }
                }
            }
            ms(t0.elapsed())
        });
        let first_tile = done.iter().cloned().fold(f64::INFINITY, f64::min);
        let total = ms(t0.elapsed());
        say(&format!("first screen: first batch {first_batch:.1} ms, first tile {first_tile:.1} ms, 40 tiles {total:.1} ms"));
        (first_batch, total, first_tile)
    };

    // ------------------------------------------------------------ a. discovery + insert
    let mut db = Database::open(&db_path).unwrap();
    let lib = db.upsert_library(&root.to_string_lossy(), "bench").unwrap();
    let t0 = Instant::now();
    let mut first_batch_ms = None;
    let mut first_insert_ms = None;
    let mut insert_ms = 0.0;
    let mut records: Vec<AssetRecord> = Vec::new();
    let mut batches = 0;
    let stats_a = scan(
        &root,
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |batch| {
            batches += 1;
            first_batch_ms.get_or_insert(ms(t0.elapsed()));
            let (recs, t) = time(|| db.insert_assets(lib.id, &batch).unwrap());
            insert_ms += t;
            first_insert_ms.get_or_insert(ms(t0.elapsed()));
            records.extend(recs);
        },
    )
    .unwrap();
    let discover_total = ms(t0.elapsed());
    let n = records.len();
    let total_bytes: u64 = records.iter().map(|r| r.file_size).sum();
    let (stats_pure, pure_ms) = time(|| {
        scan(
            &root,
            &ScanOptions::default(),
            &AtomicBool::new(false),
            |_| {},
        )
        .unwrap()
    });
    say(&format!("discovered {n} files in {discover_total:.0} ms"));

    // ------------------------------------------------------------ b. reconcile unchanged
    let (diff, reconcile_ms) = time(|| {
        let snap = db.library_snapshot(lib.id).unwrap();
        let mut rec = Reconciler::new(snap);
        let (mut new, mut changed, mut unchanged) = (0, 0, 0);
        scan(
            &root,
            &ScanOptions::default(),
            &AtomicBool::new(false),
            |b| {
                let d = rec.classify(b);
                new += d.new.len();
                changed += d.changed.len();
                unchanged += d.unchanged;
            },
        )
        .unwrap();
        let gone = rec.finish().len();
        (new, changed, unchanged, gone)
    });
    assert_eq!(
        (diff.0, diff.1, diff.3),
        (0, 0, 0),
        "tree changed during benchmark"
    );

    report.push("## Discovery and index".into());
    report.push(String::new());
    report.push("| Step | Result |".into());
    report.push("|---|---:|".into());
    report.push(format!(
        "| SVGs discovered | {n} ({:.1} MB, {} dirs, {} batches) |",
        total_bytes as f64 / MB as f64,
        stats_a.dirs_visited,
        batches
    ));
    report.push(format!(
        "| First batch (250 files) delivered | {} |",
        fmt_ms(first_batch_ms.unwrap_or(0.0))
    ));
    report.push(format!(
        "| First batch inserted in DB | {} |",
        fmt_ms(first_insert_ms.unwrap_or(0.0))
    ));
    report.push(format!(
        "| Discovery + per-batch DB insert, total | {} ({:.0} files/s) |",
        fmt_ms(discover_total),
        n as f64 / (discover_total / 1e3)
    ));
    report.push(format!("| … of which DB inserts | {} |", fmt_ms(insert_ms)));
    report.push(format!(
        "| Pure re-scan (warm cache) | {} ({:.0} files/s, {} found) |",
        fmt_ms(pure_ms),
        n as f64 / (pure_ms / 1e3),
        stats_pure.files_found
    ));
    report.push(format!(
        "| Reconcile unchanged tree (snapshot + scan + classify) | {} ({} unchanged) |",
        fmt_ms(reconcile_ms),
        diff.2
    ));
    report.push(String::new());

    // ------------------------------------------------------------ c. analyze
    let root_ref = &root;
    let limits_ref = &limits;
    let recs_ref = &records;
    say("analyzing…");
    let t0 = Instant::now();
    let analyzed: Vec<Analyzed> = parallel(n, args.threads, args.stack, |i| {
        let r = &recs_ref[i];
        let t = Instant::now();
        let analysis = if r.file_size > limits_ref.max_file_bytes {
            oversize(r.file_size)
        } else {
            match std::fs::read(to_absolute(root_ref, &r.relative_path)) {
                Ok(b) => analyze(&b, limits_ref),
                Err(e) => Analysis {
                    error: Some(e.to_string()),
                    ..oversize(r.file_size)
                },
            }
        };
        Analyzed {
            id: r.id,
            analysis,
            ms: ms(t.elapsed()),
        }
    });
    let analyze_wall = ms(t0.elapsed());
    let per_file: Vec<f64> = analyzed.iter().map(|a| a.ms).collect();
    let analyze_stats = stats(&per_file);
    let mut states: BTreeMap<&'static str, usize> = BTreeMap::new();
    for a in &analyzed {
        *states.entry(a.analysis.state.as_str()).or_default() += 1;
    }
    let mut slowest: Vec<(f64, &str)> = analyzed
        .iter()
        .zip(records.iter())
        .map(|(a, r)| (a.ms, r.relative_path.as_str()))
        .collect();
    slowest.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    let items: Vec<(AssetId, Analysis)> = analyzed
        .iter()
        .map(|a| (a.id, a.analysis.clone()))
        .collect();
    let ((), save_ms) = time(|| {
        for chunk in items.chunks(256) {
            db.save_analyses(chunk).unwrap();
        }
    });
    let text_bytes: usize = analyzed
        .iter()
        .map(|a| {
            let t = &a.analysis.text;
            t.title.len() + t.description.len() + t.visible_text.len() + t.identifiers.len()
        })
        .sum();
    say(&format!("analyzed in {analyze_wall:.0} ms"));

    report.push("## Analysis".into());
    report.push(String::new());
    report.push(format!(
        "Wall time {} for {n} files → **{:.0} files/s**, {:.1} MB/s on {} threads. States: {}. Extracted text: {:.1} MB. `save_analyses` (256/txn): {}.",
        fmt_ms(analyze_wall),
        n as f64 / (analyze_wall / 1e3),
        total_bytes as f64 / MB as f64 / (analyze_wall / 1e3),
        args.threads,
        states.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", "),
        text_bytes as f64 / MB as f64,
        fmt_ms(save_ms)
    ));
    report.push(String::new());
    report.push("| Per file (read + analyze) | n | p50 | p95 | max | mean |".into());
    report.push("|---|---:|---:|---:|---:|---:|".into());
    report.push(row("analyze", analyze_stats));
    report.push(String::new());
    report.push(format!(
        "Slowest: {}",
        slowest
            .iter()
            .take(5)
            .map(|(t, p)| format!("`{p}` {}", fmt_ms(*t)))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    report.push(String::new());
    drop(db);

    // ------------------------------------------------------------ d. cold start
    let mut cold: Vec<(f64, f64, f64, f64)> = Vec::new();
    let mut catalog = Catalog::new();
    for _ in 0..3 {
        let (db2, open_ms) = time(|| Database::open(&db_path).unwrap());
        let (rows, load_ms) = time(|| db2.load_catalog(lib.id).unwrap());
        let (cat, extend_ms) = time(|| {
            let mut c = Catalog::new();
            c.extend(rows);
            c
        });
        let (_, page_ms) = time(|| {
            let ids = cat.search("", None).unwrap();
            let first: Vec<AssetId> = ids.iter().take(200).copied().collect();
            cat.summaries(&first, None)
        });
        cold.push((open_ms, load_ms, extend_ms, page_ms));
        catalog = cat;
    }
    cold.sort_by(|a, b| {
        (a.0 + a.1 + a.2 + a.3)
            .partial_cmp(&(b.0 + b.1 + b.2 + b.3))
            .unwrap()
    });
    let (open_ms, load_ms, extend_ms, page_ms) = cold[1];
    let cold_total = open_ms + load_ms + extend_ms + page_ms;
    let db = Database::open(&db_path).unwrap();
    report.push("## Cold start (cached gallery path, median of 3)".into());
    report.push(String::new());
    report.push("| Database::open | load_catalog | Catalog::extend | first page (list + 200 summaries) | total |".into());
    report.push("|---:|---:|---:|---:|---:|".into());
    report.push(format!(
        "| {} | {} | {} | {} | **{}** |",
        fmt_ms(open_ms),
        fmt_ms(load_ms),
        fmt_ms(extend_ms),
        fmt_ms(page_ms),
        fmt_ms(cold_total)
    ));
    report.push(String::new());

    // ------------------------------------------------------------ e. search
    let queries: &[(&str, &str, &str)] = &[
        ("filename token", "pump", "path"),
        ("2 tokens", "motor controller", "path"),
        ("glob (name)", "*switch*.svg", "path"),
        ("glob (path)", "network/*/*.svg", "path"),
        ("regex", "re:pump.*(station|valve)", "path"),
        ("exclusion", "valve -legacy", "path"),
        ("no match", "zzqxj", "path"),
        ("content token", "supervisory", "content"),
        ("content token (rare)", "interlocked", "content"),
        ("phrase", "\"motor controller\"", "content"),
        ("content 3 tokens", "redundant ring ethernet", "content"),
    ];
    report.push("## Search (catalog in memory; 20 runs each)".into());
    report.push(String::new());
    report.push("| Query | Text | Hits | median | p95 | + first 100 summaries (median) |".into());
    report.push("|---|---|---:|---:|---:|---:|".into());
    let mut worst_path: f64 = 0.0;
    let mut worst_content: f64 = 0.0;
    for (label, q, class) in queries {
        let mut t = Vec::new();
        let mut t2 = Vec::new();
        let mut hits = 0;
        for _ in 0..20 {
            let (ids, a) = time(|| catalog.search(q, None).unwrap());
            hits = ids.len();
            let (_, b) = time(|| {
                let first: Vec<AssetId> = ids.iter().take(100).copied().collect();
                catalog.summaries(&first, Some(q))
            });
            t.push(a);
            t2.push(a + b);
        }
        let s = stats(&t);
        let s2 = stats(&t2);
        if *class == "path" {
            worst_path = worst_path.max(s.p95);
        } else {
            worst_content = worst_content.max(s.p95);
        }
        report.push(format!(
            "| {label} | `{}` | {hits} | {} | {} | {} |",
            q.replace('|', "\\|"),
            fmt_ms(s.p50),
            fmt_ms(s.p95),
            fmt_ms(s2.p50)
        ));
    }
    report.push(String::new());

    // ------------------------------------------------------------ f. thumbnails
    let meta: HashMap<AssetId, &Analysis> = analyzed.iter().map(|a| (a.id, &a.analysis)).collect();
    let ready: Vec<&AssetRecord> = records
        .iter()
        .filter(|r| {
            meta.get(&r.id)
                .map(|a| a.state == ProcessingState::Ready)
                .unwrap_or(false)
        })
        .filter(|r| {
            !args
                .no_render
                .iter()
                .any(|p| r.relative_path.contains(p.as_str()))
        })
        .collect();
    let per_file_mode = args.per_file.unwrap_or(n <= 200);
    let sample: Vec<&AssetRecord> = if ready.len() <= args.sample {
        ready.clone()
    } else {
        let step = ready.len() as f64 / args.sample as f64;
        (0..args.sample)
            .map(|i| ready[(i as f64 * step) as usize])
            .collect()
    };
    say(&format!("rendering {} thumbnails…", sample.len()));
    let thumbs_ref = &thumbs;
    let sample_ref = &sample;
    let t0 = Instant::now();
    let rendered: Vec<(f64, f64, Result<usize, String>)> =
        parallel(sample.len(), args.threads, args.stack, |i| {
            let r = sample_ref[i];
            let p = to_absolute(root_ref, &r.relative_path);
            let t = Instant::now();
            let bytes = std::fs::read(&p).unwrap();
            let t_render = Instant::now();
            let res = render_thumbnail(&bytes, THUMB, limits_ref, p.parent());
            let render = ms(t_render.elapsed());
            let res = res.map_err(|e| e.to_string()).map(|png| {
                let tp = thumb_path(thumbs_ref, r.id, &r.fast_fingerprint);
                std::fs::create_dir_all(tp.parent().unwrap()).unwrap();
                std::fs::write(&tp, &png).unwrap();
                png.len()
            });
            if per_file_mode {
                eprintln!("[bench]   thumb {:>9.1} ms  {}", render, r.relative_path);
            }
            (render, ms(t.elapsed()), res)
        });
    let thumb_wall = ms(t0.elapsed());
    let render_stats = stats(&rendered.iter().map(|r| r.0).collect::<Vec<_>>());
    let total_stats = stats(&rendered.iter().map(|r| r.1).collect::<Vec<_>>());
    let failures: Vec<(&str, &String)> = rendered
        .iter()
        .zip(sample.iter())
        .filter_map(|(r, rec)| r.2.as_ref().err().map(|e| (rec.relative_path.as_str(), e)))
        .collect();
    let png_bytes: usize = rendered.iter().filter_map(|r| r.2.as_ref().ok()).sum();
    // Cache hits: path derivation + read of the PNG (page cache warm: just written).
    let mut hit = Vec::new();
    for r in &sample {
        let t = Instant::now();
        let tp = thumb_path(&thumbs, r.id, &r.fast_fingerprint);
        if let Ok(b) = std::fs::read(&tp) {
            std::hint::black_box(b);
            hit.push(ms(t.elapsed()));
        }
    }
    let hit_stats = stats(&hit);
    // DB bookkeeping cost (record_thumbnail) as the db-writer would do it.
    let (_, record_ms) = time(|| {
        let mut db = Database::open(&db_path).unwrap();
        for r in &sample {
            db.record_thumbnail(
                r.id,
                THUMB,
                &format!("{}-r{RENDERER_VERSION}-s{THUMB}", r.fast_fingerprint),
            )
            .unwrap();
        }
    });
    let mut slow_thumbs: Vec<(f64, &str)> = rendered
        .iter()
        .zip(sample.iter())
        .map(|(r, s)| (r.0, s.relative_path.as_str()))
        .collect();
    slow_thumbs.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());

    report.push(format!("## Thumbnails ({THUMB}px)"));
    report.push(String::new());
    report.push(format!(
        "{} rendered in {} on {} threads ({:.0} thumbs/s), avg PNG {:.1} KB, {} failed. `record_thumbnail` ×{}: {}.",
        sample.len(),
        fmt_ms(thumb_wall),
        args.threads,
        sample.len() as f64 / (thumb_wall / 1e3),
        png_bytes as f64 / 1024.0 / (sample.len() - failures.len()).max(1) as f64,
        failures.len(),
        sample.len(),
        fmt_ms(record_ms)
    ));
    report.push(String::new());
    report.push("| Per file | n | p50 | p95 | max | mean |".into());
    report.push("|---|---:|---:|---:|---:|---:|".into());
    report.push(row("render_thumbnail", render_stats));
    report.push(row("read + render + cache write", total_stats));
    report.push(row("cache hit (read PNG, warm)", hit_stats));
    report.push(String::new());
    report.push(format!(
        "Slowest renders: {}",
        slow_thumbs
            .iter()
            .take(5)
            .map(|(t, p)| format!("`{p}` {}", fmt_ms(*t)))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    if !failures.is_empty() {
        report.push(String::new());
        report.push("Render failures (Ready files):".into());
        for (p, e) in failures.iter().take(20) {
            report.push(format!("- `{p}`: {e}"));
        }
    }
    report.push(String::new());

    // ------------------------------------------------------------ g. viewer open
    let mut viewer = Vec::new();
    for r in sample.iter().take(200) {
        let t = Instant::now();
        let b = std::fs::read(to_absolute(&root, &r.relative_path)).unwrap();
        let m = &meta[&r.id].meta;
        let out = prepare_for_viewer(&b, m);
        std::hint::black_box(out.len());
        viewer.push(ms(t.elapsed()));
    }
    let viewer_stats = stats(&viewer);

    // ------------------------------------------------------------ h. region copy
    let mut big: Vec<&&AssetRecord> = ready.iter().filter(|r| r.file_size <= 2 * MB).collect();
    big.sort_by_key(|r| std::cmp::Reverse(r.file_size));
    let mut crop_t = Vec::new();
    let mut region_t = Vec::new();
    let mut region_small_t = Vec::new();
    let mut crop_fail = Vec::new();
    for r in big.iter().take(10) {
        let Some(dbox) = db.get_doc_box(r.id).unwrap() else {
            continue;
        };
        let region = Region {
            x: dbox.min_x + dbox.width * 0.25,
            y: dbox.min_y + dbox.height * 0.25,
            width: dbox.width * 0.5,
            height: dbox.height * 0.5,
        };
        let p = to_absolute(&root, &r.relative_path);
        let t = Instant::now();
        let b = std::fs::read(&p).unwrap();
        match crop_svg(&b, region) {
            Ok(s) => {
                crop_t.push(ms(t.elapsed()));
                std::hint::black_box(s.len());
            }
            Err(e) => crop_fail.push(format!("{}: {e}", r.relative_path)),
        }
        // Same region at 0.25x: dominated by parse/crop/text layout rather than raster + PNG.
        let t = Instant::now();
        if render_region_png(&b, region, 0.25, Background::White, &limits, p.parent()).is_ok() {
            region_small_t.push(ms(t.elapsed()));
        }
        let t = Instant::now();
        match render_region_png(&b, region, 2.0, Background::White, &limits, p.parent()) {
            Ok(png) => {
                region_t.push(ms(t.elapsed()));
                std::hint::black_box(png.png.len());
            }
            Err(e) => crop_fail.push(format!("{}: {e}", r.relative_path)),
        }
    }
    let crop_stats = stats(&crop_t);
    let region_stats = stats(&region_t);
    report.push("## Viewer and region copy".into());
    report.push(String::new());
    report.push("| Operation | n | p50 | p95 | max | mean |".into());
    report.push("|---|---:|---:|---:|---:|---:|".into());
    report.push(row("viewer open: read + prepare_for_viewer", viewer_stats));
    report.push(row("crop_svg (read + crop, largest files)", crop_stats));
    report.push(row(
        "render_region_png (50% region, 2x, white)",
        region_stats,
    ));
    report.push(row(
        "render_region_png (same region, 0.25x)",
        stats(&region_small_t),
    ));
    if !crop_fail.is_empty() {
        report.push(String::new());
        for f in &crop_fail {
            report.push(format!("- crop failure: {f}"));
        }
    }
    report.push(String::new());

    // ------------------------------------------------------------ summary vs targets
    let mark = |ok: bool| if ok { "✅" } else { "⚠️" };
    targets.push((
        "Cached gallery visible (core part)".into(),
        "<500 ms".into(),
        format!("{} {}", fmt_ms(cold_total), mark(cold_total < 500.0)),
    ));
    targets.push((
        "First uncached assets (first batch + 40 tiles)".into(),
        "<1 s".into(),
        format!(
            "first tile {}, 40 tiles {} (first batch {}) {}",
            fmt_ms(first_screen_ms.2),
            fmt_ms(first_screen_ms.1),
            fmt_ms(first_screen_ms.0),
            mark(first_screen_ms.1 < 1000.0)
        ),
    ));
    targets.push((
        "Search filename/path (worst p95)".into(),
        "<50 ms".into(),
        format!("{} {}", fmt_ms(worst_path), mark(worst_path < 50.0)),
    ));
    targets.push((
        "Search full content (worst p95)".into(),
        "<100 ms".into(),
        format!("{} {}", fmt_ms(worst_content), mark(worst_content < 100.0)),
    ));
    targets.push((
        "Normal viewer open (core part, p95)".into(),
        "<200 ms".into(),
        format!(
            "{} {}",
            fmt_ms(viewer_stats.p95),
            mark(viewer_stats.p95 < 200.0)
        ),
    ));
    targets.push((
        "Thumbnail cache hit (p95)".into(),
        "<30 ms".into(),
        format!("{} {}", fmt_ms(hit_stats.p95), mark(hit_stats.p95 < 30.0)),
    ));
    targets.push((
        "Region-copy initiation (render_region_png p50)".into(),
        "<200 ms typical".into(),
        format!(
            "{} (max {}) {}",
            fmt_ms(region_stats.p50),
            fmt_ms(region_stats.max),
            mark(region_stats.p50 < 200.0)
        ),
    ));
    targets.push((
        "Thumbnail render (p95 / max) — no stall >50 ms on UI thread".into(),
        "off UI thread".into(),
        format!(
            "{} / {}",
            fmt_ms(render_stats.p95),
            fmt_ms(render_stats.max)
        ),
    ));

    println!();
    println!("## Targets (plan §34)");
    println!();
    println!("| Operation | Target | Measured |");
    println!("|---|---|---|");
    for (op, t, m) in &targets {
        println!("| {op} | {t} | {m} |");
    }
    println!();
    println!(
        "Font database load (one-off, background at startup): {}",
        fmt_ms(font_ms)
    );
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        if let Some(l) = status.lines().find(|l| l.starts_with("VmHWM:")) {
            println!();
            println!(
                "Peak RSS of the benchmark process: {}",
                l.trim_start_matches("VmHWM:").trim()
            );
        }
    }
    println!();
    for l in &report {
        println!("{l}");
    }
}
