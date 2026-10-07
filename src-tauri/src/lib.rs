//! Tauri application shell. Business logic lives in `svg-core`; this crate wires it to
//! IPC commands, events, custom URI protocols and native OS integration.

pub mod app;
pub mod commands;
pub mod error;
pub mod native;
pub mod preview;
pub mod util;

use app::{paths::AppPaths, AppCore};
use std::sync::Arc;
use svg_core::index::Database;
use tauri::{Manager, RunEvent};

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .register_asynchronous_uri_scheme_protocol("thumb", preview::protocol::thumb_handler)
        .register_asynchronous_uri_scheme_protocol("svgfile", preview::protocol::svgfile_handler)
        .setup(|app| {
            let handle = app.handle().clone();
            let paths = AppPaths::resolve(&handle)?;
            let guard = app::logging::init(&paths.log_dir);
            // Keep the log writer alive for the app's lifetime.
            app.manage(LogGuard(guard));
            tracing::info!("starting Iconarium {}", env!("CARGO_PKG_VERSION"));

            let db = match Database::open(&paths.db_path) {
                Ok(db) => db,
                Err(e) => {
                    // A corrupt index must never prevent startup: move it aside and rebuild.
                    tracing::error!(
                        "opening index {} failed ({e}); recreating",
                        paths.db_path.display()
                    );
                    let backup = paths.db_path.with_extension("corrupt");
                    let _ = std::fs::rename(&paths.db_path, backup);
                    Database::open(&paths.db_path)?
                }
            };
            let (db_tx, db_rx) = crossbeam_channel::unbounded();
            let core = Arc::new(AppCore::new(handle, paths, db, db_tx));
            core.temp.purge_all();
            preview::worker::spawn_db_writer(core.clone(), db_rx);
            preview::worker::spawn_workers(&core);
            app.manage(core.clone());

            std::thread::Builder::new()
                .name("startup".into())
                .spawn(move || {
                    core.restore_last_library();
                    svg_core::svg::renderer::warm_up_fonts();
                })?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::library::get_app_state,
            commands::library::open_library,
            commands::library::pick_and_open_library,
            commands::library::update_settings,
            commands::assets::search,
            commands::assets::get_assets,
            commands::assets::get_asset_detail,
            commands::assets::set_viewport,
            commands::clipboard::copy_assets,
            commands::clipboard::copy_region,
            commands::drag::start_drag_assets,
            commands::drag::start_drag_region,
            commands::viewer::save_region,
            commands::viewer::reveal_assets,
            commands::viewer::open_external,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Iconarium");

    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            if let Some(core) = handle.try_state::<Arc<AppCore>>() {
                core.shutdown();
            }
        }
    });
}

struct LogGuard(#[allow(dead_code)] Option<tracing_appender::non_blocking::WorkerGuard>);
