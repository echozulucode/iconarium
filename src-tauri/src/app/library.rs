//! Library lifecycle: open/restore, progressive scan + reconcile, incremental changes.

use super::events::{ChangeReason, ScanPhase};
use super::{ActiveLibrary, AppCore};
use crate::error::{CmdError, CmdResult};
use crate::preview::cache;
use crate::preview::queue::{JobFlags, P2, P3, P4};
use crate::preview::worker::fail_all_waiters;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use svg_core::library::scanner::{self, Reconciler, ScanOptions};
use svg_core::model::{AssetId, AssetRecord, LibraryId, LibraryInfo, ProcessingState};

/// Normalize a user-chosen folder path without canonicalizing (which on Windows would add
/// a `\\?\` prefix to everything shown in the UI).
fn normalize_root(path: &str) -> CmdResult<PathBuf> {
    let trimmed = path.trim();
    let trimmed = trimmed.trim_end_matches(['/', '\\']);
    let p = if trimmed.is_empty() || trimmed.ends_with(':') { PathBuf::from(path.trim()) } else { PathBuf::from(trimmed) };
    if !p.is_dir() {
        return Err(CmdError::new("not_found", format!("Folder not found: {}", p.display())));
    }
    Ok(p)
}

fn display_name(root: &Path) -> String {
    root.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.display().to_string())
}

impl AppCore {
    /// Switch the active library. Returns as soon as the cached catalog is loaded; discovery
    /// and reconciliation continue in the background.
    pub fn open_library(self: &Arc<Self>, path: &str) -> CmdResult<LibraryInfo> {
        let root = normalize_root(path)?;
        let t0 = Instant::now();
        // Library switches are serialized (startup restore vs. a user pick).
        let _switch = self.open_lock.lock();

        // Stop everything belonging to the previous library. The generation is bumped
        // *first*, so any in-flight batch/job of the old library fails its generation check.
        let gen = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.scan_cancel.lock().store(true, Ordering::SeqCst);
        *self.watcher.lock() = None;
        self.queue.clear();
        fail_all_waiters(self);

        let root_str = self.canonical_library_path(&root);
        let (info, items) = {
            let mut db = self.db.lock();
            let info = db.upsert_library(&root_str, &display_name(&root))?;
            let items = db.load_catalog(info.id)?;
            (info, items)
        };
        let total = items.len();
        {
            // Swap catalog and active library together so readers never see a mix.
            let mut cat = self.catalog.write();
            cat.clear();
            cat.extend(items);
            *self.active.write() = Some(ActiveLibrary { info: info.clone(), root: root.clone() });
        }
        self.processed.store(0, Ordering::Relaxed);
        self.events.update_status(|s| {
            s.phase = if total > 0 { ScanPhase::Reconciling } else { ScanPhase::Discovering };
            s.discovered = total;
            s.processed = 0;
            s.total = total;
            s.message = if total > 0 { "Checking for changes…".into() } else { "Scanning…".into() };
        });
        self.events.emit_load_now(total);
        tracing::info!("opened library {} ({} cached assets) in {:?}", root.display(), total, t0.elapsed());

        let cancel = Arc::new(AtomicBool::new(false));
        *self.scan_cancel.lock() = cancel.clone();
        let core = self.clone();
        let lib_id = info.id;
        std::thread::Builder::new()
            .name("scanner".into())
            .spawn(move || {
                core.run_scan(lib_id, &root, &cancel, total == 0, gen);
                if !cancel.load(Ordering::SeqCst) && core.generation() == gen {
                    core.start_watcher(lib_id, &root, gen);
                }
            })
            .map_err(CmdError::from)?;
        Ok(info)
    }

    /// Reopen the most recently used library at startup (if it still exists).
    pub fn restore_last_library(self: &Arc<Self>) {
        let recent = self.db.lock().recent_libraries(1).unwrap_or_default();
        if let Some(lib) = recent.into_iter().next() {
            if Path::new(&lib.path).is_dir() {
                if let Err(e) = self.open_library(&lib.path) {
                    tracing::warn!("restoring {}: {e}", lib.path);
                }
            }
        }
        self.mark_restore_done();
    }

