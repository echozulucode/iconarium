//! In-memory search catalog. Owned by agent B2.
pub mod catalog;
pub mod query;
pub mod ranking;
pub use catalog::Catalog;
