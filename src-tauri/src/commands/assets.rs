use crate::app::AppCore;
use crate::error::{CmdError, CmdResult};
use serde::Serialize;
use std::sync::Arc;
use svg_core::model::{AssetId, AssetSummary, ProcessingState, ViewBox};
use tauri::ipc::Response;
use tauri::State;

/// Ranked asset IDs as little-endian u32 bytes (binary IPC: 50k results ≈ 200 KB, no JSON).
#[tauri::command]
pub async fn search(core: State<'_, Arc<AppCore>>, query: String) -> CmdResult<Response> {
    let core = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let ids = core.catalog.read().search(&query, None)?;
        if !query.trim().is_empty() {
            core.queue.promote_results(&ids);
        }
        let mut bytes = Vec::with_capacity(ids.len() * 4);
        for id in ids {
            bytes.extend_from_slice(&id.to_le_bytes());
        }
        Ok(Response::new(bytes))
    })
    .await
    .map_err(|e| CmdError::new("error", e.to_string()))?
}

#[tauri::command]
pub async fn get_assets(core: State<'_, Arc<AppCore>>, ids: Vec<AssetId>, query: Option<String>) -> CmdResult<Vec<AssetSummary>> {
    let q = query.as_deref().filter(|q| !q.trim().is_empty());
    Ok(core.catalog.read().summaries(&ids, q))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetDetailDto {
    id: AssetId,
    filename: String,
    relative_path: String,
    absolute_path: String,
    file_size: u64,
    mtime_ms: i64,
    state: ProcessingState,
    parse_error: Option<String>,
    width: Option<f64>,
    height: Option<f64>,
    view_box: Option<ViewBox>,
    doc_box: Option<ViewBox>,
    element_count: Option<u32>,
    content_hash: Option<String>,
    fingerprint: String,
    title: String,
    description: String,
    size_warning: bool,
}

#[tauri::command]
pub async fn get_asset_detail(core: State<'_, Arc<AppCore>>, id: AssetId) -> CmdResult<AssetDetailDto> {
    let core = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (rec, path) = core.locate(id)?;
        // Analyze inline if needed so the viewer always gets a coordinate system.
        let (rec, meta) = match core.svg_meta(id) {
            Ok(v) => v,
            Err(e) if e.kind == "limit_exceeded" => (rec, Default::default()),
            Err(e) => return Err(e),
        };
        let text = core.catalog.read().get_text(id).cloned().unwrap_or_default();
        let warn = core.settings.read().limits.warn_file_bytes;
        Ok(AssetDetailDto {
            id,
            filename: rec.filename.clone(),
            relative_path: rec.relative_path.clone(),
            absolute_path: path.to_string_lossy().into_owned(),
            file_size: rec.file_size,
            mtime_ms: rec.mtime_ns / 1_000_000,
            state: rec.state,
            parse_error: rec.parse_error.clone(),
            width: rec.width,
            height: rec.height,
            view_box: rec.view_box,
            doc_box: meta.doc_box,
            element_count: rec.element_count,
            content_hash: rec.content_hash.clone(),
            fingerprint: rec.fast_fingerprint.clone(),
            title: text.title,
            description: text.description,
            size_warning: rec.file_size >= warn,
        })
    })
    .await
    .map_err(|e| CmdError::new("error", e.to_string()))?
}

#[tauri::command]
pub async fn set_viewport(core: State<'_, Arc<AppCore>>, visible: Vec<AssetId>, nearby: Vec<AssetId>) -> CmdResult<()> {
    let waiting: std::collections::HashSet<AssetId> = core.waiters.lock().keys().copied().collect();
    core.queue.set_viewport(&visible, &nearby, &waiting);
    Ok(())
}
