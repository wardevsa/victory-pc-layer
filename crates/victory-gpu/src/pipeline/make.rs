//! Host pipelines are made by workers, not by the title thread that first
//! records a draw with them. Translating a shader and the host driver's
//! compile take from milliseconds to seconds each (structurisation of the
//! biggest compute programs ~0.4 s, some driver compiles 3.4 s), and
//! the title's first world frame wants over 1,200 new pipelines: made
//! on its recording threads, that frame took more than 30 s and the
//! title's GPU watchdog ended the run (decision 0007, "Pipelines made off
//! the title's threads").
//!
//! A first use queues the pipeline and waits for it within a budget per
//! frame, so the cheap ones (menus, one-off passes such as a colour-grading
//! bake) are there at once as before; past the budget a draw or dispatch
//! whose pipeline is not ready is dropped, as an untranslated one is.
//!
//! The default budget is half a second per title frame. A pass the title
//! runs only once and whose pipeline takes longer is lost. Passes over the
//! screen (no vertex input, no depth) have workers and a longer budget of
//! their own ([`screen_budget`]): the title fills its lighting tables with
//! such draws once, at the world's first frame, and behind a world's worth of
//! pipelines they were dropped; that run had no lamps and no diffuse light to
//! its end (run textures-d2, the first after a new translation; Belobug
//! 18e57db8 found the same).
//! Persistent initialization producers are started at creation and finished
//! before their first dispatch, outside that frame budget: a missed head
//! blendshape mapping leaves subsequent head skinning without its inputs.

use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use super::Shader;
use super::host::{GraphicsState, HostPipeline};

/// What a host pipeline is made from, and the host pipeline once made.
pub(crate) struct Pending {
    shaders: Vec<Arc<Shader>>,
    pub(crate) graphics: Option<Box<GraphicsState>>,
    queue_state: AtomicU8,
    /// Taken by a worker from [`Queue::slow`].
    was_slow: AtomicBool,
    made: OnceLock<Option<HostPipeline>>,
    /// A thread is making it ([`Pending::make_with`]); others wait for `made`
    /// or `failed` instead of making it too.
    building: AtomicBool,
    /// Its making ended the thread making it twice ([`reap`], [`rescue`]):
    /// `made` is then never set, and no one waits for it.
    failed: AtomicBool,
    /// Made again on a thread with [`RESCUE_STACK`] after its worker ended ([`rescue`]).
    rescued: AtomicBool,
    /// When its making began ([`Pending::make`]), for [`wait`].
    began: OnceLock<Instant>,
    /// What identifies it across runs (shader containers and state), and
    /// whether a draw or dispatch has requested it yet.
    key: String,
    used: AtomicBool,
    /// Report the first request that outlived its frame's compile budget.
    reported_pending: AtomicBool,
    /// Distinguishes work created during loading from first-use creation.
    created_at: Instant,
    /// A pass over the screen ([`GraphicsState::screen_pass`]) or a confirmed
    /// opening-scene / bake producer that must never sit behind cold RT shaders.
    screen: bool,
    /// Confirmed first-scene / bake producer prepared early even on a cold cache.
    scene_input: bool,
    /// One-time data initialization must finish before its first dispatch.
    required_initialization: bool,
    /// Why this pipeline is intentionally skipped (e.g. RT resolve without BVH).
    left_out: Option<&'static str>,
    /// The frame a draw first waited for it, plus one; 0: never waited
    /// ([`wait_again_budget`]).
    first_wait: AtomicU64,
}

fn named(shaders: &[Arc<Shader>], name: &[u8]) -> bool {
    shaders
        .iter()
        .any(|s| s.stage == "compute" && s.container.windows(name.len()).any(|w| w == name))
}

/// This producer writes head mappings retained for the run. Losing its first
/// dispatch leaves later skinning reading uninitialized data even after the
/// compiler queue has drained.
fn required_initialization(shaders: &[Arc<Shader>]) -> bool {
    named(shaders, b"CS_ComputeBlendshapeMappings")
}

fn left_out(shaders: &[Arc<Shader>]) -> Option<&'static str> {
    static SKIP_RT_RESOLVE: OnceLock<bool> = OnceLock::new();
    let skip_rt = *SKIP_RT_RESOLVE.get_or_init(|| {
        let builder = std::env::var_os("VICTORY_BVH_BUILDER").is_some_and(|v| v == "1");
        if builder {
            return false;
        }
        // Without BVH, ~1,000 CS_RaytracingResolve shaders shade no hits while
        // clogging the cold compiler queue; skip unless VICTORY_RT_RESOLVE=1.
        std::env::var_os("VICTORY_RT_RESOLVE").is_none_or(|v| v == "0")
    });
    if skip_rt && named(shaders, b"CS_RaytracingResolve") {
        return Some("shades ray-tracing hits without BVH, left out");
    }
    static SKIP_ACTIVATE_RAYS: OnceLock<bool> = OnceLock::new();
    let skip_activate = *SKIP_ACTIVATE_RAYS.get_or_init(|| {
        let builder = std::env::var_os("VICTORY_BVH_BUILDER").is_some_and(|v| v == "1");
        if builder {
            return std::env::var_os("VICTORY_ACTIVATE_RAYS").is_some_and(|v| v == "0");
        }
        std::env::var_os("VICTORY_ACTIVATE_RAYS").is_none_or(|v| v == "0")
    });
    if skip_activate && named(shaders, b"CS_ScreenProbes_ActivateRays") {
        return Some("ray activation shader without BVH, left out");
    }
    None
}

/// Confirmed first-scene producers. Prepare these at creation even on a cold
/// launch so cutscene lighting, tiling, and bakes never miss their first frame.
fn scene_input(shaders: &[Arc<Shader>]) -> bool {
    shaders.iter().any(|s| {
        matches!(
            s.hash.as_str(),
            "970c4fbb3b017c0e"
                | "068c90badc86f1fd"
                | "e4a7e088c7780ce8"
                | "ebe4e53f031f5249"
                | "2cc4c197ee29e291"
                | "369cbe906ea72764"
        ) || crate::draw::BAKE_PIXEL_SHADERS.contains(&s.hash.as_str())
    })
}

// SAFETY: D3D12 objects are free-threaded; the graphics state (whose input
// elements point into its own names) is only read after creation.
unsafe impl Send for Pending {}
// SAFETY: as above.
unsafe impl Sync for Pending {}

