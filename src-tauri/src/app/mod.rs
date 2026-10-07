//! Application core: owns the database, the in-memory catalog, settings, the active
//! library, the work queue and background threads. Tauri commands are thin wrappers
//! around methods on [`AppCore`].

pub mod events;
pub mod library;
pub mod logging;
pub mod paths;
pub mod tempfiles;
pub mod watch;

use crate::preview::queue::WorkQueue;
use crate::preview::worker::DbWrite;
use crossbeam_channel::Sender;
use parking_lot::{Condvar, Mutex, RwLock};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use svg_core::config::Settings;
use svg_core::index::Database;
use svg_core::model::{AssetId, AssetRecord, LibraryInfo, ProcessingState, SvgMeta, ViewBox};
use svg_core::search::Catalog;
use tauri::{AppHandle, UriSchemeResponder};

use events::EventHub;
use paths::AppPaths;
use tempfiles::TempFiles;

#[derive(Debug, Clone)]
pub struct ActiveLibrary {
    pub info: LibraryInfo,
    pub root: PathBuf,
}

pub struct AppCore {
    pub app: AppHandle,
    pub paths: AppPaths,
    pub db: Mutex<Database>,
    pub catalog: RwLock<Catalog>,
    pub settings: RwLock<Settings>,
    pub active: RwLock<Option<ActiveLibrary>>,
    pub events: Arc<EventHub>,
    pub queue: WorkQueue,
    /// Thumbnail requests waiting for a render, keyed by asset.
    pub waiters: Mutex<HashMap<AssetId, Vec<UriSchemeResponder>>>,
    pub db_tx: Sender<DbWrite>,
    pub temp: TempFiles,
    /// Incremented on every library switch; background work for an older
    /// generation is discarded.
    pub generation: AtomicU64,
    pub(crate) scan_cancel: Mutex<Arc<AtomicBool>>,
    pub(crate) watcher: Mutex<Option<watch::WatchHandle>>,
    /// Counters for the "processing" status line.
    pub(crate) processed: AtomicU64,
    /// Generation of the scan currently running (0 = none).
    pub(crate) scan_gen: AtomicU64,
    /// Serializes library switches (startup restore vs. user pick).
    pub(crate) open_lock: Mutex<()>,
    restore: (Mutex<bool>, Condvar),
}

impl AppCore {
    pub fn new(app: AppHandle, paths: AppPaths, db: Database, db_tx: Sender<DbWrite>) -> Self {
        let settings = db.load_settings().unwrap_or_default();
        let events = EventHub::new(app.clone());
        let temp = TempFiles::new(paths.tmp_dir.clone());
        Self {
            app,
            paths,
            db: Mutex::new(db),
            catalog: RwLock::new(Catalog::new()),
            settings: RwLock::new(settings),
            active: RwLock::new(None),
            events,
            queue: WorkQueue::new(),
            waiters: Mutex::new(HashMap::new()),
            db_tx,
            temp,
            generation: AtomicU64::new(0),
            scan_cancel: Mutex::new(Arc::new(AtomicBool::new(false))),
            watcher: Mutex::new(None),
            processed: AtomicU64::new(0),
            scan_gen: AtomicU64::new(0),
            open_lock: Mutex::new(()),
            restore: (Mutex::new(false), Condvar::new()),
        }
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().clone()
    }

    pub fn active(&self) -> Option<ActiveLibrary> {
        self.active.read().clone()
    }

    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// Absolute path of an asset in the active library.
    pub fn absolute_path(&self, rec: &AssetRecord) -> Option<PathBuf> {
        let active = self.active.read();
        let lib = active.as_ref()?;
        if lib.info.id != rec.library_id {
            return None;
        }
        Some(svg_core::library::scanner::to_absolute(&lib.root, &rec.relative_path))
    }

    pub fn record(&self, id: AssetId) -> Option<AssetRecord> {
        self.catalog.read().get(id).cloned()
    }

    /// Record + absolute path, or a not-found error message.
    pub fn locate(&self, id: AssetId) -> Result<(AssetRecord, PathBuf), crate::error::CmdError> {
        let rec = self.record(id).ok_or_else(|| crate::error::CmdError::not_found(format!("Asset {id}")))?;
        let path = self.absolute_path(&rec).ok_or_else(crate::error::CmdError::no_library)?;
        Ok((rec, path))
    }

