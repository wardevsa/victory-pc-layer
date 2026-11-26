//! The machine's load, looked at once a second on a thread of its own, for
//! the box in the corner of the picture (`crate::fpsbox`): how busy the
//! processors and the card are and how much memory is left, so that whoever
//! watches the picture sees which of them holds the frame.
//!
//! - Processors: the share of all logical processors that was busy, system
//!   wide (`GetSystemTimes`); the processor time of this process, in
//!   processors (`GetProcessTimes`: 5.2 means it kept 5.2 of them busy);
//!   and its busiest thread, in per cent of one processor
//!   (`GetThreadTimes`). A thread near 100 % with the card far from it is a
//!   frame held by that thread.
//! - The card: Windows' own counters (`\GPU Engine(*)\Utilization
//!   Percentage`, the ones the Task Manager shows), which name the process
//!   and the engine of every figure. For the whole card and for this
//!   process: the busiest engine's sum, as the Task Manager does it. They
//!   work for every maker of cards and load no library of the maker's.
//! - Memory: this process's committed bytes and the PC's free physical
//!   memory. (The card's memory is asked of the host by the box itself.)
//!
//! Nothing here touches the title or the card. `VICTORY_FPS_COUNTER=0` or
//! `=fps` never starts the thread.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// One look.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Stats {
    /// Per cent of all logical processors busy, system wide.
    pub(crate) cpu_all: f32,
    /// This process's processor time, in processors.
    pub(crate) cores: f32,
    /// This process's busiest thread, in per cent of one processor.
    pub(crate) top_thread: f32,
    /// The card's busiest engine, per cent, all processes; none if the
    /// system's counters did not answer.
    pub(crate) gpu_all: Option<f32>,
    /// The same for this process alone.
    pub(crate) gpu_ours: Option<f32>,
    /// This process's committed memory, MB.
    pub(crate) ram_ours_mb: u32,
    /// The PC's free physical memory, MB.
    pub(crate) ram_free_mb: u32,
}

/// The last look, and its number (1 for the first).
static NOW: Mutex<(u64, Option<Stats>)> = Mutex::new((0, None));

/// The last look and its number; none before the first.
pub(crate) fn now() -> Option<(u64, Stats)> {
    let now = NOW.lock().unwrap_or_else(|e| e.into_inner());
    now.1.map(|s| (now.0, s))
}

/// Starts the looks, once.
pub(crate) fn start() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let spawned = std::thread::Builder::new().name("victory-stats".into()).spawn(run);
        if let Err(e) = spawned {
            tracing::warn!(%e, "the load figures of the corner box have no thread: the box shows the frame rate only");
        }
    });
}

/// A 100 ns count from the two halves Windows gives.
fn ticks(t: &windows_sys::Win32::Foundation::FILETIME) -> u64 {
    u64::from(t.dwHighDateTime) << 32 | u64::from(t.dwLowDateTime)
}

/// System wide: (idle, busy and idle together), in 100 ns of processor time.
fn system_times() -> Option<(u64, u64)> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::GetSystemTimes;
    let (mut idle, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
    // SAFETY: three structures the call fills.
    if unsafe { GetSystemTimes(&mut idle, &mut kernel, &mut user) } == 0 {
        return None;
    }
    // The kernel's time has the idle time in it.
    Some((ticks(&idle), ticks(&kernel) + ticks(&user)))
}

/// This process's processor time, in 100 ns.
fn process_time() -> u64 {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let mut t = [FILETIME::default(); 4];
    let [created, exited, kernel, user] = &mut t;
    // SAFETY: our own process and four structures the call fills.
    if unsafe { GetProcessTimes(GetCurrentProcess(), created, exited, kernel, user) } == 0 {
        return 0;
    }
    ticks(&t[2]) + ticks(&t[3])
}

