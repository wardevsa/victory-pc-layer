//! Title memory by address, for translated shaders that read and write
//! through raw 40-bit addresses (`victory_shader::emit::SLOT_MEMORY`: the
//! console's ray-tracing structure builder and the programs that walk its
//! structures; the structures themselves lie in plain title memory, in no
//! buffer object).
//!
//! The copies are the ones every other access uses ([`crate::shadow`]: a
//! title buffer's copy, or the 4 MiB window of memory outside buffers), so
//! a root view and a raw address reach the same host bytes. This module
//! keeps what a shader needs to find them:
//!
//! - the **directory**: per 64 KiB page of title address space the number
//!   of the copy that holds the page (0: none) and the page's byte offset
//!   in it; one buffer for all lists, entries only ever go from empty to
//!   filled;
//! - per view heap a run of slots with a raw unordered access view of each
//!   numbered copy (slot 0 of the run: a null view, for pages without a
//!   copy);
//! - the **faults**: the page numbers shaders found empty, read back here
//!   once a frame, so the copies exist from the next frame on. A pointer a
//!   shader takes out of memory cannot be known before it runs; the
//!   addresses a dispatch is given as root views are made ready before it.
//!
//! **A page that is not there when a shader touches it is a lost access**:
//! the read gives 0, the store is dropped, and the fault note only helps
//! the next frame. For the structure builder that is the same as a dropped
//! stage, so before a dispatch the host makes ready everything it can see
//! the dispatch name: the whole copy around each root view, around each
//! pair of root constants that is a title address and around each root
//! constant that is one in 256-byte units (the builder passes structures
//! all three ways), and the 4 MiB after each (a structure may cross a
//! window's end).
//! shortcut: what a stage reaches only through pointers it reads out of
//! memory (InitHeaders' table of structure addresses, LoadInstances'
//! instance records) is not made ready unless something else named it;
//! the fault count in the log says whether that happens. The whole answer
//! is the extent of the title's allocation around an address, which the
//! memory module of the layer knows and this one does not ask yet.
//!
//! shortcut: a copy that is numbered is kept for the rest of the run, and
//! there are at most [`COPIES`] of them (4 MiB windows mostly: 4 GiB at
//! most). Every dispatch of such a shader brings all numbered copies up to
//! date and, if it can store, declares all of them written (the bytes are
//! compared on the CPU afterwards, as for any unordered access view): a
//! dirty-page mask written by the shader would bound both.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use victory_shader::emit::{MEMORY_FAULTS, MEMORY_PAGES};
use windows::Win32::Graphics::Direct3D12::*;

use crate::descriptor::host::HostHeap;
use crate::host::{HostList, host};
use crate::shadow::Shadow;

/// Copies that can be numbered (the run of heap slots is one longer). A
/// listed frame of the opening cutscene names about 50 windows; with the
/// structure builder running the prologue used all 1023 within a second
/// and then left about 40 pages a frame reading zeros.
const COPIES: u32 = 1023;

/// [`COPIES`], or `VICTORY_MEMORY_COPIES` (up to 16383). Each copy is
/// kept for the run, mostly a 4 MiB window: 1023 hold up to 4 GiB.
fn copies_limit() -> u32 {
    static N: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *N.get_or_init(|| {
        std::env::var("VICTORY_MEMORY_COPIES")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .map_or(COPIES, |n| n.clamp(1, 16383))
    })
}

/// What [`Memory::map`] did with a page.
#[derive(Clone, Copy)]
enum Mapped {
    /// It got a copy.
    Made,
    /// It had one already.
    Had,
    /// Past the directory's reach.
    Past,
    /// Not memory the title has.
    Refused,
    /// No title buffer or window copy holds the whole page.
    NoCopy,
    /// Its copy is one shaders cannot write.
    ReadOnly,
    /// Every number is taken.
    Full,
}
/// A window of memory outside buffers (`shadow.rs`).
const CELL: u64 = 4 << 20;
/// A directory page.
const PAGE: u64 = 1 << 16;

/// Bytes of the directory brought up to its card copy at a time.
const CHUNK: u64 = 4096;

