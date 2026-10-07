//! Pathological-input hardening tests (plan §33 dataset E, WP-20).
//!
//! The cases come from `examples/gen_dataset.rs` (compiled in via `#[path]`), written at
//! reduced sizes ([`gen::PathoParams::small`]) with matching reduced [`Limits`], so the
//! same code paths trip (file-size, node-count, embedded-raster limits) without writing
//! 100 MB in a test.
//!
//! Every analysis and render runs on a freshly spawned thread with an explicit 2 MiB
//! stack — the std/tokio default the app's preview workers use — and must finish within
//! [`BUDGET`]. A stack overflow would abort the whole test binary, so these tests also
//! guard the nesting-depth limit (`Limits::max_nesting_depth`) and the render text limits
//! (`max_text_node_chars`, `max_render_text_chars`) added for the bugs recorded in
//! docs/performance.md.

#[allow(unused_imports)]
#[path = "../examples/gen_dataset.rs"]
mod gen;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use gen::{Expect, PathoCase, PathoParams};
use resvg::tiny_skia::Pixmap;
use svg_core::config::Limits;
use svg_core::error::CoreError;
use svg_core::index::Database;
use svg_core::library::scanner::{scan, ScanOptions};
use svg_core::model::{Analysis, AssetId, DiscoveredFile, ProcessingState, Region};
use svg_core::search::Catalog;
use svg_core::svg::{
    analyze, crop_svg, prepare_for_viewer, render_png, render_region_png, render_thumbnail,
    Background,
};

const BUDGET: Duration = Duration::from_secs(5);
const SEED: u64 = 0xDEC0DE;

/// Default std/tokio worker stack, as used by the app's preview workers.
const WORKER_STACK: usize = 2 * 1024 * 1024;

/// Run `f` on a new thread with `stack` bytes of stack; Err on panic or when it exceeds
/// [`BUDGET`].
fn bounded<T: Send + 'static>(
    what: &str,
    stack: usize,
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<(T, Duration), String> {
    let (tx, rx) = mpsc::channel();
    let started = Instant::now();
    std::thread::Builder::new()
        .name(format!("patho:{what}"))
        .stack_size(stack)
        .spawn(move || {
            let out = f();
            let _ = tx.send(out);
        })
        .expect("spawn");
    match rx.recv_timeout(BUDGET) {
        Ok(v) => Ok((v, started.elapsed())),
        Err(mpsc::RecvTimeoutError::Timeout) => Err(format!("{what}: exceeded {BUDGET:?}")),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(format!("{what}: panicked")),
    }
}

fn generate_small() -> (tempfile::TempDir, PathBuf, Vec<PathoCase>, Limits) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("E");
    let p = PathoParams::small();
    let cases = gen::write_pathological(&root, &p, SEED).unwrap();
    (tmp, root, cases, p.limits())
}

fn discover(root: &Path) -> Vec<DiscoveredFile> {
    let mut all = Vec::new();
    let stats = scan(
        root,
        &ScanOptions::default(),
        &AtomicBool::new(false),
        |b| all.extend(b),
    )
    .unwrap();
    assert!(!stats.cancelled);
    assert_eq!(stats.files_found, all.len());
    all
}

fn state_matches(expect: Expect, state: ProcessingState) -> bool {
    match expect {
        Expect::Ready => state == ProcessingState::Ready,
        Expect::LimitExceeded => state == ProcessingState::LimitExceeded,
        Expect::ParseError => state == ProcessingState::ParseError,
        Expect::Analyzed => matches!(
            state,
            ProcessingState::Ready | ProcessingState::LimitExceeded | ProcessingState::ParseError
        ),
        Expect::Skipped => false,
    }
}

fn analyze_file(root: &Path, rel: &str, limits: &Limits) -> Analysis {
    analyze(&std::fs::read(root.join(rel)).unwrap(), limits)
}