impl Pending {
    pub(crate) fn new(shaders: Vec<Arc<Shader>>, graphics: Option<Box<GraphicsState>>) -> Pending {
        let mut key = blake3::Hasher::new();
        for s in &shaders {
            key.update(s.stage.as_bytes());
            key.update(s.hash.as_bytes());
        }
        key.update(graphics.as_ref().map(|g| g.describe()).unwrap_or_default().as_bytes());
        let required_initialization = required_initialization(&shaders);
        let scene_input = required_initialization || scene_input(&shaders);
        let screen = (graphics.as_ref().is_some_and(|g| g.screen_pass())
            && shaders.iter().all(|s| s.stage != "mesh"))
            || scene_input;
        let key = key.finalize().to_hex()[..32].to_owned();
        let refused = left_out(&shaders).is_none() && super::library::refused(&key);
        let left_out = left_out(&shaders).or(refused.then_some("its making ended a thread in an earlier run, left out"));
        let pending = Pending {
            screen,
            scene_input,
            required_initialization,
            left_out,
            shaders,
            graphics,
            queue_state: AtomicU8::new(NOT_QUEUED),
            was_slow: AtomicBool::new(false),
            made: OnceLock::new(),
            building: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            rescued: AtomicBool::new(false),
            began: OnceLock::new(),
            key,
            used: AtomicBool::new(false),
            reported_pending: AtomicBool::new(false),
            created_at: Instant::now(),
            first_wait: AtomicU64::new(0),
        };
        if refused {
            static SAID: Mutex<Option<std::collections::HashSet<String>>> = Mutex::new(None);
            if SAID.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_default().insert(pending.key.clone()) {
                pending.say_refused();
            }
        }
        pending
    }

    /// Says, once a pipeline, that it is left out for having ended a thread.
    fn say_refused(&self) {
        tracing::info!(
            pipeline = %hashes(self),
            names = ?names(self),
            key = %self.key,
            "a pipeline left out: making it ended a thread in an earlier run (VICTORY_REFUSED_RETRY=1: tried again)"
        );
    }

    pub(super) fn key(&self) -> &str {
        &self.key
    }

    fn make(&self) {
        let _ = self.began.set(Instant::now());
        self.make_with(|| {
            let began = Instant::now();
            let made = HostPipeline::new(&self.shaders, self.graphics.as_deref());
            super::library::note_time(&self.key, began.elapsed().as_millis() as u64);
            made
        });
    }

    /// Makes it with `build` unless made, or waits for the thread making it until it is made
    /// or given up ([`reap`]). Not inside `made`'s initialisation: a thread that ends while
    /// making a pipeline (a stack overflow in vkd3d-proton under Wine ends it without
    /// unwinding) would hold that for good, and every later caller (a dispatch waiting for
    /// its compute pipeline, on the title's own thread) would wait for ever.
    fn make_with(&self, build: impl FnOnce() -> Option<HostPipeline>) {
        if self.made.get().is_none() {
            if self.building.swap(true, Ordering::AcqRel) {
                let (lock, done) = &DONE;
                let mut asked = Instant::now();
                while self.made.get().is_none() && !self.failed.load(Ordering::Acquire) {
                    {
                        let held = lock.lock().unwrap_or_else(|e| e.into_inner());
                        if self.made.get().is_some() || self.failed.load(Ordering::Acquire) {
                            break;
                        }
                        // (timed: a notification may come between the test and the wait)
                        let _ = done.wait_timeout(held, Duration::from_millis(50));
                    }
                    // Whether the thread making it ended is asked here too: with no frame
                    // budget (`VICTORY_PIPELINE_WAIT_MS=0`) `wait` never asks, and a dispatch
                    // waiting for its compute pipeline would wait for ever. Outside `DONE`,
                    // which `give_up` takes.
                    if asked.elapsed() >= Duration::from_secs(1) {
                        reap();
                        asked = Instant::now();
                    }
                }
                return;
            }
            // A failed translation must complete this queue entry and wake its waiters.
            let made = match catch_unwind(AssertUnwindSafe(build)) {
                Ok(pipeline) => pipeline,
                Err(_) => {
                    tracing::warn!(shaders = ?self.shaders, "host pipeline build panicked");
                    None
                }
            };
            let _ = self.made.set(made);
        }
        let (lock, done) = &DONE;
        let _held = lock.lock().unwrap_or_else(|e| e.into_inner());
        done.notify_all();
    }
}

/// Explain a missing host pipeline, including passes skipped before queueing.
/// A completed host pipeline has no missing reason.
#[cfg(feature = "diagnostics")]
pub(crate) fn missing(p: &Pending) -> Option<String> {
    match p.made.get() {
        Some(Some(_)) => None,
        None => Some(p.left_out.unwrap_or("being made").to_owned()),
        Some(None) => {
            let failed: Vec<String> = p
                .shaders
                .iter()
                .filter(|s| matches!(s.translated.get(), Some(None)))
                .map(|s| format!("{} {}", s.stage, s.hash))
                .collect();
            Some(if failed.is_empty() {
                format!("refused {:?}", p.shaders)
            } else {
                failed.join(" + ")
            })
        }
    }
}

/// The hashes of the shaders `p` is made of.
pub(crate) fn hashes(p: &Pending) -> String {
    let hashes: Vec<&str> = p.shaders.iter().map(|s| s.hash.as_str()).collect();
    hashes.join("+")
}

/// Signalled whenever a worker has made a pipeline.
static DONE: (Mutex<()>, Condvar) = (Mutex::new(()), Condvar::new());

/// Waiting allowed per title frame, over all threads.
fn wait_budget() -> Duration {
    static BUDGET: OnceLock<Duration> = OnceLock::new();
    *BUDGET.get_or_init(|| {
        let millis = std::env::var("VICTORY_PIPELINE_WAIT_MS")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(500);
        tracing::info!(millis, "host pipeline wait budget per frame");
        Duration::from_millis(millis)
    })
}

/// As [`wait_budget`], for passes over the screen (`VICTORY_SCREEN_WAIT_MS`,
/// 10 s; 0: like the others). Below the title's 30 s watchdog.
fn screen_budget() -> Duration {
    static BUDGET: OnceLock<Duration> = OnceLock::new();
    *BUDGET.get_or_init(|| {
        let millis = std::env::var("VICTORY_SCREEN_WAIT_MS").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(10_000);
        Duration::from_millis(millis)
    })
}

