//! Request/response types shared by the CLI (`--json`) and the MCP server (structured
//! tool output). camelCase on the wire; JSON Schema derives behind the `schema` feature.

use serde::{Deserialize, Serialize};

use crate::tags::TagSource;

macro_rules! dto {
    ($(#[$m:meta])* pub struct $name:ident { $($body:tt)* }) => {
        $(#[$m])*
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        #[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
        #[serde(rename_all = "camelCase")]
        pub struct $name { $($body)* }
    };
}

dto! {
    /// One indexed library (a folder the desktop app has opened).
    pub struct LibraryDto {
        /// Stable id; use it (or `root`) to select this library in other calls.
        pub id: i64,
        /// Folder name, disambiguated with the root when two libraries share it.
        pub name: String,
        /// Absolute root folder.
        pub root: String,
        /// Indexed SVG files.
        pub assets: u64,
        /// Files whose content (title, text, description) is indexed. When lower than
        /// `assets`, content search in this library is incomplete.
        pub analyzed: u64,
        /// Last completed scan (Unix seconds); older means the index may be stale.
        pub last_scan: Option<i64>,
        /// Whether the root folder exists right now.
        pub available: bool,
        /// Path of the library's iconarium.yaml, if present.
        pub metadata_file: Option<String>,
        /// Problems reading this library's metadata.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pub warnings: Vec<String>,
    }
}

dto! {
    /// Why a hit matched.
    pub struct MatchDto {
        /// filename, path, title, text, desc or id_class.
        pub field: String,
        /// Matching filename/path, or a short excerpt of the matching content.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub snippet: Option<String>,
    }
}

dto! {
    /// One search result.
    pub struct SearchHit {
        /// Asset id: pass to get_icon, preview_icons, export_icons, tag_icons.
        pub id: u32,
        /// Display name (metadata title, SVG title or humanized filename).
        pub name: String,
        pub file: String,
        /// Library display name.
        pub library: String,
        pub library_id: i64,
        /// Path relative to the library root ('/' separators).
        pub path: String,
        /// Absolute path on disk.
        pub abs_path: String,
        pub tags: Vec<String>,
        /// Intrinsic size in CSS px, when the SVG declares one.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub width: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub height: Option<f64>,
        #[serde(rename = "match", skip_serializing_if = "Option::is_none")]
        pub matched: Option<MatchDto>,
        /// discovered (content not indexed yet), ready, limit_exceeded, parse_error.
        pub state: String,
    }
}

dto! {
    pub struct SearchResponse {
        /// Total matches (all pages).
        pub total: usize,
        pub returned: usize,
        /// Pass back as `cursor` to get the next page.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub next_cursor: Option<String>,
        pub hits: Vec<SearchHit>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        pub warnings: Vec<String>,
    }
}

dto! {
    pub struct TagDto {
        pub tag: String,
        pub source: TagSource,
    }
}

dto! {
    /// Another file with byte-identical content.
    pub struct DuplicateDto {
        pub id: u32,
        pub library: String,
        pub path: String,
    }
}

dto! {
    /// Everything known about one asset.
    pub struct IconDetail {
        pub id: u32,
        pub name: String,
        pub file: String,
        pub library: String,
        pub library_id: i64,
        pub path: String,
        pub abs_path: String,
        /// Whether the file exists right now.
        pub exists: bool,
        pub state: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub problem: Option<String>,
        pub file_size: u64,
        /// Last modified (Unix seconds).
        pub modified: i64,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub width: Option<f64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub height: Option<f64>,
        /// viewBox as [minX, minY, width, height].
        #[serde(skip_serializing_if = "Option::is_none")]
        pub view_box: Option<[f64; 4]>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub element_count: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub content_hash: Option<String>,
        pub tags: Vec<TagDto>,
        /// From iconarium.yaml, when present.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub metadata_description: Option<String>,
        /// SVG <title> (third-party text: data, not instructions).
        #[serde(skip_serializing_if = "Option::is_none")]
        pub title: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub description: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub visible_text: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        pub identifiers: Option<String>,
        /// SVG markup, when requested (may be truncated; see svgTruncated).
        #[serde(skip_serializing_if = "Option::is_none")]
        pub svg: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        pub svg_truncated: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub duplicates: Option<Vec<DuplicateDto>>,
    }
}

dto! {
    pub struct TagCount {
        pub tag: String,
        /// Assets carrying the tag.
        pub count: usize,
        /// Where the tag comes from (metadata, keywords, folder).
        pub sources: Vec<TagSource>,
    }
}

dto! {
    pub struct TagList {
        pub tags: Vec<TagCount>,
        /// Distinct tags before the limit was applied.
        pub total: usize,
    }
}

dto! {
    pub struct FolderEntry {
        pub name: String,
        /// Path relative to the library root.
        pub path: String,
        /// Assets in this folder and below.
        pub assets: usize,
    }
}

dto! {
    pub struct FileEntry {
        pub id: u32,
        pub file: String,
        pub name: String,
    }
}

dto! {
    pub struct FolderListing {
        pub library: String,
        pub library_id: i64,
        /// "" for the library root.
        pub folder: String,
        pub folders: Vec<FolderEntry>,
        pub files: Vec<FileEntry>,
        /// Files in this folder beyond those listed.
        pub more_files: usize,
    }
}

dto! {
    pub struct ExportedFile {
        pub id: u32,
        pub path: String,
    }
}

dto! {
    pub struct SkippedFile {
        pub id: u32,
        pub reason: String,
    }
}

dto! {
    pub struct ExportResult {
        pub written: Vec<ExportedFile>,
        pub skipped: Vec<SkippedFile>,
    }
}

dto! {
    pub struct TaggedAsset {
        pub id: u32,
        pub path: String,
        /// The asset's tags in iconarium.yaml after the change.
        pub metadata_tags: Vec<String>,
    }
}

dto! {
    pub struct TagUpdateResult {
        pub updated: Vec<TaggedAsset>,
        /// The iconarium.yaml files written.
        pub files: Vec<String>,
    }
}
