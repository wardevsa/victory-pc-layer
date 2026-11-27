# Running on Linux (Fedora) with Proton

VIctory is a Windows program, and on Linux it runs under Proton, whose
vkd3d-proton turns Direct3D 12 into Vulkan. Where it stands (2026-11-26,
GE-Proton11-14, RTX 5060 8 GB, Xeon W-1390P, 62 GB, Series S at 1080p): menus
at 30 fps, the prologue at about 200 ms a frame with the card 93-100 % busy,
the world loaded in 21 s. Rooms are too dark (no ray tracing, as on Windows).

What it took, in the order it was found (each in its own code comment):

- **The log.** Proton passes no stderr through: `VICTORY_LOG_FILE` (the script
  sets `~/.local/state/victory/victory.log`); crash reports and panics go there too.
- **The pipeline cache.** vkd3d-proton refused an empty pipeline library
  (`E_INVALIDARG` for a non-null pointer of length 0), so no pipeline was ever
  kept and every start compiled some 1,300 again.
- **A pipeline that ends its thread.** One compute shader (`fdd56d97a806bcca`)
  overflows a worker's 256 MiB stack in vkd3d-proton's DXIL-to-SPIR-V, and
  Wine ends the thread without a word; every frame then waited 500 ms for it.
  Dead workers are now noticed and replaced, and the pipeline is made once more
  on a thread with a 4 GiB stack (reserved, not committed;
  `VICTORY_PIPELINE_RESCUE=0`: not). Only if that thread ends too is the pipeline
  listed in `refused-pipelines.txt` in the cache folder (`VICTORY_REFUSED_RETRY=1`)
  and left out: its dispatch is then dropped, which #39 saw right before the
  title's write to 0 at `+0x23cf683`.
- **The wineserver's views.** The title shows its physical pages in some
  30,000 views. The wineserver keeps a process's views in a list and walks
  all of them at every map and unmap, while the client holds Wine's one lock
  for all memory: 1-2 ms each, and every fault, protection change and write
  watch of every thread waited behind them (the title's own frame took a
  second). `patches/wineserver-view-tree.patch` (against GE-Proton11-14's wine) keeps them in
  a tree as well; built against GE-Proton11-14 and installed as the compat
  tool `GE-Proton11-14-victory`, which `scripts/play-proton.sh` uses when it
  is there. The title's own frame: about 18 ms.
- **Wine's memory lock otherwise.** `QueryWorkingSetEx` holds it while it reads
  `/proc/self/pagemap` (`VICTORY_COMMITTED_QUERY=1`, set by the script); write
  watches are asked in pieces of 8 MiB (`VICTORY_WATCH_CHUNK_MB`).
- **The card's memory.** Linux moves nothing that does not fit to the PC's
  memory: what the desktop asks for next is refused (the shell aborted).
  Freed texture and staging memory is used again once the card is done with
  it (`VICTORY_POOL_FENCED`), small cards keep fewer empty blocks, and under
  Wine the budget planned with is the system's less 1 GiB
  (`VICTORY_CARD_BUDGET_MB`).
- **Thread priorities.** Wine sets the niceness a thread's priority asks for
  only if it may raise priority (a hard `RLIMIT_NICE` above 20); otherwise all
  run alike and the title's threads wait for cores behind our pipeline
  workers and streaming. In the `gamemode` group (allowed -10 on Fedora): about
  105 ms a frame against 120, p90 114 against 140. The script says so when off.
- **Do not change `VICTORY_OUTPUT` on Series S**: any size but 1920x1080 made
  the title divide by zero at +0x28f03e8 while loading (its FidelityFX jitter).

## 1. What the PC needs

- **The NVIDIA driver from RPM Fusion** (`akmod-nvidia`), not nouveau.
  vkd3d-proton needs shader model 6.6 and mesh shaders, which it gets from the
  proprietary driver's Vulkan.
- **umu-launcher**, to run Windows programs under Proton outside Steam. It is not
  in Fedora's own repositories: take it from its GitHub releases. With
  `PROTONPATH=GE-Proton` (the script's default) it downloads GE-Proton itself.
  For Steam's Proton instead, see step 4.
- **Your own complete dump of the title**, outside this checkout.

## 2. Build victory.exe on Fedora

```sh
sudo dnf install clang llvm                       # clang-cl and llvm-lib for C parts of dependencies
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain none
. ~/.cargo/env
cargo install --locked cargo-xwin                 # Windows SDK and MSVC CRT, fetched on first use
cd cool_fox
cargo xwin build --release --locked -p victory --target x86_64-pc-windows-msvc
```

`rust-toolchain.toml` makes rustup install the pinned toolchain. The build ends in
`target/x86_64-pc-windows-msvc/release/victory.exe`. On first use, cargo-xwin downloads
the Windows SDK and CRT files, which come under Microsoft's license.

For the diagnostics build (log lines, F3 panel, frame recording):
`cargo xwin build --locked --profile diag -p victory --features diagnostics --target x86_64-pc-windows-msvc`,
then point `VICTORY_EXE` at `target/x86_64-pc-windows-msvc/diag/victory.exe`.

## 3. Start it

```sh
VICTORY_GAME_DIR=~/Games/MyDump scripts/play-proton.sh
```

[`scripts/play-proton.sh`](../scripts/play-proton.sh) sets what `Play-VIctory.cmd
pareto-residency` sets: the Series S profile and the residency settings. It turns
Linux paths into Wine's `Z:` paths, and keeps the shader cache in
`~/.cache/victory/cache-pareto` and saves in `~/.local/share/victory/data`, so a new
Wine prefix loses neither. The first start translates shaders and stutters; the
cache makes later ones smoother.

Settings you set yourself win, for example
`VICTORY_LOG=info VICTORY_PANEL=1 VICTORY_GAME_DIR=... scripts/play-proton.sh`.
`PROTON_LOG=1` makes Proton write its own log file into your home folder.

## 4. Or through Steam

Add `victory.exe` as a non-Steam game, force a Proton version in its properties
(Proton Experimental or GE-Proton), and set its launch options, with your own paths
written as Wine sees them:

```
VICTORY_GAME_DIR='Z:\home\you\Games\MyDump' VICTORY_CACHE_DIR='Z:\home\you\.cache\victory\cache-pareto' %command% run 'Z:\home\you\Games\MyDump\game_scarlett_master.exe' --console series-s --execute
```

The settings in `scripts/play-proton.sh` go in front of `%command%` the same way.

## What may break first

- **D3D12 features through vkd3d-proton**: mesh shaders, pipeline libraries,
  ray tracing (off by default here). Look in the log for pipelines that could not be made.
- **Our own small HLSL passes**, compiled at run time with `D3DCompile`. Proton
  ships its own `d3dcompiler_47`, which may refuse one of them.
- **Speed of noticing the title's writes**: Windows' write watch is done by Wine
  with page protection and signals, which can cost more than on Windows.
- **A missing function**: `--unimplemented trap` (the default) stops at an
  unimplemented console import and names it. A missing Wine export shows in
  Proton's log.

Logs to keep when something fails: the terminal output, `PROTON_LOG=1`'s file, and
with the diagnostics build a run with `VICTORY_LOG=info`.