/// What a pipeline still being made may take of a frame's budget after the
/// first frame that waited for it (`VICTORY_PIPELINE_WAIT_AGAIN_MS`; unset:
/// [`wait_budget`] every frame, as before). The budget is a frame's, so one
/// 3.4 s compile cost about seven frames of half a second each; with this a
/// draw whose pipeline is not made by then is dropped sooner (it pops in
/// later) instead of holding up each frame. Not for passes over the screen.
fn wait_again_budget() -> Option<Duration> {
    static BUDGET: OnceLock<Option<Duration>> = OnceLock::new();
    *BUDGET.get_or_init(|| {
        let millis = std::env::var("VICTORY_PIPELINE_WAIT_AGAIN_MS").ok()?.trim().parse::<u64>().ok()?;
        tracing::info!(millis, "host pipeline wait budget after its first frame");
        Some(Duration::from_millis(millis))
    })
}

/// The most a pipeline may wait in frame `frame`: `full` in the first frame
/// that waited for it (`first_wait` is that frame plus one, 0 if none),
/// `again` in later ones.
fn budget_for(first_wait: u64, frame: u64, full: Duration, again: Option<Duration>) -> Duration {
    if first_wait == 0 || first_wait == frame.wrapping_add(1) {
        full
    } else {
        again.unwrap_or(full)
    }
}

/// Whether passes over the screen go before the others.
fn screens_first() -> bool {
    !screen_budget().is_zero()
}

/// Pipelines queued and not made yet.
static WAITING: AtomicU64 = AtomicU64::new(0);

/// Pipelines queued and not made yet (for the frame line).
pub(crate) fn waiting() -> u64 {
    WAITING.load(Ordering::Relaxed)
}

/// The host pipeline if it is made (or is made within the frame's budget);
/// queues it on first use.
pub(crate) fn ready(p: &Arc<Pending>) -> Option<&HostPipeline> {
    if p.left_out.is_some() || p.failed.load(Ordering::Relaxed) {
        return None;
    }
    // A still-pending pipeline was requested by the title too. Prewarm it
    // on the next launch, even if this run never got to use it.
    let first_use = !p.used.load(Ordering::Relaxed) && !p.used.swap(true, Ordering::Relaxed);
    if first_use {
        super::library::note_used(&p.key);
    }
    let age_at_request = p.made.get().is_none().then(|| p.created_at.elapsed().as_millis() as u64);
    if age_at_request.is_some() {
        start(p, REQUESTED);
        if p.required_initialization || (p.graphics.is_none() && compute_waits()) {
            // OnceLock waits for a worker already compiling this pipeline,
            // or compiles it here if workers have not taken it yet. The
            // ordinary shared frame budget cannot discard a one-time write
            // or a compute dispatch.
            p.make();
        } else {
            wait(p, crate::frame::current());
        }
    }
    if p.made.get().is_none() && !p.reported_pending.swap(true, Ordering::Relaxed) {
        tracing::info!(
            frame = crate::frame::current(),
            pipeline = %p.key,
            shaders = ?p.shaders,
            screen = p.screen,
            scene_input = p.scene_input,
            age_at_request_ms = age_at_request.unwrap_or(0),
            queued = WAITING.load(Ordering::Relaxed),
            "requested pipeline still compiling after its wait budget; its draw or dispatch cannot run"
        );
    }
    if first_use && p.required_initialization {
        tracing::info!(frame = crate::frame::current(), shaders = ?p.shaders,
            ready = p.made.get().is_some_and(|made| made.is_some()),
            "persistent initialization pipeline resolved before its first dispatch");
    }
    p.made.get()?.as_ref()
}

/// Whether a dispatch waits for its compute pipeline until it is made,
/// whatever the frame's budget ([`wait_budget`]); a draw still waits within
/// the budget and is dropped while its pipeline is not there.
///
/// A dropped draw is drawn again in the next frame. A dropped dispatch may
/// not be: the title runs some compute passes once, when a scene changes
/// (a buffer reset or cleared), and what they leave is read for as long as
/// the scene lasts. In the prison corridor a compute program met for the
/// first time took 4.4 s to make on a 16 GB machine; its one dispatch was
/// dropped after the half second of budget, and the windows of the room
/// before stayed blended over the whole picture for minutes (run 195).
/// A wait of seconds once (the pipeline library has it on the next run)
/// against a picture wrong until the scene ends.
/// On by default; `VICTORY_PIPELINE_COMPUTE_WAIT=0`: within the budget, as
/// before.
fn compute_waits() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_PIPELINE_COMPUTE_WAIT").is_none_or(|v| v != "0"))
}

/// Makes `p` on this thread if it is not made yet (waiting for a worker
/// that is making it); whether it has a host pipeline.
pub(crate) fn now(p: &Arc<Pending>) -> bool {
    if p.left_out.is_some() || p.failed.load(Ordering::Relaxed) {
        return false;
    }
    p.make();
    p.made.get().is_some_and(|made| made.is_some())
}

/// At the title's creation of a pipeline: starts it at once if an earlier
/// run drew with it (during loading, so the world's first frames find it),
/// or if it is a confirmed opening-scene / bake producer.
pub(crate) fn early(p: &Arc<Pending>) {
    if p.left_out.is_some() {
        return;
    }
    if !p.shaders.is_empty() && (p.scene_input || super::library::wanted_early(&p.key)) {
        super::library::EARLY.fetch_add(1, Ordering::Relaxed);
        start(p, if p.scene_input { REQUESTED } else { EARLY });
    }
}

/// Whether the frame's budget is one for all threads together: a thread
/// that waits takes what is left of it and gives back what it did not
/// use. Counted only when a wait had ended, the budget was one per thread:
/// each of the title's five recording threads began its first wait of a
/// frame with nothing counted yet and waited the whole half second, 2.5 s
/// of waiting in every frame in which pipelines were being made (runs
/// 34-49: "2504 ms waiting for pipelines" in each such frame).
/// On by default; `VICTORY_PIPELINE_WAIT_SHARED=0` counts as before.
fn shared_budget() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_PIPELINE_WAIT_SHARED").is_none_or(|v| v != "0"))
}