#[test]
fn every_pathological_case_is_handled_cleanly() {
    let (_tmp, root, cases, limits) = generate_small();

    // Discovery: hidden entries and non-.svg names skipped, everything else found.
    let found: HashSet<String> = discover(&root)
        .into_iter()
        .map(|f| f.relative_path)
        .collect();
    let mut failures: Vec<String> = Vec::new();
    for c in &cases {
        let present = found.contains(&c.rel);
        if (c.expect == Expect::Skipped) == present {
            failures.push(format!(
                "{}: discovered={present}, expected {:?}",
                c.rel, c.expect
            ));
        }
    }
    let expected_found = cases.iter().filter(|c| c.expect != Expect::Skipped).count();
    assert_eq!(found.len(), expected_found, "discovered set: {found:?}");

    println!("| case | bytes | state | analyze ms | thumbnail | render ms |");
    println!("|---|---:|---|---:|---|---:|");
    for c in cases.iter().filter(|c| c.expect != Expect::Skipped) {
        let path = root.join(&c.rel);
        let bytes = std::fs::read(&path).unwrap();
        let len = bytes.len();
        let shared = std::sync::Arc::new(bytes);
        let stack = WORKER_STACK;

        let (b, l) = (shared.clone(), limits.clone());
        let analysis = match bounded(&format!("analyze {}", c.rel), stack, move || {
            analyze(&b, &l)
        }) {
            Ok((a, t)) => Some((a, t)),
            Err(e) => {
                failures.push(e);
                None
            }
        };
        let (b, l, dir) = (
            shared.clone(),
            limits.clone(),
            path.parent().unwrap().to_path_buf(),
        );
        let thumb = match bounded(&format!("thumbnail {}", c.rel), stack, move || {
            render_thumbnail(&b, 256, &l, Some(&dir))
        }) {
            Ok((r, t)) => Some((r, t)),
            Err(e) => {
                failures.push(e);
                None
            }
        };

        if let Some((a, _)) = &analysis {
            if !state_matches(c.expect, a.state) {
                failures.push(format!(
                    "{}: state {:?} (error {:?}), expected {:?} — {}",
                    c.rel, a.state, a.error, c.expect, c.note
                ));
            }
            if a.state != ProcessingState::Ready && a.error.as_deref().unwrap_or("").is_empty() {
                failures.push(format!(
                    "{}: non-ready state without an error message",
                    c.rel
                ));
            }
        }
        if let Some((r, _)) = &thumb {
            match (c.thumb, r) {
                (Some(true), Err(e)) => failures.push(format!("{}: thumbnail failed: {e}", c.rel)),
                (Some(false), Ok(_)) => {
                    failures.push(format!("{}: thumbnail unexpectedly rendered", c.rel))
                }
                _ => {}
            }
            if let Ok(png) = r {
                match Pixmap::decode_png(png) {
                    Ok(p) if p.width() <= 256 && p.height() <= 256 && p.width() > 0 => {}
                    Ok(p) => {
                        failures.push(format!("{}: thumbnail {}x{}", c.rel, p.width(), p.height()))
                    }
                    Err(e) => failures.push(format!("{}: thumbnail is not a PNG: {e}", c.rel)),
                }
            }
        }
        println!(
            "| {} | {} | {} | {} | {} | {} |",
            c.rel,
            len,
            analysis
                .as_ref()
                .map(|(a, _)| a.state.as_str())
                .unwrap_or("TIMEOUT/PANIC"),
            analysis
                .as_ref()
                .map(|(_, t)| format!("{:.1}", t.as_secs_f64() * 1e3))
                .unwrap_or_default(),
            match &thumb {
                Some((Ok(_), _)) => "ok".to_string(),
                Some((Err(e), _)) => format!(
                    "err: {}",
                    e.to_string().chars().take(60).collect::<String>()
                ),
                None => "TIMEOUT/PANIC".into(),
            },
            thumb
                .as_ref()
                .map(|(_, t)| format!("{:.1}", t.as_secs_f64() * 1e3))
                .unwrap_or_default(),
        );
    }
    assert!(
        failures.is_empty(),
        "{} failure(s):\n{}",
        failures.len(),
        failures.join("\n")
    );
}

