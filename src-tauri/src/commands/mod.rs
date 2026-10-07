//! Tauri command handlers (thin wrappers over AppCore / svg-core).
pub mod assets;
pub mod clipboard;
pub mod drag;
pub mod library;
pub mod viewer;

use crate::error::CmdError;

/// Run blocking work off the IPC thread.
pub async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, CmdError> + Send + 'static) -> Result<T, CmdError> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| CmdError::new("error", e.to_string()))?
}
