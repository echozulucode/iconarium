//! Worker pool: analysis (hash, limits, metadata, text) and thumbnail rendering, plus a
//! batching database writer so workers never hold the DB lock for long.

use super::cache;
use super::queue::{JobFlags, P3};
use crate::app::events::ScanPhase;
use crate::app::AppCore;
use crossbeam_channel::{Receiver, RecvTimeoutError};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use svg_core::config::Limits;
use svg_core::model::{Analysis, AssetId, AssetRecord, Complexity, ProcessingState, SearchText, SvgMeta};
use tauri::http::{header, Response, StatusCode};

pub enum DbWrite {
    Analysis(AssetId, Box<Analysis>),
    Thumb(AssetId, u32, String),
}

/// Single DB writer thread; flushes every 200 ms or 256 items.
pub fn spawn_db_writer(core: Arc<AppCore>, rx: Receiver<DbWrite>) {
    std::thread::Builder::new()
        .name("db-writer".into())
        .spawn(move || {
            let mut analyses: Vec<(AssetId, Analysis)> = Vec::new();
            let mut thumbs: Vec<(AssetId, u32, String)> = Vec::new();
            let mut last = Instant::now();
            loop {
                let msg = rx.recv_timeout(Duration::from_millis(200));
                let disconnected = matches!(msg, Err(RecvTimeoutError::Disconnected));
                match msg {
                    Ok(DbWrite::Analysis(id, a)) => analyses.push((id, *a)),
                    Ok(DbWrite::Thumb(id, size, key)) => thumbs.push((id, size, key)),
                    Err(_) => {}
                }
                let due = last.elapsed() >= Duration::from_millis(200) || analyses.len() + thumbs.len() >= 256;
                if (due || disconnected) && !(analyses.is_empty() && thumbs.is_empty()) {
                    let mut db = core.db.lock();
                    if !analyses.is_empty() {
                        if let Err(e) = db.save_analyses(&analyses) {
                            tracing::warn!("saving {} analyses failed: {e}", analyses.len());
                        }
                        analyses.clear();
                    }
                    for (id, size, key) in thumbs.drain(..) {
                        let _ = db.record_thumbnail(id, size, &key);
                    }
                    last = Instant::now();
                }
                if disconnected {
                    break;
                }
            }
        })
        .expect("spawn db writer");
}

pub fn worker_count(configured: Option<usize>) -> usize {
    configured.unwrap_or_else(|| {
        let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2);
        // Leave a core for the UI/WebView; cap to avoid thrashing disks.
        n.saturating_sub(1).clamp(1, 6)
    })
}

pub fn spawn_workers(core: &Arc<AppCore>) {
    let n = worker_count(core.settings.read().worker_threads);
    for i in 0..n {
        let core = core.clone();
        std::thread::Builder::new()
            .name(format!("preview-{i}"))
            // Defense in depth: parsing/rendering recurse per nesting level. Depth is capped
            // by Limits::max_nesting_depth, but give workers generous headroom anyway.
            .stack_size(16 * 1024 * 1024)
            .spawn(move || {
                while let Some((id, flags, prio)) = core.queue.pop() {
                    let gen = core.generation();
                    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| process(&core, id, flags, gen)));
                    if r.is_err() {
                        tracing::error!("worker panicked processing asset {id}");
                        respond_waiters(&core, id, error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal error"));
                    }
                    if prio == P3 || flags.analyze {
                        after_background_job(&core, flags);
                    }
                }
            })
            .expect("spawn worker");
    }
    tracing::info!("started {n} preview workers");
}

/// Keep the "processing" status line moving and flip to idle when the queue drains.
fn after_background_job(core: &AppCore, flags: JobFlags) {
    if flags.analyze {
        core.processed.fetch_add(1, Ordering::Relaxed);
    }
    if core.scan_running.load(Ordering::SeqCst) {
        return;
    }
    let remaining = core.queue.len();
    let status = core.events.status();
    if status.phase == ScanPhase::Processing || status.phase == ScanPhase::Idle {
        core.events.update_status(|s| {
            if remaining == 0 {
                s.phase = ScanPhase::Idle;
                s.message = "Indexed".into();
            } else {
                s.phase = ScanPhase::Processing;
                s.processed = core.processed.load(Ordering::Relaxed) as usize;
                s.total = s.processed + remaining;
                s.message = format!("Processing {remaining} remaining");
            }
        });
    }
}

