//! Throttled event delivery to the UI. Producers mark state dirty; a ticker thread
//! flushes at most every `TICK` so bursts (scans, analysis of thousands of files)
//! never flood the WebView.

use parking_lot::Mutex;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use svg_core::model::AssetId;
use tauri::{AppHandle, Emitter};

pub const EV_SCAN: &str = "scan://progress";
pub const EV_CATALOG: &str = "catalog://changed";
pub const EV_THUMB: &str = "thumb://ready";

const TICK: Duration = Duration::from_millis(250);
/// Metadata-only catalog changes (analysis results) are flushed less often.
const METADATA_EVERY_TICKS: u32 = 4;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScanPhase {
    Idle,
    Loading,
    Discovering,
    Reconciling,
    Processing,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanStatus {
    pub phase: ScanPhase,
    pub discovered: usize,
    pub processed: usize,
    pub total: usize,
    pub message: String,
}

impl Default for ScanStatus {
    fn default() -> Self {
        Self { phase: ScanPhase::Idle, discovered: 0, processed: 0, total: 0, message: String::new() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeReason {
    Load,
    Scan,
    Watch,
    Metadata,
}

#[derive(Serialize, Clone)]
struct CatalogChanged {
    total: usize,
    reason: ChangeReason,
}

#[derive(Serialize, Clone)]
struct ThumbReady {
    ids: Vec<AssetId>,
}

#[derive(Default)]
struct Pending {
    /// Highest-importance structural change since the last flush (Load > Scan/Watch).
    structural: Option<ChangeReason>,
    metadata: bool,
    thumbs: Vec<AssetId>,
    status_dirty: bool,
}

pub struct EventHub {
    app: AppHandle,
    pending: Mutex<Pending>,
    status: Mutex<ScanStatus>,
    total: Mutex<usize>,
    stop: AtomicBool,
}

impl EventHub {
    pub fn new(app: AppHandle) -> Arc<Self> {
        let hub = Arc::new(Self {
            app,
            pending: Mutex::new(Pending::default()),
            status: Mutex::new(ScanStatus::default()),
            total: Mutex::new(0),
            stop: AtomicBool::new(false),
        });
        let weak = Arc::downgrade(&hub);
        std::thread::Builder::new()
            .name("event-hub".into())
            .spawn(move || {
                let mut tick: u32 = 0;
                loop {
                    std::thread::sleep(TICK);
                    let Some(hub) = weak.upgrade() else { break };
                    if hub.stop.load(Ordering::Relaxed) {
                        break;
                    }
                    tick = tick.wrapping_add(1);
                    hub.flush(tick % METADATA_EVERY_TICKS == 0);
                }
            })
            .expect("spawn event hub");
        hub
    }

    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn status(&self) -> ScanStatus {
        self.status.lock().clone()
    }

    pub fn update_status(&self, f: impl FnOnce(&mut ScanStatus)) {
        f(&mut self.status.lock());
        self.pending.lock().status_dirty = true;
    }

    pub fn set_total(&self, total: usize) {
        *self.total.lock() = total;
    }

    /// Structural catalog change (assets added/removed/library switched).
    pub fn catalog_changed(&self, reason: ChangeReason) {
        let mut p = self.pending.lock();
        p.structural = match (p.structural, reason) {
            (Some(ChangeReason::Load), _) | (_, ChangeReason::Load) => Some(ChangeReason::Load),
            (_, r) => Some(r),
        };
    }

    /// Analysis results changed state/geometry of some assets.
    pub fn metadata_changed(&self) {
        self.pending.lock().metadata = true;
    }

    /// Thumbnails rendered in the background (no request was waiting for them).
    pub fn thumbs_ready(&self, ids: &[AssetId]) {
        self.pending.lock().thumbs.extend_from_slice(ids);
    }

    /// Emit a library load immediately (no throttle) so the UI resets promptly.
    pub fn emit_load_now(&self, total: usize) {
        self.set_total(total);
        {
            let mut p = self.pending.lock();
            p.structural = None;
            p.metadata = false;
            p.thumbs.clear();
        }
        let _ = self.app.emit(EV_CATALOG, CatalogChanged { total, reason: ChangeReason::Load });
        let _ = self.app.emit(EV_SCAN, self.status());
    }

    fn flush(&self, allow_metadata: bool) {
        let (structural, metadata, thumbs, status_dirty) = {
            let mut p = self.pending.lock();
            let metadata = allow_metadata && std::mem::take(&mut p.metadata);
            (p.structural.take(), metadata, std::mem::take(&mut p.thumbs), std::mem::take(&mut p.status_dirty))
        };
        let total = *self.total.lock();
        if status_dirty {
            let _ = self.app.emit(EV_SCAN, self.status());
        }
        if let Some(reason) = structural {
            let _ = self.app.emit(EV_CATALOG, CatalogChanged { total, reason });
        } else if metadata {
            let _ = self.app.emit(EV_CATALOG, CatalogChanged { total, reason: ChangeReason::Metadata });
        }
        if !thumbs.is_empty() {
            for chunk in thumbs.chunks(2000) {
                let _ = self.app.emit(EV_THUMB, ThumbReady { ids: chunk.to_vec() });
            }
        }
    }
}
