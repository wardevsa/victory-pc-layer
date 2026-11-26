//! One tracing setup for every binary.
//!
//! Filter with `VICTORY_LOG` (falls back to `RUST_LOG`), e.g.
//! `VICTORY_LOG=victory_loader=trace,info`. Debug and trace output exist
//! only in builds with the `diagnostics` feature; a plain release build
//! logs info and above whatever the filter asks for.
//!
//! Output is coloured only when stderr is a terminal and `NO_COLOR` is unset
//! or empty, so a log redirected to a file (`scripts\play.ps1` does) holds
//! plain text, not escape sequences.

use std::ffi::OsString;
use std::io::IsTerminal;

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
    let ansi = use_colour(
        std::io::stderr().is_terminal(),
        std::env::var_os("NO_COLOR"),
    );
    let _ = tracing_subscriber::registry()
        .with(CAP)
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_ansi(ansi)
                .with_writer(std::io::stderr),
        )
        .try_init();
}

/// Colour for a terminal, unless `NO_COLOR` is set to something
/// (https://no-color.org); never for a file or a pipe.
fn use_colour(terminal: bool, no_color: Option<OsString>) -> bool {
    terminal && no_color.is_none_or(|v| v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::use_colour;
    use std::ffi::OsString;

    #[test]
    fn colour_only_for_a_terminal() {
        assert!(use_colour(true, None));
        assert!(!use_colour(false, None));
    }

    #[test]
    fn no_color_turns_colour_off() {
        assert!(!use_colour(true, Some(OsString::from("1"))));
        assert!(!use_colour(false, Some(OsString::from("1"))));
    }

    #[test]
    fn empty_no_color_is_ignored() {
        assert!(use_colour(true, Some(OsString::new())));
    }
}
