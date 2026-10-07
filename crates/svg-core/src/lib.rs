//! svg-core: headless engine for the SVG Library Browser.
//!
//! * [`library`] — progressive recursive discovery, fingerprints, reconciliation.
//! * [`index`]   — persistent SQLite catalog (migrations, assets, metadata, text, thumbnails, settings).
//! * [`svg`]     — analysis (metadata, text extraction, complexity limits), rendering, cropping.
//! * [`search`]  — in-memory catalog with token/glob/regex search, ranking and match explanations.

pub mod config;
pub mod error;
pub mod index;
pub mod library;
pub mod model;
pub mod search;
pub mod svg;

pub use error::{CoreError, Result};