/// The processor time of each thread of this process, in 100 ns, by thread
/// id. (The snapshot lists every thread of the system: a few thousand
/// entries, about a millisecond.)
fn thread_times() -> HashMap<u32, u64> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcessId, GetThreadTimes, OpenThread, THREAD_QUERY_LIMITED_INFORMATION};
    let mut out = HashMap::new();
    // SAFETY: a snapshot handle of our own, closed below; sized structures
    // the calls fill; thread handles opened and closed here.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snapshot == INVALID_HANDLE_VALUE || snapshot.is_null() {
            return out;
        }
        let pid = GetCurrentProcessId();
        let mut entry = THREADENTRY32 { dwSize: size_of::<THREADENTRY32>() as u32, ..Default::default() };
        let mut more = Thread32First(snapshot, &mut entry) != 0;
        while more {
            if entry.th32OwnerProcessID == pid {
                let thread = OpenThread(THREAD_QUERY_LIMITED_INFORMATION, 0, entry.th32ThreadID);
                if !thread.is_null() {
                    let mut t = [FILETIME::default(); 4];
                    let [created, exited, kernel, user] = &mut t;
                    if GetThreadTimes(thread, created, exited, kernel, user) != 0 {
                        out.insert(entry.th32ThreadID, ticks(&t[2]) + ticks(&t[3]));
                    }
                    CloseHandle(thread);
                }
            }
            more = Thread32Next(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
    }
    out
}

/// This process's committed memory and the PC's free physical memory, MB.
fn memory() -> (u32, u32) {
    use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut c = PROCESS_MEMORY_COUNTERS { cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32, ..Default::default() };
    // SAFETY: plain data the call fills in.
    let mut m: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    m.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
    // SAFETY: our own process and sized structures.
    unsafe {
        K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb);
        GlobalMemoryStatusEx(&mut m);
    }
    ((c.PagefileUsage >> 20) as u32, (m.ullAvailPhys >> 20) as u32)
}

/// Windows' counters of the card's engines: one figure for each process
/// and engine.
struct Engines {
    query: windows_sys::Win32::System::Performance::PDH_HQUERY,
    counter: windows_sys::Win32::System::Performance::PDH_HCOUNTER,
    /// The answer's memory, kept from look to look (8-byte units: the
    /// items hold pointers and doubles).
    buffer: Vec<u64>,
}

/// One figure of [`Engines`]: the process, the engine's kind ("3D", "Copy",
/// "Compute_0", ...) and its per cent.
#[derive(Debug, PartialEq)]
struct Engine {
    pid: u32,
    kind: String,
    percent: f64,
}

/// What an instance name of the counters says: the process and the kind of
/// engine ("pid_1234_luid_0x0_0x1_phys_0_eng_3_engtype_3D").
fn parse_instance(name: &str) -> Option<(u32, &str)> {
    let rest = name.strip_prefix("pid_")?;
    let pid = rest.split('_').next()?.parse().ok()?;
    let kind = name.split("engtype_").nth(1)?;
    Some((pid, kind))
}

/// The busiest kind of engine: every kind's figures summed, the highest
/// sum, 100 at most. For the whole card (`pid` none) or one process.
fn busiest(engines: &[Engine], pid: Option<u32>) -> (f32, HashMap<&str, f64>) {
    let mut by_kind: HashMap<&str, f64> = HashMap::new();
    for e in engines.iter().filter(|e| pid.is_none_or(|p| p == e.pid)) {
        *by_kind.entry(e.kind.as_str()).or_default() += e.percent;
    }
    let top = by_kind.values().copied().fold(0.0, f64::max).min(100.0);
    (top as f32, by_kind)
}