/// Waits for `p` while frame `frame` has budget left.
fn wait(p: &Pending, frame: u64) {
    let screen = p.screen && screens_first();
    let budget = if screen { screen_budget() } else { wait_budget() };
    if budget.is_zero() {
        return;
    }
    // Not for one being made longer than that already: what takes seconds
    // ends in some later frame whatever this one waits, and every frame
    // that drew with it waited the whole budget for nothing (run merged1:
    // one pipeline 91 s in the making, 500 ms of each 684 ms frame).
    // Passes over the screen keep their budget. `VICTORY_PIPELINE_WAIT_LONG=1`:
    // waited for, as before. Asked before the budget is taken, so none of
    // it is held for nothing.
    if !screen && making_long(p) {
        return;
    }
    // The frame the budget belongs to, and what was waited of it (or is
    // taken by a thread waiting now); of the passes over the screen too.
    static SPENT: [Mutex<(u64, Duration)>; 2] = [const { Mutex::new((u64::MAX, Duration::ZERO)) }; 2];
    let spent_of = &SPENT[usize::from(screen)];
    let again = if screen { None } else { wait_again_budget() };
    let shared = shared_budget();
    let left = {
        let mut spent = spent_of.lock().unwrap_or_else(|e| e.into_inner());
        if spent.0 != frame {
            *spent = (frame, Duration::ZERO);
        }
        let mut left = budget.saturating_sub(spent.1);
        // Marked only when it may wait, so one dropped without waiting has
        // its full budget the next frame. Up to five recording threads may
        // wait for it at once. Only what is taken here is given back below.
        if let Some(again) = again.filter(|_| !left.is_zero()) {
            let mine = frame.wrapping_add(1);
            let first = match p.first_wait.compare_exchange(0, mine, Ordering::Relaxed, Ordering::Relaxed) {
                Ok(_) => mine,
                Err(first) => first,
            };
            left = left.min(budget_for(first, frame, budget, Some(again)));
        }
        if shared {
            spent.1 += left;
        }
        left
    };
    if left.is_zero() {
        return;
    }
    let started = Instant::now();
    let (lock, done) = &DONE;
    let mut held = lock.lock().unwrap_or_else(|e| e.into_inner());
    while p.made.get().is_none() && !p.failed.load(Ordering::Relaxed) {
        let rest = left.saturating_sub(started.elapsed());
        if rest.is_zero() {
            break;
        }
        held = done.wait_timeout(held, rest).unwrap_or_else(|e| e.into_inner()).0;
    }
    drop(held);
    let waited = started.elapsed().min(left);
    if p.made.get().is_none() && waited >= left {
        reap();
        stalled(p, waited);
    }
    if screen && waited > Duration::from_millis(500) {
        tracing::info!(
            ms = waited.as_millis() as u64,
            made = p.made.get().is_some(),
            shaders = ?p.shaders,
            "a pass over the screen was waited for (VICTORY_SCREEN_WAIT_MS=0: dropped past half a second a frame)"
        );
    }
    {
        let mut spent = spent_of.lock().unwrap_or_else(|e| e.into_inner());
        // A frame that began meanwhile has its own budget.
        if spent.0 == frame {
            if shared {
                spent.1 = spent.1.saturating_sub(left - waited);
            } else {
                spent.1 += waited;
            }
        }
    }
    #[cfg(feature = "diagnostics")]
    crate::copystat::add(
        crate::copystat::C::PipelineWaitUs,
        started.elapsed().as_micros() as u64,
    );
}

/// Whether `p` has been in the making for longer than a second.
fn making_long(p: &Pending) -> bool {
    static WAIT_LONG: OnceLock<bool> = OnceLock::new();
    !*WAIT_LONG.get_or_init(|| std::env::var_os("VICTORY_PIPELINE_WAIT_LONG").is_some_and(|v| v == "1"))
        && p.began.get().is_some_and(|t| t.elapsed() > Duration::from_secs(1))
}

const NOT_QUEUED: u8 = 0;
const EARLY: u8 = 1;
const REQUESTED: u8 = 2;
const MAKING: u8 = 3;

/// How many workers may make pipelines at once that are known to take long
/// and that no draw has asked for: a quarter of them. On a fresh cache (a
/// change of the translator or the driver) the early list begins with
/// programs that take tens of seconds each: all 16 workers were in them
/// through the whole of the title's loading, 97 pipelines made in 20 s, and
/// the first minute and a half of the world ran at 0.5 to 1.6 s a frame,
/// dropping up to 2,900 draws a frame (run shaders-1).
/// `VICTORY_PIPELINE_SLOW=0`: in the order the title creates them, as before.
fn slow_limit() -> usize {
    static LIMIT: OnceLock<usize> = OnceLock::new();
    *LIMIT.get_or_init(|| {
        if std::env::var_os("VICTORY_PIPELINE_SLOW").is_some_and(|v| v == "0") {
            usize::MAX
        } else {
            (worker_count(None, std::thread::available_parallelism().map_or(2, |n| n.get())) / 4).max(1)
        }
    })
}

struct Queue {
    /// Requested passes over the screen: before all others, and the only ones
    /// [`SCREEN_WORKERS`] take.
    screen: VecDeque<Arc<Pending>>,
    early: VecDeque<Arc<Pending>>,
    /// Those of the early ones known to take long, and how many of them are
    /// being made ([`slow_limit`]).
    slow: VecDeque<Arc<Pending>>,
    slow_now: usize,
    requested: VecDeque<Arc<Pending>>,
}

impl Queue {
    const fn new() -> Self {
        Self {
            screen: VecDeque::new(),
            early: VecDeque::new(),
            slow: VecDeque::new(),
            slow_now: 0,
            requested: VecDeque::new(),
        }
    }

    /// Queues an unrequested pipeline or promotes one queued at creation.
    /// Returns whether it is new to the queue, or `None` if nothing changed.
    fn push(&mut self, p: &Arc<Pending>, priority: u8) -> Option<bool> {
        self.push_as(p, priority, slow_limit() != usize::MAX && super::library::slow(&p.key))
    }

    fn push_as(&mut self, p: &Arc<Pending>, priority: u8, slow: bool) -> Option<bool> {
        let prev = p.queue_state.load(Ordering::Relaxed);
        if prev >= priority {
            return None;
        }
        p.queue_state.store(priority, Ordering::Relaxed);
        if priority == REQUESTED && p.screen && screens_first() {
            self.screen.push_back(p.clone());
        } else if priority == REQUESTED {
            self.requested.push_back(p.clone());
        } else if slow {
            self.slow.push_back(p.clone());
        } else {
            self.early.push_back(p.clone());
        }
        Some(prev == NOT_QUEUED)
    }

