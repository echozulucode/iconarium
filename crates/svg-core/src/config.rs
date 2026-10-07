//! Configuration: complexity limits and application settings.
//! Values are configuration, never constants scattered through the code (plan §2.5, §13).

use serde::{Deserialize, Serialize};

pub const MB: u64 = 1024 * 1024;

/// Complexity guardrails applied before expensive processing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Limits {
    /// Files at or above this size get a soft warning in the UI.
    pub warn_file_bytes: u64,
    /// Hard processing limit: larger files are LIMIT_EXCEEDED (still listed).
    pub max_file_bytes: u64,
    /// Maximum parsed XML nodes.
    pub max_nodes: u32,
    /// Maximum decoded size of embedded data: URI rasters.
    pub max_embedded_raster_bytes: u64,
    /// Characters kept per individual text node.
    pub max_text_node_chars: u32,
    /// Total searchable text kept per SVG.
    pub max_extracted_text_bytes: u32,
    /// Max rendered pixels (w*h) for any single PNG export.
    pub max_render_pixels: u64,
    /// Maximum element nesting depth. Deeply nested documents overflow parser/renderer
    /// recursion, so they are rejected before parsing.
    pub max_nesting_depth: u32,
    /// Maximum total characters inside <text> elements. Text layout cost grows faster
    /// than linearly, so text-heavy documents are listed but not rendered.
    pub max_render_text_chars: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            warn_file_bytes: 10 * MB,
            max_file_bytes: 25 * MB,
            max_nodes: 100_000,
            max_embedded_raster_bytes: 50 * MB,
            max_text_node_chars: 10_000,
            max_extracted_text_bytes: MB as u32,
            max_render_pixels: 64_000_000,
            max_nesting_depth: 256,
            max_render_text_chars: 200_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ViewerBackground {
    #[default]
    Checkerboard,
    White,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GallerySize {
    Small,
    #[default]
    Medium,
    Large,
}

/// Persisted application settings (stored as JSON in the `settings` table).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub limits: Limits,
    /// Thumbnail edge length in px.
    pub thumbnail_size: u32,
    /// Render thumbnails for the whole library in the background (P3).
    pub prefill_thumbnails: bool,
    /// Worker threads for analysis/rendering (None = auto).
    pub worker_threads: Option<usize>,
    /// Discovery batch size sent to the UI (100–500).
    pub scan_batch_size: usize,
    /// Skip dot-files and dot-directories during discovery.
    pub ignore_hidden: bool,
    /// Also place SVG markup as text/plain on the clipboard when copying SVG.
    pub clipboard_include_svg_text: bool,
    /// Also place a bitmap (CF_DIB) when copying SVG, as a fallback for raster-only receivers.
    pub clipboard_include_bitmap_with_svg: bool,
    /// Scale used for the PNG fallback that accompanies an SVG copy.
    pub clipboard_png_fallback_scale: f64,
    pub viewer_background: ViewerBackground,
    pub gallery_size: GallerySize,
    pub show_relative_dir: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            limits: Limits::default(),
            thumbnail_size: 256,
            prefill_thumbnails: true,
            worker_threads: None,
            scan_batch_size: 250,
            ignore_hidden: true,
            clipboard_include_svg_text: false,
            clipboard_include_bitmap_with_svg: true,
            clipboard_png_fallback_scale: 1.0,
            viewer_background: ViewerBackground::Checkerboard,
            gallery_size: GallerySize::Medium,
            show_relative_dir: true,
        }
    }
}

impl Settings {
    /// Clamp values into sane ranges after loading/merging untrusted input.
    pub fn sanitized(mut self) -> Self {
        self.thumbnail_size = self.thumbnail_size.clamp(64, 1024);
        self.scan_batch_size = self.scan_batch_size.clamp(50, 2000);
        self.clipboard_png_fallback_scale = self.clipboard_png_fallback_scale.clamp(0.25, 4.0);
        if let Some(n) = self.worker_threads {
            self.worker_threads = Some(n.clamp(1, 32));
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_partial_json() {
        let s = Settings::default();
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
        // Missing fields fall back to defaults.
        let partial: Settings = serde_json::from_str(r#"{"thumbnailSize":128}"#).unwrap();
        assert_eq!(partial.thumbnail_size, 128);
        assert_eq!(partial.limits, Limits::default());
    }

    #[test]
    fn sanitize_clamps() {
        let s = Settings { thumbnail_size: 5, scan_batch_size: 1_000_000, ..Default::default() }.sanitized();
        assert_eq!(s.thumbnail_size, 64);
        assert_eq!(s.scan_batch_size, 2000);
    }
}
