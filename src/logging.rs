//! Shared tracing setup: logs to stderr, and optionally to a file.
//!
//! airshell-connect uses stdout as the ssh data channel, so logs never go there —
//! stderr (surfaced by ssh) and the optional file layer are the only sinks.

use std::fs::OpenOptions;
use std::io;
use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{EnvFilter, fmt};

/// Initialise the global subscriber. Level comes from `RUST_LOG` (default `info`).
///
/// When `log_file` is set, events are appended there (no ANSI) in addition to
/// stderr. Keep the returned guard alive for the life of the process: it flushes
/// the file writer's background thread on drop, so avoid `process::exit`, which
/// skips it.
pub fn init(log_file: Option<&Path>) -> io::Result<Option<WorkerGuard>> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let stderr_layer = fmt::layer().with_writer(io::stderr);

    let registry = tracing_subscriber::registry().with(filter).with(stderr_layer);

    match log_file {
        Some(path) => {
            if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir)?;
            }
            let file = OpenOptions::new().create(true).append(true).open(path)?;
            let (writer, guard) = tracing_appender::non_blocking(file);
            registry
                .with(fmt::layer().with_ansi(false).with_writer(writer))
                .init();
            Ok(Some(guard))
        }
        None => {
            registry.init();
            Ok(None)
        }
    }
}
