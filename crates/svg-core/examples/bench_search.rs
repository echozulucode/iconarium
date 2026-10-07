//! Synthetic benchmark for the index + search layer.
//!
//! ```text
//! cargo run -p svg-core --release --example bench_search [-- <assets> <with_text>]
//! ```
//! Defaults: 50,000 assets with realistic names/paths, of which 10,000 carry ~2 KB of
//! extracted text. Measures SQLite insert / analysis save / load_catalog through an
//! on-disk temp database, catalog build, and a set of representative queries.

use std::time::{Duration, Instant};

use svg_core::index::Database;
use svg_core::model::{
    Analysis, AssetId, Complexity, DiscoveredFile, ProcessingState, SearchText, SvgMeta, ViewBox,
};
use svg_core::search::Catalog;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn pick<'a>(&mut self, v: &[&'a str]) -> &'a str {
        v[(self.next() % v.len() as u64) as usize]
    }
}

const TOP: &[&str] = &[
    "network",
    "power",
    "controls",
    "icons",
    "diagrams",
    "mechanical",
    "hydraulics",
    "ui",
    "logos",
    "legacy",
];
const SUB: &[&str] = &[
    "switches", "routers", "sensors", "motors", "valves", "pumps", "panels", "arrows", "shapes",
    "symbols", "v1", "v2", "archive", "2024",
];
const WORDS: &[&str] = &[
    "ethernet",
    "switch",
    "router",
    "motor",
    "controller",
    "pump",
    "valve",
    "sensor",
    "relay",
    "breaker",
    "fuse",
    "transformer",
    "inverter",
    "battery",
    "panel",
    "gateway",
    "firewall",
    "server",
    "cable",
    "port",
    "fan",
    "heater",
    "cooler",
    "tank",
    "pipe",
    "flange",
    "gear",
    "shaft",
    "bearing",
    "arrow",
    "circle",
    "square",
    "star",
    "logo",
    "icon",
    "outline",
    "filled",
    "small",
    "large",
    "left",
    "right",
    "up",
    "down",
    "red",
    "blue",
    "green",
];
const TEXTWORDS: &[&str] = &[
    "Ethernet",
    "Control",
    "Interface",
    "Main",
    "Bus",
    "Motor",
    "Controller",
    "Supervisory",
    "Feeder",
    "Breaker",
    "Turbine",
    "Generator",
    "Cooling",
    "Loop",
    "Pressure",
    "Temperature",
    "Flow",
    "Valve",
    "Pump",
    "Station",
    "Node",
    "Link",
    "Redundant",
    "Primary",
    "Secondary",
    "Alarm",
    "Trip",
    "Reset",
    "Manual",
    "Auto",
    "kV",
    "MW",
    "Zone",
    "A",
    "B",
    "C",
    "1",
    "2",
    "3",
    "PLC",
    "HMI",
    "SCADA",
    "Fieldbus",
    "Profinet",
    "Modbus",
    "TCP",
    "UDP",
];

fn make_files(n: usize, rng: &mut Rng) -> Vec<DiscoveredFile> {
    (0..n)
        .map(|i| {
            let depth = 1 + (rng.next() % 3) as usize;
            let mut path = String::new();
            path.push_str(rng.pick(TOP));
            for _ in 1..depth {
                path.push('/');
                path.push_str(rng.pick(SUB));
            }
            let nwords = 1 + (rng.next() % 3) as usize;
            let mut name = String::new();
            for w in 0..nwords {
                if w > 0 {
                    name.push(if rng.next().is_multiple_of(2) {
                        '-'
                    } else {
                        '_'
                    });
                }
                name.push_str(rng.pick(WORDS));
            }
            name.push_str(&format!("-{i:05}.svg"));
            path.push('/');
            path.push_str(&name);
            DiscoveredFile {
                relative_path: path,
                filename: name,
                file_size: 1000 + rng.next() % 50_000,
                mtime_ns: 1_700_000_000_000_000_000 + i as i64,
            }
        })
        .collect()
}

fn make_text(rng: &mut Rng) -> SearchText {
    let mut visible = String::with_capacity(2100);
    while visible.len() < 2000 {
        let run = 1 + (rng.next() % 4) as usize;
        for k in 0..run {
            if k > 0 {
                visible.push(' ');
            }
            visible.push_str(rng.pick(TEXTWORDS));
        }
        visible.push('\n');
    }
    SearchText {
        title: format!("{} {}", rng.pick(TEXTWORDS), rng.pick(TEXTWORDS)),
        description: format!(
            "Diagram of the {} {} subsystem",
            rng.pick(TEXTWORDS),
            rng.pick(TEXTWORDS)
        ),
        visible_text: visible,
        identifiers: format!(
            "layer1 g{} cls-{} {}",
            rng.next() % 100,
            rng.pick(WORDS),
            rng.pick(WORDS)
        ),
    }
}

