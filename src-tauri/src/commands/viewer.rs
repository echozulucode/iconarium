use super::blocking;
use crate::app::AppCore;
use crate::error::{CmdError, CmdResult};
use serde::Deserialize;
use std::sync::Arc;
use svg_core::model::{AssetId, Region};
use svg_core::svg::renderer::{render_region_png, Background};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionSaveFormat {
    Svg,
    Png,
}

/// Save a region via the native save dialog. Never touches the source file.
#[tauri::command]
pub async fn save_region(
    app: AppHandle,
    core: State<'_, Arc<AppCore>>,
    id: AssetId,
    region: Region,
    format: RegionSaveFormat,
) -> CmdResult<Option<String>> {
    let core = core.inner().clone();
    blocking(move || {
        if !region.is_valid() {
            return Err(CmdError::new("invalid_input", "Select a region first"));
        }
        let (rec, path) = core.locate(id)?;
        let bytes = core.read_asset_bytes(&rec, &path)?;
        let (ext, label, data) = match format {
            RegionSaveFormat::Svg => (
                "svg",
                "SVG image",
                svg_core::svg::crop::crop_svg(&bytes, region)?.into_bytes(),
            ),
            RegionSaveFormat::Png => {
                let limits = core.settings.read().limits.clone();
                (
                    "png",
                    "PNG image",
                    render_region_png(
                        &bytes,
                        region,
                        1.0,
                        Background::Transparent,
                        &limits,
                        path.parent(),
                    )?
                    .png,
                )
            }
        };
        let default_name = format!("{}-crop.{ext}", crate::util::file_stem(&rec.filename));
        let mut dialog = app
            .dialog()
            .file()
            .set_title("Save Selection")
            .set_file_name(&default_name)
            .add_filter(label, &[ext]);
        if let Some(dir) = path.parent() {
            dialog = dialog.set_directory(dir);
        }
        let Some(target) = dialog.blocking_save_file() else {
            return Ok(None);
        };
        let target = target
            .into_path()
            .map_err(|e| CmdError::new("invalid_input", e.to_string()))?;
        let same = if cfg!(windows) {
            target
                .to_string_lossy()
                .eq_ignore_ascii_case(&path.to_string_lossy())
        } else {
            target == path
        };
        if same {
            return Err(CmdError::new(
                "invalid_input",
                "Refusing to overwrite the source SVG",
            ));
        }
        std::fs::write(&target, data)?;
        Ok(Some(target.to_string_lossy().into_owned()))
    })
    .await
}

#[tauri::command]
pub async fn reveal_assets(
    app: AppHandle,
    core: State<'_, Arc<AppCore>>,
    ids: Vec<AssetId>,
) -> CmdResult<()> {
    let core = core.inner().clone();
    blocking(move || {
        let mut paths = Vec::new();
        for id in ids {
            paths.push(core.locate(id)?.1);
        }
        if paths.is_empty() {
            return Ok(());
        }
        app.opener()
            .reveal_items_in_dir(paths)
            .map_err(|e| CmdError::new("opener", e.to_string()))
    })
    .await
}

#[tauri::command]
pub async fn open_external(
    app: AppHandle,
    core: State<'_, Arc<AppCore>>,
    id: AssetId,
) -> CmdResult<()> {
    let core = core.inner().clone();
    blocking(move || {
        let (_, path) = core.locate(id)?;
        app.opener()
            .open_path(path.to_string_lossy(), None::<&str>)
            .map_err(|e| CmdError::new("opener", e.to_string()))
    })
    .await
}
