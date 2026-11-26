# CCS03

CCS03 builds on [victory-fork](https://github.com/fxceless/victory-fork) by
fxceless and its contributors, which is itself a fork of
[VIctory](https://github.com/Marvisak/VIctory). victory-fork carries a large
amount of rendering, lighting, frame-rate and memory work on top of VIctory,
so CCS03 starts from there and adds its own work and reviewed pull requests
on top, also from other forks (ZETTROX, Belobug's fork).

Base: victory-fork `main` at `0b0074e` (25 November: the frame-rate work, merged in CCS03 PR 27; before that `9376fd6` in CCS03 PR 19).

Every change that comes into `ccs03` is checked first: is it already in the
base, does victory-fork already solve the same problem another way, do
reviewers report problems with it, and does the whole test suite pass on
Windows with an NVIDIA GPU (32-lane waves).

## In CCS03 on top of victory-fork

| What | Author |
|---|---|
| `cargo xtask` finds the repository at run time, so the commit hook keeps working when the repository moves to another drive (CCS03 PR 3) | Apenshot123 |
| Contributor guide, agent rules, Windows setup and test scripts (`AGENTS.md`, `CONTRIBUTING.md`, `ccs03/`) | CCS03 |
| CCS03 PR 7: the DirectX 12 Agility SDK on Windows 10 (VIctory PR 968, without its packed-format mapping, which victory-fork already has) | bailu12300, Leekingut |
| CCS03 PR 8: `v_cvt_f32_f64` | 9hvn9yt2vk-netizen |
| CCS03 PR 9: `v_fma_mixlo/mixhi` over an f16 first interpolation half keeps zero (victory-fork PR 77) | m335671 |
| CCS03 PR 10: a 1080p display is reported by default, so the title's targets fit an 8 GB card (`VICTORY_DISPLAY_MODE=3840x2160` for 4K) | 9hvn9yt2vk-netizen |
| CCS03 PR 11: the depth bounds test is passed through to host pipelines and command lists (victory-fork PR 82) | Kaivyyy |
| CCS03 PR 12: texture reads wait only for remappings of their own bytes (victory-fork PR 95) | LastThylacine |
| CCS03 PR 13: no blending on integer render targets (the driver refused a mesh pipeline over it) | 9hvn9yt2vk-netizen |
| CCS03 PR 14: the capture folder is made before frames are saved | 9hvn9yt2vk-netizen |
| CCS03 PR 15: `v_ldexp_f32`, `v_frexp_mant_f32` and `v_frexp_exp_i32_f32` (CCS03 PR 1 of the previous base, again) | 9hvn9yt2vk-netizen |
| CCS03 PR 18: first-lane reads alike in a console wave need no exchange on 32-lane hosts | 9hvn9yt2vk-netizen |
| CCS03 PR 19: victory-fork `main` at `9376fd6` (the first room renders right); `v_cvt_f32_f64` keeps CCS03 PR 8's lowering, victory-fork's tests check it too | fxceless and contributors; merge CCS03 |
| CCS03 PR 20: the high-performance GPU on laptops, `VICTORY_D3D12_ADAPTER` (victory-fork PR 261) | xADEL |
| CCS03 PR 21: from ZETTROX's open pull requests: shader signature elements sorted by slot with PSV0 interpolation (#60), `v_permlane16_b32` and `v_permlanex16_b32` (#49), no colour codes in log files (#12), no multisampled linear textures (#14, VIctory PR 629) | IAmTheSween, Altorru, God489, AlanQiang0418 |
| CCS03 PR 22: the tests compile and Clippy is clean after PRs 19 to 21 | CCS03 |
| CCS03 PR 24: a constant for the ray-traced indirect light while rays do not run (`VICTORY_AMBIENT`, from Belobug's fork) | beloooo |
| CCS03 PR 25: the Series S profile: the title's own upscaler (victory-fork PR 798), the interface plane and the upscale stretch (PR 188), texture checks several at a time (PR 768) | KyleS98, sagouindupm, Filip722 |
| CCS03 PR 26: immediate samplers chosen by control flow (victory-fork PR 25, 40 programs among them cascade shadows) and the ordered pixel section as rasterizer-ordered views (PR 215, 10 programs); CCS03's own lowering moved to `lower_ccs03.rs` for the policy's size limit | Noname0202, arminkhaleghirod; CCS03 |
| CCS03 PR 27: victory-fork `main` at `0b0074e` (submit thread, copy queue, draw windows, copies in place, frame log) and its PR 823 (64-lane compute programs NVIDIA refused, page-mapping copies as one view) | fxceless, Felix2180 |
| CCS03 PR 28: textures without their largest levels on 8 GB cards (`VICTORY_TEXTURE_DROP_MIPS`, from victory-fork PR 814); the indirect budget test no longer asks which draws win | staronthehorizon; CCS03 |
| VIctory PR 684: shader tests find the PC compiler independently of the validator | Dathinel |
| PlayStation 5 pads (DualSense, DualSense Edge) read as HID devices while XInput has no first pad, over USB or Bluetooth, rumble over USB | 9hvn9yt2vk-netizen |
| Reflections bent down to the ambient light after the title's history pass (`VICTORY_REFLECT_KNEE`), after direct and indirect dispatches | 9hvn9yt2vk-netizen |
| Failing shaders logged with the title's name for their program | 9hvn9yt2vk-netizen |
| The Series S profile's water transmission programs on victory-fork's wave-wide list (they did not translate for 32-lane waves) | 9hvn9yt2vk-netizen |
| `ccs03\play.ps1`: runs with the README's switches, keeps a log and a summary per run (`-Smooth`, `-Fast`, `-Low`, `-Diag`) | 9hvn9yt2vk-netizen |
| The title's frames held to a steady step at low frame rates (`VICTORY_GAME_STEP_MS`, `play.ps1 -Steady`): its performance counter runs slower while frames are slow | 9hvn9yt2vk-netizen |
| victory-fork `main` at `32b31f7`: textures shaders write that the title reads on the CPU copied back into title memory (`VICTORY_TEXTURE_BACK=1`, victory-fork PR 999: boats float), the card compare hashed by 64-thread groups (PR 1014, opt-in), notes about unmapped blocks (`VICTORY_ABSENT_ONCE`), the copy-back measured (`VICTORY_BACK_READS`, diagnostics); `title_memory_reading` with this tree's byte ranges | Geckomacabre, cxy0072019-sketch, martindevbr, fxceless; merge 9hvn9yt2vk-netizen |

`README.md` is CCS03's own and links to victory-fork's README for the state
of the title and the technical details. When merging victory-fork `main`,
keep ours (`git checkout --ours README.md`) and check whether anything in it
needs updating.

## To bring back from the old base

Until 2026-11-24 `ccs03` was VIctory `main` plus the pull requests below. That
history is kept in the branch `ccs03-oud`. Each one comes back as its own
small pull request into `ccs03`, after checking whether victory-fork already
covers it. "Applies" says how its change lands on the new base.

| Change | Author | Applies |
|---|---|---|
| VIctory PR 477: DXIL validator without undefined behaviour on null error blobs, checked status, no leaked handles | nathanbzrr | cleanly |
| VIctory PR 596: `v_cvt_pknorm_*` with exact rounding | mercuryVM | cleanly |
| HLE: file path in DirectStorage request failure warnings | Apenshot123 | cleanly |
| HLE: `source_path` guarded against unsupported DirectStorage options | Apenshot123 | small conflict |
| CCS03 PR 2: raw buffer views that do not start on 16 bytes become null views (also VIctory PR 123) | 9hvn9yt2vk-netizen | small conflict |
| VIctory PR 605: `v_cvt_rpi_i32_f32` rounds the exact sum; NaN saturates by its sign | upipibi | small conflict |
| VIctory PR 707: `tbuffer_store_format_*` that need no conversion | mercuryVM | small conflict |
| VIctory PR 606: `ds_swizzle_b32` rotate and FFT modes | upipibi | small conflict |
| VIctory PR 642: enhanced texture barriers in `ID3D12GraphicsCommandList7::Barrier` | nathanbzrr | small conflict |
| VIctory PR 284: RDNA2's legacy multiply-adds are the fused ones | Dufftwt | small conflict |
| VIctory PR 628: `ds_min_f32` / `ds_max_f32` with RDNA2's NaN rules | mercuryVM | small conflict |
| VIctory PR 578: refuse wave64 mesh and compute programs on 32-lane hosts | wbkepler | conflicts; victory-fork has its own wave64 handling, check first |
| VIctory PR 635: buffer views past the end of their host copy are null, views running past it are cut | a VIctory contributor (its login names the title) | conflicts |
| VIctory PR 571: LDS byte and half accesses | Saitken91 | conflicts |
| VIctory PR 688: dynamic descriptor slots taken back when a heap runs short | iurysf | conflicts; victory-fork reworked descriptor tables, check first |
| VIctory PR 643: live host textures shared across aliased addresses | nathanbzrr | conflicts |
| VIctory PR 452: loops that only wait for memory stop after 4096 iterations, plus CCS03's fix that atomics are effects | AntoninMichelq, CCS03 | conflicts |
| VIctory PR 298: lane masks spilled to VGPR lanes and read back | permadiego | conflicts; victory-fork has its own lane-mask work, check first |
| VIctory PR 587: mesh output bounds, all primitives initialised, no sync on a divergent `s_barrier` | FibbS6 | larger conflicts |
| VIctory PR 160: texel offsets, comparison and gather forms of image samples | r4. | larger conflicts |
| VIctory PR 568: null descriptor of the kind the shader reads for table slots without a view | BastienRdz | larger conflicts |

Work in progress on the old base, to be rebased on `ccs03` by whoever owns
it: `gpu/win10-agility-sdk` (Windows 10 through the Agility SDK, VIctory PR
968), `shader/mad-ldexp-frexp`, `shaders/ds-bpermute-b32`,
`shaders/ds-inc-dec-rsub-u32`.

## Closed in CCS03

| PR | Why |
|---|---|
| CCS03 PR 5 (VIctory PR 573, `ds_bpermute_b32`) | victory-fork already lowers `ds_bpermute_b32`. Kept as tag `archief/pr-5` |
| CCS03 PR 6 (`ds_rsub_u32`, `ds_inc_u32`, `ds_dec_u32`) | made on the previous base; welcome again rebased on `ccs03` (the lowering applies, the tests conflict). Kept as tag `archief/pr-6` |
| CCS03 PR 7 (Agility SDK) | merged as the same change rebased on victory-fork. Kept as tag `archief/pr-7` |

## Not taken (yet)

From the old base's list, still to be looked at again on the new base:

| PR | Why |
|---|---|
| VIctory 584 | Reported to keep the title from reaching its menu (black screen) |
| VIctory 708 | Broke two `victory-core` tests on VIctory `main` |
| VIctory 626 | Only needed on Windows 10 or older D3D12 runtimes |
| VIctory 346 | Reviewers report bugs whose fixes are not in the PR yet |
| VIctory 233, 320, 439 | Running wave64 programs on 32-lane hosts: in-game reports still showed driver compiler faults and a black world on NVIDIA |
| VIctory 599 | Large conflicts in the shader lowering; helps hosts with 64-lane waves only |
| VIctory 632 | 571 covers the same LDS accesses |
| VIctory 579 | A manual wave64 switch; PR 578 already refuses those programs on 32-lane hosts |
| VIctory 192, 242, 294, 32, 573 | Conflicted with the old base; to be looked at again |

## Building

Same as victory-fork and VIctory (see [README.md](README.md)). The branch is
`ccs03`.
