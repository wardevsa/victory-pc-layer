//! The loose-LDS fallback (fork PR #799): console wave64 programs that read and
//! write LDS inside one lane-branch region, run as two host waves of 32 that
//! meet only where such a region is entered and left.

thread_local! {
    static LDS_LOOSE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Whether the lowering runs inside [`with`].
pub(super) fn on() -> bool {
    LDS_LOOSE.get()
}

/// Runs `f` with LDS reads and writes inside one lane-branch region accepted
/// (`lower::lds_meetings`): the two host waves of a console wave meet only where
/// such a region is entered and left. APPROXIMATE: a value one half writes and
/// the other half reads inside the region may be read stale. The last resort
/// for console wave64 programs a 32-lane host would otherwise refuse
/// (`VICTORY_WAVE_LDS_LOOSE=0` turns it off).
pub(super) fn with<R>(f: impl FnOnce() -> R) -> R {
    let was = LDS_LOOSE.replace(true);
    let r = f();
    LDS_LOOSE.set(was);
    r
}
