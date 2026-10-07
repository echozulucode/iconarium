//! Application directories (data, cache, thumbnails, temp, logs).

use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub db_path: PathBuf,
    pub thumbs_dir: PathBuf,
    pub tmp_dir: PathBuf,
    pub log_dir: PathBuf,
}

impl AppPaths {
    pub fn resolve(app: &AppHandle) -> std::io::Result<Self> {
        let pr = app.path();
        let data = pr
            .app_local_data_dir()
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        let cache = pr
            .app_cache_dir()
            .unwrap_or_else(|_| data.join("cache"));
        let log_dir = pr.app_log_dir().unwrap_or_else(|_| data.join("logs"));
        let paths = Self {
            db_path: data.join("library-index.sqlite3"),
            thumbs_dir: cache.join("thumbnails"),
            tmp_dir: cache.join("tmp"),
            log_dir,
        };
        std::fs::create_dir_all(&data)?;
        std::fs::create_dir_all(&paths.thumbs_dir)?;
        std::fs::create_dir_all(&paths.tmp_dir)?;
        std::fs::create_dir_all(&paths.log_dir)?;
        Ok(paths)
    }
}
