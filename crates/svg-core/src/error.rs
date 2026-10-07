use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("SVG parse error: {0}")]
    Parse(String),
    #[error("limit exceeded: {0}")]
    Limit(String),
    #[error("render error: {0}")]
    Render(String),
    #[error("invalid query: {0}")]
    Query(String),
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;
