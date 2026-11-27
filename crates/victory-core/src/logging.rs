//! One tracing setup for every binary.
//!
//! Filter with `VICTORY_LOG` (falls back to `RUST_LOG`), e.g.
//! `VICTORY_LOG=victory_loader=trace,info`. Debug and trace output exist
//! only in builds with the `diagnostics` feature; a plain release build
//! logs info and above whatever the filter asks for.
//!
//! `VICTORY_LOG_FILE=<path>` writes the log to that file instead of stderr:
//! under Proton the title's stderr reaches no terminal.

use tracing_subscriber::EnvFilter;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

/// The most verbose level this build can log.
const CAP: LevelFilter = if cfg!(feature = "diagnostics") {
    LevelFilter::TRACE
} else {
    LevelFilter::INFO
};

pub fn init(default_level: &str) {
    let filter = std::env::var("VICTORY_LOG")
        .or_else(|_| std::env::var("RUST_LOG"))
        .ok()
        .and_then(|v| EnvFilter::try_new(v).ok())
        .unwrap_or_else(|| EnvFilter::new(default_level));
    let file = std::env::var_os("VICTORY_LOG_FILE").and_then(|path| match std::fs::File::create(&path) {
        Ok(f) => Some(f),
        Err(e) => {
            eprintln!("VIctory: VICTORY_LOG_FILE={}: {e}; logging to stderr", path.to_string_lossy());
            None
        }
    });
    let layer = tracing_subscriber::fmt::layer().with_target(true);
    let registry = tracing_subscriber::registry().with(CAP).with(filter);
    let _ = match file {
        Some(f) => registry.with(layer.with_ansi(false).with_writer(std::sync::Mutex::new(f))).try_init(),
        None => registry.with(layer.with_writer(std::io::stderr)).try_init(),
    };
}
