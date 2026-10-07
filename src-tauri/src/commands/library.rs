use crate::app::events::ScanStatus;
use crate::app::AppCore;
use crate::error::{CmdError, CmdResult};
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use svg_core::config::Settings;
use svg_core::model::LibraryInfo;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStateDto {
    library: Option<LibraryInfo>,
    recent: Vec<LibraryInfo>,
    scan: ScanStatus,
    settings: Settings,
    total_assets: usize,
}

#[tauri::command]
pub async fn get_app_state(core: State<'_, Arc<AppCore>>) -> CmdResult<AppStateDto> {
    let core = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        core.wait_restore(Duration::from_secs(3));
        let recent = core.db.lock().recent_libraries(10)?;
        Ok(AppStateDto {
            library: core.active().map(|a| a.info),
            recent,
            scan: core.events.status(),
            settings: core.settings(),
            total_assets: core.catalog.read().len(),
        })
    })
    .await
    .map_err(|e| CmdError::new("error", e.to_string()))?
}

#[tauri::command]
pub async fn open_library(core: State<'_, Arc<AppCore>>, path: String) -> CmdResult<LibraryInfo> {
    let core = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || core.open_library(&path))
        .await
        .map_err(|e| CmdError::new("error", e.to_string()))?
}

#[tauri::command]
pub async fn pick_and_open_library(
    app: AppHandle,
    core: State<'_, Arc<AppCore>>,
) -> CmdResult<Option<LibraryInfo>> {
    let core = core.inner().clone();
    let start_dir = core.active().map(|a| a.root);
    tauri::async_runtime::spawn_blocking(move || {
        let mut dialog = app.dialog().file().set_title("Select SVG library folder");
        if let Some(dir) = start_dir {
            dialog = dialog.set_directory(dir);
        }
        let Some(picked) = dialog.blocking_pick_folder() else {
            return Ok(None);
        };
        let path = picked
            .into_path()
            .map_err(|e| CmdError::new("invalid_input", e.to_string()))?;
        core.open_library(&path.to_string_lossy()).map(Some)
    })
    .await
    .map_err(|e| CmdError::new("error", e.to_string()))?
}

/// Deep-merge a JSON patch object into the current settings.
fn merge(base: &mut serde_json::Value, patch: serde_json::Value) {
    match (base, patch) {
        (serde_json::Value::Object(b), serde_json::Value::Object(p)) => {
            for (k, v) in p {
                match b.get_mut(&k) {
                    Some(slot) if slot.is_object() && v.is_object() => merge(slot, v),
                    _ => {
                        b.insert(k, v);
                    }
                }
            }
        }
        (b, p) => *b = p,
    }
}

pub fn apply_patch(current: &Settings, patch: serde_json::Value) -> CmdResult<Settings> {
    let mut value =
        serde_json::to_value(current).map_err(|e| CmdError::new("error", e.to_string()))?;
    merge(&mut value, patch);
    let next: Settings = serde_json::from_value(value)
        .map_err(|e| CmdError::new("invalid_input", format!("Invalid settings: {e}")))?;
    Ok(next.sanitized())
}

#[tauri::command]
pub async fn update_settings(
    core: State<'_, Arc<AppCore>>,
    patch: serde_json::Value,
) -> CmdResult<Settings> {
    let core = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let next = apply_patch(&core.settings(), patch)?;
        core.db.lock().save_settings(&next)?;
        *core.settings.write() = next.clone();
        Ok(next)
    })
    .await
    .map_err(|e| CmdError::new("error", e.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn patch_merges_nested_and_sanitizes() {
        let s = Settings::default();
        let next = apply_patch(
            &s,
            serde_json::json!({"limits": {"maxNodes": 5}, "thumbnailSize": 1}),
        )
        .unwrap();
        assert_eq!(next.limits.max_nodes, 5);
        assert_eq!(next.limits.max_file_bytes, s.limits.max_file_bytes);
        assert_eq!(next.thumbnail_size, 64);
        assert!(apply_patch(&s, serde_json::json!({"thumbnailSize": "big"})).is_err());
    }
}
