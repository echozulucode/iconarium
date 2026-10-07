//! Clipboard publishing (plan §2.1, §2.4, §20).
//!
//! On Windows the clipboard is opened exactly once per copy and every format is written
//! inside that single open/empty/close cycle, most descriptive format first:
//! 1. `image/svg+xml` (registered) — vector content for PowerPoint/Word/Visio/browsers.
//! 2. `PNG` (registered) — alpha-preserving raster used by Office and browsers.
//! 3. `CF_BITMAP` — optional raster fallback for legacy receivers (Paint); no alpha,
//!    composited onto white.
//! 4. `CF_UNICODETEXT` SVG markup — optional (some receivers prefer text over graphics).
//!
//! The optional formats are settings so they can be tuned after Office validation
//! (docs/windows-validation.md). Other platforms (used for development and E2E tests)
//! go through clipboard-rs.

pub const FORMAT_SVG: &str = "image/svg+xml";
pub const FORMAT_PNG: &str = "PNG";

#[cfg(windows)]
pub const NEWLINE: &str = "\r\n";
#[cfg(not(windows))]
pub const NEWLINE: &str = "\n";

#[derive(Debug, Clone, Copy)]
pub struct SvgCopyOptions {
    pub include_bitmap: bool,
    pub include_text: bool,
}

/// One clipboard entry, in placement order.
pub enum Item<'a> {
    /// Data under a registered (named) clipboard format.
    Named(&'static str, &'a [u8]),
    /// Legacy bitmap from PNG bytes (converted to BMP / DIB as the platform requires).
    Bitmap(&'a [u8]),
    Text(&'a str),
    Files(&'a [String]),
}

pub fn copy_svg(
    svg: &[u8],
    png_fallback: Option<&[u8]>,
    opts: SvgCopyOptions,
) -> Result<(), String> {
    let text = if opts.include_text {
        Some(String::from_utf8_lossy(svg).into_owned())
    } else {
        None
    };
    let mut items = vec![Item::Named(FORMAT_SVG, svg)];
    if let Some(png) = png_fallback {
        items.push(Item::Named(FORMAT_PNG, png));
        if opts.include_bitmap {
            items.push(Item::Bitmap(png));
        }
    }
    if let Some(t) = text.as_deref() {
        items.push(Item::Text(t));
    }
    write(&items)
}

pub fn copy_png(png: &[u8]) -> Result<(), String> {
    write(&[Item::Named(FORMAT_PNG, png), Item::Bitmap(png)])
}

pub fn copy_text(text: &str) -> Result<(), String> {
    write(&[Item::Text(text)])
}

/// Multiple files: file list (paste into Explorer copies the files) + the paths as text.
pub fn copy_files(paths: &[String]) -> Result<(), String> {
    let text = paths.join(NEWLINE);
    write(&[Item::Files(paths), Item::Text(&text)])
}

#[cfg(windows)]
pub fn write(items: &[Item<'_>]) -> Result<(), String> {
    use clipboard_win::{options::NoClear, raw, register_format, Clipboard};

    let _guard = Clipboard::new_attempts(10).map_err(|e| format!("Clipboard is busy: {e}"))?;
    raw::empty().map_err(|e| format!("Cannot clear clipboard: {e}"))?;
    for (i, item) in items.iter().enumerate() {
        let required = i == 0;
        let res: Result<(), String> = match item {
            Item::Named(name, data) => match register_format(name) {
                Some(fmt) => raw::set_without_clear(fmt.get(), data).map_err(|e| e.to_string()),
                None => Err(format!("cannot register clipboard format {name}")),
            },
            Item::Bitmap(png) => svg_core::svg::renderer::png_to_bmp(png)
                .map_err(|e| e.to_string())
                .and_then(|bmp| raw::set_bitmap_with(&bmp, NoClear).map_err(|e| e.to_string())),
            Item::Text(t) => raw::set_string_with(t, NoClear).map_err(|e| e.to_string()),
            Item::Files(paths) => {
                raw::set_file_list_with(&paths[..], NoClear).map_err(|e| e.to_string())
            }
        };
        if let Err(e) = res {
            if required {
                return Err(format!("Copy failed: {e}"));
            }
            tracing::warn!("optional clipboard format {i} failed: {e}");
        }
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn write(items: &[Item<'_>]) -> Result<(), String> {
    use clipboard_rs::common::RustImage;
    use clipboard_rs::{Clipboard, ClipboardContent, ClipboardContext, RustImageData};

    // clipboard-rs clears the clipboard when it sets an image, so the image goes first.
    let mut contents = Vec::new();
    for item in items {
        if let Item::Bitmap(png) = item {
            if let Ok(img) = RustImageData::from_bytes(png) {
                contents.push(ClipboardContent::Image(img));
            }
        }
    }
    for item in items {
        match item {
            Item::Named(name, data) => {
                contents.push(ClipboardContent::Other((*name).to_string(), data.to_vec()))
            }
            Item::Text(t) => contents.push(ClipboardContent::Text((*t).to_string())),
            Item::Files(paths) => contents.push(ClipboardContent::Files(paths.to_vec())),
            Item::Bitmap(_) => {}
        }
    }
    let ctx = ClipboardContext::new().map_err(|e| format!("Clipboard unavailable: {e}"))?;
    ctx.set(contents).map_err(|e| format!("Copy failed: {e}"))
}