fn nested(depth: usize, view_box: bool) -> Vec<u8> {
    let mut s = String::from("<svg xmlns=\"http://www.w3.org/2000/svg\"");
    if view_box {
        s.push_str(" viewBox=\"0 0 10 10\"");
    }
    s.push('>');
    s.push_str(&"<g>".repeat(depth));
    s.push_str("<rect width=\"5\" height=\"5\"/>");
    s.push_str(&"</g>".repeat(depth));
    s.push_str("</svg>");
    s.into_bytes()
}

fn is_limit<T>(r: &svg_core::Result<T>) -> bool {
    matches!(r, Err(CoreError::Limit(_)))
}

/// Outcome of every entry point on one document, computed on a 2 MiB thread.
#[derive(Debug)]
struct EntryPoints {
    analysis: Analysis,
    thumbnail: svg_core::Result<Vec<u8>>,
    png: svg_core::Result<()>,
    region: svg_core::Result<()>,
    crop: svg_core::Result<()>,
}

fn run_all_entry_points(what: &str, doc: Vec<u8>) -> EntryPoints {
    let (out, took) = bounded(what, WORKER_STACK, move || {
        let limits = Limits::default();
        let analysis = analyze(&doc, &limits);
        // The viewer path must not recurse unboundedly either.
        let _ = prepare_for_viewer(&doc, &analysis.meta);
        let region = Region {
            x: 0.0,
            y: 0.0,
            width: 5.0,
            height: 5.0,
        };
        EntryPoints {
            thumbnail: render_thumbnail(&doc, 128, &limits, None),
            png: render_png(&doc, 1.0, Background::White, &limits, None).map(|_| ()),
            region: render_region_png(&doc, region, 1.0, Background::White, &limits, None)
                .map(|_| ()),
            crop: crop_svg(&doc, region).map(|_| ()),
            analysis,
        }
    })
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(took < BUDGET, "{what}: {took:?}");
    out
}

/// Regression (docs/performance.md, problems 1–2): nesting deep enough to overflow
/// roxmltree's recursive tokenizer (~3,000+ levels) or usvg's recursive converter
/// (~600–1,020 levels in release) used to abort the process on a 2 MiB worker stack.
/// The pre-parse depth scan must turn all of it into a clean LimitExceeded.
#[test]
fn deep_nesting_is_limit_exceeded_on_a_2mib_stack() {
    let max = Limits::default().max_nesting_depth as usize;
    for depth in [max + 1, 600, 1_000, 1_020, 3_500, 5_000, 100_000] {
        for view_box in [true, false] {
            let what = format!("{depth} levels, viewBox={view_box}");
            let r = run_all_entry_points(&what, nested(depth, view_box));
            assert_eq!(
                r.analysis.state,
                ProcessingState::LimitExceeded,
                "{what}: {r:?}"
            );
            assert!(
                r.analysis
                    .error
                    .as_deref()
                    .unwrap_or("")
                    .contains("nested too deeply"),
                "{what}: {:?}",
                r.analysis.error
            );
            assert!(is_limit(&r.thumbnail), "{what}: {:?}", r.thumbnail);
            assert!(is_limit(&r.png), "{what}: {:?}", r.png);
            assert!(is_limit(&r.region), "{what}: {:?}", r.region);
            // crop_svg only parses (hard cap 1,024): deeper input is a Limit error, shallower
            // input may crop — either way it must return.
            if depth > 1_024 {
                assert!(is_limit(&r.crop), "{what}: {:?}", r.crop);
            }
        }
    }
}