fn analysis(text: SearchText) -> Analysis {
    let vb = ViewBox {
        min_x: 0.0,
        min_y: 0.0,
        width: 64.0,
        height: 64.0,
    };
    Analysis {
        state: ProcessingState::Ready,
        error: None,
        content_hash: "0".repeat(64),
        meta: SvgMeta {
            width: Some(64.0),
            height: Some(64.0),
            view_box: Some(vb),
            doc_box: Some(vb),
            element_count: 20,
        },
        text,
        complexity: Complexity::default(),
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn time<T>(f: impl FnOnce() -> T) -> (T, Duration) {
    let t = Instant::now();
    let v = f();
    (v, t.elapsed())
}

fn bench_query(cat: &Catalog, label: &str, q: &str, iters: usize) {
    // first run (includes any lazy cache build), then steady-state median/max
    let (first, d0) = time(|| cat.search(q, None).unwrap());
    let mut ds: Vec<Duration> = (0..iters)
        .map(|_| time(|| cat.search(q, None).unwrap()).1)
        .collect();
    ds.sort();
    println!(
        "| {label:<34} | `{q}` | {:>6} | {:>8.2} | {:>8.2} | {:>8.2} |",
        first.len(),
        ms(d0),
        ms(ds[ds.len() / 2]),
        ms(*ds.last().unwrap())
    );
}

fn main() {
    let args: Vec<usize> = std::env::args()
        .skip(1)
        .filter_map(|a| a.parse().ok())
        .collect();
    let n = args.first().copied().unwrap_or(50_000);
    let n_text = args.get(1).copied().unwrap_or(10_000).min(n);
    let mut rng = Rng(0x5eed_1234_abcd_ef01);

    let files = make_files(n, &mut rng);
    let texts: Vec<SearchText> = (0..n_text).map(|_| make_text(&mut rng)).collect();
    let text_bytes: usize = texts
        .iter()
        .map(|t| t.title.len() + t.description.len() + t.visible_text.len() + t.identifiers.len())
        .sum();
    println!(
        "assets: {n}, with text: {n_text} (avg {} B text)",
        text_bytes / n_text.max(1)
    );
    println!();

    // ---------------- SQLite (on-disk temp) ----------------
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("index.sqlite");
    let mut db = Database::open(&db_path).unwrap();
    let lib = db.upsert_library("/bench/library", "bench").unwrap().id;

    let (recs, d_insert) = time(|| {
        let mut out = Vec::with_capacity(n);
        for chunk in files.chunks(250) {
            out.extend(db.insert_assets(lib, chunk).unwrap());
        }
        out
    });
    let items: Vec<(AssetId, Analysis)> = recs
        .iter()
        .zip(texts.iter())
        .map(|(r, t)| (r.id, analysis(t.clone())))
        .collect();
    let (_, d_save) = time(|| {
        for chunk in items.chunks(250) {
            db.save_analyses(chunk).unwrap();
        }
    });
    let (_, d_snapshot) = time(|| db.library_snapshot(lib).unwrap().len());
    drop(db);
    let db = Database::open(&db_path).unwrap();
    let (loaded, d_load) = time(|| db.load_catalog(lib).unwrap());
    assert_eq!(loaded.len(), n);
    let (cat, d_build) = time(|| {
        let mut c = Catalog::new();
        c.extend(loaded);
        c
    });
    let db_size = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0)
        + std::fs::metadata(dir.path().join("index.sqlite-wal"))
            .map(|m| m.len())
            .unwrap_or(0);

    println!("| Step | ms |");
    println!("|---|---:|");
    println!(
        "| insert_assets ({n}, batches of 250) | {:.1} |",
        ms(d_insert)
    );
    println!(
        "| save_analyses ({n_text}, batches of 250) | {:.1} |",
        ms(d_save)
    );
    println!("| library_snapshot ({n}) | {:.1} |", ms(d_snapshot));
    println!("| load_catalog ({n}, cold reopen) | {:.1} |", ms(d_load));
    println!("| Catalog::extend ({n}) | {:.1} |", ms(d_build));
    println!("| DB size on disk | {:.1} MB |", db_size as f64 / 1e6);
    println!();

    // ---------------- queries over the full catalog ----------------
    println!("Full catalog ({n} assets, {n_text} with text):");
    println!();
    println!("| Query | Text | Hits | First ms | Median ms | Max ms |");
    println!("|---|---|---:|---:|---:|---:|");
    let iters = 15;
    bench_query(&cat, "empty (path order)", "", iters);
    bench_query(&cat, "1 token (filename)", "switch", iters);
    bench_query(&cat, "2 tokens", "ethernet switch", iters);
    bench_query(&cat, "no hits (worst-case scan)", "zzqxv", iters);
    bench_query(&cat, "exclusion", "pump -legacy", iters);
    bench_query(&cat, "glob (filename)", "*switch*.svg", iters);
    bench_query(&cat, "glob (path)", "network/*/ethernet*", iters);
    bench_query(&cat, "regex", "re:eth.*sw", iters);
    bench_query(&cat, "content token", "turbine", iters);
    bench_query(&cat, "content 2 tokens", "motor controller", iters);
    bench_query(&cat, "content phrase", "\"control interface\"", iters);

    // ---------------- 10k text-heavy subset ----------------
    let mut sub = Catalog::new();
    let mut loaded_sub = db.load_catalog(lib).unwrap();
    loaded_sub.retain(|(_, t)| t.is_some());
    sub.extend(loaded_sub);
    println!();
    println!("Text-heavy subset ({} assets with ~2 KB text):", sub.len());
    println!();
    println!("| Query | Text | Hits | First ms | Median ms | Max ms |");
    println!("|---|---|---:|---:|---:|---:|");
    bench_query(&sub, "content token", "turbine", iters);
    bench_query(&sub, "content no hits", "zzqxv", iters);
    bench_query(&sub, "content 3 tokens", "pressure alarm modbus", iters);
    bench_query(&sub, "content phrase", "\"supervisory controller\"", iters);
    bench_query(&sub, "content regex", "re:primary\\s+loop", iters);

    // explanations for one page of results
    let ids = cat.search("motor controller", Some(100)).unwrap();
    let (s, d) = time(|| cat.summaries(&ids, Some("motor controller")));
    println!();
    println!(
        "summaries+explain for 100 results: {:.2} ms ({} with match info)",
        ms(d),
        s.iter().filter(|x| x.match_info.is_some()).count()
    );
}