    fn pop(&mut self, screens_only: bool, slow_limit: usize) -> Option<Arc<Pending>> {
        // An early entry can have been promoted and claimed already.
        let unclaimed = |list: &mut VecDeque<Arc<Pending>>| {
            std::iter::from_fn(|| list.pop_front()).find(|p| p.queue_state.load(Ordering::Relaxed) == EARLY)
        };
        let mut p = self.screen.pop_front();
        if p.is_none() && !screens_only {
            p = self.requested.pop_front().or_else(|| unclaimed(&mut self.early));
            if p.is_none() && self.slow_now < slow_limit {
                p = unclaimed(&mut self.slow);
                if let Some(p) = &p {
                    p.was_slow.store(true, Ordering::Relaxed);
                    self.slow_now += 1;
                }
            }
        }
        let p = p?;
        p.queue_state.store(MAKING, Ordering::Relaxed);
        Some(p)
    }
}

static QUEUE: Mutex<Queue> = Mutex::new(Queue::new());
static AVAILABLE: Condvar = Condvar::new();

/// Start or promote work for a draw without waiting behind the entire early
/// list. Queue changes and worker claims happen under the same lock.
fn start(p: &Arc<Pending>, priority: u8) {
    if p.queue_state.load(Ordering::Relaxed) >= priority {
        return;
    }
    if !workers() {
        if p.queue_state.swap(MAKING, Ordering::Relaxed) == NOT_QUEUED {
            p.make();
        }
        return;
    }
    let mut queue = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(new) = queue.push(p, priority) {
        if new {
            PEAK.fetch_max(WAITING.fetch_add(1, Ordering::Relaxed) + 1, Ordering::Relaxed);
        }
        // (One woken could be a worker that takes passes over the screen
        // alone: it would leave the pipeline to nobody.)
        AVAILABLE.notify_all();
    }
}

/// Workers that make passes over the screen alone, besides the others.
const SCREEN_WORKERS: usize = 2;

fn next(screens_only: bool) -> Arc<Pending> {
    let mut queue = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        if let Some(p) = queue.pop(screens_only, slow_limit()) {
            return p;
        }
        queue = AVAILABLE.wait(queue).unwrap_or_else(|e| e.into_inner());
    }
}

/// How many workers make pipelines: `asked` (`VICTORY_PIPELINE_WORKERS`, 1 to
/// 64), else two thirds of the logical processors, 2 to 16. They run below the
/// title's own threads, and a cold cache queues hundreds at once.
fn worker_count(asked: Option<usize>, processors: usize) -> usize {
    worker_count_for(asked, processors, false)
}

fn worker_count_for(asked: Option<usize>, processors: usize, limited_ram: bool) -> usize {
    match asked {
        Some(n) => n.clamp(1, 64),
        None if limited_ram => (processors * 3 / 4).clamp(2, 8),
        None => (processors * 2 / 3).clamp(2, 16),
    }
}

/// Starts the workers ([`worker_count`]) on first use.
fn workers() -> bool {
    static RUNNING: OnceLock<bool> = OnceLock::new();
    *RUNNING.get_or_init(|| {
        let mut running = false;
        let workers = worker_count_for(
            std::env::var("VICTORY_PIPELINE_WORKERS")
                .ok()
                .and_then(|v| v.trim().parse().ok()),
            std::thread::available_parallelism().map_or(2, |n| n.get()),
            crate::residency::limited_host_ram(),
        );
        tracing::info!(workers, "pipeline workers");
        let screens = if screens_first() { SCREEN_WORKERS } else { 0 };
        WORKERS.store(workers, Ordering::Relaxed);
        for i in 0..workers + screens {
            running |= spawn_worker(i);
        }
        running
    })
}

/// Workers that take any pipeline (the rest, up to [`SCREEN_WORKERS`] more,
/// take passes over the screen alone).
static WORKERS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Starts worker `i` (its slot in [`NOW_MAKING`]); whether it started.
fn spawn_worker(i: usize) -> bool {
    {
        {
            let screens_only = i >= WORKERS.load(Ordering::Relaxed);
            let spawned = std::thread::Builder::new()
                .name(format!("victory-pipelines-{i}"))
                .stack_size(victory_shader::emit::PIPELINE_STACK)
                .spawn(move || {
                    // Below the title's own threads (loading, recording),
                    // and the streaming one, which is below normal itself
                    // (`VICTORY_PIPELINE_LOWEST=0`: level with it, as before).
                    let lowest = !screens_only && std::env::var_os("VICTORY_PIPELINE_LOWEST").is_none_or(|v| v != "0");
                    // SAFETY: a plain call on our own thread.
                    unsafe {
                        use windows::Win32::System::Threading::*;
                        let priority = if lowest { THREAD_PRIORITY_LOWEST } else { THREAD_PRIORITY_BELOW_NORMAL };
                        let _ = SetThreadPriority(GetCurrentThread(), priority);
                    }
                    loop {
                        let p = next(screens_only);
                        // SAFETY: a plain query of this thread.
                        let tid = unsafe { windows::Win32::System::Threading::GetCurrentThreadId() };
                        *NOW_MAKING[i.min(NOW_MAKING.len() - 1)].lock().unwrap_or_else(|e| e.into_inner()) =
                            Some(Making { pipeline: p.clone(), thread: tid, since: Instant::now() });
                        p.make();
                        *NOW_MAKING[i.min(NOW_MAKING.len() - 1)].lock().unwrap_or_else(|e| e.into_inner()) = None;
                        if p.was_slow.load(Ordering::Relaxed) {
                            let mut queue = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
                            queue.slow_now -= 1;
                            AVAILABLE.notify_all();
                        }
                        if WAITING.fetch_sub(1, Ordering::Relaxed) == 1 {
                            drained();
                        } else {
                            super::library::save(false);
                        }
                    }
                });
            match spawned {
                Err(e) => {
                    tracing::warn!(error = %e, "no pipeline worker");
                    false
                }
                Ok(_) => true,
            }
        }
    }
}

/// What a worker is making, on which thread, since when.
struct Making {
    pipeline: Arc<Pending>,
    thread: u32,
    since: Instant,
}

/// What each worker is making ([`stalled`], [`reap`]).
static NOW_MAKING: [Mutex<Option<Making>>; 80] = [const { Mutex::new(None) }; 80];

/// Whether the thread with id `tid` has ended (or is no more).
fn ended(tid: u32) -> bool {
    use windows::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows::Win32::System::Threading::{GetExitCodeThread, OpenThread, THREAD_QUERY_LIMITED_INFORMATION};
    // SAFETY: plain queries of a thread of this process, its handle closed.
    unsafe {
        let Ok(thread) = OpenThread(THREAD_QUERY_LIMITED_INFORMATION, false, tid) else {
            return true;
        };
        let mut code = 0u32;
        let ok = GetExitCodeThread(thread, &mut code).is_ok();
        let _ = CloseHandle(thread);
        ok && code != STILL_ACTIVE.0 as u32
    }
}

