//! Native drag-out (plan §19, §28): OLE DoDragDrop with a CF_HDROP file list on Windows,
//! always DROPEFFECT_COPY so sources are never moved. Must run on the main (UI) thread.

use std::path::PathBuf;
use tauri::{AppHandle, Window};

pub fn start_file_drag(app: &AppHandle, window: Window, files: Vec<PathBuf>, preview_png: Vec<u8>) -> Result<(), String> {
    if files.is_empty() {
        return Ok(());
    }
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        #[cfg(target_os = "linux")]
        let raw_window = window.gtk_window();
        #[cfg(not(target_os = "linux"))]
        let raw_window = tauri::Result::Ok(window.clone());

        let result = match raw_window {
            Ok(w) => drag::start_drag(
                &w,
                drag::DragItem::Files(files),
                drag::Image::Raw(preview_png),
                |result, _pos| tracing::debug!("drag finished: {result:?}"),
                drag::Options { skip_animatation_on_cancel_or_failure: false, mode: drag::DragMode::Copy },
            )
            .map_err(|e| format!("Drag failed: {e}")),
            Err(e) => Err(format!("Drag failed: {e}")),
        };
        let _ = tx.send(result);
    })
    .map_err(|e| e.to_string())?;
    rx.recv().map_err(|e| e.to_string())?
}
