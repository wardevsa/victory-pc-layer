//! What chosen programs write, as the title sees it
//! (`VICTORY_WATCH_PROGRAMS=<name>,<name>`).
//!
//! For a dispatch of a program whose name starts with one of the given
//! names, the buffers it writes (unordered views through tables or root
//! views) are noted once each. Every three seconds of the wall: how often
//! each program was dispatched, and for each buffer, read from title memory
//! where the title reads them, how many of its first 64 KB are not zero,
//! whether they changed since the last look, and the values from the first
//! word that is not zero, as floats. A buffer whose values stay
//! zero, or never change while the scene moves, is one the title's CPU
//! never gets the card's results in: the boat's water heights
//! (`CS_ResolveHeightLookUps`, `CS_BatchReadback`) are the first use.

use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// Most buffers watched in all.
const MOST: usize = 24;
/// Bytes looked at per buffer.
const LOOK: usize = 65536;

#[derive(Debug)]
struct Watched {
    program: String,
    slot: String,
    address: u64,
    size: u64,
    last: Option<u64>,
}

/// A watched program's dispatches since the last look.
#[derive(Debug, Default)]
struct Dispatches {
    program: String,
    direct: u64,
    empty: u64,
    indirect: u64,
    largest: [u32; 3],
}

struct State {
    watched: Vec<Watched>,
    dispatches: Vec<Dispatches>,
    next: Instant,
}

fn names() -> &'static [String] {
    static NAMES: OnceLock<Vec<String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        std::env::var("VICTORY_WATCH_PROGRAMS")
            .map(|v| {
                v.split(',')
                    .map(|n| n.trim().to_owned())
                    .filter(|n| !n.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    })
}

fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(|| {
        Mutex::new(State {
            watched: Vec::new(),
            dispatches: Vec::new(),
            next: Instant::now() + Duration::from_secs(3),
        })
    })
}

/// Whether programs are watched at all.
pub(crate) fn on() -> bool {
    !names().is_empty()
}

/// The watched name `program` starts with, if any.
pub(crate) fn watched(program: &str) -> bool {
    names().iter().any(|n| program.starts_with(n.as_str()))
}

/// A dispatch of `program`: its group counts, none if indirect.
pub(crate) fn dispatched(program: &str, groups: Option<[u32; 3]>) {
    let mut s = state().lock().unwrap_or_else(|e| e.into_inner());
    let at = match s.dispatches.iter().position(|d| d.program == program) {
        Some(at) => at,
        None => {
            s.dispatches.push(Dispatches {
                program: program.to_owned(),
                ..Dispatches::default()
            });
            s.dispatches.len() - 1
        }
    };
    let d = &mut s.dispatches[at];
    match groups {
        None => d.indirect += 1,
        Some(g) if g.contains(&0) => d.empty += 1,
        Some(g) => {
            d.direct += 1;
            if g.iter().product::<u32>() > d.largest.iter().product::<u32>() {
                d.largest = g;
            }
        }
    }
}

/// A buffer `program` writes, at `address` for `size` bytes, through `slot`.
pub(crate) fn note(program: &str, slot: String, address: u64, size: u64) {
    if address == 0 {
        return;
    }
    let mut s = state().lock().unwrap_or_else(|e| e.into_inner());
    if s.watched.len() >= MOST
        || s.watched
            .iter()
            .any(|w| w.address == address && w.program == program)
    {
        return;
    }
    tracing::info!(
        program,
        slot,
        address = format_args!("{address:#x}"),
        size,
        "watched program writes a buffer (VICTORY_WATCH_PROGRAMS)"
    );
    s.watched.push(Watched {
        program: program.to_owned(),
        slot,
        address,
        size,
        last: None,
    });
}

/// Bytes of this process at `address`, or none where it cannot be read.
fn read(address: u64, len: usize) -> Option<Vec<u8>> {
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut bytes = vec![0u8; len];
    let mut got = 0usize;
    // SAFETY: the system checks the range; a page that cannot be read
    // fails the call instead of faulting.
    let ok = unsafe {
        ReadProcessMemory(
            GetCurrentProcess(),
            address as *const _,
            bytes.as_mut_ptr().cast(),
            len,
            &mut got,
        )
    };
    (ok != 0 && got > 0).then(|| {
        bytes.truncate(got);
        bytes
    })
}

/// A present: every three seconds, what the watched buffers hold.
pub(crate) fn sample() {
    if !on() {
        return;
    }
    let mut s = state().lock().unwrap_or_else(|e| e.into_inner());
    let now = Instant::now();
    if now < s.next {
        return;
    }
    s.next = now + Duration::from_secs(3);
    for d in &mut s.dispatches {
        tracing::info!(
            program = %d.program,
            "watched program dispatched since the last look: {} direct (largest {:?} groups), {} with no groups, {} indirect",
            d.direct,
            d.largest,
            d.empty,
            d.indirect
        );
        *d = Dispatches {
            program: std::mem::take(&mut d.program),
            ..Dispatches::default()
        };
    }
    for w in &mut s.watched {
        let len = LOOK.min(w.size as usize).max(4);
        let Some(bytes) = read(w.address, len) else {
            tracing::info!(program = %w.program, address = format_args!("{:#x}", w.address), "watched buffer: cannot be read");
            continue;
        };
        let words: Vec<u32> = bytes
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        let nonzero = words.iter().filter(|&&w| w != 0).count();
        // The first values from the first word that is not zero (on a
        // 16-byte boundary), so values far into the buffer show too.
        let from = words.iter().position(|&w| w != 0).unwrap_or(0) / 4 * 4;
        let hash = words.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &x| {
            (h ^ u64::from(x)).wrapping_mul(0x100_0000_01b3)
        });
        let changed = w
            .last
            .map_or("first look", |l| if l == hash { "same" } else { "changed" });
        w.last = Some(hash);
        let first: Vec<String> = words
            .iter()
            .skip(from)
            .take(12)
            .map(|&x| {
                let f = f32::from_bits(x);
                if f.is_finite() && (f == 0.0 || (1e-4..1e7).contains(&f.abs())) {
                    format!("{f:.3}")
                } else {
                    format!("{x:#x}")
                }
            })
            .collect();
        tracing::info!(
            program = %w.program,
            slot = %w.slot,
            address = format_args!("{:#x}", w.address),
            "watched buffer: {nonzero}/{} words not zero, {changed}, from byte {}: [{}]",
            words.len(),
            from * 4,
            first.join(", ")
        );
    }
}