/// The names the title's shader containers carry (`CS_..._1234abcd`), for
/// the log: printable runs of a stage prefix, an underscore and more.
fn names(p: &Pending) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for shader in &p.shaders {
        for name in super::container_names(&shader.container) {
            if !found.contains(&name) {
                found.push(name);
            }
        }
    }
    found
}

/// Pipelines whose worker ended while making them (a stack overflow in the
/// host's compiler ends a thread under Wine without an exception anyone can
/// catch): made once more on a thread with [`RESCUE_STACK`] ([`rescue`]), and the
/// worker started again. A pipeline whose second making ends its thread too is
/// failed, so draws go without it at once and no frame waits for it, and noted,
/// so no later run makes it. Before this, the pipeline stayed "being made" for
/// good, and every frame that drew with it waited the whole half second (run 6:
/// 500 ms of each 525 ms frame); then it was failed at the first death, and its
/// dispatch dropped (#39: `CS_Prop_BatchNoCull` under Proton, right before the
/// title's write to 0 at +0x23cf683).
fn reap() {
    for (i, slot) in NOW_MAKING.iter().enumerate() {
        let dead = {
            let mut slot = slot.lock().unwrap_or_else(|e| e.into_inner());
            match slot.as_ref() {
                Some(m) if m.since.elapsed() > Duration::from_secs(2) && ended(m.thread) => slot.take(),
                _ => None,
            }
        };
        let Some(m) = dead else { continue };
        let p = &m.pipeline;
        if p.was_slow.load(Ordering::Relaxed) {
            let mut queue = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
            queue.slow_now = queue.slow_now.saturating_sub(1);
        }
        let _ = WAITING.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |w| Some(w.saturating_sub(1)));
        let again = rescues() && !p.rescued.swap(true, Ordering::AcqRel) && rescue(p.clone());
        tracing::warn!(
            pipeline = %hashes(p),
            names = ?names(p),
            key = %p.key,
            thread = m.thread,
            after_ms = m.since.elapsed().as_millis() as u64,
            again,
            "a pipeline worker ended while making a pipeline (a stack overflow in the host's compiler?): \
             made again on a thread with a larger stack if `again` (VICTORY_PIPELINE_RESCUE=0: not), \
             and the worker is started again"
        );
        if !again {
            give_up(p);
        }
        spawn_worker(i);
    }
    let gone: Vec<Making> = {
        let mut rescuing = RESCUING.lock().unwrap_or_else(|e| e.into_inner());
        let (gone, alive) = std::mem::take(&mut *rescuing).into_iter().partition(|m| m.pipeline.made.get().is_some() || ended(m.thread));
        *rescuing = alive;
        gone
    };
    for m in gone {
        if m.pipeline.made.get().is_none() {
            tracing::warn!(
                pipeline = %hashes(&m.pipeline),
                names = ?names(&m.pipeline),
                key = %m.pipeline.key,
                after_ms = m.since.elapsed().as_millis() as u64,
                "making a pipeline ended its thread again, with a stack of 4 GiB: \
                 its draws and dispatches go without it, later runs leave it out (VICTORY_REFUSED_RETRY=1: tried again)"
            );
            give_up(&m.pipeline);
        }
    }
}

/// The stack of the thread that makes a pipeline again after its worker ended ([`rescue`]):
/// reserved only, its pages committed as they are used. vkd3d-proton converts DXIL
/// recursively over its control flow, and one compute program used all 256 MiB of
/// a worker's ([`victory_shader::emit::PIPELINE_STACK`]).
const RESCUE_STACK: usize = 4 << 30;

/// Pipelines being made again on a thread of their own ([`rescue`]), for [`reap`].
static RESCUING: Mutex<Vec<Making>> = Mutex::new(Vec::new());

/// Whether a pipeline whose worker ended is made again ([`rescue`]); `VICTORY_PIPELINE_RESCUE=0`: not.
fn rescues() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_PIPELINE_RESCUE").is_none_or(|v| v != "0"))
}

/// Makes `p` again on a thread with [`RESCUE_STACK`]; whether that thread started.
fn rescue(p: Arc<Pending>) -> bool {
    rescue_with(p, |p| HostPipeline::new(&p.shaders, p.graphics.as_deref()))
}

fn rescue_with(p: Arc<Pending>, build: impl FnOnce(&Pending) -> Option<HostPipeline> + Send + 'static) -> bool {
    let (tell, told) = std::sync::mpsc::channel();
    let pipeline = p.clone();
    let started = std::thread::Builder::new()
        .name("victory-pipeline-rescue".to_owned())
        .stack_size(RESCUE_STACK)
        .spawn(move || {
            // SAFETY: a plain query of this thread.
            let _ = tell.send(unsafe { windows::Win32::System::Threading::GetCurrentThreadId() });
            let made = catch_unwind(AssertUnwindSafe(|| build(&pipeline))).unwrap_or(None);
            let _ = pipeline.made.set(made);
            let (lock, done) = &DONE;
            let _held = lock.lock().unwrap_or_else(|e| e.into_inner());
            done.notify_all();
        });
    if started.is_err() {
        return false;
    }
    let Ok(thread) = told.recv() else { return false };
    RESCUING.lock().unwrap_or_else(|e| e.into_inner()).push(Making { pipeline: p, thread, since: Instant::now() });
    true
}

/// `p` is not made: its draws and dispatches go without it, no one waits for it, and no
/// later run makes it.
fn give_up(p: &Pending) {
    p.failed.store(true, Ordering::Release);
    super::library::note_refused(&p.key, &hashes(p));
    let (lock, done) = &DONE;
    let _held = lock.lock().unwrap_or_else(|e| e.into_inner());
    done.notify_all();
}

