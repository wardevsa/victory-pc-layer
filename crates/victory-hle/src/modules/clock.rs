//! The title's performance counter: the host's, or held to a steady step a
//! frame with `VICTORY_GAME_STEP_MS` (see `victory_core::game_clock`).
//!
//! The title times its frames with `QueryPerformanceCounter` (its timer
//! takes the frequency once, at +0x4daa2e0, and reads the counter at 267
//! places). Its `rdtsc` instructions are left alone: they time its own
//! profiling, not the world.
#![allow(non_snake_case)]
// Title code calls these through its import table, passing pointers the real
// API would trust.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use crate::{HleModule, hle_export};

/// `QueryPerformanceCounter`.
pub extern "system" fn QueryPerformanceCounter(out: *mut i64) -> i32 {
    if out.is_null() {
        return 0;
    }
    // SAFETY: the title passed a place for the count.
    unsafe { out.write_unaligned(victory_core::game_clock::title_now()) };
    log_once();
    1
}

/// `QueryPerformanceFrequency`: the host's, unchanged.
pub extern "system" fn QueryPerformanceFrequency(out: *mut i64) -> i32 {
    if out.is_null() {
        return 0;
    }
    // SAFETY: the title passed a place for the frequency.
    unsafe { out.write_unaligned(victory_core::game_clock::frequency()) };
    1
}

fn log_once() {
    static SAID: std::sync::Once = std::sync::Once::new();
    SAID.call_once(|| match victory_core::game_clock::step_ms() {
        Some(ms) => tracing::info!(
            "game clock: a frame counts at most {ms} ms for the title; slower frames run the game slower than real time instead of in bigger steps (VICTORY_GAME_STEP_MS)"
        ),
        None => tracing::info!("game clock: the title reads the host's counter (VICTORY_GAME_STEP_MS=33 holds its frames to 33 ms steps)"),
    });
}

pub(crate) fn module() -> HleModule {
    HleModule {
        name: "clock",
        dlls: &["api-ms-win-core-profile-*"],
        exports: vec![
            hle_export!(QueryPerformanceCounter),
            hle_export!(QueryPerformanceFrequency),
        ],
    }
}
