//! SVG analysis, rendering and cropping. Owned by agent B1.
//!
//! Public API (contract, see docs/implementation-plan.md §3.4):
//! * [`analyze`] — bytes → [`Analysis`] (hash, limits, metadata, searchable text). Never panics.
//! * [`renderer::render_thumbnail`] — bytes → transparent PNG fitted in size×size.
//! * [`crop::crop_svg`] — bytes + region → normalized standalone SVG string (viewBox 0 0 w h).
//! * [`renderer::render_region_png`] — bytes + region → PNG at scale with optional white bg.
//! * [`normalize::prepare_for_viewer`] — ensure root has viewBox/width/height for <img> display.

pub mod crop;
pub mod limits;
pub mod metadata;
pub mod normalize;
pub mod parser;
pub mod renderer;
pub mod text_extract;

use crate::config::Limits;
use crate::model::Analysis;

/// Bump whenever rendering output changes; part of the thumbnail cache key.
pub const RENDERER_VERSION: u32 = 1;

/// Analyze an SVG document. Always returns an Analysis; failures are reported via
/// `state` (ParseError / LimitExceeded) and `error`.
pub fn analyze(bytes: &[u8], limits: &Limits) -> Analysis {
    let _ = (bytes, limits);
    todo!("B1")
}
