use std::path::PathBuf;

use tracing_subscriber::{fmt, prelude::*, EnvFilter};

fn log_dir() -> PathBuf {
    dirs_home().join("log")
}

fn dirs_home() -> PathBuf {
    home_dir().join(".reclaude")
}

fn home_dir() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

/// Initialize tracing with JSON file output and stderr fallback.
///
/// Logs go to `~/.reclaude/log/reclaude.jsonl` (appended).
/// RUST_LOG env var controls filter level (default: info).
pub fn init() {
    let log_dir = log_dir();
    let _ = std::fs::create_dir_all(&log_dir);

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("reclaude=info"));

    let file_appender = tracing_appender::rolling::never(&log_dir, "reclaude.jsonl");

    tracing_subscriber::registry()
        .with(filter)
        .with(
            fmt::layer()
                .json()
                .with_writer(file_appender)
                .with_target(false)
                .with_thread_ids(false),
        )
        .init();
}

/// Path to the JSON log file.
pub fn log_file_path() -> PathBuf {
    log_dir().join("reclaude.jsonl")
}
