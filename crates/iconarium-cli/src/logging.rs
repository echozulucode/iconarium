//! Logging. Never to stdout: in `mcp` mode stdout carries JSON-RPC.
//!
//! * stderr: warnings and errors (override with `ICONARIUM_LOG`, e.g. `ICONARIUM_LOG=debug`).
//! * `mcp` mode also writes daily files to `<app log folder>/cli/`. A separate folder, not
//!   just a prefix: the app's appender prunes every file in its folder whose name *starts
//!   with* `iconarium`, which would include `iconarium-cli.*`.

use std::sync::OnceLock;

use tracing_subscriber::{fmt, prelude::*, EnvFilter};

static GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

pub fn init(mcp: bool) {
    let filter = || {
        EnvFilter::try_from_env("ICONARIUM_LOG")
            .unwrap_or_else(|_| EnvFilter::new(if mcp { "info" } else { "warn" }))
    };
    let stderr = fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(false)
        .with_filter(
            EnvFilter::try_from_env("ICONARIUM_LOG").unwrap_or_else(|_| EnvFilter::new("warn")),
        );
    let file = if mcp {
        svg_core::paths::app_log_dir()
            .map(|d| d.join("cli"))
            .and_then(|dir| {
                std::fs::create_dir_all(&dir).ok()?;
                let appender = tracing_appender::rolling::Builder::new()
                    .rotation(tracing_appender::rolling::Rotation::DAILY)
                    .filename_prefix("iconarium-cli")
                    .filename_suffix("log")
                    .max_log_files(7)
                    .build(&dir)
                    .ok()?;
                let (writer, guard) = tracing_appender::non_blocking(appender);
                let _ = GUARD.set(guard);
                Some(
                    fmt::layer()
                        .with_writer(writer)
                        .with_ansi(false)
                        .with_filter(filter()),
                )
            })
    } else {
        None
    };
    let _ = tracing_subscriber::registry()
        .with(stderr)
        .with(file)
        .try_init();
}
