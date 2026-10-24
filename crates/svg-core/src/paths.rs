//! Where the desktop app keeps its data, so other processes (the CLI and MCP server)
//! can find the same index and thumbnail cache.
//!
//! These mirror Tauri's `app_local_data_dir`, `app_cache_dir` and `app_log_dir` for the
//! app's identifier (Tauri resolves them with the same `dirs` functions):
//!
//! | | Windows | Linux | macOS |
//! |---|---|---|---|
//! | data (index) | `%LOCALAPPDATA%\<id>` | `$XDG_DATA_HOME/<id>` | `~/Library/Application Support/<id>` |
//! | cache (thumbnails) | `%LOCALAPPDATA%\<id>` | `$XDG_CACHE_HOME/<id>` | `~/Library/Caches/<id>` |
//! | logs | `%LOCALAPPDATA%\<id>\logs` | `$XDG_DATA_HOME/<id>/logs` | `~/Library/Logs/<id>` |

use std::path::{Path, PathBuf};

/// Tauri bundle identifier (`src-tauri/tauri.conf.json`). Changing it moves every
/// directory below.
pub const APP_IDENTIFIER: &str = "com.echozed.iconarium";

/// File name of the persistent index inside the data directory.
pub const INDEX_FILE_NAME: &str = "library-index.sqlite3";

/// The app's data directory (holds the index).
pub fn app_data_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join(APP_IDENTIFIER))
}

/// The index the desktop app maintains.
pub fn default_index_path() -> Option<PathBuf> {
    app_data_dir().map(|d| d.join(INDEX_FILE_NAME))
}

/// The app's cache directory.
pub fn app_cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join(APP_IDENTIFIER))
}

/// The app's thumbnail cache (see [`thumb_path`]).
pub fn thumbnails_dir() -> Option<PathBuf> {
    app_cache_dir().map(|d| d.join("thumbnails"))
}

/// The app's log directory.
pub fn app_log_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        dirs::home_dir().map(|h| h.join("Library").join("Logs").join(APP_IDENTIFIER))
    } else {
        app_data_dir().map(|d| d.join("logs"))
    }
}

/// Thumbnail cache key: fingerprint (path+size+mtime) + renderer version + size, so any
/// file change or renderer upgrade misses the cache.
pub fn thumb_cache_key(fingerprint: &str, size: u32) -> String {
    format!("{fingerprint}-r{}-s{size}", crate::svg::RENDERER_VERSION)
}

/// Location of one cached thumbnail PNG: `<dir>/<id % 256 as hex>/<id>-<key>.png`.
pub fn thumb_path(dir: &Path, id: crate::model::AssetId, fingerprint: &str, size: u32) -> PathBuf {
    dir.join(format!("{:02x}", id % 256))
        .join(format!("{id}-{}.png", thumb_cache_key(fingerprint, size)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumb_layout_is_stable() {
        // The app and the CLI share this cache; the layout must not drift.
        let p = thumb_path(Path::new("T"), 513, "abc123", 256);
        let expect = format!("abc123-r{}-s256", crate::svg::RENDERER_VERSION);
        assert_eq!(thumb_cache_key("abc123", 256), expect);
        assert_eq!(
            p,
            Path::new("T").join("01").join(format!("513-{expect}.png"))
        );
    }

    #[test]
    fn dirs_end_with_identifier() {
        if let Some(p) = default_index_path() {
            assert!(p.ends_with(Path::new(APP_IDENTIFIER).join(INDEX_FILE_NAME)));
        }
        if let Some(p) = thumbnails_dir() {
            assert!(p.ends_with(Path::new(APP_IDENTIFIER).join("thumbnails")));
        }
    }
}
