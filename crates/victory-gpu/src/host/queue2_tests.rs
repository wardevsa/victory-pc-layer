//! The copy queues' tests (`host::queue2_tests`), out of `host.rs` for its
//! size.

use super::*;

/// `memory[to] = memory[from] + add`, after `spin` steps.
const ADD: &str = r#"
#define RS "RootConstants(num32BitConstants=4, b0), UAV(u0)"
cbuffer What : register(b0) { uint to; uint from; uint add; uint spin; };
RWByteAddressBuffer memory : register(u0);
[RootSignature(RS)]
[numthreads(1, 1, 1)]
void main() {
uint h = 0u;
[loop]
for (uint i = 0u; i < spin; i++) {
    h = (h ^ memory.Load(from)) * 0x01000193u;
}
memory.Store(to, memory.Load(from) + add + (h & 0u));
}
"#;

#[test]
fn warp_title_bytes_go_up_and_shader_writes_come_back_whatever_the_queues() {
    use windows_sys::Win32::System::Memory::{MEM_COMMIT, MEM_RESERVE, PAGE_READWRITE, VirtualAlloc};
    const MEM_WRITE_WATCH: u32 = 0x0020_0000;
    let Some(host) = host() else {
        return;
    };
    // Never on a real card.
    // SAFETY: a plain query.
    let software = host
        .adapter
        .as_ref()
        .and_then(|a| unsafe { a.GetDesc1() }.ok())
        .is_some_and(|d| d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0);
    if !software {
        eprintln!("queue2_tests: skipped, the host device is not the software adapter (VICTORY_D3D12_WARP=1, diagnostics)");
        return;
    }
    const PIECE: usize = 64 << 10;
    const LEN: usize = 4 << 20;
    const ROUNDS: usize = 12;
    // "Title memory": write-watched, as the title's is.
    // SAFETY: a plain allocation of our own.
    let title = unsafe { VirtualAlloc(std::ptr::null(), LEN, MEM_RESERVE | MEM_COMMIT | MEM_WRITE_WATCH, PAGE_READWRITE) } as *mut u8;
    assert!(!title.is_null());
    let dword = |at: usize| unsafe { title.add(at).cast::<u32>() };
    for i in 0..LEN / 4 {
        // SAFETY: inside the allocation.
        unsafe { dword(i * 4).write_volatile(0x5A00_0000 | i as u32) };
    }
    crate::shadow::register(0x51_5151, title as u64, LEN as u64);
    let shadow = crate::shadow::find(title as u64, LEN as u64).expect("a host copy");
    if !shadow.carded() {
        eprintln!("queue2_tests: skipped, the copy is not in the card's memory (VICTORY_CARD_BUFFERS_MIN_MB=1)");
        return;
    }
    let code = compile(ADD, windows::core::s!("main"), windows::core::s!("cs_5_1")).expect("the test's shader");
    // SAFETY: the shader's own root signature; plain creation.
    let (root, pso): (ID3D12RootSignature, ID3D12PipelineState) = unsafe {
        let root: ID3D12RootSignature = host.device.CreateRootSignature(0, &code).expect("a root signature");
        let pso = host
            .device
            .CreateComputePipelineState(&D3D12_COMPUTE_PIPELINE_STATE_DESC {
                pRootSignature: std::mem::transmute_copy(&root),
                CS: D3D12_SHADER_BYTECODE {
                    pShaderBytecode: code.as_ptr().cast(),
                    BytecodeLength: code.len(),
                },
                ..Default::default()
            })
            .expect("a pipeline");
        (root, pso)
    };
    // Round k: the title writes a dword in piece 1+k; a submission's
    // shader adds k to it and stores the sum in piece 30+k, a range
    // declared as shader-written. Nothing is waited for between the
    // rounds where submissions are sent ahead.
    let from = |k: usize| (1 + k) * PIECE + 4 * k + 64;
    let to = |k: usize| (30 + k) * PIECE + 8 * k + 128;
    let titles = |k: usize| 0x0100_0000 + 0x1000 * k as u32 + 7;
    let mut lists = Vec::new();
    for k in 0..ROUNDS {
        // SAFETY: inside the allocation.
        unsafe { dword(from(k)).write_volatile(titles(k)) };
        let list = host.new_list().expect("a host list");
        shadow.refresh(&list, title as u64, LEN as u64);
        shadow.gpu_writes(&list, (title as usize + (30 + k) * PIECE) as u64, PIECE as u64);
        // SAFETY: an open list; the copy's buffer, with unordered
        // access; the shader's own root signature.
        unsafe {
            list.list.SetComputeRootSignature(&root);
            list.list.SetPipelineState(&pso);
            let constants = [to(k) as u32, from(k) as u32, k as u32, 30_000];
            list.list.SetComputeRoot32BitConstants(0, 4, constants.as_ptr().cast(), 0);
            list.list.SetComputeRootUnorderedAccessView(1, shadow.buffer.GetGPUVirtualAddress());
            list.list.Dispatch(1, 1, 1);
        }
        list.close();
        if late() {
            host.execute_own(&list);
        } else {
            host.execute(&[&list]);
        }
        lists.push(list);
    }
    settle_now(WHY_SYNC);
    for k in 0..ROUNDS {
        // SAFETY: inside the allocation.
        let (sum, kept) = unsafe { (dword(to(k)).read_volatile(), dword(from(k)).read_volatile()) };
        assert_eq!(kept, titles(k), "round {k}: the title's own dword changed");
        assert_eq!(
            sum,
            titles(k) + k as u32,
            "round {k}: the shader read {:#x} where the title had written {:#x}, or its write did not come back",
            sum.wrapping_sub(k as u32),
            titles(k)
        );
    }
    // Nothing else moved.
    let touched: std::collections::HashSet<usize> = (0..ROUNDS).flat_map(|k| [from(k), to(k)]).collect();
    for i in (0..LEN / 4).filter(|i| !touched.contains(&(i * 4))) {
        // SAFETY: inside the allocation.
        let got = unsafe { dword(i * 4).read_volatile() };
        assert_eq!(got, 0x5A00_0000 | i as u32, "dword {i} of title memory changed");
    }
    // The queues the switches name did the work.
    if let Some(second) = crate::queue2::second() {
        let (down, up) = (second.down.stats.jobs.load(Ordering::Relaxed), second.up.stats.jobs.load(Ordering::Relaxed));
        // (Sends that found no landing buffer went the old way.)
        let no_room = second.landing[1].load(Ordering::Relaxed);
        eprintln!(
            "queue2_tests: VICTORY_QUEUE2={} late {} relay {}: {down} fetches and {up} sends through the copy queues ({no_room} more found no landing buffer), the direct queue told to wait {} times",
            crate::queue2::bits(),
            late(),
            relay(),
            second.held[0].load(Ordering::Relaxed)
        );
        assert_eq!(down > 0, second.fetches(), "fetches through the copy queue");
        assert_eq!(up + no_room > 0, second.uploads(), "sends through the copy queue");
        assert_eq!(up, second.held[0].load(Ordering::Relaxed), "the direct queue waits for every send");
    } else {
        eprintln!("queue2_tests: the direct queue alone (late {}, relay {})", late(), relay());
    }
    drop(lists);
}