impl Engines {
    fn open() -> Option<Engines> {
        use windows_sys::Win32::System::Performance::{PdhAddEnglishCounterW, PdhCollectQueryData, PdhOpenQueryW};
        let path: Vec<u16> = "\\GPU Engine(*)\\Utilization Percentage".encode_utf16().chain(std::iter::once(0)).collect();
        let (mut query, mut counter) = (std::ptr::null_mut(), std::ptr::null_mut());
        // SAFETY: plain calls with a terminated string and places for the
        // two handles, kept for the life of the process.
        unsafe {
            if PdhOpenQueryW(std::ptr::null(), 0, &mut query) != 0 {
                return None;
            }
            if PdhAddEnglishCounterW(query, path.as_ptr(), 0, &mut counter) != 0 {
                return None;
            }
            // The first collection has nothing to compare with: a figure
            // comes from the second on.
            PdhCollectQueryData(query);
        }
        Some(Engines { query, counter, buffer: Vec::new() })
    }

    /// The figures since the last call.
    fn look(&mut self) -> Option<Vec<Engine>> {
        use windows_sys::Win32::System::Performance::{
            PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_MORE_DATA, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
        };
        // SAFETY: our own handles; the buffer is as long as the size given,
        // and the items and the names they point at lie inside it until the
        // next call.
        unsafe {
            if PdhCollectQueryData(self.query) != 0 {
                return None;
            }
            let (mut bytes, mut count) = (0u32, 0u32);
            let status = PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut bytes, &mut count, std::ptr::null_mut());
            if status != PDH_MORE_DATA || bytes == 0 {
                return None;
            }
            // Instances come and go between the two calls: some room over.
            bytes += 16 << 10;
            self.buffer.resize((bytes as usize).div_ceil(8), 0);
            let items = self.buffer.as_mut_ptr().cast::<PDH_FMT_COUNTERVALUE_ITEM_W>();
            if PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut bytes, &mut count, items) != 0 {
                return None;
            }
            let mut out = Vec::with_capacity(count as usize);
            for item in std::slice::from_raw_parts(items, count as usize) {
                if item.szName.is_null() || item.FmtValue.CStatus != 0 {
                    continue;
                }
                let len = (0..512).take_while(|&i| *item.szName.add(i) != 0).count();
                let name = String::from_utf16_lossy(std::slice::from_raw_parts(item.szName, len));
                if let Some((pid, kind)) = parse_instance(&name) {
                    out.push(Engine { pid, kind: kind.to_owned(), percent: item.FmtValue.Anonymous.doubleValue });
                }
            }
            Some(out)
        }
    }
}

