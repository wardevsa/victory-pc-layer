//! Host copies of textures the title fills, placed in heaps of the card's
//! memory that hold many of them (`VICTORY_TEXTURE_HEAPS=0`: each made
//! with memory of its own, as before).
//!
//! Why: in the opening cutscene's first minutes the title streams in
//! thousands of textures a minute, and each host copy was a committed
//! resource: an allocation of the graphics kernel's
//! (`NtGdiDdDDICreateAllocation`) and a wait for it to be made resident on
//! the card (`NtGdiDdDDIWaitForSynchronizationObjectFromCpu`). The profile
//! of 25 November (RTX 4060 laptop, 8 GB) has those two at 64 % of the pass
//! over the heap's texture slots, which takes 500 to 1,500 ms of each
//! frame then, on the thread that records it. A texture placed in a heap
//! that is already there takes neither: one heap of [`HEAP_BYTES`] is
//! made and waited for once for 16 to 256 textures. Most of the textures
//! streamed in are small: only those of at most [`LARGEST`] go into heaps.
//!
//! A texture's place is given back when its host copy goes, once the
//! submissions running then have finished (`crate::host::retire`). A heap
//! left empty is let go at a later call, a few kept.
//! Textures over [`LARGEST`], with a clear value or another alignment are
//! made as before.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

use windows::Win32::Graphics::Direct3D12::*;
use windows::core::Interface;

/// The bytes of one heap.
const HEAP_BYTES: u64 = 16 << 20;
/// The largest texture placed in a heap; larger ones are made as before.
const LARGEST: u64 = 1 << 20;
/// The smallest slot (a texture's own alignment); slots are whole grains.
const GRAIN: u64 = 64 << 10;
/// Empty heaps kept for textures to come.
const EMPTY_KEPT: usize = 2;
/// The most bytes of free slots in all heaps together before no new heap
/// is made: past it, a texture with no free slot of its size is made as
/// before, so that free slots never take the card's room from textures.
const SLACK_MOST: u64 = 256 << 20;

/// Heaps made, textures placed, heaps let go, and the bytes of the heaps
/// alive, since the start.
pub(crate) static STATS: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];

/// A heap of slots of one size (whole grains, from [`GRAIN`] to
/// [`LARGEST`]).
///
/// One size a heap, and a texture goes into the fullest heap of its size
/// that has a free slot: the textures still alive gather in few heaps, and
/// the others empty and are let go. (A first version placed textures of
/// all sizes in 64 MB heaps, first fit: the textures that outlived the
/// loading screen were scattered over all of them, the heaps held 2.8 GB
/// more than their textures, and the card had no room for new ones.)
struct Heap {
    heap: ID3D12Heap,
    size: u64,
    free: Vec<u32>,
    used: u32,
    /// The call at which it was left empty.
    empty_at: Option<u64>,
}

// SAFETY: a D3D12 heap is free-threaded; the rest is plain data.
unsafe impl Send for Heap {}

struct Heaps {
    list: Vec<Option<Heap>>,
    /// The place of each texture in a heap, by the texture's address:
    /// the heap's index and the slot.
    placed: Option<HashMap<usize, (usize, u32)>>,
    calls: u64,
}

static HEAPS: Mutex<Heaps> = Mutex::new(Heaps {
    list: Vec::new(),
    placed: None,
    calls: 0,
});

fn heaps() -> std::sync::MutexGuard<'static, Heaps> {
    HEAPS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Whether textures are placed in shared heaps.
pub(crate) fn on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_TEXTURE_HEAPS").is_none_or(|v| v != "0"))
}

impl Heaps {
    /// Lets go of heaps left empty at an earlier call (their last texture
    /// is gone by now), keeping [`EMPTY_KEPT`] of them.
    fn purge(&mut self) {
        let mut kept = 0;
        for entry in &mut self.list {
            let Some(heap) = entry else { continue };
            if heap.used == 0 && heap.empty_at.is_some_and(|at| at < self.calls) {
                if kept >= EMPTY_KEPT {
                    let gone = STATS[2].fetch_add(1, Relaxed) + 1;
                    let alive = STATS[3].fetch_sub(HEAP_BYTES, Relaxed) - HEAP_BYTES;
                    if gone.is_multiple_of(32) {
                        tracing::info!(let_go = gone, alive_mb = alive >> 20, "texture heaps let go");
                    }
                    *entry = None;
                } else {
                    kept += 1;
                }
            }
        }
    }

    /// A free slot of `size` in the fullest heap of that size that has one.
    fn take(&mut self, size: u64) -> Option<(usize, u32)> {
        let index = self
            .list
            .iter()
            .enumerate()
            .filter_map(|(i, h)| h.as_ref().filter(|h| h.size == size && !h.free.is_empty()).map(|h| (i, h.used)))
            .max_by_key(|&(_, used)| used)?
            .0;
        let heap = self.list[index].as_mut()?;
        let slot = heap.free.pop()?;
        heap.used += 1;
        heap.empty_at = None;
        Some((index, slot))
    }

    fn give_back(&mut self, index: usize, slot: u32) {
        let calls = self.calls;
        let Some(Some(heap)) = self.list.get_mut(index) else { return };
        heap.free.push(slot);
        heap.used = heap.used.saturating_sub(1);
        if heap.used == 0 {
            heap.empty_at = Some(calls);
        }
    }
}

