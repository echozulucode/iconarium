use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use svg_core::library::scanner::{
    scan, stat_one, to_relative, ExistingEntry, Reconciler, ScanOptions,
};
use svg_core::model::DiscoveredFile;

const SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"/>"#;

fn touch(root: &Path, rel: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, SVG).unwrap();
}

fn tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    for i in 0..7 {
        touch(r, &format!("a/icon-{i}.svg"));
    }
    touch(r, "UPPER.SVG");
    touch(r, "mixed.SvG");
    touch(r, "a/b/c/deep.svg");
    touch(r, "notes.txt");
    touch(r, "image.png");
    touch(r, "a.svg.bak");
    touch(r, ".hidden.svg");
    touch(r, ".git/objects/x.svg");
    touch(r, "a/.cache/y.svg");
    dir
}

fn collect(
    root: &Path,
    opts: &ScanOptions,
) -> (
    Vec<Vec<DiscoveredFile>>,
    svg_core::library::scanner::ScanStats,
) {
    let cancel = AtomicBool::new(false);
    let mut batches = Vec::new();
    let stats = scan(root, opts, &cancel, |b| batches.push(b)).unwrap();
    (batches, stats)
}

fn paths(batches: &[Vec<DiscoveredFile>]) -> Vec<String> {
    let mut v: Vec<String> = batches
        .iter()
        .flatten()
        .map(|f| f.relative_path.clone())
        .collect();
    v.sort();
    v
}

#[test]
fn batches_extensions_hidden_and_nesting() {
    let dir = tree();
    let opts = ScanOptions {
        batch_size: 3,
        ..Default::default()
    };
    let (batches, stats) = collect(dir.path(), &opts);
    // 7 icons + UPPER + mixed + deep = 10 → 3,3,3,1
    assert_eq!(
        batches.iter().map(Vec::len).collect::<Vec<_>>(),
        vec![3, 3, 3, 1]
    );
    assert_eq!(stats.files_found, 10);
    assert!(!stats.cancelled);
    assert_eq!(stats.errors, 0);
    assert!(
        stats.dirs_visited >= 4,
        "root, a, a/b, a/b/c: {}",
        stats.dirs_visited
    );
    let all = paths(&batches);
    assert!(all.contains(&"UPPER.SVG".to_string()));
    assert!(all.contains(&"mixed.SvG".to_string()));
    assert!(all.contains(&"a/b/c/deep.svg".to_string()));
    assert!(all.iter().all(|p| !p.contains('\\')));
    assert!(!all
        .iter()
        .any(|p| p.contains(".git") || p.contains(".hidden") || p.contains(".cache")));

    let deep = batches
        .iter()
        .flatten()
        .find(|f| f.relative_path == "a/b/c/deep.svg")
        .unwrap();
    assert_eq!(deep.filename, "deep.svg");
    assert_eq!(deep.file_size, SVG.len() as u64);
    assert!(deep.mtime_ns > 0);
}

#[test]
fn hidden_entries_included_when_not_ignored() {
    let dir = tree();
    let opts = ScanOptions {
        batch_size: 100,
        ignore_hidden: false,
        ..Default::default()
    };
    let (batches, stats) = collect(dir.path(), &opts);
    assert_eq!(batches.len(), 1);
    assert_eq!(stats.files_found, 13);
    let all = paths(&batches);
    assert!(all.contains(&".git/objects/x.svg".to_string()));
    assert!(all.contains(&".hidden.svg".to_string()));
}

#[test]
fn cancel_stops_progressively() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..50 {
        touch(dir.path(), &format!("d{}/f{i}.svg", i % 5));
    }
    let cancel = AtomicBool::new(false);
    let mut seen = 0;
    let opts = ScanOptions {
        batch_size: 5,
        ..Default::default()
    };
    let stats = scan(dir.path(), &opts, &cancel, |b| {
        seen += b.len();
        cancel.store(true, Ordering::Relaxed); // cancel after the first batch
    })
    .unwrap();
    assert!(stats.cancelled);
    assert_eq!(seen, 5);
    assert_eq!(stats.files_found, 5);

    // Pre-cancelled: nothing emitted.
    let cancel = AtomicBool::new(true);
    let mut n = 0;
    let stats = scan(dir.path(), &opts, &cancel, |b| n += b.len()).unwrap();
    assert!(stats.cancelled);
    assert_eq!(n, 0);
}

#[test]
fn missing_root_is_error() {
    let dir = tempfile::tempdir().unwrap();
    let cancel = AtomicBool::new(false);
    assert!(scan(
        &dir.path().join("nope"),
        &ScanOptions::default(),
        &cancel,
        |_| {}
    )
    .is_err());
    let file = dir.path().join("f.svg");
    fs::write(&file, SVG).unwrap();
    assert!(scan(&file, &ScanOptions::default(), &cancel, |_| {}).is_err());
}