/// Whether shaders read the directory from a copy in the card's memory
/// (`VICTORY_DIRECTORY_CARD=0`: from the PC's memory, as before).
///
/// The directory is the CPU's (it fills entries as pages get copies) and
/// lived in memory the card reads over the bus: every load or store a
/// shader makes by address looks its page up there first, so every step of
/// every ray crossed the bus twice, once for the entry and once for the
/// node (VIctory-7900XTX found the same: "every step of every ray waited
/// for it"). With the structures' pages kept the card took about 310 ms a
/// frame here and read 12.6 GB of shared memory (2026-11-26). Entries only
/// ever go from empty to filled, and few are made once a scene is in, so
/// the card's copy takes the 4 KiB pieces that changed, before the next
/// shader that reads it.
fn directory_on_card() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_DIRECTORY_CARD").is_none_or(|v| v != "0"))
}

struct Memory {
    /// Buffer, its mapping, its GPU address.
    directory: (ID3D12Resource, *mut u32, u64),
    /// The directory's copy in the card's memory and its GPU address
    /// ([`directory_on_card`]), and per [`CHUNK`] of the directory whether
    /// it changed since it last went up (a bit each).
    card: Option<(ID3D12Resource, u64)>,
    dirty: Vec<u64>,
    any_dirty: bool,
    faults: (ID3D12Resource, *mut u32, u64),
    /// Copy `n` is `copies[n - 1]`.
    copies: Vec<Arc<Shadow>>,
    /// Per host view heap (by address): the first slot of its run, and how
    /// many copies have their view there.
    heaps: HashMap<usize, (u32, u32)>,
    /// The frame the faults were last read in.
    frame: u64,
    /// Pages asked for this frame that are not the title's memory.
    refused: std::collections::HashSet<u64>,
}

// SAFETY: the mappings are of buffers the struct owns; all access is under
// the mutex.
unsafe impl Send for Memory {}

static MEMORY: Mutex<Option<Memory>> = Mutex::new(None);

/// What a draw binds.
pub(crate) struct Ready {
    pub(crate) directory: u64,
    pub(crate) faults: u64,
    /// The view heap slot of the null view; copy `n`'s view is `n` further.
    pub(crate) base: u32,
}

impl Memory {
    fn new() -> Option<Memory> {
        let host = host()?;
        let make = |dwords: u64, what: &str| {
            let (buffer, data) = host.shared_buffer(dwords * 4)?;
            crate::host::name(&buffer, || format!("title memory by address: {what}"));
            // SAFETY: a fresh mapping of at least that many bytes.
            unsafe { std::ptr::write_bytes(data, 0, (dwords * 4) as usize) };
            // SAFETY: a live buffer.
            let gpu = unsafe { buffer.GetGPUVirtualAddress() };
            Some((buffer, data.cast::<u32>(), gpu))
        };
        tracing::info!("title memory by address: a shader reads or writes through raw addresses");
        let bytes = u64::from(MEMORY_PAGES) * 2 * 4;
        // Zeroed at creation, as the CPU's directory is.
        let card = directory_on_card()
            .then(|| host.uav_buffer(bytes))
            .flatten()
            .map(|buffer| {
                crate::host::name(&buffer, || "title memory by address: directory on the card".to_owned());
                // SAFETY: a live buffer.
                let gpu = unsafe { buffer.GetGPUVirtualAddress() };
                (buffer, gpu)
            });
        tracing::info!(
            on_card = card.is_some(),
            "title memory by address: the directory shaders look pages up in (VICTORY_DIRECTORY_CARD=0: the PC's memory)"
        );
        Some(Memory {
            directory: make(u64::from(MEMORY_PAGES) * 2, "directory")?,
            card,
            dirty: vec![0; (bytes / CHUNK).div_ceil(64) as usize],
            any_dirty: false,
            faults: make(u64::from(MEMORY_FAULTS), "faults")?,
            copies: Vec::new(),
            heaps: HashMap::new(),
            frame: u64::MAX,
            refused: Default::default(),
        })
    }

    /// The directory entry of `page` changed: its piece goes up to the
    /// card's copy before the next shader that reads it.
    fn touch(&mut self, page: u64) {
        let chunk = (page * 8 / CHUNK) as usize;
        self.dirty[chunk / 64] |= 1 << (chunk % 64);
        self.any_dirty = true;
    }

