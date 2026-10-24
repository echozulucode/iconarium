//! `iconarium.yaml`: optional library metadata kept in the library's root folder.
//!
//! The file belongs to the library, not to the app: it travels with the folder (copies,
//! shares, git) and works for every tool that reads it. Format 1:
//!
//! ```yaml
//! iconarium: 1                    # format version (required for writers, assumed 1 if absent)
//! library:
//!   description: Approved corporate icons
//!   tags: [corporate]             # applied to every asset in the library
//! folders:
//!   network/switches:             # path relative to the library root, '/' separators
//!     tags: [network, switch]     # applied to every asset in the folder and its subfolders
//! assets:
//!   network/router.svg:
//!     title: Edge router          # display name override
//!     description: Cisco ISR edge router
//!     tags: [edge, cisco]
//! ```
//!
//! Readers ignore unknown keys (forward compatible) and skip files with a newer format.
//! Paths match exactly first, then case-insensitively. See `docs/library-metadata.md`.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::error::{CatalogError, Result};

pub const METADATA_FILE: &str = "iconarium.yaml";
pub const FORMAT_VERSION: u32 = 1;
const MAX_TAG_CHARS: usize = 64;
const HEADER: &str = "# Iconarium library metadata (format 1): titles, descriptions and tags for the SVGs\n\
# under this folder. Spec: https://github.com/echozulucode/iconarium/blob/main/docs/library-metadata.md\n";