/// A host texture of description `desc` placed in a shared heap, in state
/// `state`; `None` if it is not one for a heap (then it is made as before).
pub(crate) fn place(device: &ID3D12Device, desc: &D3D12_RESOURCE_DESC, state: D3D12_RESOURCE_STATES) -> Option<ID3D12Resource> {
    if !on() {
        return None;
    }
    // SAFETY: a plain query about one description.
    let info = unsafe { device.GetResourceAllocationInfo(0, std::slice::from_ref(desc)) };
    if info.SizeInBytes == u64::MAX || info.SizeInBytes > LARGEST || info.Alignment > GRAIN {
        return None;
    }
    // In whole grains, not powers of two: a texture of 600 KiB took a slot
    // of 1 MiB, and the card held about 1 GB more than its textures, so it
    // turned new ones down (283 passes, laptop run of 25 November, 1a2d180).
    let size = info.SizeInBytes.max(GRAIN).next_multiple_of(GRAIN);
    let (index, slot, heap) = {
        let mut all = heaps();
        all.calls += 1;
        all.purge();
        let (index, slot) = match all.take(size) {
            Some(found) => found,
            None => {
                let slack: u64 = all.list.iter().flatten().map(|h| h.free.len() as u64 * h.size).sum();
                if slack > SLACK_MOST {
                    return None;
                }
                let heap = make_heap(device)?;
                let made_so_far = STATS[0].fetch_add(1, Relaxed) + 1;
                let alive = STATS[3].fetch_add(HEAP_BYTES, Relaxed) + HEAP_BYTES;
                if made_so_far.is_power_of_two() || made_so_far.is_multiple_of(32) {
                    tracing::info!(
                        made = made_so_far,
                        let_go = STATS[2].load(Relaxed),
                        alive_mb = alive >> 20,
                        free_slots_mb = slack >> 20,
                        textures = STATS[1].load(Relaxed),
                        "texture heaps"
                    );
                }
                let made = Heap {
                    heap,
                    size,
                    free: (0..(HEAP_BYTES / size) as u32).rev().collect(),
                    used: 0,
                    empty_at: None,
                };
                match all.list.iter().position(Option::is_none) {
                    Some(empty) => all.list[empty] = Some(made),
                    None => all.list.push(Some(made)),
                }
                all.take(size)?
            }
        };
        let heap = all.list[index].as_ref()?.heap.clone();
        (index, slot, heap)
    };
    let mut texture: Option<ID3D12Resource> = None;
    // SAFETY: a texture at its own slot of the heap, which nothing else
    // uses; the out pointer is ours.
    let made = unsafe { device.CreatePlacedResource(&heap, u64::from(slot) * size, desc, state, None, &mut texture) };
    let mut all = heaps();
    match (made, texture) {
        (Ok(()), Some(texture)) => {
            all.placed.get_or_insert_with(HashMap::new).insert(texture.as_raw() as usize, (index, slot));
            if STATS[1].fetch_add(1, Relaxed) == 0 {
                tracing::info!("host copies of small textures the title fills are placed in shared heaps of the card's memory (VICTORY_TEXTURE_HEAPS=0: each with its own)");
            }
            Some(texture)
        }
        (made, _) => {
            static SAID: std::sync::Once = std::sync::Once::new();
            SAID.call_once(|| tracing::warn!(error = ?made.err(), "a texture could not be placed in a shared heap; made with its own memory"));
            all.give_back(index, slot);
            None
        }
    }
}

fn make_heap(device: &ID3D12Device) -> Option<ID3D12Heap> {
    let desc = |flags: D3D12_HEAP_FLAGS| D3D12_HEAP_DESC {
        SizeInBytes: HEAP_BYTES,
        Properties: D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            ..Default::default()
        },
        Alignment: 0,
        Flags: flags,
    };
    for flags in [
        D3D12_HEAP_FLAG_ALLOW_ONLY_NON_RT_DS_TEXTURES | D3D12_HEAP_FLAG_CREATE_NOT_ZEROED,
        D3D12_HEAP_FLAG_ALLOW_ONLY_NON_RT_DS_TEXTURES,
    ] {
        let mut made: Option<ID3D12Heap> = None;
        let d = desc(flags);
        // SAFETY: plain creation; the out pointer is ours.
        if unsafe { device.CreateHeap::<ID3D12Heap>(&raw const d, &raw mut made) }.is_ok() && made.is_some() {
            return made;
        }
    }
    static SAID: std::sync::Once = std::sync::Once::new();
    SAID.call_once(|| tracing::warn!("no heap for host textures: they are made with their own memory"));
    None
}

/// A texture's place in a heap, given back when this is dropped.
struct Place(usize, u32);

impl Drop for Place {
    fn drop(&mut self) {
        heaps().give_back(self.0, self.1);
    }
}

/// The bytes of the free slots in all heaps.
pub(crate) fn slack() -> u64 {
    heaps().list.iter().flatten().map(|h| h.free.len() as u64 * h.size).sum()
}

/// The host copy `texture` goes: its place in a heap, if it has one, is
/// free again once the submissions running now have finished.
pub(crate) fn release(texture: &ID3D12Resource) {
    let place = heaps().placed.as_mut().and_then(|p| p.remove(&(texture.as_raw() as usize)));
    if let Some((index, slot)) = place {
        crate::host::retire(Place(index, slot));
    }
}