/// A wait used up what was left of the frame's budget and `p` is still not
/// made: said, at most every 2 s, with what the workers are making, so a
/// pipeline that nobody makes (or whose making does not end) is named.
fn stalled(p: &Pending, waited: Duration) {
    static LAST: Mutex<Option<Instant>> = Mutex::new(None);
    {
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        if last.is_some_and(|t| t.elapsed() < Duration::from_secs(2)) {
            return;
        }
        *last = Some(Instant::now());
    }
    let workers: Vec<String> = NOW_MAKING
        .iter()
        .enumerate()
        .filter_map(|(i, w)| {
            let w = w.lock().unwrap_or_else(|e| e.into_inner());
            w.as_ref().map(|m| {
                format!("{i}: {} on thread {} for {} ms", hashes(&m.pipeline), m.thread, m.since.elapsed().as_millis())
            })
        })
        .collect();
    let queued = {
        let q = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
        format!(
            "screen {}, requested {}, early {}, slow {} ({} of them being made)",
            q.screen.len(),
            q.requested.len(),
            q.early.len(),
            q.slow.len(),
            q.slow_now
        )
    };
    tracing::warn!(
        pipeline = %hashes(p),
        key = %p.key,
        screen = p.screen,
        queue_state = p.queue_state.load(Ordering::Relaxed),
        waited_ms = waited.as_millis() as u64,
        waiting = WAITING.load(Ordering::Relaxed),
        %queued,
        making = ?workers,
        "a pipeline waited for the whole of the frame's budget and is not made"
    );
}

/// The most pipelines waiting at once since the workers last ran out.
static PEAK: AtomicU64 = AtomicU64::new(0);