/// The thread: a look a second, for as long as the process runs.
fn run() {
    let mut engines = Engines::open();
    if engines.is_none() {
        tracing::info!("the system's counters of the card's engines did not open: the corner box shows no card load");
    }
    let processors = std::thread::available_parallelism().map_or(1, |n| n.get()) as f64;
    // SAFETY: a plain call.
    let pid = unsafe { windows_sys::Win32::System::Threading::GetCurrentProcessId() };
    let mut last = (Instant::now(), system_times(), process_time(), thread_times());
    let mut looks = 0u64;
    loop {
        std::thread::sleep(Duration::from_secs(1));
        let now = (Instant::now(), system_times(), process_time(), thread_times());
        // 100 ns of wall time since the last look.
        let wall = (now.0 - last.0).as_secs_f64() * 1e7;
        if wall <= 0.0 {
            continue;
        }
        let cpu_all = match (last.1, now.1) {
            (Some((idle0, all0)), Some((idle1, all1))) if all1 > all0 => {
                100.0 * (1.0 - (idle1.saturating_sub(idle0)) as f64 / (all1 - all0) as f64)
            }
            _ => 0.0,
        };
        let cores = now.2.saturating_sub(last.2) as f64 / wall;
        let top_thread = now
            .3
            .iter()
            .map(|(id, &t)| t.saturating_sub(last.3.get(id).copied().unwrap_or(t)))
            .max()
            .unwrap_or(0) as f64
            / wall
            * 100.0;
        let looked = engines.as_mut().and_then(Engines::look);
        let (gpu_all, gpu_ours) = match &looked {
            Some(e) => (Some(busiest(e, None).0), Some(busiest(e, Some(pid)).0)),
            None => (None, None),
        };
        let (ram_ours_mb, ram_free_mb) = memory();
        let stats = Stats {
            cpu_all: cpu_all.clamp(0.0, 100.0) as f32,
            cores: cores.clamp(0.0, processors) as f32,
            top_thread: top_thread.clamp(0.0, 100.0) as f32,
            gpu_all,
            gpu_ours,
            ram_ours_mb,
            ram_free_mb,
        };
        looks += 1;
        *NOW.lock().unwrap_or_else(|e| e.into_inner()) = (looks, Some(stats));
        // In the log every fifth look, with the card's engines by kind: the
        // box has room for one figure only.
        if looks.is_multiple_of(5) {
            let kinds = |pid: Option<u32>| {
                let Some(e) = &looked else { return "not known".to_owned() };
                let mut kinds: Vec<(&str, f64)> = busiest(e, pid).1.into_iter().filter(|(_, p)| *p >= 0.5).collect();
                kinds.sort_by(|a, b| b.1.total_cmp(&a.1));
                if kinds.is_empty() {
                    return "nothing at work".to_owned();
                }
                kinds.iter().map(|(k, p)| format!("{k} {p:.0} %")).collect::<Vec<_>>().join(", ")
            };
            tracing::info!(
                "the machine's load over the last second (the corner box's figures): processors {:.0} % busy, all programs; this \
                 process kept {:.1} of {processors:.0} processors busy, its busiest thread {:.0} % of one; the card's engines, all \
                 programs: {}; this process's: {}; this process has {} MB committed, the PC {} MB of memory free",
                stats.cpu_all,
                stats.cores,
                stats.top_thread,
                kinds(None),
                kinds(Some(pid)),
                stats.ram_ours_mb,
                stats.ram_free_mb,
            );
        }
        last = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_instance_name_says_the_process_and_the_engine() {
        assert_eq!(parse_instance("pid_1234_luid_0x00000000_0x0000E3B5_phys_0_eng_0_engtype_3D"), Some((1234, "3D")));
        assert_eq!(parse_instance("pid_8_luid_0x0_0x1_phys_0_eng_5_engtype_Compute_0"), Some((8, "Compute_0")));
        assert_eq!(parse_instance("_Total"), None);
        assert_eq!(parse_instance("pid_x_engtype_3D"), None);
    }

    #[test]
    fn the_cards_load_is_its_busiest_kind_of_engine() {
        let e = |pid, kind: &str, percent| Engine { pid, kind: kind.to_owned(), percent };
        let engines = [e(1, "3D", 30.0), e(2, "3D", 25.0), e(1, "Copy", 40.0), e(2, "VideoEncode", 70.0), e(1, "3D", 5.0)];
        // All programs: 3D 60, Copy 40, VideoEncode 70.
        assert_eq!(busiest(&engines, None).0, 70.0);
        // Process 1: 3D 35, Copy 40.
        assert_eq!(busiest(&engines, Some(1)).0, 40.0);
        assert_eq!(busiest(&engines, Some(9)).0, 0.0);
        // Sums over 100 (several engines of a kind) show as 100.
        assert_eq!(busiest(&[e(1, "3D", 80.0), e(1, "3D", 60.0)], None).0, 100.0);
    }

    /// The looks that need no card: they answer on any machine.
    #[test]
    fn the_processors_and_the_memory_answer() {
        let (idle, all) = system_times().expect("system times");
        assert!(all >= idle && all > 0);
        // Some work, so that this thread has time to show.
        let mut x = 0u64;
        for i in 0..20_000_000u64 {
            x = x.wrapping_mul(31).wrapping_add(i);
        }
        std::hint::black_box(x);
        assert!(process_time() > 0);
        let threads = thread_times();
        assert!(!threads.is_empty(), "this process has threads");
        let (ours, free) = memory();
        assert!(ours > 0 && free > 0);
    }
}
