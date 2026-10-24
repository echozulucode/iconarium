//! Tags for an asset, derived from data that already exists. There is no tag store in the
//! index; tags come from:
//!
//! | Source | Example |
//! |---|---|
//! | [`TagSource::Metadata`] | `iconarium.yaml`: library, folder (inherited) and asset tags |
//! | [`TagSource::Keywords`] | `Keywords: user, person` inside `<desc>` (e.g. excalidraw-to-svg output) |
//! | [`TagSource::Folder`] | `network/switches/24/x.svg` → `network`, `switches` (size-only folders skipped) |

use serde::{Deserialize, Serialize};

use crate::metadata::{normalize_tag, LoadedMetadata};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum TagSource {
    /// `iconarium.yaml` in the library root.
    Metadata,
    /// A `Keywords:` list in the SVG's `<desc>`.
    Keywords,
    /// A folder in the asset's relative path.
    Folder,
}

impl TagSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            TagSource::Metadata => "metadata",
            TagSource::Keywords => "keywords",
            TagSource::Folder => "folder",
        }
    }
}

/// Folder names that carry no meaning as tags: sizes and numbers (`24`, `48px`, `16x16`, `@2x`).
fn is_noise_folder(name: &str) -> bool {
    let n = name
        .trim_start_matches('@')
        .trim_end_matches("px")
        .trim_end_matches('x');
    !n.is_empty()
        && n.split('x')
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Tags after `Keywords:` in a (whitespace-collapsed) description, up to a following
/// ` Text:` label or the end. Case-insensitive labels.
pub fn keywords_from_description(desc: &str) -> Vec<String> {
    // ASCII-only lowercasing keeps byte offsets identical to `desc` (full Unicode
    // lowercasing can change lengths, e.g. 'İ'), so the slices below stay valid.
    let lc = desc.to_ascii_lowercase();
    let Some(start) = lc.find("keywords:") else {
        return Vec::new();
    };
    let rest_start = start + "keywords:".len();
    let end = lc[rest_start..]
        .find(" text:")
        .map(|i| rest_start + i)
        .unwrap_or(lc.len());
    desc[rest_start..end]
        .split([',', ';'])
        .filter_map(normalize_tag)
        .collect()
}

/// All tags of one asset with their source; each tag appears once (first source wins, in
/// the order metadata → keywords → folder).
pub fn derive(
    relative_path: &str,
    description: Option<&str>,
    metadata: Option<&LoadedMetadata>,
) -> Vec<(String, TagSource)> {
    let mut out: Vec<(String, TagSource)> = Vec::new();
    let mut push = |t: String, s: TagSource| {
        if !out.iter().any(|(x, _)| *x == t) {
            out.push((t, s));
        }
    };
    if let Some(m) = metadata {
        let lib = m.meta.library.tags.iter();
        let folders = m.folders_for(relative_path).flat_map(|f| f.tags.iter());
        let asset = m
            .asset(relative_path)
            .into_iter()
            .flat_map(|a| a.tags.iter());
        for t in asset.chain(folders).chain(lib) {
            if let Some(t) = normalize_tag(t) {
                push(t, TagSource::Metadata);
            }
        }
    }
    if let Some(d) = description {
        for t in keywords_from_description(d) {
            push(t, TagSource::Keywords);
        }
    }
    let mut parts: Vec<&str> = relative_path.split('/').collect();
    parts.pop(); // filename
    for p in parts {
        if is_noise_folder(p) {
            continue;
        }
        if let Some(t) = normalize_tag(p) {
            push(t, TagSource::Folder);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metadata::parse;

    #[test]
    fn keywords_parse_from_collapsed_description() {
        let d = "Excalidraw library c4.excalidrawlib, item #1 C4 person actor Keywords: user, Person, actor, c4 model Text: User | [person]";
        assert_eq!(
            keywords_from_description(d),
            vec!["user", "person", "actor", "c4 model"]
        );
        assert_eq!(keywords_from_description("keywords: a; b"), vec!["a", "b"]);
        assert!(keywords_from_description("no labels here").is_empty());
        // Characters whose lowercase form has a different byte length must not panic.
        let k = keywords_from_description("İİ ẞ Keywords: a, İb Text: x");
        assert_eq!(k.len(), 2);
        assert_eq!(k[0], "a");
    }

    #[test]
    fn folders_skip_sizes() {
        let t = derive("UI/Icons/24/@2x/48px/16x16/Home Page/x.svg", None, None);
        let names: Vec<_> = t.iter().map(|(t, _)| t.as_str()).collect();
        assert_eq!(names, vec!["ui", "icons", "home page"]);
        assert!(t.iter().all(|(_, s)| *s == TagSource::Folder));
        assert!(derive("root.svg", None, None).is_empty());
    }

    #[test]
    fn sources_are_merged_without_duplicates() {
        let meta = parse("library: {tags: [corp]}\nfolders: {network: {tags: [Network, lan]}}\nassets: {network/r.svg: {tags: [edge]}}").unwrap();
        let loaded = LoadedMetadata::new(std::path::PathBuf::from("x"), None, meta);
        let t = derive("network/r.svg", Some("Keywords: edge, wan"), Some(&loaded));
        assert_eq!(
            t,
            vec![
                ("edge".into(), TagSource::Metadata),
                ("network".into(), TagSource::Metadata),
                ("lan".into(), TagSource::Metadata),
                ("corp".into(), TagSource::Metadata),
                ("wan".into(), TagSource::Keywords),
            ]
        );
    }
}
