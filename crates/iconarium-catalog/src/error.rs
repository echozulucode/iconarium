use thiserror::Error;

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error(transparent)]
    Core(#[from] svg_core::CoreError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// Unknown asset, library, folder or file.
    #[error("{0}")]
    NotFound(String),
    /// Bad argument (ambiguous library, invalid tag, unsupported option…).
    #[error("{0}")]
    Invalid(String),
    /// The operation is not allowed with the current configuration (e.g. a write outside
    /// the allowed roots).
    #[error("{0}")]
    Denied(String),
    /// A pagination cursor from before the catalog was rebuilt.
    #[error("results changed since this cursor was issued; run the search again")]
    StaleCursor,
    /// The asset exists but can't be rendered or read (over a limit, unparseable, missing).
    #[error("{0}")]
    Unavailable(String),
}

pub type Result<T> = std::result::Result<T, CatalogError>;
