# victory-pc-layer

The title of an Xbox Series dump the user owns, run on a Windows PC with an
8 GB NVIDIA card. Since 2026-11-26 this repository starts again from
**[cool_fox](https://github.com/DoubleGhosty/cool_fox)** (DoubleGhosty and
the community behind it, "CGBJ"): on the test PC it gave the best picture of
every tree tried, and the next work is its frame rate. The earlier tree (on
fxceless's victory-fork, with ray-traced light at about 2 fps) is kept in the
branch `base-victory-fork`.

**This repository contains no game files, code or assets.** You need your own
copy of the game. It is not affiliated with or endorsed by the game's
developer or publisher.

## Use

```powershell
cargo build --release -p victory
$env:VICTORY_GAME_DIR = '<folder of your dump>'
powershell -ExecutionPolicy Bypass -File .\jugar.ps1                 # Series S, compares on the card
powershell -ExecutionPolicy Bypass -File .\jugar.ps1 -Fabrica        # cool_fox's defaults alone
powershell -ExecutionPolicy Bypass -File .\jugar.ps1 -SinPelo        # hair strands skipped (faster)
powershell -ExecutionPolicy Bypass -File .\jugar.ps1 -Salida 1664x936  # less for the card
powershell -ExecutionPolicy Bypass -File .\jugar.ps1 -Rapido         # Play-VIctory.cmd's switch set
powershell -ExecutionPolicy Bypass -File .\jugar.ps1 -Preparar       # after each update, game closed
```

`-Preparar` translates every shader into the cache (`victory translate
--warm all`, with the host's wave size and view heap from the last run's
log) and makes the host pipelines earlier runs wrote down (`victory warm
--used`), so that the first run after a change of the translator does not
translate them while playing: after the merge of cool_fox `64b8967` some
700 pipelines waited at once and frames took 500-800 ms.

`jugar.ps1` runs cool_fox with its own defaults, which were the fastest on
the test PC, and keeps the picture as `3277b15` drew it (hair strands
drawn, no stand-in light): the work here is frame rate, not the picture. The log goes to `run-<console>.log`.

Through the AmericasLauncher (isaac-btw), turn its **Pipeline** button off
for this runner: with it on, every launch first runs `victory translate
--pipelines` over all of the title's shaders (20-40 minutes here), which
uses no cache and writes nothing the game reads.

## Results on the test PC

NVIDIA RTX 2060 SUPER 8 GB, AMD Ryzen 5 5600GT, 32 GB RAM, Windows 11.

| Run | Frame rate | Notes |
|-----|------------|-------|
| cool_fox `3277b15`, Series S, every switch at its default (2026-11-26) | 3.2-4 fps in the cell, 5 at the boat, up to 8 in cutscenes | the best picture so far; the card about 150 ms a frame, preparing about 110, copying back about 70; no traced light (a constant stands in) |
| the same with `Play-VIctory.cmd`'s switch set (now `-Rapido`) | about 2 fps | the card 250-380 ms a frame |
| cool_fox `64b8967`, shaders prepared ahead (`-Preparar`), defaults, at the boat | 3.2 fps median, 4-5 seen | preparing 142 ms, the card 136, copying back 51 (medians); the CPU compared 228 MB a frame; a crash in the title's script after the package lands in the water (fxceless/victory-fork issue 1209, every build) |
| the same with title buffers from 1 MiB compared on the card, hash passes and copies joined (`88cbf04`) | 6.3 fps median, 5-8 seen | preparing 98 ms, the card 58, copying back 60 (medians); the CPU compares 37 MB a frame instead of 265 |
| the same with the card kept awake and 6-core pools (`0abc767`) | 5.5-6.3 fps | the submitting thread waits 93-104 ms a frame for the card in about 5 settles, all for small fills of upload memory: next, `VICTORY_FILL_TITLE=1` |
| the earlier tree (`base-victory-fork`), Series X with ray tracing | 1.6-2 fps | traced light in the cell, textures noisy |

## Credits

- **[cool_fox](https://github.com/DoubleGhosty/cool_fox)**: the base, and
  through it codec_d's cool_fox, avatarmanz's VIctory-7900XTX, DexNom,
  Belobug and their contributors (see below).
- **[VIctory](https://github.com/Marvisak/VIctory)** by Marvisak and its
  contributors, the base of all of them.

---
# cool_fox (CGBJ Runtime Continuation)

[![CI](https://github.com/DoubleGhosty/cool_fox/actions/workflows/ci.yml/badge.svg)](https://github.com/DoubleGhosty/cool_fox/actions/workflows/ci.yml)
[![PortWatch VI](https://img.shields.io/badge/PortWatch%20VI-91.5%25%20Playable-brightgreen)](https://portwatch-vi.xyz/#effort-cgbj)

Active community continuation and development branch of `cool_fox` (the CGBJ runtime, ranked **#1 / 91.5% Playable** on PortWatch VI).

This build unites the cutting-edge optimizations from leading PortWatch projects (`avatarmanz`, `codec-d`, `DexNom`, `Belobug`, and community contributors) into a unified, verified runtime for both AMD Radeon and NVIDIA GeForce GPUs.

---

## Key Features & Improvements

- **Pareto Memory Residency & 32 GB RAM Support:** Adaptive GPU memory residency limits, preventing WDDM paging exhaustion (`0x8007000E`) on mid-range and 32 GB systems.
- **Shader LOD Slot Fix (PR #2):** Leaves unused LOD slots undefined in array texture coordinate queries, resolving DXIL validator errors on retail shaders.
- **NVIDIA & Video Stream Synchronization (PR #6):** Synchronous flush for decoded video frame copies, eliminating stutter and repeated frames in opening videos during multithreaded recording (`VICTORY_THIN=1`), and constant ambient lighting (`VICTORY_AMBIENT=constant:0.8`).
- **Wine / Proton & DXIL Robustness (PR #10):** Code-generated root signatures replacing fragile comment pragmas for vkd3d-shader, single-latch DXIL loop CFG normalization, and 256 MiB pipeline worker stack.
- **Voxel Visibility Recovery:** Restores wave64 mask provenance when restored from scalar copies, enabling foliage and volumetric voxel visibility shaders to pass compilation.
- **Custom Resolution & Scaling:** `VICTORY_OUTPUT=1920x1080` (or `2560x1440`) and `VICTORY_RENDER_SCALE=0.667..1.0` with the 4K backbuffer pool check bypass.
- **Frame Rate Unlock:** `VICTORY_FRAME_INTERVAL_MS=16.667` (60 FPS) or `8.333` (120 FPS) while keeping game simulation clock strictly at 1.00x.
- **Renderer Port 5 (Zero-Copy CBV):** `VICTORY_PORT_CBV=1` reads constant buffers as raw bytes in-place, eliminating ~22,000 buffer copies per frame.

---

## How to Run

You need Windows 10/11 x64, a CPU with AVX2, a Direct3D 12 GPU with shader model 6.6 and mesh shaders, Rust (1.98+), MSVC build tools, and your own legal Xbox Series dump of the game.

### Build from Source

```powershell
git clone https://github.com/DoubleGhosty/cool_fox.git
cd cool_fox
cargo build --release --locked -p victory
```

### Launch

```powershell
$env:VICTORY_GAME_DIR = 'D:\Games\MyDump'
.\Play-VIctory.cmd
```

For 4GB / 8GB VRAM laptops or low-memory systems, you can also launch with:

```powershell
.\Play-VIctory-4GB.cmd
```

---

## Upstream & Acknowledgements

- **codec-d / CGBJ:** Original `cool_fox` runtime and pareto residency architecture.
- **AvatarManz (VIctory-7900XTX):** Multi-threaded command list recording, framerate unlocking, and renderer ports.
- **DexNom:** Wave64 lowering, scalar recovery, and shader structure passes.
- **Belobug:** Ambient lighting and integer target blend protections.
- **Marvisak & fxceless:** Foundational VIctory compatibility layer.

**Legal notice:** This repository contains no copyrighted game code, assets, or executables. All code is licensed under GPL-3.0-or-later.
copy victory.example.toml victory.toml      # then set your paths, outside this folder
cargo build --locked --profile diag -p victory --features diagnostics
scripts\play.ps1 -Exe <path of your copy's executable>
```

`scripts\play.ps1 -Safe` leaves out the newest, faster ways; try it first when
something is wrong. `-RayTracing` turns the title's ray-traced indirect light on.

Before the first run, ahead of time (both can be stopped and go on later):

```powershell
target\diag\victory.exe shaders <the shader archives>   # every shader translated, minutes
target\diag\victory.exe warm --used                     # pipelines earlier runs drew with, for a new driver or build
```

Commands, debugging switches and upstream's own description:
[docs/README-fxceless.md](docs/README-fxceless.md). Every `VICTORY_...` switch is
described in a comment where it is read.

## What is different here

In short; the reasons and the measurements are in [docs/findings.md](docs/findings.md).

- **List calls are written down, not carried out, on the title's threads**
  (`VICTORY_THIN`): eight threads of ours translate them.
- **Submissions are prepared by a thread of ours and joined**
  (`VICTORY_ASYNC_SUBMIT`, `VICTORY_JOIN_PARTS`), with what can be spread over
  threads spread: refreshes, compares (AVX2), descriptor records, landings.
- **What shaders wrote comes back beside the write guards**, by the dwords that
  changed, on a thread of its own (`VICTORY_RELAY`, `VICTORY_LAND`).
- **Buffer fills that need bytes still on the card are done on the card**
  (`VICTORY_CARD_STEPS`) instead of waiting for it.
- **The title's writes are caught more cheaply**: one view opened at a fault,
  blocks that rest, places in the title's code that look before they write.
- **Textures go up on a copy queue of their own** and a slot shows one only after
  its upload ran, which also ended a device loss that came every 15 minutes or so.
- **Boats float**: textures that shaders write and the title's CPU reads go back
  to title memory (from fxceless and Geckomacabre, on by default here).
- **Frame record and replay** with per-pass times and shader variants compared
  turn by turn ([docs/replay.md](docs/replay.md)).

## Credits

This is a tree of other people's work with more on top. The history is kept, and
commits taken from elsewhere name the commit and its author.

- **[VIctory](https://github.com/Marvisak/VIctory)** by Marvisak and its
  contributors: the loader, the runtime services, the Direct3D 12 back end and
  the shader recompiler.
- **[fxceless/victory-fork](https://github.com/fxceless/victory-fork)**: the fork
  this one started from, and the people whose pull requests it carries.
- **[DexNom/VIctory](https://github.com/DexNom/VIctory)**: ray tracing fixes,
  cube gradients, sparse textures.
- **[Belobug/VIctory-Belobug-AMD](https://github.com/Belobug/VIctory-Belobug-AMD)**:
  the ambient light idea, pipeline workers, and work by romainhedouin.
- **[FQY55kai/ZJOU-VICTORY](https://github.com/FQY55kai/ZJOU-VICTORY)**.
- **[myokex/victory-amd](https://github.com/myokex/victory-amd)**: the guard over
  every view of a page (with Spexivus, joroyt and tightfall), and Joshua Odom's
  reads into title memory.
- **Geckomacabre** and **cxy0072019-sketch**: the texture read-back.
- **[XCL](https://github.com/ivall/xcl)** (GPL-2.0-only) was read for how the same
  title behaves under another layer; no code of it is here.
- This fork's changes were made with [Claude Code](https://claude.com/claude-code).

## License

GPL-3.0-or-later, as VIctory. See [LICENSE](LICENSE); the one file that is not
covered is named in [docs/asset-policy.md](docs/asset-policy.md).