#[test]
fn stat_one_and_to_relative() {
    let dir = tree();
    let r = dir.path();
    let f = stat_one(r, "a/b/c/deep.svg").unwrap();
    assert_eq!(f.filename, "deep.svg");
    assert_eq!(f.file_size, SVG.len() as u64);
    assert!(stat_one(r, "a/b/c/missing.svg").is_none());
    assert!(stat_one(r, "notes.txt").is_none());
    assert!(stat_one(r, "a").is_none());
    assert!(stat_one(r, "../x.svg").is_none());
    assert_eq!(
        to_relative(r, &r.join("a").join("icon-1.svg")).as_deref(),
        Some("a/icon-1.svg")
    );
    assert_eq!(to_relative(r, Path::new("/somewhere/else.svg")), None);

    // Scan and stat_one agree.
    let (batches, _) = collect(r, &ScanOptions::default());
    for f in batches.iter().flatten() {
        assert_eq!(stat_one(r, &f.relative_path).as_ref(), Some(f));
    }
}

fn df(path: &str, size: u64, mtime: i64) -> DiscoveredFile {
    DiscoveredFile {
        relative_path: path.into(),
        filename: path.rsplit('/').next().unwrap().into(),
        file_size: size,
        mtime_ns: mtime,
    }
}

#[test]
fn reconciler_classifies_new_changed_unchanged_deleted() {
    let mut existing = HashMap::new();
    existing.insert(
        "same.svg".to_string(),
        ExistingEntry {
            id: 1,
            file_size: 10,
            mtime_ns: 100,
        },
    );
    existing.insert(
        "size.svg".to_string(),
        ExistingEntry {
            id: 2,
            file_size: 10,
            mtime_ns: 100,
        },
    );
    existing.insert(
        "time.svg".to_string(),
        ExistingEntry {
            id: 3,
            file_size: 10,
            mtime_ns: 100,
        },
    );
    existing.insert(
        "gone.svg".to_string(),
        ExistingEntry {
            id: 4,
            file_size: 10,
            mtime_ns: 100,
        },
    );
    existing.insert(
        "gone2.svg".to_string(),
        ExistingEntry {
            id: 9,
            file_size: 1,
            mtime_ns: 1,
        },
    );
    let mut rec = Reconciler::new(existing);

    let d1 = rec.classify(vec![
        df("same.svg", 10, 100),
        df("size.svg", 11, 100),
        df("new.svg", 5, 5),
    ]);
    assert_eq!(d1.unchanged, 1);
    assert_eq!(d1.new, vec![df("new.svg", 5, 5)]);
    assert_eq!(d1.changed, vec![(2, df("size.svg", 11, 100))]);

    // Second batch, including a duplicate report of an already-seen path.
    let d2 = rec.classify(vec![df("time.svg", 10, 101), df("same.svg", 10, 100)]);
    assert_eq!(d2.unchanged, 0);
    assert!(d2.new.is_empty());
    assert_eq!(d2.changed, vec![(3, df("time.svg", 10, 101))]);

    assert_eq!(rec.finish(), vec![4, 9]);
}

#[test]
fn reconcile_against_real_scan() {
    let dir = tree();
    let r = dir.path();
    let (batches, _) = collect(r, &ScanOptions::default());
    let mut existing: HashMap<String, ExistingEntry> = batches
        .iter()
        .flatten()
        .enumerate()
        .map(|(i, f)| {
            (
                f.relative_path.clone(),
                ExistingEntry {
                    id: i as u32 + 1,
                    file_size: f.file_size,
                    mtime_ns: f.mtime_ns,
                },
            )
        })
        .collect();
    existing.insert(
        "deleted.svg".into(),
        ExistingEntry {
            id: 999,
            file_size: 1,
            mtime_ns: 1,
        },
    );
    // Modify one file's size.
    fs::write(r.join("UPPER.SVG"), format!("{SVG}\n<!-- more -->")).unwrap();
    touch(r, "a/brand-new.svg");

    let mut rc = Reconciler::new(existing);
    let mut new = Vec::new();
    let mut changed = Vec::new();
    let mut unchanged = 0;
    let cancel = AtomicBool::new(false);
    scan(
        r,
        &ScanOptions {
            batch_size: 2,
            ..Default::default()
        },
        &cancel,
        |b| {
            let d = rc.classify(b);
            new.extend(d.new);
            changed.extend(d.changed);
            unchanged += d.unchanged;
        },
    )
    .unwrap();
    assert_eq!(
        new.iter()
            .map(|f| f.relative_path.as_str())
            .collect::<Vec<_>>(),
        vec!["a/brand-new.svg"]
    );
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0].1.relative_path, "UPPER.SVG");
    assert_eq!(unchanged, 9);
    assert_eq!(rc.finish(), vec![999]);
}
