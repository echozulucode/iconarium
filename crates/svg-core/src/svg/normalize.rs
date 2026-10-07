//! Viewer preparation: make the SVG scale linearly from `doc_box` to the `<img>` box.
//!
//! * Root without `viewBox` but with a known `doc_box` → inject `viewBox` (the
//!   document then maps linearly onto whatever box the viewer gives the image).
//! * When injecting a viewBox into a root with neither `width` nor `height`, also
//!   inject them from `doc_box` so the image has a sane intrinsic size.
//! * Root `<svg>` without a namespace → inject the SVG `xmlns` (otherwise browsers do
//!   not treat it as SVG).
//! * `preserveAspectRatio` is left as is (the viewer's image box matches the aspect).
//!
//! Returns `Cow::Borrowed` when nothing needs to change (or the input cannot be parsed).

use std::borrow::Cow;

use super::parser::{
    decode_text, fmt_num, needs_svg_xmlns, parse_xml, rewrite_root, root_layout, root_svg, SVG_NS,
};
use crate::model::SvgMeta;

pub fn prepare_for_viewer<'a>(bytes: &'a [u8], meta: &SvgMeta) -> Cow<'a, [u8]> {
    let Ok(decoded) = decode_text(bytes) else {
        return Cow::Borrowed(bytes);
    };
    let text: &str = &decoded.text;
    let Ok(doc) = parse_xml(text, u32::MAX - 1, super::parser::HARD_MAX_DEPTH) else {
        return Cow::Borrowed(bytes);
    };
    let Ok(root) = root_svg(&doc) else {
        return Cow::Borrowed(bytes);
    };
    let Some(layout) = root_layout(text, root) else {
        return Cow::Borrowed(bytes);
    };

    let tag = &layout.tag;
    let mut add: Vec<(String, String)> = Vec::new();
    if needs_svg_xmlns(root) {
        add.push(("xmlns".into(), SVG_NS.into()));
    }
    if let Some(db) = meta.doc_box {
        if tag.attr("viewBox").is_none() {
            add.push((
                "viewBox".into(),
                format!(
                    "{} {} {} {}",
                    fmt_num(db.min_x),
                    fmt_num(db.min_y),
                    fmt_num(db.width),
                    fmt_num(db.height)
                ),
            ));
            if tag.attr("width").is_none() && tag.attr("height").is_none() {
                add.push(("width".into(), fmt_num(db.width)));
                add.push(("height".into(), fmt_num(db.height)));
            }
        }
    }
    if add.is_empty() && !decoded.transcoded {
        return Cow::Borrowed(bytes);
    }
    let mut out = rewrite_root(text, &layout, &[], &add);
    if decoded.transcoded {
        // Output is UTF-8: fix a stale XML declaration encoding.
        if let (Some(s), true) = (out.find("<?xml"), out.trim_start().starts_with("<?xml")) {
            if let Some(e) = out[s..].find("?>") {
                out.replace_range(s..s + e + 2, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
            }
        }
    }
    Cow::Owned(out.into_bytes())
}