    /// Progressive discovery + reconciliation against the persisted index.
    pub(crate) fn run_scan(self: &Arc<Self>, lib_id: LibraryId, root: &Path, cancel: &AtomicBool, first_scan: bool, gen: u64) {
        if self.generation() != gen {
            return;
        }
        self.scan_gen.store(gen, Ordering::SeqCst);
        let t0 = Instant::now();
        let settings = self.settings();
        let opts = ScanOptions { batch_size: settings.scan_batch_size, ignore_hidden: settings.ignore_hidden, follow_links: false };
        let snapshot = match self.db.lock().library_snapshot(lib_id) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("snapshot failed: {e}");
                let _ = self.scan_gen.compare_exchange(gen, 0, Ordering::SeqCst, Ordering::SeqCst);
                return;
            }
        };
        let mut reconciler = Reconciler::new(snapshot);
        let mut discovered = 0usize;
        let thumbs_dir = self.paths.thumbs_dir.clone();
        let thumb_size = settings.thumbnail_size;
        let phase = if first_scan { ScanPhase::Discovering } else { ScanPhase::Reconciling };

        let result = scanner::scan(root, &opts, cancel, |batch| {
            if self.generation() != gen {
                return;
            }
            discovered += batch.len();
            let diff = reconciler.classify(batch);
            let mut changed_ids: Vec<AssetId> = Vec::new();
            if !diff.new.is_empty() || !diff.changed.is_empty() {
                let (new_recs, changed_recs, old_fps) = {
                    let mut db = self.db.lock();
                    let new_recs = db.insert_assets(lib_id, &diff.new).unwrap_or_else(|e| {
                        tracing::error!("insert_assets: {e}");
                        vec![]
                    });
                    let old_fps: Vec<(AssetId, String)> = {
                        let cat = self.catalog.read();
                        diff.changed.iter().filter_map(|(id, _)| cat.get(*id).map(|r| (*id, r.fast_fingerprint.clone()))).collect()
                    };
                    let changed_recs = db.update_changed(&diff.changed).unwrap_or_else(|e| {
                        tracing::error!("update_changed: {e}");
                        vec![]
                    });
                    (new_recs, changed_recs, old_fps)
                };
                for (id, fp) in old_fps {
                    cache::remove_thumb(&thumbs_dir, id, &fp, thumb_size);
                }
                let mut cat = self.catalog.write();
                // Re-check under the catalog lock: a library switch swaps the catalog
                // while holding this lock, so this batch can never land in another library.
                if self.generation() != gen {
                    return;
                }
                for rec in new_recs.into_iter().chain(changed_recs) {
                    changed_ids.push(rec.id);
                    cat.upsert(rec, None);
                }
                self.events.set_total(cat.len());
            }
            if !changed_ids.is_empty() {
                let flags = if settings.prefill_thumbnails { JobFlags::BOTH } else { JobFlags::ANALYZE };
                self.queue.push_many(changed_ids, P3, flags);
                self.events.catalog_changed(ChangeReason::Scan);
            }
            let total = self.catalog.read().len();
            self.events.update_status(|s| {
                s.phase = phase.clone();
                s.discovered = discovered;
                s.total = total;
                s.message = if first_scan {
                    format!("Indexing {discovered}…")
                } else {
                    "Checking for changes…".into()
                };
            });
        });

        let cancelled = cancel.load(Ordering::SeqCst) || self.generation() != gen;
        match result {
            Ok(stats) if !cancelled && !stats.cancelled => {
                let removed = reconciler.finish();
                if !removed.is_empty() {
                    self.remove_assets(&removed, gen);
                }
                let _ = self.db.lock().set_library_scanned(lib_id, crate::util::unix_now());
                tracing::info!(
                    "scan of {} finished: {} files, {} removed, {} errors in {:?}",
                    root.display(),
                    stats.files_found,
                    removed.len(),
                    stats.errors,
                    t0.elapsed()
                );
                self.enqueue_background(lib_id);
            }
            Ok(_) => tracing::info!("scan cancelled after {:?}", t0.elapsed()),
            Err(e) => {
                tracing::error!("scan failed: {e}");
                self.events.update_status(|s| {
                    s.phase = ScanPhase::Idle;
                    s.message = format!("Scan failed: {e}");
                });
            }
        }
        let _ = self.scan_gen.compare_exchange(gen, 0, Ordering::SeqCst, Ordering::SeqCst);
        if !cancelled {
            let remaining = self.queue.len();
            let total = self.catalog.read().len();
            self.events.set_total(total);
            self.events.catalog_changed(ChangeReason::Scan);
            let _ = remaining;
            self.events.update_status(|s| {
                s.total = total;
                s.discovered = total;
            });
            self.update_processing_status();
        }
    }

    /// Queue analysis for anything still `Discovered` (e.g. interrupted previous session)
    /// and, when enabled, thumbnails for assets whose cache file is missing.
    fn enqueue_background(&self, lib_id: LibraryId) {
        let pending = self.db.lock().pending_analysis(lib_id).unwrap_or_default();
        let settings = self.settings();
        let flags = if settings.prefill_thumbnails { JobFlags::BOTH } else { JobFlags::ANALYZE };
        self.queue.push_many(pending, P3, flags);
        if settings.prefill_thumbnails {
            let size = settings.thumbnail_size;
            let candidates: Vec<(AssetId, String)> = {
                let cat = self.catalog.read();
                cat.ids()
                    .filter_map(|id| cat.get(id))
                    .filter(|r| r.state == ProcessingState::Ready)
                    .map(|r| (r.id, r.fast_fingerprint.clone()))
                    .collect()
            };
            let missing = candidates
                .into_iter()
                .filter(|(id, fp)| !cache::thumb_path(&self.paths.thumbs_dir, *id, fp, size).exists())
                .map(|(id, _)| id);
            self.queue.push_many(missing, P4, JobFlags::THUMB);
        }
    }

    pub(crate) fn remove_assets(&self, ids: &[AssetId], gen: u64) {
        if ids.is_empty() {
            return;
        }
        if let Err(e) = self.db.lock().remove_assets(ids) {
            tracing::error!("remove_assets: {e}");
        }
        let size = self.settings.read().thumbnail_size;
        let stale: Vec<(AssetId, String)> = {
            let mut cat = self.catalog.write();
            if self.generation() != gen {
                return;
            }
            let stale = ids.iter().filter_map(|&id| cat.get(id).map(|r| (id, r.fast_fingerprint.clone()))).collect();
            for &id in ids {
                cat.remove(id);
            }
            self.events.set_total(cat.len());
            stale
        };
        // File deletion happens outside the catalog lock so search never stalls on I/O.
        for (id, fp) in stale {
            cache::remove_thumb(&self.paths.thumbs_dir, id, &fp, size);
        }
    }

    /// On Windows, paths are case-insensitive: reuse the stored spelling of an existing
    /// library so `C:\Lib` and `c:\lib` don't become two libraries.
    fn canonical_library_path(&self, root: &Path) -> String {
        let s = root.to_string_lossy().into_owned();
        if cfg!(windows) {
            if let Ok(libs) = self.db.lock().recent_libraries(10_000) {
                if let Some(l) = libs.into_iter().find(|l| l.path.eq_ignore_ascii_case(&s)) {
                    return l.path;
                }
            }
        }
        s
    }

    /// Apply watcher-reported changes for individual relative paths.
    pub(crate) fn apply_path_changes(&self, lib_id: LibraryId, root: &Path, rel_paths: &[String], gen: u64) {
        if self.generation() != gen {
            return;
        }
        let settings = self.settings();
        let size = settings.thumbnail_size;
        let mut removed: Vec<AssetId> = Vec::new();
        let mut touched: Vec<AssetRecord> = Vec::new();
        for rel in rel_paths {
            let existing = self.db.lock().get_asset_by_path(lib_id, rel).ok().flatten();
            match (scanner::stat_one(root, rel), existing) {
                (Some(df), None) => {
                    if let Ok(mut recs) = self.db.lock().insert_assets(lib_id, std::slice::from_ref(&df)) {
                        touched.append(&mut recs);
                    }
                }
                (Some(df), Some(old)) => {
                    if df.file_size != old.file_size || df.mtime_ns != old.mtime_ns {
                        cache::remove_thumb(&self.paths.thumbs_dir, old.id, &old.fast_fingerprint, size);
                        if let Ok(mut recs) = self.db.lock().update_changed(&[(old.id, df)]) {
                            touched.append(&mut recs);
                        }
                    }
                }
                (None, Some(old)) => removed.push(old.id),
                (None, None) => {}
            }
        }
        self.remove_assets(&removed, gen);
        if !touched.is_empty() {
            let ids: Vec<AssetId> = touched.iter().map(|r| r.id).collect();
            {
                let mut cat = self.catalog.write();
                if self.generation() != gen {
                    return;
                }
                for rec in touched {
                    cat.upsert(rec, None);
                }
                self.events.set_total(cat.len());
            }
            self.queue.push_many(ids, P2, JobFlags::BOTH);
        }
        if !removed.is_empty() || !rel_paths.is_empty() {
            self.events.catalog_changed(ChangeReason::Watch);
        }
    }

    /// Full reconcile requested by the watcher (bursts, directory renames, overflow).
    pub(crate) fn reconcile_now(self: &Arc<Self>, lib_id: LibraryId, root: &Path, gen: u64) {
        let cancel = self.scan_cancel.lock().clone();
        if cancel.load(Ordering::SeqCst) || self.generation() != gen {
            return;
        }
        // Avoid overlapping with an in-flight scan: wait briefly for it to finish.
        let deadline = Instant::now() + Duration::from_secs(30);
        while self.scan_in_progress() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
        self.run_scan(lib_id, root, &cancel, false, gen);
    }
}