/// Just under the nesting limit everything still works on a 2 MiB stack — including
/// usvg's recursive converter in release builds.
#[test]
fn nesting_at_the_limit_renders_on_a_2mib_stack() {
    let depth = Limits::default().max_nesting_depth as usize - 2; // + <svg> + <rect>
    for view_box in [true, false] {
        let what = format!("{depth} levels, viewBox={view_box}");
        let r = run_all_entry_points(&what, nested(depth, view_box));
        assert_eq!(r.analysis.state, ProcessingState::Ready, "{what}: {r:?}");
        assert!(r.thumbnail.is_ok(), "{what}: {:?}", r.thumbnail);
        assert!(r.png.is_ok(), "{what}: {:?}", r.png);
        assert!(r.crop.is_ok(), "{what}: {:?}", r.crop);
    }
}

fn text_doc(runs: usize, chars_per_run: usize) -> Vec<u8> {
    let mut s = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"800\" height=\"{0}\" viewBox=\"0 0 800 {0}\">",
        runs * 14 + 20
    );
    let mut run = String::new();
    while run.len() < chars_per_run {
        run.push_str("supervisory control network ");
    }
    run.truncate(chars_per_run);
    for i in 0..runs {
        s.push_str(&format!(
            "<text x=\"4\" y=\"{}\" font-size=\"12\">{run}</text>",
            12 + i * 14
        ));
    }
    s.push_str("</svg>");
    s.into_bytes()
}

/// Regression (docs/performance.md, problem 3): one 200 KB `<text>` took 14.6 s to render
/// and the 5 MB dataset file exhausted 6 GB; 5 MB spread over 21k `<text>` took 33 s and
/// 2.7 GB. Both kinds must now be rejected before layout, quickly, on a 2 MiB stack.
#[test]
fn massive_text_is_limit_exceeded_quickly() {
    let limits = Limits::default();
    let cases = [
        ("one 200 KB run", text_doc(1, 200_000), "Text run too long"),
        (
            "one run just over max_text_node_chars",
            text_doc(1, limits.max_text_node_chars as usize + 1),
            "Text run too long",
        ),
        (
            "1,000 × 250-char runs",
            text_doc(1_000, 250),
            "Too much text",
        ),
        (
            "20,000 × 250-char runs (5 MB)",
            text_doc(20_000, 250),
            "Too much text",
        ),
    ];
    for (what, doc, reason) in cases {
        let r = run_all_entry_points(what, doc);
        assert_eq!(r.analysis.state, ProcessingState::LimitExceeded, "{what}");
        assert!(
            r.analysis.error.as_deref().unwrap_or("").contains(reason),
            "{what}: {:?}",
            r.analysis.error
        );
        assert!(is_limit(&r.thumbnail), "{what}: {:?}", r.thumbnail);
        assert!(is_limit(&r.png), "{what}: {:?}", r.png);
    }
}

/// The permitted maxima still render within budget on a 2 MiB stack.
#[test]
fn text_at_the_limits_renders_in_bounded_time() {
    let limits = Limits::default();
    let one_run = text_doc(1, limits.max_text_node_chars as usize);
    let total = text_doc(100, 1_000); // 100k characters, half the default total limit
    for (what, doc) in [
        ("max single run", one_run),
        ("100 × 1,000-char runs", total),
    ] {
        let r = run_all_entry_points(what, doc);
        assert_eq!(
            r.analysis.state,
            ProcessingState::Ready,
            "{what}: {:?}",
            r.analysis.error
        );
        assert!(r.thumbnail.is_ok(), "{what}: {:?}", r.thumbnail);
    }
}

