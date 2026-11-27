# victory-pc-layer

The title of an Xbox Series dump the user owns, run on a Windows PC with an
8 GB NVIDIA card. Since 2026-11-26 this repository starts again from
**[cool_fox](https://github.com/DoubleGhosty/cool_fox)** (DoubleGhosty and
the community behind it, "CGBJ"): on the test PC it gave the best picture of
every tree tried. The work here is its frame rate, with the picture left as
it is. The earlier tree (on fxceless's victory-fork, with ray-traced light at
about 2 fps) is kept in the branch `base-victory-fork`.

**This repository contains no game files, code or assets.** You need your own
copy of the game. It is not affiliated with or endorsed by the game's
developer or publisher.

## Use

```powershell
cargo build --release -p victory
$env:VICTORY_GAME_DIR = '<folder of your dump>'
powershell -ExecutionPolicy Bypass -File .\jugar.ps1 -Preparar       # after each update, game closed
powershell -ExecutionPolicy Bypass -File .\jugar.ps1                 # play: Series S, our speed switches
```

| Switch | What it does |
|---|---|
| `-Preparar` | No game: every shader translated into the cache (`victory translate --warm all`, with the host's wave size and view heap from the last run's log), then the host pipelines earlier runs wrote down (`victory warm --used`). Run it after every update: the first run after a change of the translator otherwise translates them while playing (some 700 pipelines waiting at once, frames of 500-800 ms). |
| `-Fabrica` | cool_fox's defaults alone, to compare |
| `-Diag` | The diagnostics build (`cargo build --release -p victory --features diagnostics --target-dir target\diag`): the log names what each wait for the card waited for |
| `-SinPelo` | Hair strands skipped: faster, simpler hair |
| `-Salida 1664x936` | A smaller output, less for the card |
| `-Rapido` | The switch set of cool_fox's `Play-VIctory.cmd` (slower here) |

What `jugar.ps1` turns on, each measured on the test PC (the picture is
untouched: hair strands drawn, no stand-in light, as cool_fox `3277b15`
drew it):

- `VICTORY_CARD_BUFFERS_MIN_MB=1`, `VICTORY_CARD_BUFFERS_MB=1024`: what
  shaders wrote into title buffers from 1 MiB is found on the card, not by
  the CPU (on an 8 GB card cool_fox does so only from 16 MiB): the CPU
  compared 228 MB a frame, now 37. 3.2 -> 6.3 fps median.
- `VICTORY_CARD_REHASH_RUNS`, `VICTORY_JOIN_COPIES`, `VICTORY_WIDE_BACK`:
  the card's hash passes and adjacent copies joined, the search for shader
  writes on several threads.
