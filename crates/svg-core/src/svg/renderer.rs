//! Rasterization with resvg/usvg/tiny-skia using a single process-wide font database.
//!
//! Every entry point applies the complexity limits before rendering (file size, XML
//! node count, embedded raster bytes) and converts panics inside the renderer into
//! [`CoreError::Render`].

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::sync::{Arc, OnceLock};

use resvg::tiny_skia::{self, Pixmap, Transform};
use resvg::usvg::{self, fontdb};

use super::crop::crop_svg;
use super::limits::{check_complexity, check_file_size, collect_complexity};
use super::parser::{
    decode_text, needs_svg_xmlns, parse_xml, rewrite_root, root_layout, root_svg, ParseFailure,
    SVG_NS,
};
use crate::config::Limits;
use crate::error::{CoreError, Result};
use crate::model::Region;

/// Default font size (CSS px) for text without `font-size`; matches browsers (16px)
/// so rasterized output agrees with the WebView viewer.
pub const DEFAULT_FONT_SIZE: f32 = 16.0;

static FONTDB: OnceLock<Arc<fontdb::Database>> = OnceLock::new();

/// The shared font database (system fonts, loaded once on first use).
pub fn shared_fontdb() -> Arc<fontdb::Database> {
    FONTDB
        .get_or_init(|| {
            let started = std::time::Instant::now();
            let mut db = fontdb::Database::new();
            db.load_system_fonts();
            tracing::info!(
                faces = db.len(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "font database loaded"
            );
            Arc::new(db)
        })
        .clone()
}

/// Load the system font database now. Call once at startup on a background thread so
/// the first thumbnail does not pay the font-scan cost.
pub fn warm_up_fonts() {
    let _ = shared_fontdb();
}

/// Background for PNG output. Transparent unless explicitly requested (plan §2.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Background {
    #[default]
    Transparent,
    White,
}

/// A rendered PNG and its pixel dimensions.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedPng {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// usvg options used everywhere: shared fonts, browser-like default font size.
pub fn usvg_options(resources_dir: Option<&Path>) -> usvg::Options<'static> {
    usvg::Options {
        resources_dir: resources_dir.map(Path::to_path_buf),
        fontdb: shared_fontdb(),
        font_size: DEFAULT_FONT_SIZE,
        ..Default::default()
    }
}

/// Canvas size usvg would use for this document (CSS px). Used as the metadata
/// fallback when neither a viewBox nor absolute width+height are present.
pub fn usvg_canvas_size(doc: &roxmltree::Document<'_>) -> Option<(f64, f64)> {
    let opts = usvg_options(None);
    let tree = catch_unwind(AssertUnwindSafe(|| usvg::Tree::from_xmltree(doc, &opts)))
        .ok()?
        .ok()?;
    let s = tree.size();
    let (w, h) = (s.width() as f64, s.height() as f64);
    (w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0).then_some((w, h))
}

fn failure_to_error(f: ParseFailure) -> CoreError {
    match f {
        ParseFailure::Limit(m) => CoreError::Limit(m),
        ParseFailure::Malformed(m) => CoreError::Parse(m),
    }
}

/// Apply limits and build a usvg tree from document text.
fn tree_from_text(text: &str, limits: &Limits, resources_dir: Option<&Path>) -> Result<usvg::Tree> {
    let doc = parse_xml(text, limits.max_nodes).map_err(failure_to_error)?;
    let root = root_svg(&doc).map_err(CoreError::Parse)?;
    let complexity = collect_complexity(&doc, text.len() as u64);
    check_complexity(&complexity, limits).map_err(CoreError::Limit)?;

    let opts = usvg_options(resources_dir);
    let build = |doc: &roxmltree::Document<'_>| {
        catch_unwind(AssertUnwindSafe(|| usvg::Tree::from_xmltree(doc, &opts)))
            .map_err(|_| CoreError::Render("Renderer failed on this document".into()))?
            .map_err(|e| CoreError::Render(format!("Cannot render SVG: {e}")))
    };
    if needs_svg_xmlns(root) {
        // Lenient: treat a no-namespace <svg> as SVG (browsers would not).
        let layout = root_layout(text, root)
            .ok_or_else(|| CoreError::Parse("Cannot locate root element".into()))?;
        let fixed = rewrite_root(text, &layout, &[], &[("xmlns".into(), SVG_NS.into())]);
        let doc2 = parse_xml(&fixed, limits.max_nodes).map_err(failure_to_error)?;
        build(&doc2)
    } else {
        build(&doc)
    }
}

/// Size check + decode + [`tree_from_text`].
fn tree_from_bytes(
    bytes: &[u8],
    limits: &Limits,
    resources_dir: Option<&Path>,
) -> Result<usvg::Tree> {
    check_file_size(bytes.len() as u64, limits).map_err(CoreError::Limit)?;
    if bytes.is_empty() {
        return Err(CoreError::Parse("Empty file".into()));
    }
    let decoded = decode_text(bytes).map_err(CoreError::Parse)?;
    tree_from_text(&decoded.text, limits, resources_dir)
}

fn rasterize(
    tree: &usvg::Tree,
    width: u32,
    height: u32,
    transform: Transform,
    bg: Background,
) -> Result<Vec<u8>> {
    let mut pixmap = Pixmap::new(width, height)
        .ok_or_else(|| CoreError::Render(format!("Cannot allocate {width}×{height} image")))?;
    if bg == Background::White {
        pixmap.fill(tiny_skia::Color::WHITE);
    }
    catch_unwind(AssertUnwindSafe(|| {
        resvg::render(tree, transform, &mut pixmap.as_mut())
    }))
    .map_err(|_| CoreError::Render("Renderer failed on this document".into()))?;
    pixmap
        .encode_png()
        .map_err(|e| CoreError::Render(format!("PNG encoding failed: {e}")))
}