#[test]
fn analysis_details_of_tricky_inputs() {
    let (_tmp, root, _cases, limits) = generate_small();
    let a = |rel: &str| analyze_file(&root, rel, &limits);

    // Entities declared in an internal DTD are expanded (Illustrator exports rely on it).
    let ent = a("dtd/entities-illustrator-style.svg");
    assert_eq!(ent.state, ProcessingState::Ready, "{:?}", ent.error);
    assert!(
        ent.text.title.contains("Acme Process Controls"),
        "{:?}",
        ent.text
    );

    // External entities are never resolved.
    let xxe = a("dtd/xxe-external-entity.svg");
    assert!(!xxe.text.visible_text.contains("root:"), "{:?}", xxe.text);

    // Flags.
    let script = a("content/script-and-handlers.svg");
    assert!(script.complexity.has_scripts);
    for rel in [
        "external/http-image.svg",
        "external/file-uri-image.svg",
        "external/relative-image.svg",
        "external/use-other-file.svg",
    ] {
        assert!(a(rel).complexity.has_external_refs, "{rel}");
    }
    assert!(!a("embedded/png-small.svg").complexity.has_external_refs);

    // Specific limit reasons.
    let raster = a("embedded/png-huge.svg");
    assert_eq!(raster.state, ProcessingState::LimitExceeded);
    assert!(
        raster
            .error
            .as_deref()
            .unwrap_or("")
            .contains("Embedded images"),
        "{:?}",
        raster.error
    );
    let nodes = a("limits/node-bomb.svg");
    assert!(
        nodes.error.as_deref().unwrap_or("").contains("nodes"),
        "{:?}",
        nodes.error
    );
    let over = a("limits/just-over-limit-4.2mb.svg");
    assert!(
        over.error.as_deref().unwrap_or("").contains("File exceeds"),
        "{:?}",
        over.error
    );

    // Render text limits and nesting limit.
    let big = a("text/massive-text-single-node.svg");
    assert_eq!(big.state, ProcessingState::LimitExceeded);
    assert!(
        big.error
            .as_deref()
            .unwrap_or("")
            .contains("Text run too long"),
        "{:?}",
        big.error
    );
    let many = a("text/massive-text-many-nodes.svg");
    assert_eq!(many.state, ProcessingState::LimitExceeded);
    assert!(
        many.error
            .as_deref()
            .unwrap_or("")
            .contains("Too much text"),
        "{:?}",
        many.error
    );
    let run = a("text/long-text-run-9000.svg");
    assert_eq!(run.state, ProcessingState::Ready);
    let n = run.text.visible_text.chars().count();
    assert!((8_990..=9_000).contains(&n), "{n}"); // whole run kept (whitespace collapsed)
    for rel in [
        "limits/nested-groups-5000.svg",
        "limits/nested-groups-1000.svg",
    ] {
        let n = a(rel);
        assert!(
            n.error
                .as_deref()
                .unwrap_or("")
                .contains("nested too deeply"),
            "{rel}: {:?}",
            n.error
        );
    }

    // Encodings decode to the same text.
    for rel in [
        "encoding/utf16le-bom.svg",
        "encoding/utf16be-bom.svg",
        "encoding/utf8-bom.svg",
    ] {
        let t = a(rel);
        assert!(
            t.text.title.contains("Größe Ω"),
            "{rel}: {:?}",
            t.text.title
        );
        assert!(
            t.text.visible_text.contains("Grüße ☃"),
            "{rel}: {:?}",
            t.text.visible_text
        );
    }
    assert!(a("encoding/latin1-declared.svg")
        .text
        .title
        .contains("Café Æø"));

    // Geometry.
    let nz = a("geometry/nonzero-origin-viewbox.svg");
    let db = nz.meta.doc_box.unwrap();
    assert_eq!(
        (db.min_x, db.min_y, db.width, db.height),
        (-500.0, 300.0, 1000.0, 800.0)
    );
    let nv = a("geometry/no-viewbox-no-size.svg");
    assert!(nv.meta.doc_box.is_some(), "fallback document box expected");
    let pct = a("geometry/percent-size.svg");
    assert_eq!(pct.meta.width, None);
    assert_eq!(pct.meta.view_box.unwrap().width, 400.0);
}

