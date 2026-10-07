//! Geometry metadata: width/height in CSS px, viewBox, the effective document box
//! (the coordinate system for regions) and element count.
//!
//! `doc_box` resolution order:
//! 1. the root `viewBox`;
//! 2. `0 0 width height` when both are absolute lengths;
//! 3. otherwise what the renderer (usvg) uses as the canvas: with no viewBox and a
//!    missing / percentage width or height, usvg sizes the canvas to the content
//!    bounding box's right/bottom edge in user units (1 user unit = 1 px). We parse with
//!    usvg (shared font database, so text extents count) only in this case, so
//!    thumbnails, viewer and crops agree on the same coordinate system;
//! 4. finally `0 0 100 100` (usvg's default size) if usvg cannot produce a size.

use roxmltree::{Document, Node};

use super::parser::{parse_positive_length, parse_view_box};
use super::renderer;
use crate::model::{SvgMeta, ViewBox};

/// Compute geometry metadata for a parsed document whose root is `<svg>`.
pub fn extract_meta(doc: &Document<'_>, root: Node<'_, '_>) -> SvgMeta {
    extract_meta_with(doc, root, true)
}

/// Like [`extract_meta`]; with `render_fallback == false` the (expensive) usvg
/// fallback for `doc_box` is skipped and `doc_box` is `None` when it would be needed.
/// Used for documents that exceeded a limit and will not be rendered.
pub fn extract_meta_with(doc: &Document<'_>, root: Node<'_, '_>, render_fallback: bool) -> SvgMeta {
    let width = root.attribute("width").and_then(parse_positive_length);
    let height = root.attribute("height").and_then(parse_positive_length);
    let view_box = root.attribute("viewBox").and_then(parse_view_box);
    let element_count = doc
        .descendants()
        .filter(|n| n.is_element())
        .count()
        .min(u32::MAX as usize) as u32;

    let doc_box = match (view_box, width, height) {
        (Some(vb), _, _) => Some(vb),
        (None, Some(w), Some(h)) => Some(ViewBox {
            min_x: 0.0,
            min_y: 0.0,
            width: w,
            height: h,
        }),
        _ if render_fallback => Some(
            renderer::usvg_canvas_size(doc)
                .map(|(w, h)| ViewBox {
                    min_x: 0.0,
                    min_y: 0.0,
                    width: w,
                    height: h,
                })
                .unwrap_or(ViewBox {
                    min_x: 0.0,
                    min_y: 0.0,
                    width: 100.0,
                    height: 100.0,
                }),
        ),
        _ => None,
    };

    SvgMeta {
        width,
        height,
        view_box,
        doc_box,
        element_count,
    }
}

/// Physical CSS px per user unit along x and y for a root with absolute size and a
/// viewBox. `None` when the document has no meaningful physical scale (missing or
/// relative width/height, or no viewBox), in which case 1 user unit = 1 px.
///
/// Follows `preserveAspectRatio`: `none` gives independent x/y scales, `slice` the
/// larger, otherwise (`meet`, default) the smaller uniform scale. When only one of
/// width/height is absolute, the other follows the viewBox aspect ratio.
pub fn px_per_user_unit(root: Node<'_, '_>) -> Option<(f64, f64)> {
    let vb = root.attribute("viewBox").and_then(parse_view_box)?;
    let w = root.attribute("width").and_then(parse_positive_length);
    let h = root.attribute("height").and_then(parse_positive_length);
    let (sx, sy) = match (w, h) {
        (Some(w), Some(h)) => (w / vb.width, h / vb.height),
        (Some(w), None) => (w / vb.width, w / vb.width),
        (None, Some(h)) => (h / vb.height, h / vb.height),
        (None, None) => return None,
    };
    let par = root.attribute("preserveAspectRatio").unwrap_or("").trim();
    let scales = if par.split_whitespace().any(|t| t == "none") {
        (sx, sy)
    } else if par.split_whitespace().any(|t| t == "slice") {
        let s = sx.max(sy);
        (s, s)
    } else {
        let s = sx.min(sy);
        (s, s)
    };
    (scales.0.is_finite() && scales.1.is_finite() && scales.0 > 0.0 && scales.1 > 0.0)
        .then_some(scales)
}
