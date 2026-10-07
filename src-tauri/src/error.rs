//! Command error type serialized to the UI as `{ kind, message }`.

use serde::Serialize;
use svg_core::CoreError;

#[derive(Debug, Serialize)]
pub struct CmdError {
    pub kind: &'static str,
    pub message: String,
}

impl CmdError {
    pub fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
    pub fn not_found(what: impl std::fmt::Display) -> Self {
        Self::new("not_found", format!("{what} not found"))
    }
    pub fn no_library() -> Self {
        Self::new("no_library", "No library is open")
    }
}

impl std::fmt::Display for CmdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

impl From<CoreError> for CmdError {
    fn from(e: CoreError) -> Self {
        let kind = match &e {
            CoreError::Query(_) => "invalid_query",
            CoreError::Limit(_) => "limit_exceeded",
            CoreError::Parse(_) => "parse_error",
            CoreError::Render(_) => "render_error",
            CoreError::Invalid(_) => "invalid_input",
            CoreError::Io(_) => "io",
            CoreError::Db(_) => "database",
            CoreError::Other(_) => "error",
        };
        let message = match e {
            CoreError::Query(m)
            | CoreError::Limit(m)
            | CoreError::Parse(m)
            | CoreError::Render(m)
            | CoreError::Invalid(m)
            | CoreError::Other(m) => m,
            other => other.to_string(),
        };
        Self { kind, message }
    }
}

impl From<std::io::Error> for CmdError {
    fn from(e: std::io::Error) -> Self {
        Self::new("io", e.to_string())
    }
}

impl From<tauri::Error> for CmdError {
    fn from(e: tauri::Error) -> Self {
        Self::new("tauri", e.to_string())
    }
}

pub type CmdResult<T> = Result<T, CmdError>;