#[test]
fn unicode_names_round_trip_through_index_and_search() {
    let (_tmp, root, _cases, limits) = generate_small();
    let files = discover(&root);
    let names: HashSet<&str> = files.iter().map(|f| f.relative_path.as_str()).collect();
    for rel in [
        "ünïcødé/目录/Ελληνικά σύμβολα.svg",
        "unicode/café-nfc.svg",
        "unicode/cafe\u{0301}-nfd.svg",
        "unicode/🚀-launch-👍🏽-👨\u{200D}👩\u{200D}👧.svg",
        "special chars/pump (copy) #2 [final] & co's ~$tmp; %20.svg",
        "UPPERCASE-EXTENSION.SVG",
    ] {
        assert!(names.contains(rel), "missing {rel}");
    }

    let mut db = Database::open_in_memory().unwrap();
    let lib = db.upsert_library(&root.to_string_lossy(), "E").unwrap();
    let recs = db.insert_assets(lib.id, &files).unwrap();
    let analyses: Vec<(AssetId, Analysis)> = recs
        .iter()
        .map(|r| (r.id, analyze_file(&root, &r.relative_path, &limits)))
        .collect();
    db.save_analyses(&analyses).unwrap();
    let mut cat = Catalog::new();
    cat.extend(db.load_catalog(lib.id).unwrap());
    assert_eq!(cat.len(), files.len());

    let find = |q: &str| -> Vec<String> {
        cat.search(q, None)
            .unwrap()
            .into_iter()
            .map(|id| cat.get(id).unwrap().relative_path.clone())
            .collect()
    };
    assert!(
        find("网络交换机").iter().any(|p| p.contains("网络交换机")),
        "CJK title"
    );
    assert!(
        find("主控制器").iter().any(|p| p.contains("网络交换机")),
        "CJK visible text"
    );
    assert!(
        find("Ελληνικά").iter().any(|p| p.contains("Ελληνικά")),
        "Greek path"
    );
    assert!(
        find("αντλία").iter().any(|p| p.contains("Ελληνικά")),
        "Greek content"
    );
    assert!(find("펌프").iter().any(|p| p.contains("펌프")), "Hangul");
    assert!(find("مضخة").iter().any(|p| p.contains("rtl")), "Arabic");
    assert!(find("🚀").iter().any(|p| p.contains("launch")), "emoji");
    assert!(
        find("café-nfc").iter().any(|p| p.ends_with("café-nfc.svg")),
        "NFC name"
    );
    assert!(
        find("Acme").iter().any(|p| p.contains("entities")),
        "entity-expanded text"
    );
    assert!(
        find("\"pump (copy)\"")
            .iter()
            .any(|p| p.starts_with("special chars/")),
        "special chars"
    );
}

#[test]
fn crop_on_non_zero_viewbox_origin() {
    let src = gen::NONZERO_ORIGIN.as_bytes();
    let limits = Limits::default();
    let a = analyze(src, &limits);
    assert_eq!(a.state, ProcessingState::Ready);
    let db = a.meta.doc_box.unwrap();
    assert_eq!((db.min_x, db.min_y), (-500.0, 300.0));

    // Quadrants in user space: red (-500,300) green (0,300) blue (-500,700) yellow (0,700).
    let quads: [(f64, f64, [u8; 3]); 4] = [
        (-500.0, 300.0, [0xE5, 0x39, 0x35]),
        (0.0, 300.0, [0x43, 0xA0, 0x47]),
        (-500.0, 700.0, [0x1E, 0x88, 0xE5]),
        (0.0, 700.0, [0xFD, 0xD8, 0x35]),
    ];
    for (x, y, rgb) in quads {
        let region = Region {
            x: x + 50.0,
            y: y + 50.0,
            width: 400.0,
            height: 300.0,
        };
        let svg = crop_svg(src, region).unwrap();
        let ca = analyze(svg.as_bytes(), &limits);
        assert_eq!(ca.state, ProcessingState::Ready, "{:?}", ca.error);
        let vb = ca.meta.view_box.unwrap();
        assert_eq!(
            (vb.min_x, vb.min_y, vb.width, vb.height),
            (0.0, 0.0, 400.0, 300.0)
        );
        // Source is 500x400 px for 1000x800 units → 0.5 px/unit, so 200x150 px at scale 1.
        let started = Instant::now();
        let png = render_region_png(src, region, 1.0, Background::White, &limits, None).unwrap();
        assert!(started.elapsed() < BUDGET);
        assert_eq!((png.width, png.height), (200, 150));
        let pm = Pixmap::decode_png(&png.png).unwrap();
        for (px, py) in [(5, 5), (100, 75), (194, 144)] {
            let c = pm.pixel(px, py).unwrap();
            assert_eq!(
                [c.red(), c.green(), c.blue()],
                rgb,
                "region at ({x},{y}) pixel ({px},{py})"
            );
        }
    }
    // A region straddling all four quadrants around the centre (0, 700).
    let png = render_region_png(
        src,
        Region {
            x: -100.0,
            y: 600.0,
            width: 200.0,
            height: 200.0,
        },
        2.0,
        Background::Transparent,
        &limits,
        None,
    )
    .unwrap();
    let pm = Pixmap::decode_png(&png.png).unwrap();
    let at = |x: u32, y: u32| {
        let c = pm.pixel(x, y).unwrap();
        [c.red(), c.green(), c.blue()]
    };
    let (w, h) = (pm.width(), pm.height());
    assert_eq!(at(2, 2), [0xE5, 0x39, 0x35]);
    assert_eq!(at(w - 3, 2), [0x43, 0xA0, 0x47]);
    assert_eq!(at(2, h - 3), [0x1E, 0x88, 0xE5]);
    assert_eq!(at(w - 3, h - 3), [0xFD, 0xD8, 0x35]);
}

