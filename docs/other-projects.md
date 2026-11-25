# Other projects (survey of 2026-11-24)

What else runs Xbox Series titles on a PC, and where each stands for the
title this repository is tested with. From each project's README and
repository on that date; their claims are theirs, not checked here.

| Project | Base, licence | The title | Worth looking at |
|---------|---------------|-----------|------------------|
| [VIctory](https://github.com/Marvisak/VIctory) (Marvisak) | own, GPL-3.0+ | menus at 30 fps, world reached but dark (its README) | the base of this repository |
| [victory-fork](https://github.com/fxceless/victory-fork) (fxceless) | VIctory, GPL-3.0+ | first gameplay scene rendered, 2-4 fps on an RTX 4070 | the base of this repository; most of the world's rendering work |
| [XCL](https://github.com/xcl-dev/xcl) (ivall, xcl-dev) | own (Rust), **GPL-2.0-only** | listed as "a newer retail game": menus at a steady 30 fps, gameplay not reached | ray tracing that works (the title's own BVH builder, BVH instructions emulated in shaders), exact texture decoding, the console FSR upscaler, wave64-on-wave32 notes; its licence keeps its code out of a GPL-3.0 tree, so ideas only, rewritten |
| [klbf-lab](https://github.com/PastiiJay/klbf-lab) (PastiiJay) | victory-fork, GPL-3.0 | ~2 fps; an AMD test branch at ~5.4 fps on an RX 9070 XT with ambient light back (purple) | the same idea as this repository (combine fixes); AMD-side fixes |
| [Heatwave](https://github.com/TOGZ819/Heatwave), [VIcky](https://github.com/Xeltax/VIcky) and many VIctory forks | VIctory or victory-fork | as their base, most unchanged | VIcky: 628 more shader translations (texture atomics, branch specialisation) in an offline scan |
| [TuxBox](https://github.com/brcly/TuxBox) | own; Linux (Proton) first | player selection prompt only | Linux |
| [xbox-emulator](https://github.com/omonkeyz/xbox-emulator) (TomBox continuation) | own (Rust), GPL-3.0 | not tested with it | other titles (racing game playable, battle-royale lobby) |
| [ScarlettX](https://github.com/Freminet/ScarlettX), [AnyBox](https://github.com/boykopovar/AnyBox), [AnyXboxX](https://github.com/LooperSalty/AnyXboxX), [xvalkyrie](https://github.com/sammwyy/xvalkyrie) | own | no | early; ScarlettX does not lift console shaders at all |

## What this means here

- victory-fork is the furthest along for this title, so it stays the base.
- Device method 0x218 (`docs/unknown-pipeline-methods.md`), implemented here
  and in none of the projects above as far as their READMEs say, took the
  world from about 3 to 12 frames per second on an RTX 2060 SUPER, with a
  grey picture still to fix.
- The black shadows are the title's ray tracing; XCL has that working and is
  the reference to learn from (rewriting, not copying, because of its
  licence).

## Tried on the test PC (2026-11-25)

NVIDIA RTX 2060 SUPER 8 GB, built from source after reading the changes
for network, process and file-deletion code; each with its own cache and a
copy of the saves.

| Project | Base | On this PC | Taken from it |
|---------|------|------------|---------------|
| [VIctory-Belobug-AMD](https://github.com/Belobug/VIctory-Belobug-AMD) (Belobug) | VIctory | the world in blocks of colour: 152 shaders do not translate for 32-lane waves (made for AMD) | nothing kept: its ambient light was tried and taken back out |
| [DexNom/VIctory](https://github.com/DexNom/VIctory) (DexNom) | VIctory | every shader translates for 32 lanes, but 25 s a frame and the picture grey and black | nothing |
| [victory-perf](https://github.com/vitutu1/victory-perf) (vitutu1) | victory-fork | 3-4 fps at half render scale (Series S), blurry and dark | nothing yet |
| [victory-fork](https://github.com/fxceless/victory-fork) `d510ee4` | VIctory | with its newer switches on, the loading picture and grey flicker over the world | merged (`9c184b3`); the newer switches behind `-Rapido` |
| [Flamingo](https://github.com/realblastex/flamingo) (realblastex) | victory-fork | 2.4-2.5 fps (Series S), no shader untranslated or refused; the card about 230 ms busy of 410 | the `DispatchX` thread-count fix for the structure builder |
| [VIctory](https://github.com/Marvisak/VIctory) `2e319e5` | own | being tried | |

Looked at, not tried: [VItoriaBr](https://github.com/uiliannn/VItoriaBr)
(uiliannn; NVIDIA RTX 5060 Ti, up to 6 fps, 397 compute pipelines refused
there), [ztrx](https://github.com/Zettrox/ztrx), and a repository named
for an emulator that had no code in it, only a README.
