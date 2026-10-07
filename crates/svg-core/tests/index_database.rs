use svg_core::config::{GallerySize, Settings};
use svg_core::index::migrations;
use svg_core::index::Database;
use svg_core::library::fingerprint::fast_fingerprint;
use svg_core::model::{
    Analysis, Complexity, DiscoveredFile, ProcessingState, SearchText, SvgMeta, ViewBox,
};

fn df(path: &str, size: u64, mtime: i64) -> DiscoveredFile {
    DiscoveredFile {
        relative_path: path.into(),
        filename: path.rsplit('/').next().unwrap().into(),
        file_size: size,
        mtime_ns: mtime,
    }
}

fn analysis(title: &str, text: &str) -> Analysis {
    let vb = ViewBox {
        min_x: 0.0,
        min_y: 0.0,
        width: 100.0,
        height: 50.0,
    };
    Analysis {
        state: ProcessingState::Ready,
        error: None,
        content_hash: "ab".repeat(32),
        meta: SvgMeta {
            width: Some(200.0),
            height: Some(100.0),
            view_box: Some(vb),
            doc_box: Some(vb),
            element_count: 12,
        },
        text: SearchText {
            title: title.into(),
            description: "a description".into(),
            visible_text: text.into(),
            identifiers: "layer1 cls-a".into(),
        },
        complexity: Complexity {
            file_size: 1234,
            node_count: 40,
            embedded_raster_bytes: 0,
            max_text_node_chars: 20,
            render_text_chars: 20,
            has_scripts: false,
            has_external_refs: true,
        },
    }
}

#[test]
fn migrations_idempotent_on_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sub/index.sqlite");
    let lib_id;
    {
        let mut db = Database::open(&path).unwrap();
        assert_eq!(db.schema_version().unwrap(), migrations::latest_version());
        lib_id = db.upsert_library("C:/Lib", "Lib").unwrap().id;
        db.insert_assets(lib_id, &[df("a.svg", 1, 1)]).unwrap();
    }
    for _ in 0..2 {
        let db = Database::open(&path).unwrap();
        assert_eq!(db.schema_version().unwrap(), migrations::latest_version());
        assert_eq!(db.asset_count(lib_id).unwrap(), 1);
    }
}

#[test]
fn libraries_upsert_recent_and_scanned() {
    let mut db = Database::open_in_memory().unwrap();
    let a = db.upsert_library("/libs/a", "A").unwrap();
    let b = db.upsert_library("/libs/b", "B").unwrap();
    assert_ne!(a.id, b.id);
    assert!(a.last_scan.is_none());
    assert!(a.created_at > 0 && a.last_opened >= a.created_at);
    let recent = db.recent_libraries(10).unwrap();
    assert_eq!(
        recent.iter().map(|l| l.id).collect::<Vec<_>>(),
        vec![b.id, a.id]
    );

    // Re-opening A keeps its id and moves it to the front.
    let a2 = db.upsert_library("/libs/a", "A renamed").unwrap();
    assert_eq!(a2.id, a.id);
    assert_eq!(a2.display_name, "A renamed");
    assert_eq!(a2.created_at, a.created_at);
    let recent = db.recent_libraries(10).unwrap();
    assert_eq!(
        recent.iter().map(|l| l.id).collect::<Vec<_>>(),
        vec![a.id, b.id]
    );
    assert_eq!(db.recent_libraries(1).unwrap().len(), 1);

    db.set_library_scanned(a.id, 1_700_000_000).unwrap();
    assert_eq!(
        db.get_library(a.id).unwrap().unwrap().last_scan,
        Some(1_700_000_000)
    );
    assert!(db.get_library(9999).unwrap().is_none());
}

