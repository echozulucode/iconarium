//! Complexity guardrails (plan §2.5, §13): cheap pre-parse checks on raw bytes and
//! post-parse checks on the XML tree. All thresholds come from [`Limits`].

use roxmltree::Document;

use super::parser::{fmt_size, group_thousands, href};
use crate::config::Limits;
use crate::model::Complexity;

/// Outcome of a limit check: `Err(message)` holds a user-facing explanation.
pub type LimitResult = std::result::Result<(), String>;

/// Pre-parse check: the raw file size.
pub fn check_file_size(len: u64, limits: &Limits) -> LimitResult {
    if len > limits.max_file_bytes {
        Err(format!(
            "File exceeds rendering limit ({} > {})",
            fmt_size(len),
            fmt_size(limits.max_file_bytes)
        ))
    } else {
        Ok(())
    }
}

/// Estimated decoded size of a `data:` URI payload (base64 → len*3/4).
pub fn data_uri_decoded_len(uri: &str) -> u64 {
    let Some(comma) = uri.find(',') else { return 0 };
    let header = &uri[..comma];
    let payload = uri[comma + 1..].trim();
    if header.to_ascii_lowercase().ends_with(";base64") {
        let n = payload.bytes().filter(|b| !b.is_ascii_whitespace()).count() as u64;
        n * 3 / 4
    } else {
        payload.len() as u64
    }
}

fn starts_with_ci(s: &str, prefix: &str) -> bool {
    s.len() >= prefix.len() && s.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

/// Whether an href value points outside the document (network, file system,
/// or a relative path). Fragment-only (`#id`) and `data:` references are internal.
pub fn is_external_ref(value: &str) -> bool {
    let v = value.trim();
    if v.is_empty()
        || v.starts_with('#')
        || starts_with_ci(v, "data:")
        || starts_with_ci(v, "javascript:")
    {
        return false;
    }
    true
}

/// Post-parse complexity statistics. `file_size` is filled from `file_len`.
pub fn collect_complexity(doc: &Document<'_>, file_len: u64) -> Complexity {
    let mut c = Complexity {
        file_size: file_len,
        ..Default::default()
    };
    let mut nodes: u64 = 0;
    for node in doc.descendants() {
        nodes += 1;
        if node.is_text() {
            // Only text that the renderer lays out counts (not CSS in <style>, scripts, metadata).
            let in_text_el = node
                .ancestors()
                .skip(1)
                .find(|a| a.is_element())
                .is_some_and(|p| matches!(p.tag_name().name(), "text" | "tspan" | "textPath" | "a" | "tref"))
                && node.ancestors().any(|a| a.is_element() && a.tag_name().name() == "text");
            if in_text_el {
                let chars = node.text().map(|t| t.chars().count()).unwrap_or(0);
                c.max_text_node_chars = c.max_text_node_chars.max(chars.min(u32::MAX as usize) as u32);
                c.render_text_chars = c.render_text_chars.saturating_add(chars as u64);
            }
            continue;
        }
        if !node.is_element() {
            continue;
        }
        if node.tag_name().name() == "script" {
            c.has_scripts = true;
        }
        for a in node.attributes() {
            let name = a.name();
            if a.namespace().is_none() && name.len() > 2 && starts_with_ci(name, "on") {
                c.has_scripts = true;
            }
        }
        if let Some(h) = href(node) {
            let h = h.trim();
            if starts_with_ci(h, "javascript:") {
                c.has_scripts = true;
            } else if starts_with_ci(h, "data:image/") {
                c.embedded_raster_bytes = c
                    .embedded_raster_bytes
                    .saturating_add(data_uri_decoded_len(h));
            } else if is_external_ref(h) {
                c.has_external_refs = true;
            }
        }
    }
    c.node_count = nodes.min(u32::MAX as u64) as u32;
    c
}

/// Post-parse limit check on collected complexity.
pub fn check_complexity(c: &Complexity, limits: &Limits) -> LimitResult {
    if c.node_count > limits.max_nodes {
        return Err(format!(
            "Too many XML nodes ({} > {})",
            group_thousands(c.node_count as u64),
            group_thousands(limits.max_nodes as u64)
        ));
    }
    if c.embedded_raster_bytes > limits.max_embedded_raster_bytes {
        return Err(format!(
            "Embedded images exceed limit ({} > {})",
            fmt_size(c.embedded_raster_bytes),
            fmt_size(limits.max_embedded_raster_bytes)
        ));
    }
    if c.max_text_node_chars > limits.max_text_node_chars {
        return Err(format!(
            "Text run too long to render ({} > {} characters)",
            group_thousands(c.max_text_node_chars as u64),
            group_thousands(limits.max_text_node_chars as u64)
        ));
    }
    if c.render_text_chars > limits.max_render_text_chars {
        return Err(format!(
            "Too much text to render ({} > {} characters)",
            group_thousands(c.render_text_chars),
            group_thousands(limits.max_render_text_chars)
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_uri_estimates() {
        assert_eq!(data_uri_decoded_len("data:image/png;base64,AAAA"), 3);
        assert_eq!(data_uri_decoded_len("data:image/png;base64, AA AA\n"), 3);
        assert_eq!(data_uri_decoded_len("data:image/svg+xml,%3Csvg"), 6);
        assert_eq!(data_uri_decoded_len("data:nocomma"), 0);
    }

    #[test]
    fn external_refs() {
        assert!(!is_external_ref("#a"));
        assert!(!is_external_ref("data:image/png;base64,AA"));
        assert!(is_external_ref("http://x/y.png"));
        assert!(is_external_ref("file:///c:/x.png"));
        assert!(is_external_ref("img/a.png"));
        assert!(is_external_ref("other.svg#id"));
    }

    #[test]
    fn size_message() {
        let l = Limits::default();
        let e = check_file_size((31.2 * 1024.0 * 1024.0) as u64, &l).unwrap_err();
        assert_eq!(e, "File exceeds rendering limit (31.2 MB > 25 MB)");
        assert!(check_file_size(l.max_file_bytes, &l).is_ok());
    }
}