    /// Read an asset's bytes, enforcing the configured hard size limit.
    pub fn read_asset_bytes(&self, rec: &AssetRecord, path: &Path) -> Result<Vec<u8>, crate::error::CmdError> {
        let limits = self.settings.read().limits.clone();
        let size = std::fs::metadata(path)?.len();
        if size > limits.max_file_bytes || rec.state == ProcessingState::LimitExceeded {
            return Err(crate::error::CmdError::new(
                "limit_exceeded",
                format!(
                    "{} exceeds the processing limit ({} > {})",
                    rec.filename,
                    crate::util::human_bytes(size),
                    crate::util::human_bytes(limits.max_file_bytes)
                ),
            ));
        }
        Ok(std::fs::read(path)?)
    }

    /// Geometry metadata for the viewer and region tools. Analyzes inline (and records the
    /// result) when the background pipeline has not reached this asset yet.
    pub fn svg_meta(&self, id: AssetId) -> Result<(AssetRecord, SvgMeta), crate::error::CmdError> {
        let (rec, path) = self.locate(id)?;
        let doc_box: Option<ViewBox> = if rec.state == ProcessingState::Ready {
            self.db.lock().get_doc_box(id).ok().flatten()
        } else {
            None
        };
        if let Some(doc_box) = doc_box {
            let meta = SvgMeta {
                width: rec.width,
                height: rec.height,
                view_box: rec.view_box,
                doc_box: Some(doc_box),
                element_count: rec.element_count.unwrap_or(0),
            };
            return Ok((rec, meta));
        }
        if !rec.state.renderable() {
            return Ok((rec, SvgMeta::default()));
        }
        let bytes = self.read_asset_bytes(&rec, &path)?;
        let limits = self.settings.read().limits.clone();
        let analysis = svg_core::svg::analyze(&bytes, &limits);
        let meta = analysis.meta.clone();
        let rec = crate::preview::worker::apply_analysis(self, rec, analysis);
        Ok((rec, meta))
    }

    /// Whether a discovery/reconcile pass for the *current* library is running.
    pub fn scan_in_progress(&self) -> bool {
        let g = self.scan_gen.load(Ordering::SeqCst);
        g != 0 && g == self.generation()
    }

    /// Refresh the status line from queue state once no scan is running.
    pub fn update_processing_status(&self) {
        let analyzing = self.queue.analyze_pending();
        let remaining = self.queue.len();
        let processed = self.processed.load(Ordering::Relaxed) as usize;
        self.events.update_status(|s| {
            if remaining == 0 {
                s.phase = events::ScanPhase::Idle;
                s.message = "Indexed".into();
            } else if analyzing > 0 {
                s.phase = events::ScanPhase::Processing;
                s.processed = processed;
                s.total = processed + analyzing;
                s.message = format!("Reading contents ({analyzing} left)");
            } else {
                // Contents fully indexed; only background thumbnail prefill remains.
                s.phase = events::ScanPhase::Idle;
                s.message = format!("Indexed · generating previews ({remaining} left)");
            }
        });
    }

    /// Whether any catalog asset lives under the directory `rel_dir` ('/'-separated).
    pub fn has_assets_under(&self, rel_dir: &str) -> bool {
        let prefix = format!("{}/", rel_dir.trim_end_matches('/'));
        let cat = self.catalog.read();
        let found = cat.ids().any(|id| cat.get(id).is_some_and(|r| r.relative_path.starts_with(&prefix)));
        found
    }

    // ---- startup restore handshake -------------------------------------------------

    pub fn mark_restore_done(&self) {
        let (m, cv) = &self.restore;
        *m.lock() = true;
        cv.notify_all();
    }

    /// Wait (bounded) for the startup restore of the last library so the first
    /// `get_app_state` sees it.
    pub fn wait_restore(&self, timeout: Duration) {
        let (m, cv) = &self.restore;
        let mut done = m.lock();
        if !*done {
            let _ = cv.wait_for(&mut done, timeout);
        }
    }

    pub fn shutdown(&self) {
        self.scan_cancel.lock().store(true, Ordering::SeqCst);
        *self.watcher.lock() = None;
        self.queue.shutdown();
        self.events.shutdown();
        self.temp.purge_all();
    }
}