#[test]
fn generator_is_deterministic_and_datasets_are_valid() {
    let tmp = tempfile::tempdir().unwrap();
    let p = PathoParams::small();
    gen::write_pathological(&tmp.path().join("e1"), &p, SEED).unwrap();
    gen::write_pathological(&tmp.path().join("e2"), &p, SEED).unwrap();
    let h1 = gen::hash_tree(&tmp.path().join("e1")).unwrap();
    let h2 = gen::hash_tree(&tmp.path().join("e2")).unwrap();
    assert_eq!(h1, h2, "dataset E not reproducible");

    let icons = tmp.path().join("icons");
    let s = gen::generate_tree(&icons, 400, 7, false).unwrap();
    assert_eq!(s.svg_files, 400);
    let again = tmp.path().join("icons2");
    gen::generate_tree(&again, 400, 7, false).unwrap();
    assert_eq!(
        gen::hash_tree(&icons).unwrap(),
        gen::hash_tree(&again).unwrap()
    );
    let other = tmp.path().join("icons3");
    gen::generate_tree(&other, 400, 8, false).unwrap();
    assert_ne!(
        gen::hash_tree(&icons).unwrap().0,
        gen::hash_tree(&other).unwrap().0
    );

    let diagrams = tmp.path().join("diagrams");
    gen::generate_tree(&diagrams, 120, 7, true).unwrap();

    let limits = Limits::default();
    for (root, expected) in [(&icons, 400), (&diagrams, 120)] {
        let files = discover(root);
        assert_eq!(files.len(), expected, "hidden dirs/decoys must be skipped");
        for (i, f) in files.iter().enumerate() {
            let bytes = std::fs::read(root.join(&f.relative_path)).unwrap();
            let a = analyze(&bytes, &limits);
            assert_eq!(
                a.state,
                ProcessingState::Ready,
                "{}: {:?}",
                f.relative_path,
                a.error
            );
            if i % 8 == 0 {
                render_thumbnail(&bytes, 128, &limits, None)
                    .unwrap_or_else(|e| panic!("{}: {e}", f.relative_path));
            }
        }
    }
    // Diagrams carry substantial searchable text.
    let files = discover(&diagrams);
    let a = analyze(
        &std::fs::read(diagrams.join(&files[0].relative_path)).unwrap(),
        &limits,
    );
    assert!(
        a.text.visible_text.len() > 500,
        "{}",
        a.text.visible_text.len()
    );
    assert!(!a.text.title.is_empty() && !a.text.description.is_empty());
    assert!(a.text.identifiers.contains("node"));
}