fn tree_size(tree: &usvg::Tree) -> Result<(f64, f64)> {
    let s = tree.size();
    let (w, h) = (s.width() as f64, s.height() as f64);
    if w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0 {
        Ok((w, h))
    } else {
        Err(CoreError::Render("Document has no usable size".into()))
    }
}

/// Pixel dimensions for `w×h × scale`, reducing scale so `pw*ph ≤ max_pixels`.
/// Returns (pw, ph, effective scale).
fn fit_scaled(w: f64, h: f64, scale: f64, max_pixels: u64) -> (u32, u32, f64) {
    let max_pixels = max_pixels.max(1) as f64;
    let dims = |s: f64| {
        let pw = ((w * s) - 1e-6).ceil().clamp(1.0, u32::MAX as f64);
        let ph = ((h * s) - 1e-6).ceil().clamp(1.0, u32::MAX as f64);
        (pw, ph)
    };
    let mut s = scale;
    for _ in 0..16 {
        let (pw, ph) = dims(s);
        if pw * ph <= max_pixels {
            return (pw as u32, ph as u32, s);
        }
        s *= (max_pixels / (pw * ph)).sqrt() * 0.999;
    }
    (1, 1, s)
}

fn validate_scale(scale: f64) -> Result<()> {
    if scale.is_finite() && scale > 0.0 {
        Ok(())
    } else {
        Err(CoreError::Invalid(format!("Invalid render scale {scale}")))
    }
}

fn render_tree_scaled(
    tree: &usvg::Tree,
    scale: f64,
    background: Background,
    limits: &Limits,
) -> Result<RenderedPng> {
    validate_scale(scale)?;
    let (w, h) = tree_size(tree)?;
    let (pw, ph, s) = fit_scaled(w, h, scale, limits.max_render_pixels);
    let png = rasterize(
        tree,
        pw,
        ph,
        Transform::from_scale(s as f32, s as f32),
        background,
    )?;
    Ok(RenderedPng {
        png,
        width: pw,
        height: ph,
    })
}

/// Render a transparent PNG thumbnail whose dimensions fit inside `size`×`size`
/// with the document's aspect ratio preserved (the PNG is the fitted size, not padded).
pub fn render_thumbnail(
    bytes: &[u8],
    size: u32,
    limits: &Limits,
    resources_dir: Option<&Path>,
) -> Result<Vec<u8>> {
    if size == 0 {
        return Err(CoreError::Invalid("Thumbnail size must be > 0".into()));
    }
    let tree = tree_from_bytes(bytes, limits, resources_dir)?;
    let (w, h) = tree_size(&tree)?;
    let f = (size as f64 / w).min(size as f64 / h);
    let pw = (w * f).round().clamp(1.0, size as f64) as u32;
    let ph = (h * f).round().clamp(1.0, size as f64) as u32;
    let t = Transform::from_scale((pw as f64 / w) as f32, (ph as f64 / h) as f32);
    rasterize(&tree, pw, ph, t, Background::Transparent)
}

/// Render the full document at its CSS size × `scale`. The scale is reduced if needed
/// so the output has at most `limits.max_render_pixels` pixels.
pub fn render_png(
    bytes: &[u8],
    scale: f64,
    background: Background,
    limits: &Limits,
    resources_dir: Option<&Path>,
) -> Result<RenderedPng> {
    validate_scale(scale)?;
    let tree = tree_from_bytes(bytes, limits, resources_dir)?;
    render_tree_scaled(&tree, scale, background, limits)
}

/// Render a region (SVG user units, same space as `SvgMeta::doc_box`). Renders the
/// output of [`crop_svg`], so PNG and SVG region exports are pixel-consistent.
pub fn render_region_png(
    bytes: &[u8],
    region: Region,
    scale: f64,
    background: Background,
    limits: &Limits,
    resources_dir: Option<&Path>,
) -> Result<RenderedPng> {
    validate_scale(scale)?;
    check_file_size(bytes.len() as u64, limits).map_err(CoreError::Limit)?;
    let cropped = crop_svg(bytes, region)?;
    // The wrapper adds a handful of nodes; don't let that tip a document over the limit.
    let relaxed = Limits {
        max_nodes: limits.max_nodes.saturating_add(16),
        ..limits.clone()
    };
    let tree = tree_from_text(&cropped, &relaxed, resources_dir)?;
    render_tree_scaled(&tree, scale, background, limits)
}

/// Parse already-limit-checked SVG text into a usvg tree (shared fonts). Exposed for
/// tests and diagnostics.
pub fn parse_tree(text: &str, limits: &Limits, resources_dir: Option<&Path>) -> Result<usvg::Tree> {
    tree_from_text(text, limits, resources_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_scaled_clamps() {
        assert_eq!(fit_scaled(100.0, 50.0, 2.0, u64::MAX), (200, 100, 2.0));
        let (pw, ph, s) = fit_scaled(1000.0, 1000.0, 1.0, 10_000);
        assert!(pw as u64 * ph as u64 <= 10_000);
        assert!(s < 0.11 && pw >= 99);
        assert_eq!(fit_scaled(10.4, 3.0, 1.0, u64::MAX).0, 11);
    }
}
