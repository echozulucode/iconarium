//! Display names for assets.

const MAX_TITLE_CHARS: usize = 80;

/// Titles that editors put in by default and that say nothing about the drawing.
fn is_generic_title(t: &str) -> bool {
    let lc = t.trim().to_lowercase();
    lc.is_empty()
        || lc == "untitled"
        || lc == "svg"
        || lc.starts_with("layer ")
        || lc.starts_with("drawing")
        || lc.starts_with("artboard")
        || lc.starts_with("created with")
}

/// Filename stem with separators turned into spaces: `network-switch_24.svg` → `network switch 24`.
pub fn humanize_filename(filename: &str) -> String {
    let stem = match filename.rfind('.') {
        Some(i) if i > 0 => &filename[..i],
        _ => filename,
    };
    stem.split(['-', '_', '.', ' '])
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The name to show for an asset: the metadata title, else the SVG `<title>` when it is
/// short and meaningful, else the humanized filename.
pub fn display_name(
    filename: &str,
    svg_title: Option<&str>,
    metadata_title: Option<&str>,
) -> String {
    if let Some(t) = metadata_title.map(str::trim).filter(|t| !t.is_empty()) {
        return t.to_string();
    }
    if let Some(t) = svg_title.map(str::trim) {
        if !is_generic_title(t) && t.chars().count() <= MAX_TITLE_CHARS {
            return t.to_string();
        }
    }
    humanize_filename(filename)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_preference() {
        assert_eq!(
            display_name("r.svg", Some("Router"), Some("Edge router")),
            "Edge router"
        );
        assert_eq!(display_name("r.svg", Some("Router"), None), "Router");
        assert_eq!(
            display_name("network-switch_24.svg", Some("Layer 1"), None),
            "network switch 24"
        );
        assert_eq!(display_name("x.svg", Some(&"long ".repeat(30)), None), "x");
        assert_eq!(display_name(".svg", None, Some("  ")), "svg");
    }
}
