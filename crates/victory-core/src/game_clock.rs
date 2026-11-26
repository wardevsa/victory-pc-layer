//! The title's clock, held to a steady step a frame (`VICTORY_GAME_STEP_MS`).
//!
//! The title measures a frame by its performance counter and moves the
//! world by what it measured. At 3-4 frames a second that is 250-300 ms a
//! step: a boat at speed goes through a shore between two steps, the people
//! on it are thrown off, and a frame of input moves the wheel a quarter of a
//! second. With `VICTORY_GAME_STEP_MS=33` the counter the title reads runs
//! slower while frames are slow, so that a frame counts about 33 ms for the
//! title whatever it took on the wall: the world moves in the steps it was
//! made for, and the game runs slower than real time (at 3.5 frames a
//! second about 12 % of it) rather than in jumps. Frames already shorter
//! than the step count as they are.
//!
//! The counter stays continuous and never goes back: at each present the
//! rate is set for the next frame from the frames before it, starting from
//! where the counter stands. A frame much longer than the ones before it
//! (a stall) counts two steps at full rate and its remainder at an eighth,
//! never nothing, so a title that waits on the counter still sees it move.
//! The rate never drops below 1/32. Before the first present the counter
//! is the host's.

use std::sync::Mutex;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};

/// The lowest rate the title's counter runs at, against the wall.
const LOWEST_RATE: f64 = 1.0 / 32.0;

/// A frame's step past this many steps counts an eighth.
const STEPS_AT_FULL_RATE: f64 = 2.0;

/// What one frame was: its length on the wall and for the title, in
/// counter ticks, and the rate set for the next.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Step {
    pub wall: i64,
    pub title: i64,
    pub rate: f64,
}

/// A counter that maps the host's ticks to the title's.
#[derive(Debug)]
pub struct Clock {
    /// Ticks a frame counts at most for the title.
    step: i64,
    /// Even when the three below are whole, odd while they are written.
    seq: AtomicU64,
    /// Host ticks at the last present (0: none yet, the counter is the
    /// host's).
    wall_at: AtomicI64,
    /// The title's ticks at the last present.
    title_at: AtomicI64,
    /// `f64` bits: title ticks per host tick since the last present.
    rate: AtomicU64,
    /// The most the counter has said: it never says less.
    said: AtomicI64,
    /// For [`Clock::frame`] only: the frame before (its host and title
    /// ticks at its present, and the smoothed length of frames).
    frames: Mutex<Frames>,
}

#[derive(Debug, Default)]
struct Frames {
    wall: i64,
    title: i64,
    smoothed: f64,
}

impl Clock {
    /// A clock whose frames count at most `step` ticks for the title.
    pub fn new(step: i64) -> Self {
        Self {
            step: step.max(1),
            seq: AtomicU64::new(0),
            wall_at: AtomicI64::new(0),
            title_at: AtomicI64::new(0),
            rate: AtomicU64::new(1f64.to_bits()),
            said: AtomicI64::new(i64::MIN),
            frames: Mutex::new(Frames::default()),
        }
    }

    /// The title's ticks at host ticks `wall`, from the rate in force.
    fn at(&self, wall: i64) -> i64 {
        let (wall_at, title_at, rate) = loop {
            let before = self.seq.load(Ordering::Acquire);
            if before & 1 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let read = (
                self.wall_at.load(Ordering::Acquire),
                self.title_at.load(Ordering::Acquire),
                f64::from_bits(self.rate.load(Ordering::Acquire)),
            );
            if self.seq.load(Ordering::Acquire) == before {
                break read;
            }
        };
        if wall_at == 0 {
            return wall;
        }
        let mut gone = (wall - wall_at).max(0) as f64 * rate;
        let full = STEPS_AT_FULL_RATE * self.step as f64;
        if gone > full {
            gone = full + (gone - full) / 8.0;
        }
        title_at + gone as i64
    }

    /// The title's counter at host ticks `wall`: never less than it said
    /// before.
    pub fn now(&self, wall: i64) -> i64 {
        let title = self.at(wall);
        let before = self.said.fetch_max(title, Ordering::AcqRel);
        title.max(before)
    }

    /// A present at host ticks `wall`: sets the rate for the next frame.
    pub fn frame(&self, wall: i64) -> Step {
        let mut frames = self.frames.lock().unwrap_or_else(|e| e.into_inner());
        let title = self.now(wall);
        let first = frames.wall == 0;
        let length = if first {
            self.step
        } else {
            (wall - frames.wall).max(1)
        };
        frames.smoothed = if first || frames.smoothed <= 0.0 {
            length as f64
        } else {
            0.5 * frames.smoothed + 0.5 * length as f64
        };
        let rate = (self.step as f64 / frames.smoothed).clamp(LOWEST_RATE, 1.0);
        let step = Step {
            wall: if first { 0 } else { length },
            title: if first { 0 } else { title - frames.title },
            rate,
        };
        self.seq.fetch_add(1, Ordering::AcqRel);
        self.wall_at.store(wall.max(1), Ordering::Release);
        self.title_at.store(title, Ordering::Release);
        self.rate.store(rate.to_bits(), Ordering::Release);
        self.seq.fetch_add(1, Ordering::AcqRel);
        frames.wall = wall;
        frames.title = title;
        step
    }
}

