//! [`IndexReader`]: every library of an Iconarium index, in memory, kept fresh.
//!
//! The reader never holds a database connection between operations: it opens the index
//! query-only (`Database::open_existing_query_only`), reads, and closes. On Windows a
//! long-lived handle would block the app's corrupt-index recovery (rename the file aside).
//!
//! Freshness: [`IndexReader::refresh_in_background`] compares a cheap per-library
//! signature (asset count, analyzed count, max id, last analysis time, last scan, root
//! availability, `iconarium.yaml` mtime) and rebuilds only libraries that changed, on a
//! background thread, then swaps the snapshot. Thumbnail bookkeeping writes by the app do
//! not change signatures, so a busy app doesn't cause rebuild storms.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use parking_lot::{Mutex, RwLock};
use svg_core::config::Settings;
use svg_core::index::Database;
use svg_core::model::{AssetId, AssetRecord, LibraryId, LibraryInfo, LibraryStats, SearchText};
use svg_core::search::Catalog;

use crate::error::Result;
use crate::metadata::{self, LoadedMetadata, METADATA_FILE};
use crate::tags::{self, TagSource};

/// Minimum time between background freshness checks.
pub const DEFAULT_REFRESH_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq)]
struct Signature {
    stats: LibraryStats,
    last_scan: Option<i64>,
    path: String,
    display_name: String,
    available: bool,
    metadata_mtime: Option<SystemTime>,
}

/// One library, loaded.
#[derive(Debug)]
pub struct LibraryData {
    pub info: LibraryInfo,
    pub stats: LibraryStats,
    pub root: PathBuf,
    /// Root folder existed when loaded.
    pub available: bool,
    /// Search catalog. Searchable text is the SVG's own plus iconarium.yaml titles and
    /// descriptions (description field) and metadata tags (identifier field).
    pub catalog: Catalog,
    /// Derived tags per asset (see [`crate::tags`]).
    pub tags: HashMap<AssetId, Vec<(String, TagSource)>>,
    /// Lower-cased relative path → asset id.
    pub by_path: HashMap<String, AssetId>,
    pub metadata: Option<LoadedMetadata>,
    pub warnings: Vec<String>,
    signature: Signature,
}

impl LibraryData {
    /// Absolute path of a library-relative path.
    pub fn absolute(&self, relative_path: &str) -> PathBuf {
        svg_core::library::scanner::to_absolute(&self.root, relative_path)
    }

    pub fn asset_by_path(&self, relative_path: &str) -> Option<&AssetRecord> {
        let key = metadata::normalize_key(relative_path).to_lowercase();
        self.by_path.get(&key).and_then(|id| self.catalog.get(*id))
    }

    /// Display name of an asset (metadata title → SVG title → humanized filename).
    pub fn asset_name(&self, rec: &AssetRecord) -> String {
        let svg_title = self.catalog.get_text(rec.id).map(|t| t.title.as_str());
        let meta_title = self
            .metadata
            .as_ref()
            .and_then(|m| m.asset(&rec.relative_path))
            .and_then(|a| a.title.as_deref());
        crate::names::display_name(&rec.filename, svg_title, meta_title)
    }

    pub fn tag_names(&self, id: AssetId) -> Vec<String> {
        self.tags
            .get(&id)
            .map(|v| v.iter().map(|(t, _)| t.clone()).collect())
            .unwrap_or_default()
    }
}

/// An immutable view of the whole index.
#[derive(Debug)]
pub struct Snapshot {
    /// Increments on every rebuild; part of pagination cursors.
    pub generation: u64,
    /// Most recently opened first.
    pub libraries: Vec<Arc<LibraryData>>,
    pub settings: Settings,
    names: HashMap<LibraryId, String>,
}

impl Snapshot {
    pub fn library(&self, id: LibraryId) -> Option<&Arc<LibraryData>> {
        self.libraries.iter().find(|l| l.info.id == id)
    }

    /// Display name, disambiguated with the root when two libraries share a folder name.
    pub fn library_name(&self, id: LibraryId) -> &str {
        self.names.get(&id).map(String::as_str).unwrap_or("?")
    }

    /// The library holding asset `id` (ids are unique across the index).
    pub fn find_asset(&self, id: AssetId) -> Option<(&Arc<LibraryData>, &AssetRecord)> {
        self.libraries
            .iter()
            .find_map(|l| l.catalog.get(id).map(|r| (l, r)))
    }