    /// Brings the card's copy of the directory up to date in `list`, ahead
    /// of the shader about to be recorded, and leaves it readable by
    /// shaders. `first` is whether this is the list's first shader that
    /// reads it: the buffer then starts the list in the common state (a
    /// buffer goes back to it at the end of every submission). The GPU
    /// address shaders read the directory at.
    fn directory_for(&mut self, list: &HostList, first: bool) -> u64 {
        let Some((card, gpu)) = self.card.clone() else {
            return self.directory.2;
        };
        let readable = D3D12_RESOURCE_STATES(
            D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE.0 | D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE.0,
        );
        let l = &list.list;
        if self.any_dirty {
            if !first {
                // SAFETY: an open list; the buffer is readable in it so far.
                unsafe { l.ResourceBarrier(&[crate::host::transition(&card, readable, D3D12_RESOURCE_STATE_COPY_DEST)]) };
            }
            let mut pieces = 0u32;
            for (word, bits) in self.dirty.iter_mut().enumerate() {
                let mut left = std::mem::take(bits);
                while left != 0 {
                    let chunk = word as u64 * 64 + u64::from(left.trailing_zeros());
                    left &= left - 1;
                    // SAFETY: an open list; both buffers hold the piece.
                    unsafe { l.CopyBufferRegion(&card, chunk * CHUNK, &self.directory.0, chunk * CHUNK, CHUNK) };
                    pieces += 1;
                }
            }
            self.any_dirty = false;
            DIRECTORY_PIECES.fetch_add(u64::from(pieces), std::sync::atomic::Ordering::Relaxed);
            // SAFETY: as above; the copies are recorded.
            unsafe { l.ResourceBarrier(&[crate::host::transition(&card, D3D12_RESOURCE_STATE_COPY_DEST, readable)]) };
            list.keep(&self.directory.0);
        } else if first {
            // SAFETY: an open list; the buffer starts it in the common state.
            unsafe { l.ResourceBarrier(&[crate::host::transition(&card, D3D12_RESOURCE_STATE_COMMON, readable)]) };
        }
        if first {
            list.keep(&card);
        }
        gpu
    }

    /// Gives the page of `address` its copy, if it has none and the memory
    /// is the title's.
    fn map(&mut self, address: u64) -> Mapped {
        let page = address / PAGE;
        // The last entry stays empty: addresses past the directory read it.
        if page >= u64::from(MEMORY_PAGES) - 1 {
            return Mapped::Past;
        }
        // SAFETY: inside the directory's mapping.
        let entry = unsafe { self.directory.1.add(page as usize * 2) };
        // SAFETY: as above.
        if unsafe { entry.read() } != 0 {
            return Mapped::Had;
        }
        let start = page * PAGE;
        // A shader's pointer, or a constant taken for an address, may be
        // anything: only memory the title has.
        if self.refused.contains(&page) {
            return Mapped::Refused;
        }
        if !crate::texture::committed(start as *const u8, PAGE as usize) {
            self.refused.insert(page);
            return Mapped::Refused;
        }
        // A new window's copy lives in the PC's memory the card shares: past
        // the system's budget for it the card is lost (see `shared_full`).
        if crate::shadow::find_existing(start).is_none() && shared_full() {
            return Mapped::Full;
        }
        // The copy a root view at that address would get; a buffer that
        // ends inside the page is passed over for what holds all of it.
        let whole = |s: &Arc<Shadow>| s.offset_in(start).is_some_and(|_| s.left(start) >= PAGE);
        let Some(shadow) = crate::shadow::find(start, 1)
            .filter(whole)
            .or_else(|| crate::shadow::find(start, PAGE).filter(whole))
        else {
            return Mapped::NoCopy;
        };
        // SAFETY: a live buffer.
        let flags = unsafe { shadow.buffer.GetDesc() }.Flags;
        if flags.0 & D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS.0 == 0 {
            return Mapped::ReadOnly;
        }
        let n = match self.copies.iter().position(|c| Arc::ptr_eq(c, &shadow)) {
            Some(n) => n,
            None if self.copies.len() < copies_limit() as usize => {
                self.copies.push(shadow.clone());
                let n = self.copies.len() - 1;
                // Every page the copy holds whole, not only the one asked
                // for: a structure is more than 64 KiB.
                // shortcut: a page of a title buffer inside this one that
                // has a copy of its own is given this copy too.
                let (first, len) = shadow.span();
                let mut at = first.next_multiple_of(PAGE);
                while at + PAGE <= first + len && at / PAGE < u64::from(MEMORY_PAGES) - 1 {
                    // SAFETY: inside the directory's mapping.
                    let other = unsafe { self.directory.1.add((at / PAGE) as usize * 2) };
                    // SAFETY: as above; the offset first, as below.
                    let filled = unsafe {
                        if other.read() == 0 {
                            other.add(1).write_volatile((at - first) as u32);
                            other.write_volatile(n as u32 + 1);
                            true
                        } else {
                            false
                        }
                    };
                    if filled {
                        self.touch(at / PAGE);
                    }
                    at += PAGE;
                }
                n
            }
            None => {
                static ONCE: std::sync::Once = std::sync::Once::new();
                ONCE.call_once(|| {
                    tracing::warn!(
                        copies = copies_limit(),
                        "title memory by address: no number left for another copy; its pages read zeros (VICTORY_MEMORY_COPIES raises the limit)"
                    )
                });
                return Mapped::Full;
            }
        };
        let offset = shadow.offset_in(start).unwrap_or(0);
        // The offset first: a shader that sees the number sees it too.
        // SAFETY: inside the directory's mapping.
        unsafe {
            entry.add(1).write_volatile(offset as u32);
            entry.write_volatile(n as u32 + 1);
        }
        self.touch(page);
        Mapped::Made
    }

