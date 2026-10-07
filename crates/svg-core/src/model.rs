//! Shared data model. These types are the contract between the core library,
//! the Tauri shell and (via serde, camelCase) the TypeScript frontend.
//! Mirror changes in `src/api/types.ts`.

use serde::{Deserialize, Serialize};

/// Stable asset identifier (SQLite rowid). Fits in u32 for compact transport.
pub type AssetId = u32;
pub type LibraryId = i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProcessingState {
    /// Known from discovery only (path/size/mtime). Metadata not yet extracted.
    #[default]
    Discovered,
    /// Metadata + text extracted; renderable.
    Ready,
    /// Exceeds a configured complexity limit; listed but not rendered.
    LimitExceeded,
    /// Malformed / unreadable SVG.
    ParseError,
    /// Was indexed but no longer on disk (transient, before removal).
    Missing,
}

impl ProcessingState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Discovered => "discovered",
            Self::Ready => "ready",
            Self::LimitExceeded => "limit_exceeded",
            Self::ParseError => "parse_error",
            Self::Missing => "missing",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "ready" => Self::Ready,
            "limit_exceeded" => Self::LimitExceeded,
            "parse_error" => Self::ParseError,
            "missing" => Self::Missing,
            _ => Self::Discovered,
        }
    }
    /// Whether previews may be rendered for an asset in this state.
    pub fn renderable(&self) -> bool {
        matches!(self, Self::Discovered | Self::Ready)
    }
}

/// A rectangle in SVG user units (the document's viewBox coordinate system).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Region {
    pub fn is_valid(&self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
            && self.width > 0.0
            && self.height > 0.0
    }
}

/// viewBox="min_x min_y width height"
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewBox {
    pub min_x: f64,
    pub min_y: f64,
    pub width: f64,
    pub height: f64,
}

/// Minimal record produced by discovery. Everything else is filled in later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredFile {
    /// Relative to the library root, always '/'-separated.
    pub relative_path: String,
    pub filename: String,
    pub file_size: u64,
    pub mtime_ns: i64,
}

/// Full persisted asset row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetRecord {
    pub id: AssetId,
    pub library_id: LibraryId,
    pub relative_path: String,
    pub filename: String,
    pub file_size: u64,
    pub mtime_ns: i64,
    /// Cheap change fingerprint: blake3(relative_path, size, mtime) → 16 hex chars.
    pub fast_fingerprint: String,
    /// Full BLAKE3 of the content (hex), computed lazily.
    pub content_hash: Option<String>,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub view_box: Option<ViewBox>,
    pub element_count: Option<u32>,
    pub state: ProcessingState,
    pub parse_error: Option<String>,
}

impl AssetRecord {
    /// Directory part of the relative path ("" for root-level files).
    pub fn rel_dir(&self) -> &str {
        match self.relative_path.rfind('/') {
            Some(i) => &self.relative_path[..i],
            None => "",
        }
    }
}

/// Searchable text extracted from an SVG. All fields already normalized
/// (entities decoded, whitespace collapsed, de-duplicated, capped).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchText {
    pub title: String,
    pub description: String,
    /// Concatenated <text>/<tspan>/<textPath> content, one run per line.
    pub visible_text: String,
    /// Element ids and class names, space separated.
    pub identifiers: String,
}

impl SearchText {
    pub fn is_empty(&self) -> bool {
        self.title.is_empty()
            && self.description.is_empty()
            && self.visible_text.is_empty()
            && self.identifiers.is_empty()
    }
}

/// Geometry metadata.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SvgMeta {
    /// Root width/height converted to CSS px when absolute (px, pt, mm, cm, in, pc, unitless).
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub view_box: Option<ViewBox>,
    /// Effective document box in user units: viewBox if present, else 0 0 width height,
    /// else a renderer-derived fallback. This is the coordinate system for regions.
    pub doc_box: Option<ViewBox>,
    pub element_count: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
pub struct Complexity {
    pub file_size: u64,
    pub node_count: u32,
    pub embedded_raster_bytes: u64,
    /// Longest single text run inside a <text> element (characters).
    pub max_text_node_chars: u32,
    /// Total characters inside <text> elements (drives text layout cost).
    pub render_text_chars: u64,
    pub has_scripts: bool,
    pub has_external_refs: bool,
}

/// Result of analyzing one SVG file's bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Analysis {
    pub state: ProcessingState,
    pub error: Option<String>,
    pub content_hash: String,
    pub meta: SvgMeta,
    pub text: SearchText,
    pub complexity: Complexity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchField {
    Filename,
    Path,
    Title,
    Text,
    Desc,
    IdClass,
}

/// Why an asset matched a query (shown in the gallery when useful).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatchInfo {
    pub field: MatchField,
    /// Short excerpt (≤ ~120 chars) around the match.
    pub snippet: String,
}

/// Compact card data for the gallery.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetSummary {
    pub id: AssetId,
    pub filename: String,
    pub rel_dir: String,
    pub state: ProcessingState,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub fingerprint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_info: Option<MatchInfo>,
}

impl From<&AssetRecord> for AssetSummary {
    fn from(r: &AssetRecord) -> Self {
        Self {
            id: r.id,
            filename: r.filename.clone(),
            rel_dir: r.rel_dir().to_string(),
            state: r.state,
            width: r.width,
            height: r.height,
            fingerprint: r.fast_fingerprint.clone(),
            match_info: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryInfo {
    pub id: LibraryId,
    pub path: String,
    pub display_name: String,
    /// Unix seconds.
    pub created_at: i64,
    pub last_opened: i64,
    pub last_scan: Option<i64>,
}