    pub fn asset_count(&self) -> usize {
        self.libraries.iter().map(|l| l.catalog.len()).sum()
    }
}

#[derive(Debug, Default)]
struct RefreshState {
    last_check: Option<Instant>,
    running: bool,
    last_error: Option<String>,
}

/// Reader over one index file.
#[derive(Debug)]
pub struct IndexReader {
    path: PathBuf,
    current: RwLock<Arc<Snapshot>>,
    refresh: Mutex<RefreshState>,
    /// Held for a whole rebuild so two rebuilds never race (an older snapshot must not
    /// replace a newer one, and generations must stay unique).
    rebuild: Mutex<()>,
    interval: Duration,
}

impl IndexReader {
    /// Open and fully load the index at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        let db = Database::open_existing_query_only(path)?;
        let (snapshot, _) = load(&db, None, 1)?;
        Ok(Self {
            path: path.to_path_buf(),
            current: RwLock::new(Arc::new(snapshot)),
            refresh: Mutex::new(RefreshState {
                last_check: Some(Instant::now()),
                ..Default::default()
            }),
            rebuild: Mutex::new(()),
            interval: DEFAULT_REFRESH_INTERVAL,
        })
    }

    /// Change the minimum time between background freshness checks.
    pub fn with_refresh_interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The current snapshot (cheap; never blocks on a rebuild).
    pub fn snapshot(&self) -> Arc<Snapshot> {
        self.current.read().clone()
    }

    /// A short-lived query-only connection, for lookups not kept in memory.
    pub fn db(&self) -> Result<Database> {
        Ok(Database::open_existing_query_only(&self.path)?)
    }

    /// The last background refresh error (e.g. the app upgraded the schema), if any.
    pub fn health(&self) -> Option<String> {
        self.refresh.lock().last_error.clone()
    }

    /// Check for changes and rebuild synchronously. Returns whether anything changed.
    pub fn refresh_now(&self) -> Result<bool> {
        let _one_at_a_time = self.rebuild.lock();
        let prev = self.snapshot();
        let db = self.db()?;
        let (mut next, changed) = load(&db, Some(&prev), prev.generation)?;
        drop(db);
        if changed {
            let mut cur = self.current.write();
            next.generation = cur.generation + 1;
            *cur = Arc::new(next);
        }
        let mut st = self.refresh.lock();
        st.last_check = Some(Instant::now());
        st.last_error = None;
        Ok(changed)
    }

    /// If the last check is older than the refresh interval and none is running, check
    /// and rebuild on a background thread. Returns immediately.
    pub fn refresh_in_background(self: &Arc<Self>) {
        {
            let mut st = self.refresh.lock();
            let due = st.last_check.is_none_or(|t| t.elapsed() >= self.interval);
            if st.running || !due {
                return;
            }
            st.running = true;
        }
        let me = self.clone();
        let spawned = std::thread::Builder::new()
            .name("index-refresh".into())
            .spawn(move || {
                // Whatever happens (including a panic on hostile input), clear `running`
                // so later refreshes are not blocked forever.
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| me.refresh_now()));
                let mut st = me.refresh.lock();
                st.running = false;
                st.last_check = Some(Instant::now());
                let err = match r {
                    Ok(Ok(_)) => None,
                    Ok(Err(e)) => Some(e.to_string()),
                    Err(_) => Some("internal error while reloading the index".to_string()),
                };
                if let Some(e) = err {
                    tracing::warn!("index refresh failed: {e}");
                    st.last_error = Some(e);
                }
            });
        if spawned.is_err() {
            self.refresh.lock().running = false;
        }
    }
}

fn signature(db: &Database, info: &LibraryInfo) -> Result<Signature> {
    let root = Path::new(&info.path);
    Ok(Signature {
        stats: db.library_stats(info.id)?,
        last_scan: info.last_scan,
        path: info.path.clone(),
        display_name: info.display_name.clone(),
        available: root.is_dir(),
        metadata_mtime: std::fs::metadata(root.join(METADATA_FILE))
            .and_then(|m| m.modified())
            .ok(),
    })
}

