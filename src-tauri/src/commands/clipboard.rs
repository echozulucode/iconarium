use super::blocking;
use crate::app::AppCore;
use crate::error::{CmdError, CmdResult};
use crate::native::clipboard::{self as clip, SvgCopyOptions};
use serde::Deserialize;
use std::sync::Arc;
use svg_core::model::{AssetId, Region};
use svg_core::svg::renderer::{render_png, render_region_png, Background};
use tauri::State;

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetCopyFormat {
    Svg,
    Png,
    PngWhite,
    Path,
    Filename,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionCopyFormat {
    Svg,
    Png,
    Png2x,
    PngWhite,
}

fn clip_err(e: String) -> CmdError {
    CmdError::new("clipboard", e)
}

/// Intrinsic size used for PNG scaling: width/height, else the viewBox size
/// (the common `viewBox="0 0 24 24"` icon form has no width/height).
fn intrinsic(rec: &svg_core::model::AssetRecord) -> (Option<f64>, Option<f64>) {
    match (rec.width, rec.height, rec.view_box) {
        (None, None, Some(vb)) => (Some(vb.width), Some(vb.height)),
        (w, h, _) => (w, h),
    }
}

/// Scale for whole-asset PNG copies: 1× for normal documents, upscaled so tiny icons
/// (e.g. 24 px) still paste at a usable size (longest side ≥ 256 px, at most 8×).
pub fn asset_png_scale(width: Option<f64>, height: Option<f64>) -> f64 {
    let longest = width.unwrap_or(0.0).max(height.unwrap_or(0.0));
    if longest > 0.0 && longest < 256.0 {
        (256.0 / longest).min(8.0)
    } else {
        1.0
    }
}

#[tauri::command]
pub async fn copy_assets(
    core: State<'_, Arc<AppCore>>,
    ids: Vec<AssetId>,
    format: AssetCopyFormat,
) -> CmdResult<()> {
    let core = core.inner().clone();
    blocking(move || {
        if ids.is_empty() {
            return Ok(());
        }
        let mut located = Vec::with_capacity(ids.len());
        for id in &ids {
            located.push(core.locate(*id)?);
        }
        let paths: Vec<String> = located
            .iter()
            .map(|(_, p)| p.to_string_lossy().into_owned())
            .collect();
        match format {
            AssetCopyFormat::Path => clip::copy_text(&paths.join(clip::NEWLINE)).map_err(clip_err),
            AssetCopyFormat::Filename => {
                let names: Vec<&str> = located.iter().map(|(r, _)| r.filename.as_str()).collect();
                clip::copy_text(&names.join(clip::NEWLINE)).map_err(clip_err)
            }
            AssetCopyFormat::Svg if located.len() > 1 => clip::copy_files(&paths).map_err(clip_err),
            AssetCopyFormat::Svg => {
                let (rec, path) = &located[0];
                let bytes = core.read_asset_bytes(rec, path)?;
                let settings = core.settings();
                let scale = {
                    let (w, h) = intrinsic(rec);
                    asset_png_scale(w, h)
                } * settings.clipboard_png_fallback_scale;
                let png = render_png(
                    &bytes,
                    scale,
                    Background::Transparent,
                    &settings.limits,
                    path.parent(),
                )
                .ok();
                clip::copy_svg(
                    &bytes,
                    png.as_ref().map(|p| p.png.as_slice()),
                    SvgCopyOptions {
                        include_bitmap: settings.clipboard_include_bitmap_with_svg,
                        include_text: settings.clipboard_include_svg_text,
                    },
                )
                .map_err(clip_err)
            }
            AssetCopyFormat::Png | AssetCopyFormat::PngWhite => {
                let (rec, path) = &located[0];
                let bytes = core.read_asset_bytes(rec, path)?;
                let limits = core.settings.read().limits.clone();
                let bg = if matches!(format, AssetCopyFormat::PngWhite) {
                    Background::White
                } else {
                    Background::Transparent
                };
                let png = render_png(
                    &bytes,
                    {
                        let (w, h) = intrinsic(rec);
                        asset_png_scale(w, h)
                    },
                    bg,
                    &limits,
                    path.parent(),
                )?;
                clip::copy_png(&png.png).map_err(clip_err)
            }
        }
    })
    .await
}

#[tauri::command]
pub async fn copy_region(
    core: State<'_, Arc<AppCore>>,
    id: AssetId,
    region: Region,
    format: RegionCopyFormat,
) -> CmdResult<()> {
    let core = core.inner().clone();
    blocking(move || {
        if !region.is_valid() {
            return Err(CmdError::new("invalid_input", "Select a region first"));
        }
        let (rec, path) = core.locate(id)?;
        let bytes = core.read_asset_bytes(&rec, &path)?;
        let settings = core.settings();
        let limits = &settings.limits;
        match format {
            RegionCopyFormat::Svg => {
                let cropped = svg_core::svg::crop::crop_svg(&bytes, region)?;
                let png = render_region_png(
                    &bytes,
                    region,
                    settings.clipboard_png_fallback_scale,
                    Background::Transparent,
                    limits,
                    path.parent(),
                )
                .ok();
                clip::copy_svg(
                    cropped.as_bytes(),
                    png.as_ref().map(|p| p.png.as_slice()),
                    SvgCopyOptions {
                        include_bitmap: settings.clipboard_include_bitmap_with_svg,
                        include_text: settings.clipboard_include_svg_text,
                    },
                )
                .map_err(clip_err)
            }
            RegionCopyFormat::Png | RegionCopyFormat::Png2x | RegionCopyFormat::PngWhite => {
                let scale = if matches!(format, RegionCopyFormat::Png2x) {
                    2.0
                } else {
                    1.0
                };
                let bg = if matches!(format, RegionCopyFormat::PngWhite) {
                    Background::White
                } else {
                    Background::Transparent
                };
                let png = render_region_png(&bytes, region, scale, bg, limits, path.parent())?;
                clip::copy_png(&png.png).map_err(clip_err)
            }
        }
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn icon_upscale() {
        assert_eq!(asset_png_scale(Some(24.0), Some(24.0)), 8.0);
        assert_eq!(asset_png_scale(Some(128.0), Some(64.0)), 2.0);
        assert_eq!(asset_png_scale(Some(1200.0), Some(800.0)), 1.0);
        assert_eq!(asset_png_scale(None, None), 1.0);
    }
}