fn process(core: &AppCore, id: AssetId, flags: JobFlags, gen: u64) {
    let Some(mut rec) = core.record(id) else {
        respond_waiters(core, id, error_response(StatusCode::NOT_FOUND, "unknown asset"));
        return;
    };
    let Some(path) = core.absolute_path(&rec) else {
        respond_waiters(core, id, error_response(StatusCode::NOT_FOUND, "no library"));
        return;
    };
    let settings = core.settings();
    let limits = &settings.limits;
    let mut bytes: Option<Vec<u8>> = None;

    if rec.state == ProcessingState::Discovered && (flags.analyze || flags.thumb) {
        let analysis = if rec.file_size > limits.max_file_bytes {
            Some(oversize_analysis(rec.file_size, limits))
        } else {
            match std::fs::read(&path) {
                Ok(b) => {
                    let a = svg_core::svg::analyze(&b, limits);
                    bytes = Some(b);
                    Some(a)
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    // Deleted since discovery; the watcher/reconcile will remove it.
                    None
                }
                Err(e) => Some(read_error_analysis(rec.file_size, &e)),
            }
        };
        let Some(analysis) = analysis else {
            respond_waiters(core, id, error_response(StatusCode::NOT_FOUND, "file missing"));
            return;
        };
        if core.generation() != gen {
            return;
        }
        rec = apply_analysis(core, rec, analysis);
    }

    if !flags.thumb {
        return;
    }
    if !rec.state.renderable() || rec.state == ProcessingState::Discovered {
        respond_waiters(core, id, error_response(StatusCode::UNPROCESSABLE_ENTITY, rec.state.as_str()));
        return;
    }
    let size = settings.thumbnail_size;
    let tpath = cache::thumb_path(&core.paths.thumbs_dir, id, &rec.fast_fingerprint, size);
    if let Ok(png) = std::fs::read(&tpath) {
        respond_waiters(core, id, png_response(png));
        return;
    }
    let bytes = match bytes {
        Some(b) => b,
        None => match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                respond_waiters(core, id, error_response(StatusCode::NOT_FOUND, &e.to_string()));
                return;
            }
        },
    };
    match svg_core::svg::render_thumbnail(&bytes, size, limits, path.parent()) {
        Ok(png) => {
            if core.generation() != gen {
                respond_waiters(core, id, png_response(png));
                return;
            }
            if let Err(e) = cache::write_thumb(&tpath, &png) {
                tracing::warn!("writing thumbnail {}: {e}", tpath.display());
            } else {
                let _ = core.db_tx.send(DbWrite::Thumb(id, size, cache::cache_key(&rec.fast_fingerprint, size)));
            }
            let had_waiters = respond_waiters(core, id, png_response(png));
            if !had_waiters && core.queue.in_viewport(id) {
                core.events.thumbs_ready(&[id]);
            }
        }
        Err(e) => {
            tracing::debug!("thumbnail render failed for {}: {e}", rec.relative_path);
            respond_waiters(core, id, error_response(StatusCode::UNPROCESSABLE_ENTITY, &e.to_string()));
        }
    }
}

/// Fold an analysis into the catalog (immediately visible to search) and queue the DB write.
pub fn apply_analysis(core: &AppCore, mut rec: AssetRecord, analysis: Analysis) -> AssetRecord {
    rec.content_hash = if analysis.content_hash.is_empty() { None } else { Some(analysis.content_hash.clone()) };
    rec.width = analysis.meta.width;
    rec.height = analysis.meta.height;
    rec.view_box = analysis.meta.view_box;
    rec.element_count = Some(analysis.meta.element_count);
    rec.state = analysis.state;
    rec.parse_error = analysis.error.clone();
    {
        let mut cat = core.catalog.write();
        // Only update if the asset is still the same file (not replaced by a rescan meanwhile).
        let current = cat.get(rec.id).map(|r| r.fast_fingerprint.clone());
        if current.as_deref() == Some(rec.fast_fingerprint.as_str()) {
            cat.update_record(rec.clone());
            cat.set_text(rec.id, analysis.text.clone());
        }
    }
    let _ = core.db_tx.send(DbWrite::Analysis(rec.id, Box::new(analysis)));
    core.events.metadata_changed();
    rec
}

fn oversize_analysis(size: u64, limits: &Limits) -> Analysis {
    Analysis {
        state: ProcessingState::LimitExceeded,
        error: Some(format!(
            "File exceeds rendering limit ({} > {})",
            crate::util::human_bytes(size),
            crate::util::human_bytes(limits.max_file_bytes)
        )),
        content_hash: String::new(),
        meta: SvgMeta::default(),
        text: SearchText::default(),
        complexity: Complexity { file_size: size, ..Default::default() },
    }
}

fn read_error_analysis(size: u64, e: &std::io::Error) -> Analysis {
    Analysis {
        state: ProcessingState::ParseError,
        error: Some(format!("Cannot read file: {e}")),
        content_hash: String::new(),
        meta: SvgMeta::default(),
        text: SearchText::default(),
        complexity: Complexity { file_size: size, ..Default::default() },
    }
}

pub fn png_response(png: Vec<u8>) -> Response<Vec<u8>> {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "image/png")
        .header(header::CACHE_CONTROL, "max-age=600")
        .body(png)
        .unwrap()
}

pub fn error_response(status: StatusCode, msg: &str) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain")
        .header(header::CACHE_CONTROL, "no-store")
        .body(msg.as_bytes().to_vec())
        .unwrap()
}

/// Answer every waiting thumbnail request for `id`. Returns whether anyone was waiting.
pub fn respond_waiters(core: &AppCore, id: AssetId, resp: Response<Vec<u8>>) -> bool {
    let waiters = core.waiters.lock().remove(&id);
    match waiters {
        Some(list) if !list.is_empty() => {
            let (parts, body) = resp.into_parts();
            for r in list {
                let mut b = Response::builder().status(parts.status);
                for (k, v) in parts.headers.iter() {
                    b = b.header(k, v);
                }
                r.respond(b.body(body.clone()).unwrap());
            }
            true
        }
        _ => false,
    }
}

/// Fail every waiting request (library switch / shutdown).
pub fn fail_all_waiters(core: &AppCore) {
    let all: Vec<_> = core.waiters.lock().drain().collect();
    for (_, list) in all {
        for r in list {
            r.respond(error_response(StatusCode::GONE, "library changed"));
        }
    }
}