/// `memory[to] = first` at once; then `spin` steps, each a store to
/// `memory[to + 4]`; then `memory[to + 4] = 0` and `memory[to] = last`.
/// Whatever `spin` is, the bytes it leaves are the same.
const TWICE: &str = r#"
#define RS "RootConstants(num32BitConstants=4, b0), UAV(u0)"
cbuffer What : register(b0) { uint to; uint first; uint last; uint spin; };
RWByteAddressBuffer memory : register(u0);
[RootSignature(RS)]
[numthreads(1, 1, 1)]
void main() {
memory.Store(to, first);
[loop]
for (uint i = 0u; i < spin; i++) {
    memory.Store(to + 4u, i);
}
memory.Store(to + 4u, 0u);
memory.Store(to, last);
}
"#;

/// The hole a fetch on the copy queue had (`VICTORY_QUEUE2` bit 1 with
/// the relay): flight A writes a piece; flight B, sent behind it,
/// writes the same piece through a half-way value and leaves it as A
/// left it. A is landed while B is half way: the fetch, beside B on
/// the copy queue, brings B's half-way bytes into title memory. B's
/// compare finds the piece as A's compare left it and flags nothing,
/// so B's landing fetched nothing and title memory kept the half-way
/// bytes. With the fix B's landing fetches the piece all the same
/// (`BESIDE`).
///
/// `VICTORY_D3D12_WARP=1 VICTORY_CARD_BUFFERS_MIN_MB=1 VICTORY_QUEUE2=1
///  VICTORY_RELAY=1 cargo test -p victory-gpu --features diagnostics
///  fetched_again -- --nocapture` (alone: it lands its two flights
/// itself; `VICTORY_QUEUE2_REFETCH=0` shows the fault).
#[test]
fn warp_a_piece_fetched_while_a_later_flight_writes_it_is_fetched_again() {
    use windows_sys::Win32::System::Memory::{MEM_COMMIT, MEM_RESERVE, PAGE_READWRITE, VirtualAlloc};
    const MEM_WRITE_WATCH: u32 = 0x0020_0000;
    let Some(host) = host() else {
        return;
    };
    // Never on a real card.
    // SAFETY: a plain query.
    let software = host
        .adapter
        .as_ref()
        .and_then(|a| unsafe { a.GetDesc1() }.ok())
        .is_some_and(|d| d.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0);
    if !software {
        eprintln!("queue2_tests: skipped, the host device is not the software adapter (VICTORY_D3D12_WARP=1, diagnostics)");
        return;
    }
    if !relay() || !crate::queue2::second().is_some_and(|s| s.fetches()) {
        eprintln!("queue2_tests: skipped, fetches do not go to a copy queue beside the relay (VICTORY_RELAY=1 VICTORY_QUEUE2=1)");
        return;
    }
    const PIECE: usize = 64 << 10;
    const LEN: usize = 4 << 20;
    // SAFETY: a plain allocation of our own.
    let title = unsafe { VirtualAlloc(std::ptr::null(), LEN, MEM_RESERVE | MEM_COMMIT | MEM_WRITE_WATCH, PAGE_READWRITE) } as *mut u8;
    assert!(!title.is_null());
    let dword = |at: usize| unsafe { title.add(at).cast::<u32>() };
    for i in 0..LEN / 4 {
        // SAFETY: inside the allocation.
        unsafe { dword(i * 4).write_volatile(0x5A00_0000 | i as u32) };
    }
    crate::shadow::register(0x52_5252, title as u64, LEN as u64);
    let shadow = crate::shadow::find(title as u64, LEN as u64).expect("a host copy");
    if !shadow.carded() {
        eprintln!("queue2_tests: skipped, the copy is not in the card's memory (VICTORY_CARD_BUFFERS_MIN_MB=1)");
        return;
    }
    let code = compile(TWICE, windows::core::s!("main"), windows::core::s!("cs_5_1")).expect("the test's shader");
    // SAFETY: the shader's own root signature; plain creation.
    let (root, pso): (ID3D12RootSignature, ID3D12PipelineState) = unsafe {
        let root: ID3D12RootSignature = host.device.CreateRootSignature(0, &code).expect("a root signature");
        let pso = host
            .device
            .CreateComputePipelineState(&D3D12_COMPUTE_PIPELINE_STATE_DESC {
                pRootSignature: std::mem::transmute_copy(&root),
                CS: D3D12_SHADER_BYTECODE {
                    pShaderBytecode: code.as_ptr().cast(),
                    BytecodeLength: code.len(),
                },
                ..Default::default()
            })
            .expect("a pipeline");
        (root, pso)
    };
    // A list whose shader writes the dword at 256 of `piece`, declared.
    let record = |piece: usize, first: u32, last: u32, spin: u32| {
        let list = host.new_list().expect("a host list");
        shadow.refresh(&list, title as u64, LEN as u64);
        shadow.gpu_writes(&list, (title as usize + piece * PIECE) as u64, PIECE as u64);
        // SAFETY: an open list; the copy's buffer, with unordered
        // access; the shader's own root signature.
        unsafe {
            list.list.SetComputeRootSignature(&root);
            list.list.SetPipelineState(&pso);
            let constants = [(piece * PIECE + 256) as u32, first, last, spin];
            list.list.SetComputeRoot32BitConstants(0, 4, constants.as_ptr().cast(), 0);
            list.list.SetComputeRootUnorderedAccessView(1, shadow.buffer.GetGPUVirtualAddress());
            list.list.Dispatch(1, 1, 1);
        }
        list.close();
        list
    };
    // SAFETY: a plain query.
    let completed = || unsafe { host.fence.GetCompletedValue() };
    let wait_for = |value: u64| {
        while completed() < value {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    };
    // How many steps keep the card busy for some seconds: timed on
    // another piece, waited for.
    const TRIAL: u32 = 1 << 20;
    let trial = record(40, 1, 2, TRIAL);
    let timing = std::time::Instant::now();
    host.execute(&[&trial]);
    let took = timing.elapsed().as_secs_f64().max(1e-4);
    let spin = (f64::from(TRIAL) * 3.0 / took).clamp(f64::from(TRIAL), 2.0e9) as u32;
    // SAFETY: inside the allocation.
    assert_eq!(unsafe { dword(40 * PIECE + 256).read_volatile() }, 2, "the trial's write did not come back");
    const AT: usize = 20 * PIECE + 256;
    const HALF_WAY: u32 = 0xDEAD_0001;
    const LEFT: u32 = 0x600D_0002;
    // Flight A, and the card has run it.
    let a = record(20, HALF_WAY, LEFT, 16);
    let (submitted, mut sent_a) = host.submit(&[&a], true).expect("flight A sent ahead");
    let fence_a = *submitted;
    drop(submitted);
    wait_for(fence_a);
    // Flight B: the same piece, through the half-way value, for seconds.
    let b = record(20, HALF_WAY, LEFT, spin);
    let (submitted, mut sent_b) = host.submit(&[&b], true).expect("flight B sent ahead");
    let fence_b = *submitted;
    drop(submitted);
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(
        completed() < fence_b,
        "flight B had run after 0.3 s ({spin} steps; the trial's {TRIAL} took {took:.3} s): nothing to show"
    );
    // A lands, as `Host::relay_land` lands it, while B is half way.
    let mut fetch = None;
    {
        let plan = sent_a.card.as_ref().expect("flight A has a plan for the card's copies");
        host.relay_card_back(&[(sent_a.number, plan)], &mut fetch, fence_a, sent_a.number);
    }
    for (copy, ranges) in &sent_a.back {
        copy.copy_back_done(ranges);
    }
    // SAFETY: inside the allocation.
    let seen = unsafe { dword(AT).read_volatile() };
    let b_ran = completed() >= fence_b;
    eprintln!(
        "queue2_tests: after A's landing title memory holds {seen:#x} (half way {HALF_WAY:#x}, left {LEFT:#x}); B had run: {b_ran}; {spin} steps"
    );
    assert_eq!(
        seen, HALF_WAY,
        "the fetch for A did not come down beside B (it waited for B, or B had not begun): this machine cannot show the fault"
    );
    // B has run; it left the piece as A left it, so its compare flags
    // nothing. Its landing must bring the piece down all the same.
    wait_for(fence_b);
    {
        let plan = sent_b.card.as_ref().expect("flight B has a plan for the card's copies");
        host.relay_card_back(&[(sent_b.number, plan)], &mut fetch, fence_b, sent_b.number);
    }
    for (copy, ranges) in &sent_b.back {
        copy.copy_back_done(ranges);
    }
    // SAFETY: inside the allocation.
    let (left, beside) = unsafe { (dword(AT).read_volatile(), dword(AT + 4).read_volatile()) };
    assert_eq!(
        left, LEFT,
        "title memory kept flight B's half-way bytes: B's compare found the piece as A left it and B's landing fetched nothing"
    );
    assert_eq!(beside, 0, "the dword beside it holds a half-way count");
    for sent in [&mut sent_a, &mut sent_b] {
        if let Some(plan) = sent.card.take() {
            host.card_done(plan);
        }
    }
    drop((a, b, trial));
}
