//! SVG analysis, rendering and cropping. Owned by agent B1.
//!
//! Public API (contract, see docs/implementation-plan.md §3.4):
//! * [`analyze`] — bytes → [`Analysis`] (hash, limits, metadata, searchable text). Never panics.
//! * [`renderer::render_thumbnail`] — bytes → transparent PNG fitted in size×size.
//! * [`renderer::render_png`] — full document at CSS size × scale (pixel-capped).
//! * [`crop::crop_svg`] — bytes + region → normalized standalone SVG string (viewBox 0 0 w h).
//! * [`renderer::render_region_png`] — bytes + region → PNG at scale with optional white bg.
//! * [`normalize::prepare_for_viewer`] — ensure root has viewBox/width/height for <img> display.
//! * [`renderer::warm_up_fonts`] — load the shared font database (call at startup, background thread).

pub mod crop;
pub mod limits;
pub mod metadata;
pub mod normalize;
pub mod parser;
pub mod renderer;
pub mod text_extract;

use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::config::Limits;
use crate::model::{Analysis, Complexity, ProcessingState, SearchText, SvgMeta};

pub use crop::crop_svg;
pub use normalize::prepare_for_viewer;
pub use renderer::{
    render_png, render_region_png, render_thumbnail, warm_up_fonts, Background, RenderedPng,
};

/// Bump whenever rendering output changes; part of the thumbnail cache key.
pub const RENDERER_VERSION: u32 = 1;

struct Failure {
    state: ProcessingState,
    message: String,
    complexity: Option<Complexity>,
    meta: Option<SvgMeta>,
    /// Searchable text is still extracted (capped) for over-limit documents, so they
    /// remain findable even though they are not rendered.
    text: Option<SearchText>,
}

impl Failure {
    fn parse(message: impl Into<String>) -> Box<Self> {
        Box::new(Self {
            state: ProcessingState::ParseError,
            message: message.into(),
            complexity: None,
            meta: None,
            text: None,
        })
    }
}

fn analyze_parsed(
    bytes: &[u8],
    limits: &Limits,
) -> Result<(SvgMeta, SearchText, Complexity), Box<Failure>> {
    let decoded = parser::decode_text(bytes).map_err(Failure::parse)?;
    let text: &str = &decoded.text;
    let doc =
        parser::parse_xml(text, limits.max_nodes, limits.max_nesting_depth).map_err(
            |f| match f {
                parser::ParseFailure::Limit(m) => {
                    let too_many_nodes = m.starts_with("Too many");
                    Box::new(Failure {
                        state: ProcessingState::LimitExceeded,
                        message: m,
                        complexity: Some(Complexity {
                            file_size: bytes.len() as u64,
                            node_count: if too_many_nodes {
                                limits.max_nodes.saturating_add(1)
                            } else {
                                0
                            },
                            ..Default::default()
                        }),
                        meta: None,
                        text: None,
                    })
                }
                parser::ParseFailure::Malformed(m) => Failure::parse(m),
            },
        )?;
    let root = parser::root_svg(&doc).map_err(Failure::parse)?;

    let complexity = limits::collect_complexity(&doc, bytes.len() as u64);
    if let Err(message) = limits::check_complexity(&complexity, limits) {
        return Err(Box::new(Failure {
            state: ProcessingState::LimitExceeded,
            message,
            meta: Some(metadata::extract_meta_with(&doc, root, false)),
            complexity: Some(complexity),
            text: Some(text_extract::extract_text(&doc, root, limits)),
        }));
    }
    let meta = metadata::extract_meta(&doc, root);
    let text = text_extract::extract_text(&doc, root, limits);
    Ok((meta, text, complexity))
}

/// Analyze an SVG document. Always returns an Analysis; failures are reported via
/// `state` (ParseError / LimitExceeded) and `error`.
pub fn analyze(bytes: &[u8], limits: &Limits) -> Analysis {
    let base_complexity = Complexity {
        file_size: bytes.len() as u64,
        ..Default::default()
    };
    let mut analysis = Analysis {
        state: ProcessingState::Ready,
        error: None,
        content_hash: blake3::hash(bytes).to_hex().to_string(),
        meta: SvgMeta::default(),
        text: SearchText::default(),
        complexity: base_complexity.clone(),
    };
    if bytes.is_empty() {
        analysis.state = ProcessingState::ParseError;
        analysis.error = Some("Empty file".into());
        return analysis;
    }
    if let Err(m) = limits::check_file_size(bytes.len() as u64, limits) {
        analysis.state = ProcessingState::LimitExceeded;
        analysis.error = Some(m);
        return analysis;
    }
    match catch_unwind(AssertUnwindSafe(|| analyze_parsed(bytes, limits))) {
        Ok(Ok((meta, text, complexity))) => {
            analysis.meta = meta;
            analysis.text = text;
            analysis.complexity = complexity;
        }
        Ok(Err(f)) => {
            analysis.state = f.state;
            analysis.error = Some(f.message);
            analysis.complexity = f.complexity.unwrap_or(base_complexity);
            analysis.meta = f.meta.unwrap_or_default();
            analysis.text = f.text.unwrap_or_default();
        }
        Err(_) => {
            tracing::error!("panic while analyzing SVG ({} bytes)", bytes.len());
            analysis.state = ProcessingState::ParseError;
            analysis.error = Some("Internal error while analyzing this file".into());
        }
    }
    analysis
}
