//! APIs used by readers outside the app (iconarium-cli / MCP server): query-only open,
//! library stats, duplicate lookup, scored search, match descriptions, pixmap rendering.

use svg_core::config::Limits;
use svg_core::index::{migrations, Database};
use svg_core::model::{DiscoveredFile, MatchField, ProcessingState};
use svg_core::search::Catalog;
use svg_core::svg::{
    analyze, render_pixmap, render_thumbnail, render_thumbnail_pixmap, Background,
};

fn df(path: &str) -> DiscoveredFile {
    DiscoveredFile {
        relative_path: path.into(),
        filename: path.rsplit('/').next().unwrap().into(),
        file_size: 10,
        mtime_ns: 1,
    }
}

const ROUTER: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 40 20"><title>Edge Router</title><rect width="40" height="20" fill="#36c"/></svg>"##;

fn seeded(path: &std::path::Path) -> (i64, Vec<u32>) {
    let mut db = Database::open(path).unwrap();
    let lib = db.upsert_library("/lib/a", "a").unwrap();
    let recs = db
        .insert_assets(
            lib.id,
            &[
                df("network/router.svg"),
                df("network/switch.svg"),
                df("misc/x.svg"),
            ],
        )
        .unwrap();
    let a = analyze(ROUTER.as_bytes(), &Limits::default());
    db.save_analysis(recs[0].id, &a).unwrap();
    (lib.id, recs.iter().map(|r| r.id).collect())
}

#[test]
fn query_only_open_reads_but_never_writes_or_creates() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nope.sqlite3");
    let err = Database::open_existing_query_only(&missing).unwrap_err();
    assert!(err.to_string().contains("index not found"), "{err}");
    assert!(!missing.exists(), "must not create the file");

    let path = dir.path().join("index.sqlite3");
    let (lib, ids) = seeded(&path);
    let mut ro = Database::open_existing_query_only(&path).unwrap();
    assert_eq!(ro.get_library(lib).unwrap().unwrap().path, "/lib/a");
    assert_eq!(ro.load_catalog(lib).unwrap().len(), 3);
    assert!(ro.get_asset(ids[0]).unwrap().is_some());
    // Writes are refused.
    assert!(ro.upsert_library("/lib/b", "b").is_err());
    assert!(ro.remove_assets(&ids).is_err());
}

#[test]
fn query_only_open_rejects_other_schema_versions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.sqlite3");
    seeded(&path);
    let latest = migrations::latest_version();
    let set = |v: u32| {
        let c = rusqlite::Connection::open(&path).unwrap();
        c.pragma_update(None, "user_version", v).unwrap();
    };
    set(latest + 1);
    let e = Database::open_existing_query_only(&path)
        .unwrap_err()
        .to_string();
    assert!(e.contains("older than the Iconarium"), "{e}");
    set(0);
    let e = Database::open_existing_query_only(&path)
        .unwrap_err()
        .to_string();
    assert!(e.contains("not an Iconarium index"), "{e}");
    set(latest);
    assert!(Database::open_existing_query_only(&path).is_ok());
}

#[test]
fn library_stats_track_assets_and_analysis_but_not_thumbnails() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.sqlite3");
    let (lib, ids) = seeded(&path);
    let mut db = Database::open(&path).unwrap();
    let s1 = db.library_stats(lib).unwrap();
    assert_eq!((s1.assets, s1.analyzed), (3, 1));
    assert_eq!(s1.max_asset_id, *ids.iter().max().unwrap() as i64);
    assert!(s1.last_analyzed_at.is_some());

    db.record_thumbnail(ids[0], 256, "k").unwrap();
    assert_eq!(
        db.library_stats(lib).unwrap(),
        s1,
        "thumbnail rows must not change the signature"
    );

    db.insert_assets(lib, &[df("new.svg")]).unwrap();
    let s2 = db.library_stats(lib).unwrap();
    assert_eq!(s2.assets, 4);
    assert!(s2.max_asset_id > s1.max_asset_id);

    let empty = db.upsert_library("/lib/empty", "empty").unwrap();
    assert_eq!(db.library_stats(empty.id).unwrap().assets, 0);
}

#[test]
fn duplicates_by_content_hash() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.sqlite3");
    let (_, ids) = seeded(&path);
    let mut db = Database::open(&path).unwrap();
    let a = analyze(ROUTER.as_bytes(), &Limits::default());
    db.save_analysis(ids[2], &a).unwrap();
    let dups = db.assets_with_hash(&a.content_hash).unwrap();
    assert_eq!(
        dups.iter().map(|r| r.id).collect::<Vec<_>>(),
        vec![ids[0], ids[2]]
    );
    assert_eq!(dups[0].state, ProcessingState::Ready);
}

#[test]
fn scored_search_and_match_descriptions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.sqlite3");
    let (lib, ids) = seeded(&path);
    let db = Database::open(&path).unwrap();
    let mut cat = Catalog::new();
    cat.extend(db.load_catalog(lib).unwrap());

    let scored = cat.search_scored("router", None).unwrap();
    assert_eq!(scored[0].0, ids[0]);
    assert!(scored[0].1 > 0);
    assert_eq!(
        cat.search("router", None).unwrap(),
        scored.iter().map(|h| h.0).collect::<Vec<_>>()
    );
    assert!(cat
        .search_scored("", None)
        .unwrap()
        .iter()
        .all(|h| h.1 == 0));

    // Filename match: explain() is silent, describe_match() names the field.
    assert!(cat.explain(ids[1], "switch").is_none());
    let m = cat.describe_match(ids[1], "switch").unwrap();
    assert_eq!(
        (m.field, m.snippet.as_str()),
        (MatchField::Filename, "switch.svg")
    );
    // Path match.
    let m = cat.describe_match(ids[1], "network").unwrap();
    assert_eq!(
        (m.field, m.snippet.as_str()),
        (MatchField::Path, "network/switch.svg")
    );
    // Content match keeps the snippet.
    let m = cat.describe_match(ids[0], "edge").unwrap();
    assert_eq!(m.field, MatchField::Title);
    assert!(m.snippet.contains("Edge Router"));
    // Non-match and empty query.
    assert!(cat.describe_match(ids[2], "router").is_none());
    assert!(cat.describe_match(ids[0], "").is_none());
}

#[test]
fn pixmap_renders_match_png_renders() {
    let limits = Limits::default();
    let pm = render_thumbnail_pixmap(ROUTER.as_bytes(), 64, &limits, None).unwrap();
    assert_eq!((pm.width(), pm.height()), (64, 32));
    assert_eq!(
        pm.encode_png().unwrap(),
        render_thumbnail(ROUTER.as_bytes(), 64, &limits, None).unwrap()
    );
    let full = render_pixmap(ROUTER.as_bytes(), 2.0, Background::White, &limits, None).unwrap();
    assert_eq!((full.width(), full.height()), (80, 40));
}
