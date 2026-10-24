//! End-to-end behaviour of the catalog over a real (fixture) index.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use iconarium_catalog::fixtures::{self, Fixture};
use iconarium_catalog::tags::TagSource;
use iconarium_catalog::*;
use svg_core::index::Database;
use svg_core::model::DiscoveredFile;

fn setup() -> (tempfile::TempDir, Fixture, IndexReader) {
    let dir = tempfile::tempdir().unwrap();
    let fx = fixtures::build(dir.path());
    let reader = IndexReader::open(&fx.index).unwrap();
    (dir, fx, reader)
}

fn search(r: &IndexReader, q: &str) -> dto::SearchResponse {
    r.search(&SearchRequest {
        query: q.into(),
        ..Default::default()
    })
    .unwrap()
}

#[test]
fn libraries_are_listed_and_disambiguated() {
    let (_d, fx, r) = setup();
    let libs = r.list_libraries();
    assert_eq!(libs.len(), 2);
    // Most recently opened first.
    assert_eq!(libs[0].id, fx.main_id);
    assert!(
        libs.iter().all(|l| l.name.starts_with("icons (")),
        "{libs:?}"
    );
    let main = &libs[0];
    assert_eq!((main.assets, main.analyzed), (5, 4));
    assert!(main.available);
    assert!(main
        .metadata_file
        .as_deref()
        .unwrap()
        .ends_with("iconarium.yaml"));
}

#[test]
fn search_ranks_merges_and_explains() {
    let (_d, fx, r) = setup();
    let res = search(&r, "router");
    assert_eq!(res.total, 1);
    let h = &res.hits[0];
    assert_eq!(h.id, fx.ids["network/router.svg"]);
    assert_eq!(h.name, "Cisco edge router", "metadata title wins");
    assert_eq!(h.library_id, fx.main_id);
    assert!(PathBuf::from(&h.abs_path).is_file());
    assert_eq!(h.matched.as_ref().unwrap().field, "filename");
    assert_eq!(
        h.tags,
        vec!["edge", "wan", "networking", "corporate", "network"]
    );
    assert_eq!(h.state, "ready");

    // Content match in the other library, via <desc> keywords.
    let res = search(&r, "actor");
    assert_eq!(res.hits[0].id, fx.ids["c4/person.svg"]);
    assert_eq!(res.hits[0].matched.as_ref().unwrap().field, "desc");

    // Metadata description and tags are searchable.
    assert_eq!(
        search(&r, "branch office").hits[0].id,
        fx.ids["network/router.svg"]
    );
    assert_eq!(search(&r, "wan").total, 1);

    // Coverage warning for the unanalyzed file, only for content-style queries.
    assert!(search(&r, "star")
        .warnings
        .iter()
        .any(|w| w.contains("80%")));
    assert!(r
        .search(&SearchRequest::default())
        .unwrap()
        .warnings
        .is_empty());
}

