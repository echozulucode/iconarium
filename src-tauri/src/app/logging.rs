//! tracing setup: daily rolling log file + stderr. RUST_LOG overrides the default filter.

use std::path::Path;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

pub fn init(log_dir: &Path) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,svg_core=info,iconarium_lib=debug"));
    let file = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("iconarium")
        .filename_suffix("log")
        .max_log_files(7)
        .build(log_dir)
        .ok();
    match file {
        Some(appender) => {
            let (nb, guard) = tracing_appender::non_blocking(appender);
            let _ = tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().with_writer(std::io::stderr))
                .with(fmt::layer().with_ansi(false).with_writer(nb))
                .try_init();
            Some(guard)
        }
        None => {
            let _ = tracing_subscriber::registry()
                .with(filter)
                .with(fmt::layer().with_writer(std::io::stderr))
                .try_init();
            None
        }
    }
}