/// The workers ran out of work: logged after a batch of 10 or more, and
/// the cache saved (at most every 20 s).
fn drained() {
    use super::library::{EARLY, LOADED, MADE};
    if PEAK.swap(0, Ordering::Relaxed) >= 10 {
        tracing::info!(
            made = MADE.load(Ordering::Relaxed),
            loaded = LOADED.load(Ordering::Relaxed),
            started_early = EARLY.load(Ordering::Relaxed),
            "host pipelines all made"
        );
    }
    super::library::save(false);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialization_producers_are_started_early_without_classifying_recurring_skinning() {
        let pending = |stage: &'static str, text: &[u8]| {
            // Real containers identify their stage. Keep the synthetic
            // containers distinct too: the shader factory deduplicates bytes.
            let data = [stage.as_bytes(), b":", text].concat();
            Pending::new(vec![super::super::shader(stage, &data)], None)
        };
        let name = b"CS_ComputeBlendshapeMappings";
        let p = pending("compute", name);
        assert!(p.required_initialization && p.scene_input && p.screen);
        assert!(!pending("pixel", name).required_initialization);
        assert!(!pending("compute", b"CS_PedStandardSkinning_0").required_initialization);
    }

    #[cfg(feature = "diagnostics")]
    #[test]
    fn an_intentionally_skipped_pipeline_reports_its_reason() {
        let mut pending = Pending::new(Vec::new(), None);
        assert_eq!(missing(&pending).as_deref(), Some("being made"));
        pending.left_out = Some("sample pass disabled");
        let pending = Arc::new(pending);
        assert!(ready(&pending).is_none());
        assert_eq!(pending.queue_state.load(Ordering::Relaxed), NOT_QUEUED);
        assert_eq!(missing(&pending).as_deref(), pending.left_out);
    }

    #[test]
    fn first_initialization_dispatch_waits_for_the_compiler_past_the_frame_budget() {
        let mut p = Pending::new(Vec::new(), None);
        p.required_initialization = true;
        // A worker has already claimed this pipeline, but its driver compile
        // has not finished. No real GPU or proprietary shader is needed.
        p.queue_state.store(MAKING, Ordering::Relaxed);
        let p = Arc::new(p);
        let entered = std::sync::Barrier::new(2);
        let (release, gate) = std::sync::mpsc::channel();
        let (returned, result) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let worker = p.clone();
            let worker_entered = &entered;
            scope.spawn(move || {
                worker.make_with(|| {
                    worker_entered.wait();
                    gate.recv().unwrap();
                    None
                });
            });
            entered.wait();
            scope.spawn(|| returned.send(ready(&p).is_some()).unwrap());
            // The default frame budget is 500 ms. The dispatch must still
            // be waiting after that budget, rather than returning pending.
            let early = result.recv_timeout(Duration::from_millis(650));
            release.send(()).unwrap();
            assert!(matches!(early, Err(std::sync::mpsc::RecvTimeoutError::Timeout)));
            assert!(!result.recv_timeout(Duration::from_secs(10)).unwrap());
        });
        assert!(p.made.get().is_some());
        assert!(!p.reported_pending.load(Ordering::Relaxed));
    }

    #[test]
    fn worker_count_follows_the_processors_and_the_switch() {
        assert_eq!(worker_count(None, 1), 2);
        assert_eq!(worker_count(None, 24), 16);
        assert_eq!(worker_count(None, 64), 16);
        assert_eq!(worker_count(Some(0), 24), 1);
        assert_eq!(worker_count(Some(20), 24), 20);
        assert_eq!(worker_count_for(None, 8, true), 6);
        assert_eq!(worker_count_for(None, 16, true), 8);
        assert_eq!(worker_count_for(None, 24, true), 8);
        assert_eq!(worker_count_for(Some(12), 8, true), 12);
    }

    /// A thread that ends while making a pipeline without unwinding (ExitThread: what a
    /// stack overflow in vkd3d-proton does under Wine) leaves it "being made". A second
    /// caller waits instead of making it too or waiting for ever; [`rescue_with`] makes it
    /// and wakes it, and a rescue that ends its thread too has [`reap`] give it up.
    #[test]
    fn a_pipeline_whose_maker_ended_is_made_again_or_given_up() {
        fn ends_its_thread(p: Arc<Pending>) {
            std::thread::spawn(move || {
                p.make_with(|| {
                    // SAFETY: ends this test thread, which holds nothing but `p`'s claim.
                    unsafe { windows::Win32::System::Threading::ExitThread(0) }
                })
            });
        }
        let waits_for = |p: &Arc<Pending>| {
            let (tell, told) = std::sync::mpsc::channel();
            let q = p.clone();
            std::thread::spawn(move || {
                q.make_with(|| panic!("the second caller must not make it too"));
                let _ = tell.send(q.made.get().is_some());
            });
            told
        };
        for rescue_ends_too in [false, true] {
            let mut p = Pending::new(Vec::new(), None);
            p.key = format!("rescue-test-{rescue_ends_too}");
            let p = Arc::new(p);
            ends_its_thread(p.clone());
            let started = Instant::now();
            while !p.building.load(Ordering::Acquire) && started.elapsed() < Duration::from_secs(10) {
                std::thread::yield_now();
            }
            let told = waits_for(&p);
            assert!(told.recv_timeout(Duration::from_millis(300)).is_err(), "the second caller waits");
            assert!(rescue_with(p.clone(), move |_| {
                if rescue_ends_too {
                    // SAFETY: as above, the rescue thread.
                    unsafe { windows::Win32::System::Threading::ExitThread(0) }
                }
                None
            }));
            if rescue_ends_too {
                let started = Instant::now();
                while !p.failed.load(Ordering::Acquire) && started.elapsed() < Duration::from_secs(10) {
                    reap();
                    std::thread::sleep(Duration::from_millis(20));
                }
                assert!(p.failed.load(Ordering::Acquire), "given up once the rescue ended too");
                assert_eq!(told.recv_timeout(Duration::from_secs(10)), Ok(false));
            } else {
                assert_eq!(told.recv_timeout(Duration::from_secs(10)), Ok(true), "made by the rescue");
                assert!(!p.failed.load(Ordering::Acquire));
            }
        }
    }

    #[test]
    fn a_panicking_pipeline_is_refused_and_the_worker_continues() {
        let bad = Arc::new(Pending::new(Vec::new(), None));
        let good = Arc::new(Pending::new(Vec::new(), None));
        let failed = bad.clone();
        let next = good.clone();
        let worker = std::thread::spawn(move || {
            failed.make_with(|| panic!("synthetic pipeline build panic"));
            next.make_with(|| None);
        });

        worker.join().expect("the pipeline worker should survive");
        assert!(matches!(bad.made.get(), Some(None)));
        assert!(matches!(good.made.get(), Some(None)));

        bad.make_with(|| panic!("the refused pipeline should stay complete"));
    }

    /// A first use queues the pipeline and, within the budget, gets the
    /// worker's answer (here: no host pipeline for no shaders).
    #[test]
    fn first_use_is_made_by_a_worker() {
        let p = Arc::new(Pending::new(Vec::new(), None));
        assert!(ready(&p).is_none());
        assert_ne!(p.queue_state.load(Ordering::Relaxed), NOT_QUEUED);
        let started = Instant::now();
        while p.made.get().is_none() && started.elapsed() < Duration::from_secs(10) {
            std::thread::yield_now();
        }
        assert!(matches!(p.made.get(), Some(None)));
    }

    #[test]
    fn pending_pipeline_is_recorded_for_the_next_launch() {
        let mut p = Pending::new(Vec::new(), None);
        p.key = "pending-pipeline-test".to_owned();
        // Simulate a worker that has taken the job but cannot finish it yet.
        p.queue_state.store(MAKING, Ordering::Relaxed);
        let p = Arc::new(p);

        assert!(!super::super::library::wanted_early(&p.key));
        // A compute pipeline is made at once when dispatches wait for theirs
        // (#1312, the default; it has no shaders here, so nothing comes of
        // it); either way it is noted for the next launch.
        assert!(ready(&p).is_none());
        assert!(super::super::library::wanted_early(&p.key));
    }

    #[test]
    fn draws_promote_early_pipelines_without_claiming_them_twice() {
        let earlier = Arc::new(Pending::new(Vec::new(), None));
        let promoted = Arc::new(Pending::new(Vec::new(), None));
        let requested = Arc::new(Pending::new(Vec::new(), None));
        let mut queue = Queue::new();

        assert_eq!(queue.push(&earlier, EARLY), Some(true));
        assert_eq!(queue.push(&promoted, EARLY), Some(true));
        assert_eq!(queue.push(&requested, REQUESTED), Some(true));
        assert_eq!(queue.push(&promoted, REQUESTED), Some(false));
        assert_eq!(queue.push(&promoted, REQUESTED), None);

        assert!(Arc::ptr_eq(&queue.pop(false, 1).unwrap(), &requested));
        assert!(Arc::ptr_eq(&queue.pop(false, 1).unwrap(), &promoted));
        assert_eq!(queue.push(&promoted, REQUESTED), None);
        assert!(Arc::ptr_eq(&queue.pop(false, 1).unwrap(), &earlier));
        assert!(queue.pop(false, 1).is_none());
    }

    #[test]
    fn pipelines_known_to_take_long_leave_workers_for_the_others() {
        let new = || Arc::new(Pending::new(Vec::new(), None));
        let (long, longer, asked, quick) = (new(), new(), new(), new());
        let mut queue = Queue::new();
        for p in [&long, &longer, &asked] {
            assert_eq!(queue.push_as(p, EARLY, true), Some(true));
        }
        assert_eq!(queue.push_as(&quick, EARLY, false), Some(true));
        // The quick one first, then one long one at a time; one a draw asks
        // for waits for nobody.
        assert!(Arc::ptr_eq(&queue.pop(false, 1).unwrap(), &quick));
        assert!(Arc::ptr_eq(&queue.pop(false, 1).unwrap(), &long));
        assert!(queue.pop(false, 1).is_none());
        assert_eq!(queue.push_as(&asked, REQUESTED, true), Some(false));
        assert!(Arc::ptr_eq(&queue.pop(false, 1).unwrap(), &asked));
        assert!(!asked.was_slow.load(Ordering::Relaxed));
        assert!(queue.pop(false, 1).is_none());
        queue.slow_now -= 1;
        assert!(Arc::ptr_eq(&queue.pop(false, 1).unwrap(), &longer));
        assert!(queue.pop(false, 1).is_none());
    }

    /// A pipeline still being made waits the full budget in the first frame
    /// that waits for it, and `VICTORY_PIPELINE_WAIT_AGAIN_MS` in later ones.
    #[test]
    fn later_frames_wait_the_again_budget() {
        let full = Duration::from_millis(500);
        let again = Some(Duration::from_millis(8));
        assert_eq!(budget_for(0, 7, full, again), full);
        assert_eq!(budget_for(8, 7, full, again), full);
        assert_eq!(budget_for(8, 9, full, again), Duration::from_millis(8));
        assert_eq!(budget_for(8, 9, full, None), full);
    }

    /// Passes over the screen go before all others, and workers of their own
    /// take nothing else.
    #[test]
    fn passes_over_the_screen_go_first() {
        let of = |screen| {
            let mut p = Pending::new(Vec::new(), None);
            p.screen = screen;
            Arc::new(p)
        };
        let (world, screen) = (of(false), of(true));
        let mut queue = Queue::new();
        assert_eq!(queue.push(&world, REQUESTED), Some(true));
        assert_eq!(queue.push(&screen, REQUESTED), Some(true));
        assert!(Arc::ptr_eq(&queue.pop(true, 1).unwrap(), &screen));
        assert!(queue.pop(true, 1).is_none());
        assert!(Arc::ptr_eq(&queue.pop(false, 1).unwrap(), &world));
    }
}