    /// The pages shaders found empty since the last frame.
    fn take_faults(&mut self) {
        let mut pages = Vec::new();
        for i in 1..MEMORY_FAULTS as usize {
            // SAFETY: inside the faults' mapping; the GPU may be writing,
            // a number missed now is written again next frame.
            let at = unsafe { self.faults.1.add(i) };
            // SAFETY: as above.
            let page = unsafe { at.read_volatile() };
            if page != 0 {
                pages.push(page);
                // SAFETY: as above.
                unsafe { at.write_volatile(0) };
            }
        }
        if pages.is_empty() {
            return;
        }
        let first = u64::from(pages[0]) * PAGE;
        // What became of each: [made, had one meanwhile, past the
        // directory, not the title's memory, in no copy, in a copy shaders
        // cannot write, no number left].
        let mut seen = [0u32; 7];
        for &page in &pages {
            seen[self.map(u64::from(page) * PAGE) as usize] += 1;
        }
        // Each is an access that read 0 or was dropped.
        static SAID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        if SAID.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 50 {
            let bytes: u64 = self.copies.iter().map(|c| c.span().1).sum();
            tracing::warn!(
                pages = pages.len(),
                first = format_args!("{first:#x}"),
                made = seen[Mapped::Made as usize],
                had = seen[Mapped::Had as usize],
                past = seen[Mapped::Past as usize],
                not_title = seen[Mapped::Refused as usize],
                no_copy = seen[Mapped::NoCopy as usize],
                read_only = seen[Mapped::ReadOnly as usize],
                no_number = seen[Mapped::Full as usize],
                copies = self.copies.len(),
                copies_mb = bytes >> 20,
                "title memory by address: shaders touched pages that had no copy (those accesses read 0 or were dropped)"
            );
        }
    }

    /// What a dispatch names: the copy around `address` and the 4 MiB
    /// after it.
    fn name(&mut self, address: u64) {
        let _ = self.map(address);
        let _ = self.map((address & !(CELL - 1)) + CELL);
    }
}

/// Whether the PC's memory the card shares is used past the most new
/// copies of title memory by address may take it to: that share of the
/// system's budget for it (`VICTORY_SHARED_MOST`, in hundredths: 90). Asked
/// of the system at most every 100 ms.
///
/// With the structures' pages kept and up to 16,383 copies, a timing run
/// had 15,550 MB in use of a budget of 15,294 MB when the card was lost
/// (0x887A0005, 2026-11-26), and another ended in the driver while a
/// submission was prepared. The copies are made when a page is first asked
/// for; past this share a new page is refused (read as zeros, its light
/// missing) rather than the card lost.
fn shared_full() -> bool {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    static MOST: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    static LAST: AtomicU64 = AtomicU64::new(0);
    static FULL: AtomicBool = AtomicBool::new(false);
    let most = *MOST.get_or_init(|| {
        std::env::var("VICTORY_SHARED_MOST").ok().and_then(|v| v.trim().parse::<u64>().ok()).filter(|&n| n > 0 && n <= 100).unwrap_or(90)
    });
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    let last = LAST.load(Ordering::Relaxed);
    if now >= last + 100 && LAST.compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed).is_ok() {
        let full = host()
            .and_then(|h| h.video_memory())
            .is_some_and(|[_, _, budget, used]| budget > 0 && used > budget / 100 * most);
        if full && !FULL.load(Ordering::Relaxed) {
            static SAID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            if SAID.fetch_add(1, Ordering::Relaxed) < 20 {
                tracing::warn!(
                    most,
                    "title memory by address: the PC's memory shared with the card is past that share of its budget: no new copies (VICTORY_SHARED_MOST)"
                );
            }
        }
        FULL.store(full, Ordering::Relaxed);
    }
    FULL.load(Ordering::Relaxed)
}