/// The step a frame counts at most, in milliseconds, from
/// `VICTORY_GAME_STEP_MS` (unset, `0` or `off`: none, the title reads the
/// host's counter).
pub fn step_ms() -> Option<f64> {
    static STEP: OnceLock<Option<f64>> = OnceLock::new();
    *STEP.get_or_init(|| {
        let text = std::env::var("VICTORY_GAME_STEP_MS").ok()?;
        let ms = text.trim().parse::<f64>().ok()?;
        (ms.is_finite() && ms >= 1.0).then_some(ms.min(1000.0))
    })
}

#[cfg(windows)]
mod host {
    use super::{Clock, Step, step_ms};
    use std::sync::OnceLock;
    use windows_sys::Win32::System::Performance::{
        QueryPerformanceCounter, QueryPerformanceFrequency,
    };

    fn ticks() -> i64 {
        let mut now = 0i64;
        // SAFETY: a valid out pointer.
        unsafe { QueryPerformanceCounter(&mut now) };
        now
    }

    /// The host's counter frequency.
    pub fn frequency() -> i64 {
        static FREQUENCY: OnceLock<i64> = OnceLock::new();
        *FREQUENCY.get_or_init(|| {
            let mut f = 0i64;
            // SAFETY: a valid out pointer.
            unsafe { QueryPerformanceFrequency(&mut f) };
            f.max(1)
        })
    }

    fn clock() -> Option<&'static Clock> {
        static CLOCK: OnceLock<Option<Clock>> = OnceLock::new();
        CLOCK
            .get_or_init(|| {
                step_ms().map(|ms| Clock::new((ms * frequency() as f64 / 1000.0) as i64))
            })
            .as_ref()
    }

    /// What the title's `QueryPerformanceCounter` says.
    pub fn title_now() -> i64 {
        let wall = ticks();
        clock().map_or(wall, |c| c.now(wall))
    }

    /// A present: the frame it ends, if the clock is held.
    pub fn frame() -> Option<Step> {
        clock().map(|c| c.frame(ticks()))
    }
}

#[cfg(windows)]
pub use host::{frame, frequency, title_now};

#[cfg(test)]
mod tests {
    use super::*;

    /// 1 tick = 1 ms, a step of 33.
    fn clock() -> Clock {
        Clock::new(33)
    }

    #[test]
    fn before_the_first_present_the_counter_is_the_hosts() {
        let c = clock();
        assert_eq!(c.now(1000), 1000);
        assert_eq!(c.now(1500), 1500);
    }

    #[test]
    fn slow_frames_count_a_step_each() {
        let c = clock();
        c.frame(1000);
        let mut wall = 1000;
        let mut last = Step {
            wall: 0,
            title: 0,
            rate: 1.0,
        };
        for _ in 0..10 {
            wall += 300;
            last = c.frame(wall);
        }
        assert_eq!(last.wall, 300);
        assert!((last.title - 33).abs() <= 1, "{last:?}");
        assert!((last.rate - 0.11).abs() < 0.01, "{last:?}");
    }

    #[test]
    fn fast_frames_count_as_they_are() {
        let c = clock();
        c.frame(1000);
        let mut wall = 1000;
        let mut last = None;
        for _ in 0..10 {
            wall += 20;
            last = Some(c.frame(wall));
        }
        let last = last.unwrap();
        assert_eq!(last.title, 20);
        assert_eq!(last.rate, 1.0);
    }

    #[test]
    fn the_counter_is_continuous_and_never_goes_back() {
        let c = clock();
        c.frame(1000);
        let mut wall = 1000;
        let mut said = c.now(wall);
        for length in [300, 300, 20, 20, 2000, 300, 16, 300] {
            for i in 1..=length {
                let now = c.now(wall + i);
                assert!(now >= said);
                said = now;
            }
            wall += length;
            let before = c.now(wall);
            c.frame(wall);
            assert_eq!(c.now(wall), before);
        }
    }

    #[test]
    fn a_stall_still_moves_the_counter() {
        let c = clock();
        c.frame(1000);
        c.frame(1300);
        let at = c.now(1300);
        let late = c.now(1300 + 60_000);
        assert!(late - at > 66, "{at} {late}");
    }

    #[test]
    fn the_rate_has_a_floor() {
        let c = clock();
        c.frame(1000);
        let step = c.frame(1000 + 100_000);
        assert_eq!(step.rate, LOWEST_RATE);
    }
}