/// Fold iconarium.yaml text into the searchable text so titles, descriptions and tags
/// from metadata are found by ordinary search.
fn augment_text(
    text: Option<SearchText>,
    rel: &str,
    tags: &[(String, TagSource)],
    meta: Option<&LoadedMetadata>,
) -> Option<SearchText> {
    let asset = meta.and_then(|m| m.asset(rel));
    let meta_tags: Vec<&str> = tags
        .iter()
        .filter(|(_, s)| *s == TagSource::Metadata)
        .map(|(t, _)| t.as_str())
        .collect();
    let has_extra = asset.is_some_and(|a| a.title.is_some() || a.description.is_some())
        || !meta_tags.is_empty();
    if !has_extra {
        return text;
    }
    let mut t = text.unwrap_or_default();
    let mut extra: Vec<&str> = Vec::new();
    if let Some(a) = asset {
        extra.extend(a.title.as_deref());
        extra.extend(a.description.as_deref());
    }
    if !extra.is_empty() {
        let joined = extra.join(" ");
        t.description = if t.description.is_empty() {
            joined
        } else {
            format!("{} {joined}", t.description)
        };
    }
    if !meta_tags.is_empty() {
        let joined = meta_tags.join(" ");
        t.identifiers = if t.identifiers.is_empty() {
            joined
        } else {
            format!("{} {joined}", t.identifiers)
        };
    }
    Some(t)
}

fn build_library(db: &Database, info: &LibraryInfo, sig: Signature) -> Result<LibraryData> {
    let root = PathBuf::from(&info.path);
    let mut warnings = Vec::new();
    let metadata = if sig.available {
        match metadata::load(&root) {
            Ok(m) => m,
            Err(e) => {
                warnings.push(format!("ignored {METADATA_FILE}: {e}"));
                None
            }
        }
    } else {
        None
    };
    let items = db.load_catalog(info.id)?;
    let mut tag_map = HashMap::with_capacity(items.len());
    let mut by_path = HashMap::with_capacity(items.len());
    let mut catalog = Catalog::new();
    catalog.extend(items.into_iter().map(|(rec, text)| {
        let t = tags::derive(
            &rec.relative_path,
            text.as_ref().map(|t| t.description.as_str()),
            metadata.as_ref(),
        );
        let text = augment_text(text, &rec.relative_path, &t, metadata.as_ref());
        by_path.insert(rec.relative_path.to_lowercase(), rec.id);
        tag_map.insert(rec.id, t);
        (rec, text)
    }));
    Ok(LibraryData {
        info: info.clone(),
        stats: sig.stats,
        root,
        available: sig.available,
        catalog,
        tags: tag_map,
        by_path,
        metadata,
        warnings,
        signature: sig,
    })
}

/// Build a snapshot, reusing unchanged libraries from `prev`. Returns whether it differs.
fn load(db: &Database, prev: Option<&Snapshot>, generation: u64) -> Result<(Snapshot, bool)> {
    let infos = db.recent_libraries(usize::MAX >> 1)?;
    let settings = db.load_settings()?;
    let mut changed = prev.is_none_or(|p| {
        p.settings != settings
            || p.libraries.len() != infos.len()
            || p.libraries
                .iter()
                .zip(&infos)
                .any(|(a, b)| a.info.id != b.id)
    });
    let mut libraries = Vec::with_capacity(infos.len());
    for info in &infos {
        let sig = signature(db, info)?;
        let reuse = prev
            .and_then(|p| p.library(info.id))
            .filter(|l| l.signature == sig)
            .cloned();
        match reuse {
            Some(l) => libraries.push(l),
            None => {
                changed = true;
                libraries.push(Arc::new(build_library(db, info, sig)?));
            }
        }
    }
    // Disambiguate shared folder names with the root path.
    let mut count: HashMap<String, usize> = HashMap::new();
    for l in &libraries {
        *count.entry(l.info.display_name.to_lowercase()).or_default() += 1;
    }
    let names = libraries
        .iter()
        .map(|l| {
            let n = &l.info.display_name;
            let name = if count[&n.to_lowercase()] > 1 {
                format!("{n} ({})", l.info.path)
            } else {
                n.clone()
            };
            (l.info.id, name)
        })
        .collect();
    Ok((
        Snapshot {
            generation,
            libraries,
            settings,
            names,
        },
        changed,
    ))
}
