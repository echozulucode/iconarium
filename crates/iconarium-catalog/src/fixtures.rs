//! A small on-disk test setup: two libraries (sharing a folder name), real SVG files, an
//! iconarium.yaml, and an index built with the same svg-core calls the app uses. One file
//! is left unanalyzed (as if the app had not finished) and one is invalid.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use svg_core::config::Limits;
use svg_core::index::Database;
use svg_core::model::{AssetId, DiscoveredFile};
use svg_core::svg::analyze;

pub struct Fixture {
    pub index: PathBuf,
    /// "icons" library root (has iconarium.yaml).
    pub main_root: PathBuf,
    /// Second library, also named "icons".
    pub other_root: PathBuf,
    pub main_id: i64,
    pub other_id: i64,
    /// Relative path → asset id (both libraries; paths are distinct).
    pub ids: HashMap<String, AssetId>,
}

pub const MAIN_FILES: &[(&str, &str)] = &[
    (
        "network/router.svg",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 40 20"><title>Edge Router</title><rect width="40" height="20" fill="#3366cc"/></svg>"##,
    ),
    (
        "network/switches/24/switch.svg",
        r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32"><rect width="32" height="32" fill="#2a9d8f"/></svg>"##,
    ),
    (
        "ui/home.svg",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><desc>Keywords: house, start Text: Home</desc><path d="M2 12 12 2 22 12v10H2z" fill="#e76f51"/><text x="4" y="20" font-size="6">Home</text></svg>"##,
    ),
    (
        "ui/broken.svg",
        "<svg xmlns=\"http://www.w3.org/2000/svg\"><rect",
    ),
    (
        "ui/star-unanalyzed.svg",
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><title>Gold Star</title><circle cx="5" cy="5" r="5" fill="gold"/></svg>"##,
    ),
];

pub const OTHER_FILES: &[(&str, &str)] = &[(
    "c4/person.svg",
    r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20"><title>Person</title><desc>C4 person. Keywords: user, actor, c4 model Text: User</desc><circle cx="10" cy="6" r="5" fill="#0c37a2"/></svg>"##,
)];

pub const MAIN_METADATA: &str = "\
iconarium: 1
library:
  tags: [corporate]
folders:
  network:
    tags: [networking]
assets:
  network/router.svg:
    title: Cisco edge router
    description: Branch office WAN edge
    tags: [edge, wan]
";

fn write_files(root: &Path, files: &[(&str, &str)]) -> Vec<DiscoveredFile> {
    files
        .iter()
        .map(|(rel, body)| {
            let p = svg_core::library::scanner::to_absolute(root, rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, body).unwrap();
            DiscoveredFile {
                relative_path: rel.to_string(),
                filename: rel.rsplit('/').next().unwrap().to_string(),
                file_size: body.len() as u64,
                mtime_ns: 1_700_000_000_000_000_000,
            }
        })
        .collect()
}

/// Build the fixture under `dir`.
pub fn build(dir: &Path) -> Fixture {
    let main_root = dir.join("work").join("icons");
    let other_root = dir.join("other").join("icons");
    let main_files = write_files(&main_root, MAIN_FILES);
    let other_files = write_files(&other_root, OTHER_FILES);
    std::fs::write(main_root.join("iconarium.yaml"), MAIN_METADATA).unwrap();

    let index = dir.join("index.sqlite3");
    let mut db = Database::open(&index).unwrap();
    let limits = Limits::default();
    let mut ids = HashMap::new();
    let mut add = |db: &mut Database, root: &Path, files: &[DiscoveredFile]| -> i64 {
        let lib = db.upsert_library(&root.to_string_lossy(), "icons").unwrap();
        let recs = db.insert_assets(lib.id, files).unwrap();
        for r in &recs {
            ids.insert(r.relative_path.clone(), r.id);
            if r.relative_path.contains("unanalyzed") {
                continue;
            }
            let bytes = std::fs::read(svg_core::library::scanner::to_absolute(
                root,
                &r.relative_path,
            ))
            .unwrap();
            db.save_analysis(r.id, &analyze(&bytes, &limits)).unwrap();
        }
        db.set_library_scanned(lib.id, 1_700_000_000).unwrap();
        lib.id
    };
    let other_id = add(&mut db, &other_root, &other_files);
    let main_id = add(&mut db, &main_root, &main_files);
    Fixture {
        index,
        main_root,
        other_root,
        main_id,
        other_id,
        ids,
    }
}