#[test]
fn asset_crud_round_trip() {
    let mut db = Database::open_in_memory().unwrap();
    let lib = db.upsert_library("/l", "l").unwrap().id;
    let recs = db
        .insert_assets(lib, &[df("a/x.svg", 10, 100), df("y.svg", 20, 200)])
        .unwrap();
    assert_eq!(recs.len(), 2);
    let x = &recs[0];
    assert_eq!(x.state, ProcessingState::Discovered);
    assert_eq!(x.fast_fingerprint, fast_fingerprint("a/x.svg", 10, 100));
    assert_eq!(x.rel_dir(), "a");
    assert_eq!(db.get_asset(x.id).unwrap().as_ref(), Some(x));
    assert_eq!(
        db.get_asset_by_path(lib, "y.svg").unwrap().unwrap().id,
        recs[1].id
    );
    assert_eq!(
        db.pending_analysis(lib).unwrap(),
        vec![recs[0].id, recs[1].id]
    );

    let snap = db.library_snapshot(lib).unwrap();
    assert_eq!(snap.len(), 2);
    assert_eq!(snap["a/x.svg"].id, x.id);
    assert_eq!(snap["a/x.svg"].file_size, 10);
    assert_eq!(snap["a/x.svg"].mtime_ns, 100);

    // Analysis
    db.save_analysis(x.id, &analysis("Pump", "Motor Controller"))
        .unwrap();
    let got = db.get_asset(x.id).unwrap().unwrap();
    assert_eq!(got.state, ProcessingState::Ready);
    assert_eq!(got.width, Some(200.0));
    assert_eq!(
        got.view_box,
        Some(ViewBox {
            min_x: 0.0,
            min_y: 0.0,
            width: 100.0,
            height: 50.0
        })
    );
    assert_eq!(got.element_count, Some(12));
    assert_eq!(got.content_hash.as_deref(), Some("ab".repeat(32).as_str()));
    assert_eq!(db.get_search_text(x.id).unwrap().unwrap().title, "Pump");
    assert_eq!(db.get_doc_box(x.id).unwrap().unwrap().width, 100.0);
    assert_eq!(db.pending_analysis(lib).unwrap(), vec![recs[1].id]);

    // Parse error via batch save
    let mut bad = analysis("", "");
    bad.state = ProcessingState::ParseError;
    bad.error = Some("unexpected EOF".into());
    bad.meta = SvgMeta::default();
    bad.text = SearchText::default();
    db.save_analyses(&[(recs[1].id, bad), (424242, analysis("ghost", ""))])
        .unwrap();
    let y = db.get_asset(recs[1].id).unwrap().unwrap();
    assert_eq!(y.state, ProcessingState::ParseError);
    assert_eq!(y.parse_error.as_deref(), Some("unexpected EOF"));
    assert_eq!(y.element_count, None);
    assert!(db.get_search_text(y.id).unwrap().is_none());
    assert!(db.pending_analysis(lib).unwrap().is_empty());

    // Thumbnails
    db.record_thumbnail(x.id, 256, "key-1").unwrap();
    db.record_thumbnail(x.id, 256, "key-2").unwrap();
    db.record_thumbnail(x.id, 128, "key-s").unwrap();
    assert_eq!(
        db.thumbnail_key(x.id, 256).unwrap().as_deref(),
        Some("key-2")
    );
    db.clear_thumbnails(x.id).unwrap();
    assert!(db.thumbnail_key(x.id, 128).unwrap().is_none());
    db.record_thumbnail(x.id, 256, "key-3").unwrap();

    // Changed → reset, derived rows dropped
    let upd = db
        .update_changed(&[(x.id, df("a/x.svg", 11, 101)), (777, df("nope.svg", 1, 1))])
        .unwrap();
    assert_eq!(upd.len(), 1);
    let u = &upd[0];
    assert_eq!(u.id, x.id);
    assert_eq!(u.file_size, 11);
    assert_eq!(u.state, ProcessingState::Discovered);
    assert_eq!(u.fast_fingerprint, fast_fingerprint("a/x.svg", 11, 101));
    assert!(u.content_hash.is_none() && u.view_box.is_none() && u.width.is_none());
    assert!(db.get_search_text(x.id).unwrap().is_none());
    assert!(db.get_doc_box(x.id).unwrap().is_none());
    assert!(db.thumbnail_key(x.id, 256).unwrap().is_none());
    assert_eq!(db.get_asset(x.id).unwrap().as_ref(), Some(u));

    // Re-inserting an existing path keeps the id.
    let again = db.insert_assets(lib, &[df("y.svg", 21, 201)]).unwrap();
    assert_eq!(again[0].id, recs[1].id);
    assert_eq!(
        db.get_asset(recs[1].id).unwrap().unwrap().state,
        ProcessingState::Discovered
    );

    // Remove
    db.remove_assets(&[x.id]).unwrap();
    assert!(db.get_asset(x.id).unwrap().is_none());
    assert_eq!(db.asset_count(lib).unwrap(), 1);
}