#[test]
fn filters_and_pagination() {
    let (_d, fx, r) = setup();
    let all = r.search(&SearchRequest::default()).unwrap();
    assert_eq!(all.total, 6);
    // Library-major order: main library (most recent) first, in path order.
    assert_eq!(all.hits[0].library_id, fx.main_id);
    assert_eq!(all.hits.last().unwrap().library_id, fx.other_id);

    let only_other = r
        .search(&SearchRequest {
            libraries: vec![fx.other_id.to_string()],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(only_other.total, 1);
    let by_root = r
        .search(&SearchRequest {
            libraries: vec![fx.other_root.to_string_lossy().into_owned()],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(by_root.total, 1);
    let e = r.search(&SearchRequest {
        libraries: vec!["icons".into()],
        ..Default::default()
    });
    assert!(matches!(e, Err(CatalogError::Invalid(m)) if m.contains("ambiguous")));

    let net = r
        .search(&SearchRequest {
            folder: Some("Network/".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(net.total, 2);
    let tagged = r
        .search(&SearchRequest {
            tags: vec!["Networking".into(), "edge".into()],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        tagged.hits.iter().map(|h| h.id).collect::<Vec<_>>(),
        vec![fx.ids["network/router.svg"]]
    );
    let switches = r
        .search(&SearchRequest {
            tags: vec!["switches".into()],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        switches.total, 1,
        "folder tag; the size folder '24' is not a tag"
    );

    // Pages.
    let p1 = r
        .search(&SearchRequest {
            limit: Some(4),
            ..Default::default()
        })
        .unwrap();
    assert_eq!((p1.returned, p1.total), (4, 6));
    let cursor = p1.next_cursor.clone().unwrap();
    let p2 = r
        .search(&SearchRequest {
            limit: Some(4),
            cursor: Some(cursor.clone()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(p2.returned, 2);
    assert!(p2.next_cursor.is_none());
    let mut seen: Vec<u32> = p1.hits.iter().chain(&p2.hits).map(|h| h.id).collect();
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 6);

    // Bad queries are errors, not panics.
    assert!(r
        .search(&SearchRequest {
            query: "re:(".into(),
            ..Default::default()
        })
        .is_err());
}

#[test]
fn details() {
    let (_d, fx, r) = setup();
    let id = fx.ids["ui/home.svg"];
    let d = r
        .detail(
            &IconRef::Id(id),
            DetailOptions {
                text: true,
                svg: true,
                duplicates: true,
                max_svg_bytes: 50,
            },
        )
        .unwrap();
    assert_eq!(d.name, "home");
    assert!(d.exists);
    assert_eq!(d.view_box, Some([0.0, 0.0, 24.0, 24.0]));
    assert_eq!(d.visible_text.as_deref(), Some("Home"));
    assert!(d.svg_truncated && d.svg.as_ref().unwrap().len() <= 50);
    assert_eq!(d.duplicates, Some(vec![]));
    assert!(d
        .tags
        .iter()
        .any(|t| t.tag == "house" && t.source == TagSource::Keywords));

    let by_path = r
        .detail(
            &IconRef::Path {
                library: fx.main_id.to_string(),
                path: "UI\\Home.svg".into(),
            },
            DetailOptions::default(),
        )
        .unwrap();
    assert_eq!(by_path.id, id);
    assert!(by_path.svg.is_none() && by_path.title.is_none());

    let broken = r.detail(
        &IconRef::Id(fx.ids["ui/broken.svg"]),
        DetailOptions {
            svg: true,
            ..Default::default()
        },
    );
    assert!(matches!(broken, Err(CatalogError::Unavailable(_))));
    assert!(matches!(
        r.detail(&IconRef::Id(999_999), DetailOptions::default()),
        Err(CatalogError::NotFound(_))
    ));
}

#[test]
fn tags_and_folders() {
    let (_d, fx, r) = setup();
    let t = r.list_tags(&[], None, 100).unwrap();
    let corp = t.tags.iter().find(|c| c.tag == "corporate").unwrap();
    assert_eq!(corp.count, 5);
    assert_eq!(corp.sources, vec![TagSource::Metadata]);
    let pref = r.list_tags(&[], Some("net"), 100).unwrap();
    assert!(pref.tags.iter().all(|c| c.tag.starts_with("net")));
    assert_eq!(r.list_tags(&[], None, 1).unwrap().tags.len(), 1);

    let root = r
        .list_folders(Some(&fx.main_id.to_string()), None, 10)
        .unwrap();
    assert_eq!(
        root.folders
            .iter()
            .map(|f| (f.name.as_str(), f.assets))
            .collect::<Vec<_>>(),
        vec![("network", 2), ("ui", 3)]
    );
    assert!(root.files.is_empty());
    let ui = r
        .list_folders(Some(&fx.main_id.to_string()), Some("ui"), 2)
        .unwrap();
    assert_eq!(ui.files.len(), 2);
    assert_eq!(ui.more_files, 1);
    assert!(
        r.list_folders(None, None, 10).is_err(),
        "two libraries: must choose"
    );
    assert!(matches!(
        r.list_folders(Some(&fx.main_id.to_string()), Some("nope"), 10),
        Err(CatalogError::NotFound(_))
    ));
}

#[test]
fn preview_sheet_with_placeholder() {
    let (_d, fx, r) = setup();
    let ids = [
        fx.ids["network/router.svg"],
        fx.ids["ui/broken.svg"],
        fx.ids["c4/person.svg"],
    ];
    let p = r.preview(&ids, PreviewOptions::default()).unwrap();
    assert_eq!((p.width, p.height), (3 * 128, 128 + 18));
    assert_eq!(p.legend.len(), 3);
    assert!(p.legend[0].problem.is_none());
    assert!(p.legend[1]
        .problem
        .as_deref()
        .unwrap()
        .contains("not a valid SVG"));
    assert_eq!(&p.png[1..4], b"PNG");
}

#[test]
fn export_svg_and_png_with_allowlist() {
    let (dir, fx, r) = setup();
    let out = dir.path().join("exports");
    std::fs::create_dir_all(&out).unwrap();
    let ids = [
        fx.ids["network/router.svg"],
        fx.ids["ui/home.svg"],
        fx.ids["ui/broken.svg"],
    ];
    let opts = ExportOptions {
        dest: out.join("svg"),
        format: ExportFormat::Svg,
        size: 256,
        overwrite: false,
        naming: ExportNaming::Name,
    };
    let roots = vec![out.clone()];
    let res = r.export(&ids, &opts, Some(&roots)).unwrap();
    assert_eq!(
        res.written.len(),
        3,
        "SVG copy works even for unparseable files"
    );
    assert!(res.written[0].path.ends_with("cisco-edge-router.svg"));
    let again = r.export(&ids[..1], &opts, Some(&roots)).unwrap();
    assert_eq!(again.skipped.len(), 1, "no overwrite by default");

    let png = r
        .export(
            &ids,
            &ExportOptions {
                dest: out.join("png"),
                format: ExportFormat::Png,
                size: 64,
                naming: ExportNaming::Original,
                ..opts.clone()
            },
            Some(&roots),
        )
        .unwrap();
    assert_eq!(png.written.len(), 2);
    assert_eq!(png.skipped.len(), 1, "broken file can't render");
    let bytes = std::fs::read(&png.written[0].path).unwrap();
    assert_eq!(&bytes[1..4], b"PNG");

    let denied = r.export(
        &ids,
        &ExportOptions {
            dest: dir.path().join("elsewhere"),
            ..opts
        },
        Some(&roots),
    );
    assert!(matches!(denied, Err(CatalogError::Denied(_))));
}

#[test]
fn tag_edits_write_metadata_and_refresh() {
    let (_d, fx, r) = setup();
    let person = fx.ids["c4/person.svg"];
    let res = r
        .update_tags(&[person], &["Stakeholder".into()], &[])
        .unwrap();
    assert_eq!(res.updated[0].metadata_tags, vec!["stakeholder"]);
    assert!(fx.other_root.join("iconarium.yaml").is_file());
    let hits = r
        .search(&SearchRequest {
            tags: vec!["stakeholder".into()],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        hits.hits[0].id, person,
        "visible immediately after the write"
    );
    assert!(r.update_tags(&[person], &[], &[]).is_err());
}

#[test]
fn background_refresh_sees_app_changes_and_ignores_thumbnail_churn() {
    let (_d, fx, r) = setup();
    let r = Arc::new(r.with_refresh_interval(Duration::from_millis(0)));
    let g0 = r.snapshot().generation;

    // Thumbnail bookkeeping (what a busy app does constantly) changes nothing.
    {
        let mut db = Database::open(&fx.index).unwrap();
        db.record_thumbnail(fx.ids["ui/home.svg"], 256, "k")
            .unwrap();
    }
    assert!(!r.refresh_now().unwrap());
    assert_eq!(r.snapshot().generation, g0);

    // The app discovers a new file.
    std::fs::write(
        fx.main_root.join("ui/new-bell.svg"),
        "<svg xmlns='http://www.w3.org/2000/svg' width='4' height='4'/>",
    )
    .unwrap();
    {
        let mut db = Database::open(&fx.index).unwrap();
        db.insert_assets(
            fx.main_id,
            &[DiscoveredFile {
                relative_path: "ui/new-bell.svg".into(),
                filename: "new-bell.svg".into(),
                file_size: 10,
                mtime_ns: 1,
            }],
        )
        .unwrap();
    }
    let old_cursor = r
        .search(&SearchRequest {
            limit: Some(1),
            ..Default::default()
        })
        .unwrap()
        .next_cursor
        .unwrap();
    r.refresh_in_background();
    for _ in 0..200 {
        if r.snapshot().generation > g0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(r.snapshot().generation > g0);
    assert_eq!(search(&r, "bell").total, 1);
    let stale = r.search(&SearchRequest {
        cursor: Some(old_cursor),
        ..Default::default()
    });
    assert!(matches!(stale, Err(CatalogError::StaleCursor)));
    // Unchanged libraries are reused, not rebuilt.
    let snap = r.snapshot();
    assert_eq!(snap.libraries.len(), 2);
}

#[test]
fn opening_never_creates_an_index() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("missing.sqlite3");
    assert!(IndexReader::open(&p).is_err());
    assert!(!p.exists());
}
