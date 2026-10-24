//! iconarium-catalog: the read side of an Iconarium index, for tools outside the app.
//!
//! * [`reader::IndexReader`] — loads every library of the app's index (query-only, never
//!   migrates or writes it) into memory and keeps it fresh cheaply.
//! * [`ops`] — search with library/folder/tag filters and cursors, icon details, tag and
//!   folder listings, contact-sheet previews, export, and tag edits.
//! * [`metadata`] — `iconarium.yaml`, the library-owned metadata file (titles,
//!   descriptions, tags).
//! * [`tags`] — tags derived from metadata, `<desc>` keywords and folder names.
//! * [`dto`] — wire types shared by the CLI's `--json` output and the MCP server.

pub mod dto;
pub mod error;
pub mod metadata;
pub mod names;
pub mod ops;
pub mod reader;
pub mod sheet;
pub mod tags;

#[cfg(any(test, feature = "fixtures"))]
pub mod fixtures;

pub use error::{CatalogError, Result};
pub use ops::{
    DetailOptions, ExportFormat, ExportNaming, ExportOptions, IconRef, Preview, PreviewOptions,
    SearchRequest,
};
pub use reader::{IndexReader, LibraryData, Snapshot};
pub use sheet::SheetBackground;
pub use svg_core::paths::default_index_path;