- `VICTORY_CARD_AWAKE`: the card's clocks kept up through the short gaps
  while the CPU prepares; helper pools sized for a 6-core CPU (after
  Maykzera's victory-rtx3060).
- `VICTORY_FILL_TITLE`: a small fill of the title's own constants goes on
  without waiting for the card (the title's heap counts as written by
  shaders as a whole): 5 waits a frame (93-104 ms) down to 1. 7-10 fps,
  11 in cutscenes.
- `VICTORY_CARD_STEPS=1` and `VICTORY_PLACED_ORDER=1`: the one fill a frame that still waited for every flight (65-83 ms) is done on the card in order: stage 1 also does fills of a list's scratch memory (our change, scratch the card can write), and a submission is cut behind a copy at its place in a list that writes what its fills read. The boat mission now plays through; a quiet stretch at 9.7-9.8 fps.
- `VICTORY_TEXTURE_GUARD=0`: entering a new place, texture refreshes waited 1.3-3.2 s a frame for the runtime's remapping, which stands in line behind the texture guards' protection changes (some 100,000 a second); textures are hashed at their check instead, and the colour noise the guard let through goes.
- Every run writes down the pipelines the title makes
  (`VICTORY_PIPELINE_BOOK`), for `-Preparar`.

Tried and left out: `VICTORY_RELAY_COPY` (copying back 81 -> 28 ms, but
preparing 66 -> 95, the same frame rate, textures torn for a moment as they
appear); `Play-VIctory.cmd`'s whole set (the card 250-380 ms a frame: the
renderer ports have it read constants from the PC's memory).

Through the AmericasLauncher (isaac-btw), turn its **Pipeline** button off
for this runner: with it on, every launch first runs `victory translate
--pipelines` over all of the title's shaders (20-40 minutes here), which
uses no cache and writes nothing the game reads.

## Results on the test PC

NVIDIA RTX 2060 SUPER 8 GB, AMD Ryzen 5 5600GT, 32 GB RAM, Windows 11,
Series S profile.

| Run (2026-11-26) | Frame rate | Where a frame went |
|-----|------------|-------|
| cool_fox `3277b15`, every switch at its default | 3.2-4 fps in the cell, 5 at the boat, up to 8 in cutscenes | the card about 150 ms, preparing 110, copying back 70 |
| the same with `Play-VIctory.cmd`'s switch set (`-Rapido`) | about 2 fps | the card 250-380 ms |
| cool_fox `64b8967`, first run after the update | 1-2 fps | about 700 pipelines being made while playing |
| the same, shaders prepared ahead (`-Preparar`) | 3.2 median, 4-5 at the boat | preparing 142, the card 136, copying back 51 (medians) |
| + compares on the card from 1 MiB, passes and copies joined (`88cbf04`) | 6.3 median, 5-8 | preparing 98, the card 58, copying back 60 |
| + the card kept awake, 6-core pools (`0abc767`) | 5.5-6.3; cutscenes start much sooner | the submitting thread waits 93-104 ms for the card in 5 settles |
| + `VICTORY_FILL_TITLE` (`09fab1c`) | **7-10 fps, 11 in cutscenes** | 1 settle; copying back 81 |
| + `VICTORY_RELAY_COPY` (`d516213`, taken back) | 6-8 | copying back 28, preparing 95; torn textures |
| + cool_fox `72944b1` merged (`be542a8`) | 4.3 median, steady 6-8 at the end | the water whitish, the card 75-104 ms; the water transmission computes (refused by its LDS rule) translate as before, and the three new scene kernels and the heap relocation compute run only on request (`VICTORY_SCENE_KERNELS=all`, `VICTORY_DEFRAG=1`), as before it; the package landed without the crash |
| the earlier tree (`base-victory-fork`), Series X with ray tracing | 1.6-2 | traced light in the cell, textures noisy |

What remains, from the diagnostics run: one wait a frame (13-47 ms) for a
fill of 16-48 bytes in the title's heap, which the skinning passes declare
written as a whole (the card steps, tried, did no fill on the card); preparing about 70-100 ms
(descriptor records about 24, copies about 25); medium textures still show
coloured blocks (later). The title crashes in its own script after the
package lands in the water in the boat mission, in every build
([fxceless/victory-fork#1209](https://github.com/fxceless/victory-fork/issues/1209)).

## Other trees looked at (2026-11-26)

| Tree | What it reports | Here |
|---|---|---|
| [trustno1-coder/VIctory-NVIDIA](https://github.com/trustno1-coder/VIctory-NVIDIA) and [Maykzera/victory-rtx3060](https://github.com/Maykzera/victory-rtx3060) | 14-17 fps on an RTX 3060, the world at half scale, rays off | built and run by the user: much worse here; its 6-core pool sizes are used |
| [ZigZagBoi1/ScarlettPort](https://github.com/ZigZagBoi1/ScarlettPort) 0.4.9 | about 3 fps on an RTX 3060 with the full picture | not faster |
| [KevinPratap/ApexIV](https://github.com/KevinPratap/ApexIV) | a background heap-buffer slot walk, 4.8 -> 6.6 fps | another lineage: its switches are not in cool_fox; a port to look at |
| [xcl-dev/xcl](https://github.com/xcl-dev/xcl), Jackfrost-cloud/XSEmu (an XCL copy) | about 5 fps | another design |
| [JozefDeTremmerie/CCS03](https://github.com/JozefDeTremmerie/CCS03) | 3-7.8 fps on an RTX 4060 laptop | merged into the earlier tree and taken back: slower here |
| [rorodev6/victory-nvidia-megafork](https://github.com/rorodev6/victory-nvidia-megafork) | VIctory-NVIDIA with the Windows 10 Agility SDK and a pipeline panic fix (cool_fox has its own) | the VIctory-NVIDIA line, much worse here |

## Credits

- **[cool_fox](https://github.com/DoubleGhosty/cool_fox)**: the base, and
  through it codec_d's cool_fox, avatarmanz's VIctory-7900XTX, DexNom,
  Belobug and their contributors (see below).
- **[Maykzera/victory-rtx3060](https://github.com/Maykzera/victory-rtx3060)**:
  the helper pool sizes for a 6-core CPU.
- **[VIctory](https://github.com/Marvisak/VIctory)** by Marvisak and its
  contributors, the base of all of them.
---

# cool_fox

[![CI](https://github.com/DoubleGhosty/cool_fox/actions/workflows/ci.yml/badge.svg)](https://github.com/DoubleGhosty/cool_fox/actions/workflows/ci.yml)

Experimental compatibility layer and shader translation runtime for running Xbox Series (Scarlett / GDK) x64 executables natively on Windows D3D12 (AMD Radeon and NVIDIA GeForce).

> **Legal notice:** This repository contains no copyrighted game code, keys, assets, or executables. It does not provide or link to any third-party software, and does not instruct or encourage bypassing console protections. Use only with software you are legally entitled to run. All original code here is licensed under **GPL-3.0-or-later** (see [LICENSE](LICENSE) and [docs/asset-policy.md](docs/asset-policy.md)).

---

## Key Features & Recent Improvements

- **Pareto Memory Residency & 32 GB RAM Support:** Adaptive GPU memory residency limits, preventing WDDM paging exhaustion (`0x8007000E`) on 16–32 GB systems.
- **Pipeline & Descriptor Stability:**
  - `VICTORY_PIPELINE_WAIT_MS=0` & `VICTORY_LIBRARY_STORE_LATER=1` in launchers to eliminate 500 ms frame stalls and 200 ms+ pipeline-library save hitches.
  - Panic-safe pipeline worker queue (`Pending::make_with`) so a failed translation never hangs waiting threads.
  - `submit_split` dynamic descriptor slot release (`list::tables_done`) and `alias_of` reference pinning (`Held` guard) to prevent slot exhaustion and use-after-free during async texture aliasing.
  - DirectStorage staging buffer cleanup and idle list/upload trimming (`VICTORY_LIST_IDLE_TRIM_S`).
  - Bindless heap root-signature flags (`CBV_SRV_UAV_HEAP_DIRECTLY_INDEXED`, `SAMPLER_HEAP_DIRECTLY_INDEXED`) are now probed at startup and omitted on runtimes that reject them, fixing "Unsupported bit-flag set" errors on older D3D12 runtimes.
  - Offline batch shader translator (`victory shaders`) catches panics per-shader so a single void-value assert no longer kills the entire translation run.
- **Shader & Ray Tracing Fixes:**
  - Shader LOD slot fix (leaves unused LOD slots undefined in array texture coordinate queries to pass DXIL validation).
  - Wave32 cascade lowering, scoped submerged shader lowering, and dynamic view address buffer indexing (`SLOT_VIEW_ADDRESSES`) for ray-tracing BVH descriptors.
  - Voxel visibility wave64 mask provenance recovery and blendshape pre-dispatch initialization.
- **NVIDIA & Video Synchronization:** Synchronous flush for decoded video frame copies (`VICTORY_THIN=0`, `VICTORY_JOIN_PARTS=0` by default in launchers for NVIDIA compatibility) and constant ambient lighting (`VICTORY_AMBIENT=constant:0.8`).
- **Wine / Proton & DXIL Robustness:** Code-generated root signatures for `vkd3d-shader`, single-latch DXIL loop CFG normalization, and 256 MiB pipeline worker stack.
- **Custom Resolution, Scaling & Framerate:**
  - `VICTORY_OUTPUT=1920x1080` (or `2560x1440`) and `VICTORY_RENDER_SCALE=0.667..1.0`.
  - `VICTORY_FRAME_INTERVAL_MS=16.667` (60 FPS) or `8.333` (120 FPS) while keeping simulation timing at 1.00x.
- **Renderer Ports (Performance):**
  - `VICTORY_PORT_GUARD=1` and `VICTORY_PORT_JOURNAL=1` now enabled by default in launchers: write-guard lock sharing and journal splitting save 2–5 ms per frame on all tested GPUs.
  - `VICTORY_PORT_CONSTANTS=1` and `VICTORY_PORT_CBV=1` remain opt-in for NVIDIA 8 GB stability; AMD users should use `Play-VIctory-AMD.cmd` which enables all ports.
  - `VICTORY_LEVELS_ON_DEMAND=1` enabled by default: card memory allocated only for texture mip levels the title holds, saving up to 3 GB of VRAM.

---

## How to Run

### Requirements
- Windows 10/11 x64
- CPU with AVX2 support
- Direct3D 12 GPU with Shader Model 6.6 and Mesh Shaders
- Rust toolchain (see `rust-toolchain.toml`) and MSVC C++ Build Tools (with Windows SDK)

### Build from Source

```powershell
git clone https://github.com/DoubleGhosty/cool_fox.git
cd cool_fox
cargo build --release --locked -p victory
```

On **Windows 10**, build with the D3D12 Agility SDK instead. Windows 10's own D3D12 has
no shader model 6.6 bindless heaps, and root signatures then fail with `0x80070057`:

```powershell
cargo build --release --locked -p victory --features agility-sdk
powershell -ExecutionPolicy Bypass -File scripts\get-agility-sdk.ps1
```

The feature makes `victory.exe` ask for Microsoft's `D3D12Core.dll` 1.619.6 in `D3D12\`
beside it, and the script fetches it from NuGet and puts it there. Such a build needs the
DLL: without it D3D12 does not start (`0x887E0003`). The default build is unchanged
and does not need it. Not for Wine or Proton.

The GPU tests on Windows 10 need the same. Build the test programs, run the script once
more (it also fills `target\debug\deps\D3D12\`), then run the same tests:

```powershell
cargo test --locked -p victory-shader -p victory-gpu --features victory-shader/agility-sdk,victory-gpu/agility-sdk --no-run
powershell -ExecutionPolicy Bypass -File scripts\get-agility-sdk.ps1
cargo test --locked -p victory-shader -p victory-gpu --features victory-shader/agility-sdk,victory-gpu/agility-sdk
```

### Launch

```powershell
$env:VICTORY_GAME_DIR = 'D:\path\to\your\title'
.\Play-VIctory.cmd
```

For AMD Radeon GPUs (RX 7000+, 16 GB+ VRAM) with all renderer ports enabled:

```powershell
.\Play-VIctory-AMD.cmd
```

For 4 GB VRAM GPUs or low-memory systems:

```powershell
.\Play-VIctory-4GB.cmd
```

| Launcher | Target | Renderer Ports | Notes |
|---|---|---|---|
| `Play-VIctory.cmd` | NVIDIA + AMD, 8 GB+ | 6, 7 on; 1, 5 opt-in | Safe default for all GPUs |
| `Play-VIctory-AMD.cmd` | AMD 16 GB+ | 1, 5, 6, 7 all on | Maximum performance; all ports from top community forks |
| `Play-VIctory-4GB.cmd` | 4 GB VRAM | 6, 7 on; 1, 5 off | Series S profile, reduced card buffers |
| `scripts\play.ps1` | Diagnostics | 6, 7 on; `-Safe` disables | Full logging; `-RayTracing` for RT |

### Diagnostics & Pre-warming

```powershell
copy victory.example.toml victory.toml      # set your local paths outside this repo
cargo build --locked --profile diag -p victory --features diagnostics
scripts\play.ps1 -Exe <path_to_executable>
```

`scripts\play.ps1 -Safe` disables newer experimental paths; `-RayTracing` enables ray-traced indirect lighting.

To pre-translate shaders or pre-warm cached pipelines ahead of time:

```powershell
target\diag\victory.exe shaders <archive_paths>
target\diag\victory.exe warm --used
```

`warm` has nothing to make until one run has written the pipelines down
(`scripts\play.ps1 -Exe <path> -Record`). Some switches are part of the shader
cache's key, so both commands should see the ones the runs do: run them as
`scripts\play.ps1 -Warm` and `scripts\play.ps1 -Shaders <archive_paths>`,
with the same `-Safe`, `-RayTracing` and `-More` as play.

Commands, debugging switches, and technical notes are documented in [docs/README-fxceless.md](docs/README-fxceless.md) and [docs/findings.md](docs/findings.md). Every `VICTORY_...` environment variable is also documented where it is read in the source.

---

## Performance Tuning (Renderer Ports)

Each renderer port eliminates a class of per-frame CPU work. They are tested on AMD first and may have edge cases on NVIDIA with limited VRAM.

| Port | Switch | What it does | Saving | Default |
|---|---|---|---|---|
| 1 | `VICTORY_PORT_CONSTANTS=1` | Title's 174 MB constants heap read in place by the card; no uploads or fills | ~6 ms, ~17 MB/frame fewer copies | Off (8 GB NVIDIA: C++ exception #15) |
| 5 | `VICTORY_PORT_CBV=1` | Constant buffers read as raw bytes; ~22,000 fills/frame eliminated | ~4.7 ms | Off (8 GB NVIDIA: dropped surfaces) |
| 6 | `VICTORY_PORT_GUARD=1` | Write-guard lock shared by faults and refreshes, one claim per 64 KiB block | ~2 ms | **On** |
| 7 | `VICTORY_PORT_JOURNAL=1` | Journal notes split into 64 parts with their own locks; heaps synced at submit | ~1–3 ms | **On** |
| RT | `VICTORY_PORT_RT=1` | Ray-tracing submission ordering optimized; fills forwarded from copies | Removes 15–61 ms stalls with RT on | Off |

Additional performance switches:
- `VICTORY_LEVELS_ON_DEMAND=1` — allocate card memory only for texture levels the title holds (saves up to 3 GB VRAM)
- `VICTORY_THIN=1` — record list calls instead of executing on title threads (AMD stable; NVIDIA: head skin may not draw)
- `VICTORY_JOIN_PARTS=8` — join submission parts across 8 threads
- `VICTORY_UPLOAD_AHEAD=1` — speculative texture uploads before the title asks
- `VICTORY_CARD_BUFFERS_MIN_KB=64` — minimum size for card-side buffer copies (lower = more buffers on card)
- `VICTORY_CARD_STEPS=2` — buffer fills on the GPU instead of stalling the CPU

---

## Architecture Highlights

- **Recorded Command Lists (`VICTORY_THIN`):** List calls are recorded on the title's threads and translated across worker threads.
- **Parallel Submission Prep (`VICTORY_ASYNC_SUBMIT`, `VICTORY_JOIN_PARTS`):** Frame preparation, AVX2 comparisons, descriptor records, and landings are parallelized and joined in submission order.
- **Write-Guard Relay (`VICTORY_RELAY`, `VICTORY_LAND`):** GPU-written buffers are read back by changed dwords on a dedicated relay thread.
- **On-Card Buffer Fills (`VICTORY_CARD_STEPS`):** Buffer fills depending on card-resident bytes execute on the GPU instead of stalling the CPU.
- **Dedicated Texture Copy Queue & Read-Back:** Textures upload on a dedicated copy queue; GPU-written textures read by the CPU are copied back cleanly.
- **Frame Record & Replay:** Per-pass timing and shader variant comparison via `VICTORY_RECORD` ([docs/replay.md](docs/replay.md)).

---

## Community & Portwatch

The [portwatch-vi.xyz](https://portwatch-vi.xyz/#top) tracker ranks compatibility layers by progress. As of November 2026, the leading open-source forks and their key techniques:

| Project | GPU Target | Techniques adopted here |
|---|---|---|
| [Belobug](https://github.com/Belobug/VIctory-Belobug-AMD) | AMD | PORT_CBV, PORT_CONSTANTS, PORT_GUARD, PORT_JOURNAL, ambient lighting, pipeline workers |
| [Kelvaroq](https://github.com/stellarr-studio/kelvaroq) | NVIDIA | Unified memory layer, async present, mesh via amplification, changed-page queries |
| [DexNom](https://github.com/DexNom/VIctory) | AMD | Ray tracing, sparse textures, parallel preludes, FSR 4.1.2 |
| [fxceless/victory-fork](https://github.com/fxceless/victory-fork) | NVIDIA | Shader cleanup passes, levels on demand, lookup cache, async submit |
| [avatarmanz](https://github.com/avatarmanz/VIctory-7900XTX) | AMD | Multi-threaded recording, renderer ports, frame record & replay |

Techniques from these projects not yet in `cool_fox` that are under investigation:
- **Unified memory layer** (Kelvaroq): serve GPU lookups from game memory with no copy, reducing per-frame copies from >100 MB to <8 MB.
- **Async present** (Kelvaroq): pipeline up to 2 frames ahead, decoupling CPU submission from GPU completion.
- **Descriptor-table patching** (Belobug): stores report the slots they write instead of scanning ~600,000 slots per frame.
- **Mesh draws via amplification stage** (Kelvaroq): eliminates the indirect-draw clamp pass.

---

## Credits & Upstream

This repository builds on the work of many contributors across the VIctory ecosystem, preserving commit history and attribution:

- **[VIctory](https://github.com/Marvisak/VIctory)** by Marvisak and contributors: loader, HLE services, D3D12 backend, and shader recompiler.
- **[fxceless/victory-fork](https://github.com/fxceless/victory-fork)**: base fork and upstream pull requests.
- **codec-d / CGBJ**: `cool_fox` runtime and pareto residency architecture.
- **[avatarmanz/VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX)**: multi-threaded command list recording, framerate unlocking, and renderer ports.
- **[DexNom/VIctory](https://github.com/DexNom/VIctory)**: ray tracing fixes, cube gradients, sparse textures, and Wave64/DXIL passes.
- **[Belobug/VIctory-Belobug-AMD](https://github.com/Belobug/VIctory-Belobug-AMD)**: ambient lighting, pipeline workers, and integer target blend protections.
- **[FQY55kai/ZJOU-VICTORY](https://github.com/FQY55kai/ZJOU-VICTORY)**, **[myokex/victory-amd](https://github.com/myokex/victory-amd)**, **Geckomacabre**, and **cxy0072019-sketch**.
- **[XCL](https://github.com/ivall/xcl)** (GPL-2.0-only) was studied as reference; no XCL code is included.

---

## License

GPL-3.0-or-later. See [LICENSE](LICENSE) and [docs/asset-policy.md](docs/asset-policy.md).