/// `VICTORY_BVH_KEEP=1` (with `VICTORY_BVH_QUARANTINE=2`): the copies of
/// windows of title memory outside buffers are taken once and then left to
/// the card. What the structure builder stores in them by address stays in
/// them (quarantined), but every list that read title memory by address
/// brought each copy's written pages up from title memory again, whole,
/// over what the builder had built there: the interiors stayed black, and
/// about 500 MB a frame went up (2026-11-26). VIctory-7900XTX keeps the
/// structures' pages on the card the same way (only the dwords the title
/// changed go up there). Title buffers' copies (the lists' upload memory
/// with the instances among them) are brought up to date as before.
/// shortcut: a structure streamed in later into a page already taken is
/// not seen.
fn keep_windows() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_BVH_KEEP").is_some_and(|v| v == "1"))
}

/// Before a draw whose shader reads title memory by address: what its
/// root arguments name gets its copies (`addresses`: its root views;
/// `constants`: its root constants, see the module), every numbered copy
/// has its view in `heap`'s run and is brought up to date when `list` is
/// submitted (and, with `writes`, copied back after). None if the host
/// cannot hold it; the draw is then dropped.
pub(crate) fn ready(
    list: &HostList,
    heap: &HostHeap,
    addresses: &[u64],
    constants: &[u32],
    writes: bool,
) -> Option<Ready> {
    let mut guard = MEMORY.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        *guard = Memory::new();
    }
    let m = guard.as_mut()?;
    let frame = crate::frame::current();
    if m.frame != frame {
        m.frame = frame;
        m.refused.clear();
        m.take_faults();
    }
    let numbered = m.copies.len();
    for &address in addresses {
        m.name(address);
    }
    // A title address in two constants (low dword, then bits 32 to 39), or
    // in one in 256-byte units. A constant that only looks like one names
    // memory nothing reads; one that is not the title's memory is refused.
    for (i, &c) in constants.iter().enumerate() {
        if let Some(&high) = constants.get(i + 1)
            && (1..=0xFF).contains(&high)
            && c >= 0x1_0000
        {
            m.name(u64::from(c) | u64::from(high) << 32);
        }
        if c >= 0x0100_0000 {
            m.name(u64::from(c) << 8);
        }
    }
    let key = std::ptr::from_ref(heap) as usize;
    let (base, made) = match m.heaps.get(&key) {
        Some(&run) => run,
        None => {
            // Held for the heap's life: not given back with a list's slots.
            let base = heap.dynamic(copies_limit() + 1)?;
            heap.null_raw_uav_at(base);
            (base, 0)
        }
    };
    for (n, shadow) in m.copies.iter().enumerate().skip(made as usize) {
        let (_, len) = shadow.span();
        heap.raw_uav_at(base + 1 + n as u32, &shadow.buffer, 0, len);
    }
    m.heaps.insert(key, (base, m.copies.len() as u32));
    // Once per list, and for copies numbered since: the builder alone
    // dispatches hundreds of times a frame.
    // shortcut: a copy another list numbers meanwhile is brought up to date
    // by that list's submission, which may run after this one's.
    const READ: usize = usize::MAX - 0x6d;
    const WRITE: usize = usize::MAX - 0x6e;
    let first_read = list.first_use(READ);
    let first_write = writes && list.first_use(WRITE);
    let keep = keep_windows();
    for (n, shadow) in m.copies.iter().enumerate() {
        let (start, len) = shadow.span();
        // `VICTORY_BVH_KEEP=1`: a window of title memory outside buffers
        // (where the structures lie) is taken once, when numbered, and not
        // brought up to date from title memory again (see `keep_windows`).
        if keep && shadow.is_window() && n < numbered {
            continue;
        }
        if first_read || n >= numbered {
            shadow.refresh(list, start, len);
        }
        if writes && (first_write || n >= numbered) {
            shadow.gpu_writes(list, start, len);
        }
    }
    if first_read {
        list.keep(&m.directory.0);
        list.keep(&m.faults.0);
    }
    let directory = m.directory_for(list, first_read);
    Some(Ready {
        directory,
        faults: m.faults.2,
        base,
    })
}

/// 4 KiB pieces of the directory sent up to its card copy since the start
/// ([`directory_on_card`]).
static DIRECTORY_PIECES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// For the frame summary: pieces of the directory sent up so far.
pub(crate) fn directory_pieces() -> u64 {
    DIRECTORY_PIECES.load(std::sync::atomic::Ordering::Relaxed)
}
