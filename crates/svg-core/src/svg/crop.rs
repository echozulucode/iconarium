//! Region crop as SVG (plan §2.3, §25) — a *text-preserving wrap*, never a
//! re-serialization (which would outline text and drop CSS):
//!
//! ```text
//! <prolog: XML decl, DOCTYPE/entities, comments — kept byte-for-byte>
//! <svg {original root attributes and namespaces, minus width/height/viewBox/x/y/
//!       preserveAspectRatio} width="W" height="H" viewBox="0 0 w h">
//!   <defs><clipPath id="UNIQUE"><rect x="0" y="0" width="w" height="h"/></clipPath></defs>
//!   <g clip-path="url(#UNIQUE)">
//!     <g transform="translate(-x -y)"> {original root content, byte-for-byte} </g>
//!   </g>
//! </svg>
//! ```
//!
//! The clip sits on the outer, untransformed group, so its rectangle is expressed in
//! the new root's user space (0 0 w h); the translation is on the inner group.
//! `W×H` = `w×h` × the source's px-per-user-unit when the source root has an absolute
//! width/height and a viewBox (physical size preserved); otherwise `W×H = w×h`.
//!
//! Known limitations: CSS selectors that depend on the root's direct children
//! (`svg > rect`) no longer match because content is wrapped in groups; CSS
//! `width`/`height` in a root `style` attribute override the new size in browsers.

use std::collections::HashSet;

use super::metadata::px_per_user_unit;
use super::parser::{
    build_start_tag, decode_text, fmt_num, needs_svg_xmlns, parse_xml, root_layout, root_svg,
    ParseFailure, SVG_NS,
};
use crate::error::{CoreError, Result};
use crate::model::Region;

/// Root attributes replaced by the crop.
const GEOMETRY_ATTRS: &[&str] = &[
    "width",
    "height",
    "viewBox",
    "x",
    "y",
    "preserveAspectRatio",
];

const CLIP_ID_BASE: &str = "svgb-crop-clip";

/// Largest coordinate magnitude accepted in a region.
const MAX_COORD: f64 = 1e12;

/// Crop `bytes` to `region` (absolute user-space coordinates of the source, i.e. the
/// same space as `SvgMeta::doc_box`). The region is used as given — it may extend
/// beyond the document; clipping handles it.
pub fn crop_svg(bytes: &[u8], region: Region) -> Result<String> {
    if !region.is_valid()
        || [region.x, region.y, region.width, region.height]
            .iter()
            .any(|v| v.abs() > MAX_COORD)
    {
        return Err(CoreError::Invalid(format!(
            "Invalid region ({}, {}, {} × {})",
            region.x, region.y, region.width, region.height
        )));
    }
    if bytes.is_empty() {
        return Err(CoreError::Parse("Empty file".into()));
    }
    let decoded = decode_text(bytes).map_err(CoreError::Parse)?;
    let text: &str = &decoded.text;
    let doc = parse_xml(text, u32::MAX - 1).map_err(|f| match f {
        ParseFailure::Limit(m) => CoreError::Limit(m),
        ParseFailure::Malformed(m) => CoreError::Parse(m),
    })?;
    let root = root_svg(&doc).map_err(CoreError::Parse)?;
    let layout = root_layout(text, root)
        .ok_or_else(|| CoreError::Parse("Cannot locate root element".into()))?;

    // Output physical size.
    let (w, h) = (region.width, region.height);
    let ppu = px_per_user_unit(root);
    let (out_w, out_h) = match ppu {
        Some((sx, sy)) => (w * sx, h * sy),
        None => (w, h),
    };
    let non_uniform =
        matches!(ppu, Some((sx, sy)) if (sx - sy).abs() > 1e-12 * sx.abs().max(sy.abs()));

    // Unique clip id.
    let ids: HashSet<&str> = doc
        .descendants()
        .filter_map(|n| n.attribute("id"))
        .collect();
    let mut clip_id = CLIP_ID_BASE.to_string();
    let mut k = 1;
    while ids.contains(clip_id.as_str()) {
        clip_id = format!("{CLIP_ID_BASE}-{k}");
        k += 1;
    }

    let tag = &layout.tag;
    let p = tag.prefix().map(|p| format!("{p}:")).unwrap_or_default();

    let mut add: Vec<(String, String)> = Vec::new();
    if needs_svg_xmlns(root) {
        add.push(("xmlns".into(), SVG_NS.into()));
    }
    add.push(("width".into(), fmt_num(out_w)));
    add.push(("height".into(), fmt_num(out_h)));
    add.push((
        "viewBox".into(),
        format!("0 0 {} {}", fmt_num(w), fmt_num(h)),
    ));
    if non_uniform {
        add.push(("preserveAspectRatio".into(), "none".into()));
    }

    let content = &text[layout.content.clone()];
    let mut out = String::with_capacity(text.len() + 512);

    // Prolog (byte-for-byte unless we transcoded, then the XML declaration's encoding is wrong).
    let prolog = &text[..tag.start];
    if decoded.transcoded && prolog.trim_start().starts_with("<?xml") {
        let decl_start = prolog.find("<?xml").unwrap_or(0);
        match prolog[decl_start..].find("?>") {
            Some(e) => {
                out.push_str(&prolog[..decl_start]);
                out.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
                out.push_str(&prolog[decl_start + e + 2..]);
            }
            None => out.push_str(prolog),
        }
    } else {
        out.push_str(prolog);
    }

    out.push_str(&build_start_tag(tag, GEOMETRY_ATTRS, &add));
    out.push('>');
    out.push_str(&format!(
        r#"<{p}defs><{p}clipPath id="{clip_id}"><{p}rect x="0" y="0" width="{}" height="{}"/></{p}clipPath></{p}defs><{p}g clip-path="url(#{clip_id})"><{p}g transform="translate({} {})">"#,
        fmt_num(w),
        fmt_num(h),
        fmt_num(-region.x),
        fmt_num(-region.y),
    ));
    out.push_str(content);
    out.push_str(&format!("</{p}g></{p}g></{}>", tag.qname));
    // Anything after the root (trailing comments / whitespace) is kept.
    out.push_str(&text[layout.end..]);
    Ok(out)
}