#[test]
fn cascade_delete_removes_derived_rows() {
    let mut db = Database::open_in_memory().unwrap();
    let lib = db.upsert_library("/l", "l").unwrap().id;
    let recs = db
        .insert_assets(lib, &[df("a.svg", 1, 1), df("b.svg", 1, 1)])
        .unwrap();
    for r in &recs {
        db.save_analysis(r.id, &analysis("t", "x")).unwrap();
        db.record_thumbnail(r.id, 256, "k").unwrap();
    }
    db.remove_assets(&[recs[0].id]).unwrap();
    assert!(db.get_search_text(recs[0].id).unwrap().is_none());
    assert!(db.get_doc_box(recs[0].id).unwrap().is_none());
    assert!(db.thumbnail_key(recs[0].id, 256).unwrap().is_none());
    assert!(db.get_search_text(recs[1].id).unwrap().is_some());

    // Library delete cascades through assets to everything else.
    db.remove_library(lib).unwrap();
    assert_eq!(db.asset_count(lib).unwrap(), 0);
    assert!(db.get_search_text(recs[1].id).unwrap().is_none());
    assert!(db.thumbnail_key(recs[1].id, 256).unwrap().is_none());
}

#[test]
fn load_catalog_returns_records_and_text() {
    let mut db = Database::open_in_memory().unwrap();
    let lib = db.upsert_library("/l", "l").unwrap().id;
    let other = db.upsert_library("/o", "o").unwrap().id;
    let recs = db
        .insert_assets(lib, &[df("a.svg", 1, 1), df("b/c.svg", 2, 2)])
        .unwrap();
    db.insert_assets(other, &[df("z.svg", 1, 1)]).unwrap();
    db.save_analysis(recs[1].id, &analysis("Title C", "Ethernet Switch"))
        .unwrap();

    let mut cat = db.load_catalog(lib).unwrap();
    cat.sort_by_key(|(r, _)| r.id);
    assert_eq!(cat.len(), 2);
    assert_eq!(cat[0].0, recs[0]);
    assert!(cat[0].1.is_none());
    assert_eq!(cat[1].0.relative_path, "b/c.svg");
    assert_eq!(cat[1].0.state, ProcessingState::Ready);
    let t = cat[1].1.as_ref().unwrap();
    assert_eq!(t.title, "Title C");
    assert_eq!(t.visible_text, "Ethernet Switch");
    assert_eq!(t.identifiers, "layer1 cls-a");
}

#[test]
fn settings_round_trip_and_defaults() {
    let mut db = Database::open_in_memory().unwrap();
    assert_eq!(db.load_settings().unwrap(), Settings::default());
    let s = Settings {
        thumbnail_size: 192,
        gallery_size: GallerySize::Large,
        ..Default::default()
    };
    db.save_settings(&s).unwrap();
    assert_eq!(db.load_settings().unwrap(), s);
    // Out-of-range values are sanitized on load.
    let wild = Settings {
        thumbnail_size: 1,
        ..Default::default()
    };
    db.save_settings(&wild).unwrap();
    assert_eq!(db.load_settings().unwrap().thumbnail_size, 64);
}

#[test]
fn checked_save_skips_stale_fingerprint() {
    use svg_core::model::DiscoveredFile;
    let mut db = Database::open_in_memory().unwrap();
    let lib = db.upsert_library("/lib", "lib").unwrap();
    let f = DiscoveredFile {
        relative_path: "a.svg".into(),
        filename: "a.svg".into(),
        file_size: 10,
        mtime_ns: 1,
    };
    let rec = db.insert_assets(lib.id, std::slice::from_ref(&f)).unwrap().remove(0);
    // File changes before the (old) analysis is saved.
    let changed = DiscoveredFile {
        file_size: 20,
        mtime_ns: 2,
        ..f
    };
    db.update_changed(&[(rec.id, changed)]).unwrap();
    db.save_analyses_checked(&[(
        rec.id,
        rec.fast_fingerprint.clone(),
        analysis("stale", "old text"),
    )])
    .unwrap();
    let now = db.get_asset(rec.id).unwrap().unwrap();
    assert_eq!(now.state, svg_core::model::ProcessingState::Discovered);
    assert!(db.get_search_text(rec.id).unwrap().is_none());
    // Current fingerprint applies.
    db.save_analyses_checked(&[(
        rec.id,
        now.fast_fingerprint.clone(),
        analysis("fresh", "new text"),
    )])
    .unwrap();
    assert_eq!(
        db.get_asset(rec.id).unwrap().unwrap().state,
        svg_core::model::ProcessingState::Ready
    );
}