fn default_format() -> u32 {
    FORMAT_VERSION
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LibraryMetadata {
    #[serde(rename = "iconarium", default = "default_format")]
    pub format: u32,
    #[serde(skip_serializing_if = "LibrarySection::is_empty")]
    pub library: LibrarySection,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub folders: BTreeMap<String, FolderMeta>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub assets: BTreeMap<String, AssetMeta>,
    /// Keys this version doesn't know, kept so rewrites don't drop them.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl Default for LibraryMetadata {
    fn default() -> Self {
        Self {
            format: FORMAT_VERSION,
            library: LibrarySection::default(),
            folders: BTreeMap::new(),
            assets: BTreeMap::new(),
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LibrarySection {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl LibrarySection {
    fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.description.is_none()
            && self.tags.is_empty()
            && self.extra.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FolderMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AssetMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl AssetMeta {
    fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.description.is_none()
            && self.tags.is_empty()
            && self.extra.is_empty()
    }
}

/// Normalize a tag: trim, collapse inner whitespace, lowercase. `None` if empty, too long
/// or containing characters that would break list syntax.
pub fn normalize_tag(raw: &str) -> Option<String> {
    let t = raw
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    if t.is_empty() || t.chars().count() > MAX_TAG_CHARS || t.contains([',', ';', '\n']) {
        None
    } else {
        Some(t)
    }
}

/// Normalize a relative path key: '\' → '/', no leading/trailing '/', no empty segments.
pub fn normalize_key(raw: &str) -> String {
    raw.replace('\\', "/")
        .split('/')
        .filter(|s| !s.is_empty() && *s != ".")
        .collect::<Vec<_>>()
        .join("/")
}

/// A parsed metadata file with lookup indexes.
#[derive(Debug, Clone)]
pub struct LoadedMetadata {
    pub path: PathBuf,
    pub modified: Option<SystemTime>,
    pub meta: LibraryMetadata,
    assets_ci: HashMap<String, String>,
    folders: Vec<(String, FolderMeta)>,
}

impl LoadedMetadata {
    pub(crate) fn new(
        path: PathBuf,
        modified: Option<SystemTime>,
        mut meta: LibraryMetadata,
    ) -> Self {
        // Normalize keys once so lookups are simple.
        meta.assets = std::mem::take(&mut meta.assets)
            .into_iter()
            .map(|(k, v)| (normalize_key(&k), v))
            .collect();
        meta.folders = std::mem::take(&mut meta.folders)
            .into_iter()
            .map(|(k, v)| (normalize_key(&k), v))
            .collect();
        let assets_ci = meta
            .assets
            .keys()
            .map(|k| (k.to_lowercase(), k.clone()))
            .collect();
        let folders = meta
            .folders
            .iter()
            .map(|(k, v)| (k.to_lowercase(), v.clone()))
            .collect();
        Self {
            path,
            modified,
            meta,
            assets_ci,
            folders,
        }
    }

    /// Entry for a library-relative asset path (exact, then case-insensitive).
    pub fn asset(&self, rel: &str) -> Option<&AssetMeta> {
        self.meta.assets.get(rel).or_else(|| {
            self.assets_ci
                .get(&rel.to_lowercase())
                .and_then(|k| self.meta.assets.get(k))
        })
    }

    /// Folder entries that apply to `rel` (its folder and every ancestor folder).
    pub fn folders_for<'a>(&'a self, rel: &str) -> impl Iterator<Item = &'a FolderMeta> + 'a {
        let lc = rel.to_lowercase();
        self.folders.iter().filter_map(move |(k, v)| {
            (lc.len() > k.len() && lc.starts_with(k.as_str()) && lc.as_bytes()[k.len()] == b'/')
                .then_some(v)
        })
    }
}

/// Read `<root>/iconarium.yaml`. `Ok(None)` when there is no file. A file that fails to
/// parse, or uses a newer format, is an error: callers usually log it and carry on.
pub fn load(root: &Path) -> Result<Option<LoadedMetadata>> {
    let path = root.join(METADATA_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    let meta =
        parse(&text).map_err(|e| CatalogError::Invalid(format!("{}: {e}", path.display())))?;
    Ok(Some(LoadedMetadata::new(path, modified, meta)))
}

/// Parse metadata text (empty text is an empty document).
pub fn parse(text: &str) -> std::result::Result<LibraryMetadata, String> {
    if text.trim().is_empty() {
        return Ok(LibraryMetadata::default());
    }
    let meta: LibraryMetadata = serde_saphyr::from_str(text).map_err(|e| e.to_string())?;
    if meta.format > FORMAT_VERSION {
        return Err(format!(
            "format {} is newer than this tool supports ({FORMAT_VERSION})",
            meta.format
        ));
    }
    Ok(meta)
}

/// Serialize with the standard header. Maps are ordered, so output is deterministic.
pub fn to_yaml(meta: &LibraryMetadata) -> Result<String> {
    let mut m = meta.clone();
    m.format = FORMAT_VERSION;
    let body = serde_saphyr::to_string(&m)
        .map_err(|e| CatalogError::Invalid(format!("cannot serialize metadata: {e}")))?;
    Ok(format!("{HEADER}{body}"))
}

/// Serializes metadata writes within this process (MCP calls can run concurrently).
static WRITE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Add and/or remove tags on assets (library-relative paths) in `<root>/iconarium.yaml`,
/// creating the file if needed.
///
/// * Only the touched assets change; every other entry, section and unknown key is kept
///   as parsed (keys are not renormalized).
/// * Existing keys are reused when they match case-insensitively, so edits don't fork entries.
/// * Tags of touched assets are normalized and de-duplicated.
/// * If a rewrite would change anything beyond the edit (comments, formatting, key order),
///   the original is first copied to `iconarium.yaml.bak`.
/// * Writes atomically (unique temp file + rename).
///
/// Returns each asset's resulting metadata tags.
pub fn update_tags(
    root: &Path,
    rel_paths: &[String],
    add: &[String],
    remove: &[String],
) -> Result<Vec<(String, Vec<String>)>> {
    let norm = |v: &[String]| -> Result<Vec<String>> {
        v.iter()
            .map(|t| {
                normalize_tag(t).ok_or_else(|| {
                    CatalogError::Invalid(format!(
                        "invalid tag {t:?} (1–{MAX_TAG_CHARS} characters, no commas/semicolons)"
                    ))
                })
            })
            .collect()
    };
    let (add, remove) = (norm(add)?, norm(remove)?);
    let _guard = WRITE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let path = root.join(METADATA_FILE);
    let existing = match std::fs::read_to_string(&path) {
        Ok(t) => Some(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let mut meta = match &existing {
        Some(t) => {
            parse(t).map_err(|e| CatalogError::Invalid(format!("{}: {e}", path.display())))?
        }
        None => LibraryMetadata::default(),
    };
    let lossy = match &existing {
        Some(t) => to_yaml(&meta)? != *t,
        None => false,
    };

    let mut out = Vec::with_capacity(rel_paths.len());
    for rel in rel_paths {
        let rel = normalize_key(rel);
        let key = meta
            .assets
            .keys()
            .find(|k| normalize_key(k).to_lowercase() == rel.to_lowercase())
            .cloned()
            .unwrap_or_else(|| rel.clone());
        let entry = meta.assets.entry(key.clone()).or_default();
        let mut tags: Vec<String> = Vec::new();
        for t in entry
            .tags
            .iter()
            .filter_map(|t| normalize_tag(t))
            .chain(add.iter().cloned())
        {
            if !tags.contains(&t) && !remove.contains(&t) {
                tags.push(t);
            }
        }
        tags.sort();
        entry.tags = tags.clone();
        if entry.is_empty() {
            meta.assets.remove(&key);
        }
        out.push((rel, tags));
    }

    if lossy {
        if let Some(text) = &existing {
            std::fs::write(root.join(format!("{METADATA_FILE}.bak")), text)?;
        }
    }
    let yaml = to_yaml(&meta)?;
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = root.join(format!(".{METADATA_FILE}.{}-{n}.tmp", std::process::id()));
    std::fs::write(&tmp, yaml)?;
    if let Err(e) = std::fs::rename(&tmp, &path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
iconarium: 1
library:
  description: Approved icons
  tags: [corporate]
folders:
  network/:
    tags: [network]
  network/switches:
    tags: [switch]
assets:
  'network\\Router.svg':
    title: Edge router
    tags: [edge, cisco]
future_key: ignored
";

    #[test]
    fn parses_and_looks_up() {
        let m = LoadedMetadata::new(PathBuf::from("x"), None, parse(SAMPLE).unwrap());
        assert_eq!(m.meta.library.tags, vec!["corporate"]);
        let a = m
            .asset("network/router.svg")
            .expect("case-insensitive match");
        assert_eq!(a.title.as_deref(), Some("Edge router"));
        let tags: Vec<_> = m
            .folders_for("network/switches/24port.svg")
            .flat_map(|f| f.tags.clone())
            .collect();
        assert_eq!(tags, vec!["network", "switch"]);
        assert_eq!(
            m.folders_for("networking/x.svg").count(),
            0,
            "prefix must end at a '/'"
        );
        assert_eq!(m.folders_for("network").count(), 0);
    }

    #[test]
    fn rejects_newer_format_and_tolerates_empty() {
        assert!(parse("iconarium: 2\n").unwrap_err().contains("newer"));
        assert_eq!(parse("").unwrap(), LibraryMetadata::default());
        assert!(parse("assets: [1, 2]").is_err());
    }

    #[test]
    fn tag_normalization() {
        assert_eq!(
            normalize_tag("  Load   Balancer "),
            Some("load balancer".into())
        );
        assert_eq!(normalize_tag("a,b"), None);
        assert_eq!(normalize_tag(""), None);
        assert_eq!(normalize_tag(&"x".repeat(65)), None);
    }

    #[test]
    fn update_tags_round_trip_with_backup() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // Create from scratch.
        let r = update_tags(
            root,
            &["a/b.svg".into()],
            &["Edge".into(), "net".into()],
            &[],
        )
        .unwrap();
        assert_eq!(
            r,
            vec![(
                "a/b.svg".to_string(),
                vec!["edge".to_string(), "net".to_string()]
            )]
        );
        let text = std::fs::read_to_string(root.join(METADATA_FILE)).unwrap();
        assert!(text.starts_with("# Iconarium library metadata"));
        assert!(
            !root.join("iconarium.yaml.bak").exists(),
            "only our header: no backup"
        );
        let back = load(root).unwrap().unwrap();
        assert_eq!(back.asset("a/b.svg").unwrap().tags, vec!["edge", "net"]);

        // A hand-edited file with comments gets a backup; keys match case-insensitively.
        std::fs::write(root.join(METADATA_FILE), format!("# my notes\n{SAMPLE}")).unwrap();
        let r = update_tags(
            root,
            &["NETWORK/router.svg".into()],
            &["wan".into()],
            &["cisco".into()],
        )
        .unwrap();
        assert_eq!(r[0].1, vec!["edge", "wan"]);
        assert!(root.join("iconarium.yaml.bak").exists());
        let back = load(root).unwrap().unwrap();
        assert_eq!(back.meta.assets.len(), 1, "no forked entry");
        assert_eq!(
            back.asset("network/Router.svg").unwrap().title.as_deref(),
            Some("Edge router")
        );
        assert_eq!(
            back.meta.library.tags,
            vec!["corporate"],
            "other sections survive"
        );

        // Unknown keys survive the rewrite.
        let text = std::fs::read_to_string(root.join(METADATA_FILE)).unwrap();
        assert!(text.contains("future_key"), "{text}");

        // A canonical file (written by us) is not backed up again.
        std::fs::remove_file(root.join("iconarium.yaml.bak")).unwrap();
        update_tags(root, &["a/b.svg".into()], &["x".into()], &[]).unwrap();
        assert!(!root.join("iconarium.yaml.bak").exists());

        // Hand-written, un-normalized tags of a touched entry are normalized and deduped.
        std::fs::write(
            root.join(METADATA_FILE),
            "iconarium: 1\nassets:\n  q.svg:\n    tags: ['Load  Balancer', load balancer, Edge]\n",
        )
        .unwrap();
        let r = update_tags(root, &["q.svg".into()], &[], &["edge".into()]).unwrap();
        assert_eq!(r[0].1, vec!["load balancer"]);

        // Removing the last tag of an otherwise empty entry drops the entry.
        update_tags(
            root,
            &["a/b.svg".into()],
            &[],
            &["edge".into(), "net".into()],
        )
        .unwrap();
        assert!(load(root).unwrap().unwrap().asset("a/b.svg").is_none());

        assert!(update_tags(root, &["x.svg".into()], &["bad,tag".into()], &[]).is_err());
    }
}
