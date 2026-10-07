//! Clipboard publishing (plan §2.1, §2.4, §20).
//!
//! Formats placed for an SVG copy, in order:
//! 1. bitmap (CF_DIB/CF_DIBV5) — optional fallback for raster-only receivers (Paint).
//!    clipboard-rs clears the clipboard when setting an image, so it must go first.
//! 2. `PNG` (registered format) — alpha-preserving raster used by Office/browsers.
//! 3. `image/svg+xml` (registered format) — vector content for PowerPoint/Word/Visio.
//! 4. text/plain SVG markup — optional (some receivers prefer text over graphics).
//!
//! The choices are settings so they can be tuned after Office validation (docs/windows-validation.md).

use clipboard_rs::common::RustImage;
use clipboard_rs::{Clipboard, ClipboardContent, ClipboardContext, RustImageData};

pub const FORMAT_SVG: &str = "image/svg+xml";
pub const FORMAT_PNG: &str = "PNG";

#[derive(Debug, Clone, Copy)]
pub struct SvgCopyOptions {
    pub include_bitmap: bool,
    pub include_text: bool,
}

fn ctx() -> Result<ClipboardContext, String> {
    ClipboardContext::new().map_err(|e| format!("Clipboard unavailable: {e}"))
}

fn image_content(png: &[u8]) -> Option<ClipboardContent> {
    RustImageData::from_bytes(png).ok().map(ClipboardContent::Image)
}

pub fn copy_svg(svg: &[u8], png_fallback: Option<&[u8]>, opts: SvgCopyOptions) -> Result<(), String> {
    let mut contents = Vec::new();
    if let Some(png) = png_fallback {
        if opts.include_bitmap {
            if let Some(img) = image_content(png) {
                contents.push(img);
            }
        }
        contents.push(ClipboardContent::Other(FORMAT_PNG.into(), png.to_vec()));
    }
    contents.push(ClipboardContent::Other(FORMAT_SVG.into(), svg.to_vec()));
    if opts.include_text {
        contents.push(ClipboardContent::Text(String::from_utf8_lossy(svg).into_owned()));
    }
    ctx()?.set(contents).map_err(|e| format!("Copy failed: {e}"))
}

pub fn copy_png(png: &[u8]) -> Result<(), String> {
    let mut contents = Vec::new();
    if let Some(img) = image_content(png) {
        contents.push(img);
    }
    contents.push(ClipboardContent::Other(FORMAT_PNG.into(), png.to_vec()));
    ctx()?.set(contents).map_err(|e| format!("Copy failed: {e}"))
}

pub fn copy_text(text: &str) -> Result<(), String> {
    ctx()?.set_text(text.to_string()).map_err(|e| format!("Copy failed: {e}"))
}

/// Multiple files: CF_HDROP file list (paste into Explorer copies the files) + the paths as text.
pub fn copy_files(paths: &[String]) -> Result<(), String> {
    let text = paths.join(crate::native::clipboard::NEWLINE);
    ctx()?
        .set(vec![ClipboardContent::Files(paths.to_vec()), ClipboardContent::Text(text)])
        .map_err(|e| format!("Copy failed: {e}"))
}

#[cfg(windows)]
pub const NEWLINE: &str = "\r\n";
#[cfg(not(windows))]
pub const NEWLINE: &str = "\n";
