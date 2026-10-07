use super::blocking;
use crate::app::AppCore;
use crate::error::{CmdError, CmdResult};
use crate::native::drag::start_file_drag;
use crate::preview::cache;
use std::path::PathBuf;
use std::sync::Arc;
use svg_core::model::{AssetId, Region};
use svg_core::svg::renderer::{render_region_png, render_thumbnail, Background};
use tauri::{AppHandle, State, Window};

/// Small preview image for the drag cursor: the cached thumbnail when available.
fn drag_preview(core: &AppCore, id: AssetId) -> Vec<u8> {
    let Some(rec) = core.record(id) else { return Vec::new() };
    let size = core.settings.read().thumbnail_size;
    if let Ok(png) = std::fs::read(cache::thumb_path(&core.paths.thumbs_dir, id, &rec.fast_fingerprint, size)) {
        return png;
    }
    let Some(path) = core.absolute_path(&rec) else { return Vec::new() };
    let limits = core.settings.read().limits.clone();
    core.read_asset_bytes(&rec, &path)
        .ok()
        .and_then(|b| render_thumbnail(&b, 128, &limits, path.parent()).ok())
        .unwrap_or_default()
}

#[tauri::command]
pub async fn start_drag_assets(app: AppHandle, window: Window, core: State<'_, Arc<AppCore>>, ids: Vec<AssetId>) -> CmdResult<()> {
    let core = core.inner().clone();
    let (files, preview) = blocking({
        let core = core.clone();
        move || {
            let mut files: Vec<PathBuf> = Vec::new();
            for id in &ids {
                let (_, p) = core.locate(*id)?;
                if p.exists() {
                    files.push(p);
                }
            }
            let preview = ids.first().map(|id| drag_preview(&core, *id)).unwrap_or_default();
            Ok((files, preview))
        }
    })
    .await?;
    blocking(move || start_file_drag(&app, window, files, preview).map_err(|e| CmdError::new("drag", e))).await
}

#[tauri::command]
pub async fn start_drag_region(
    app: AppHandle,
    window: Window,
    core: State<'_, Arc<AppCore>>,
    id: AssetId,
    region: Region,
) -> CmdResult<()> {
    let core = core.inner().clone();
    let (file, preview) = blocking(move || {
        if !region.is_valid() {
            return Err(CmdError::new("invalid_input", "Select a region first"));
        }
        let (rec, path) = core.locate(id)?;
        let bytes = core.read_asset_bytes(&rec, &path)?;
        let cropped = svg_core::svg::crop::crop_svg(&bytes, region)?;
        let name = format!("{}-crop.svg", crate::util::file_stem(&rec.filename));
        let file = core.temp.write(&name, cropped.as_bytes())?;
        let limits = core.settings.read().limits.clone();
        let longest = region.width.max(region.height);
        let scale = if longest > 0.0 { (160.0 / longest).min(4.0) } else { 1.0 };
        let preview = render_region_png(&bytes, region, scale, Background::Transparent, &limits, path.parent())
            .map(|p| p.png)
            .unwrap_or_default();
        Ok((file, preview))
    })
    .await?;
    blocking(move || start_file_drag(&app, window, vec![file], preview).map_err(|e| CmdError::new("drag", e))).await
}
