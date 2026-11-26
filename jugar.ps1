# Launches the title with the settings that matter on a weaker GPU.
# Usage:  .\jugar.ps1            (1920x1080)
#         .\jugar.ps1 1280x720   (another resolution)
#         .\jugar.ps1 -Diag      (the diagnostics build, target\diag: logs why
#                                 draws are dropped and dumps the arguments of
#                                 unknown pipeline methods to pipeline-dumps\)
#         .\jugar.ps1 -Diag -Tiempos  (the card's time of every command, by
#                                 program, in diag-<res>.log)
#         .\jugar.ps1 -Precompiled  (makes device method 0x218's compute
#                                 pipelines, VICTORY_PRECOMPILED=1: faster, but
#                                 the picture can go grey; work in progress)
#         .\jugar.ps1 -Precompiled -Calls h0000-7fff  (only 0x218 programs whose
#                                 hash starts in that range: the same ones
#                                 every run; plain numbers select calls)
#         .\jugar.ps1 -Precompiled -Skip <hash>,<hash>  (every 0x218 program
#                                 but these)
#         .\jugar.ps1 -Fast      (keeps the texture guard: a little faster,
#                                 but the intro flickers)
#         .\jugar.ps1 -SinWide   (no wave-wide form of wave64 programs: if
#                                 the card hangs with it)
#         .\jugar.ps1 -SinRT     (no ray-tracing structure builder: if the
#                                 title crashes with it)
#         .\jugar.ps1 -RTCompleto  (the builder's results copied back into
#                                 title memory, no quarantine: a test now that
#                                 its dispatches stay inside their arrays)
#         .\jugar.ps1 -RTRefrescar  (the structures' pages brought up from
#                                 title memory again every list: interiors black)
#         .\jugar.ps1 -RTEncierro  (none of the builder's results copied
#                                 back: the old default, dark)
#         .\jugar.ps1 -Rapido    (victory-fork's newer speed switches: faster
#                                 on its PC, flickered on this one)
#         .\jugar.ps1 -Mejor     (victory-fork's best set, run 279: 9-12 fps
#                                 on an RTX 4070, water read back; with
#                                 -SinRT it is that set as tested there)
#         .\jugar.ps1 -Luz 0.6   (the least indirect light, tinted by the sky:
#                                 interiors readable; 0.3 darker, 1 brighter)
#         .\jugar.ps1 -Texturas baja -VentanasMB 1536  (ray-tracing copies in
#                                 the card's memory instead of over the bus)
#         .\jugar.ps1 -CopiasRT 1023  (copies of memory the ray tracing
#                                 reaches: fewer for less RAM; 2047 by default)
#         .\jugar.ps1 -Texturas alta|media|baja  (texture quality; media by
#                                 default, which fits an 8 GB card)
#         .\jugar.ps1 -CardMB 256  (less card memory for buffer copies, more
#                                 for textures, but much slower; 1024 by default)
# The title's executable is VICTORY_GAME_DIR if that names an .exe; if it
# names a folder, the largest .exe in it. Or pass it with -Game.
param(
    [string]$Resolution = "1920x1080",
    [switch]$Diag,
    [switch]$Precompiled,
    [switch]$Fast,
    [switch]$SinWide,
    [switch]$SinRT,
    [switch]$RTCompleto,
    [switch]$RTEncierro,
    [switch]$RTRefrescar,
    [switch]$Rapido,
    [switch]$Mejor,
    [string]$Luz = "",
    [int]$VentanasMB = 0,
    [switch]$Tiempos,
    [switch]$SinPelo,
    [int]$CopiasRT = 2047,
    [int]$CardMB = 1024,
    [ValidateSet("alta", "media", "baja")]
    [string]$Texturas = "media",
    [string]$Calls = "",
    [string]$Skip = "",
    [string]$Game = $(
        if ($env:VICTORY_GAME_DIR -like "*.exe") { $env:VICTORY_GAME_DIR }
        elseif ($env:VICTORY_GAME_DIR -and (Test-Path $env:VICTORY_GAME_DIR -PathType Container)) {
            Get-ChildItem $env:VICTORY_GAME_DIR -Filter *.exe -File |
                Sort-Object Length -Descending | Select-Object -First 1 -ExpandProperty FullName
        }
    )
)

if (-not $Game -or -not (Test-Path $Game)) {
    Write-Error "Title executable not found. Set VICTORY_GAME_DIR to it (or to its folder), or pass -Game <path to the .exe>."
    exit 1
}

# The display size the console reports to the game (the emulator says 4K by default).
$env:VICTORY_DISPLAY_MODE = $Resolution
# Sends work to the GPU without waiting for each submission, and lands the
# results on a thread of its own (implies VICTORY_LATE_WAIT=3).
$env:VICTORY_RELAY = "2"
# Finds what shaders wrote on several threads (faster copy-back).
$env:VICTORY_WIDE_BACK = "1"
# Title buffers of 1 MB or more keep a copy in the card's memory (up to
# -CardMB, 1024 by default) instead of being copied every frame. On an 8 GB
# card 256 left textures about 700 MB more (10,800 waiting for room instead
# of 13,100), but copying back what shaders wrote went from 180-280 to
# 440-620 ms a frame: about 3 fps fell to 1.7.
$env:VICTORY_CARD_BUFFERS_MIN_MB = "1"
$env:VICTORY_CARD_BUFFERS_MB = "$CardMB"
# Textures in view memory are checked at every refresh instead of every 8th
# frame while their guard sees no write: the startup screens and the opening
# cutscene otherwise flicker and repeat frames (victory-fork issue #269).
# Costs some milliseconds a frame; -Fast keeps the guard.
$env:VICTORY_TEXTURE_GUARD = if ($Fast) { "1" } else { "0" }
# Texture quality: large textures the title fills go without their most
# detailed level (media: a quarter of the memory and upload) or two (baja).
# With every level (alta) an 8 GB card filled up and 13,000 textures waited
# for room for the rest of the run.
$env:VICTORY_TEXTURE_DROP_MIPS = switch ($Texturas) { "alta" { "0" } "media" { "1" } "baja" { "2" } }
# Textures picked by index: up to 384 MB and 400 ms of uploads a frame
# (192 MB / 250 ms otherwise), so the queue drains faster at 3 fps (after
# klbf-lab's per-frame budget; victory-fork names it VICTORY_TEXTURE_STAGE_MB/MS
# and triples it on its own while a backlog of 1,024 or more drains).
$env:VICTORY_TEXTURE_STAGE_MB = "384"
$env:VICTORY_TEXTURE_STAGE_MS = "400"
if ($Mejor) {
    # victory-fork's run 279 (2026-11-25), "the set that plays best so far":
    # the switches of its diagnostics/README.md plus VICTORY_CARD_HASH_LEN=1.
    # 9-12 fps in rooms and on the water on an RTX 4070 12 GB, water right
    # (boats float: VICTORY_TEXTURE_BACK=1). Card windows and buffer copies as
    # victory-nvidia-8gb sizes them for an 8 GB card (2048 / 1024 MB).
    $env:VICTORY_LEVELS_ON_DEMAND = "1"
    $env:VICTORY_ASYNC_SUBMIT = "1"
    $env:VICTORY_QUEUE2 = "1"
    $env:VICTORY_INDIRECT_WINDOW = "1"
    $env:VICTORY_TEXTURE_NO_WAIT = "1"
    $env:VICTORY_MERGE_GAP = "64"
    $env:VICTORY_MERGE_RUNS = "4096"
    $env:VICTORY_STAGING_POOL = "0"
    $env:VICTORY_COPY_PLAIN = "1"
    $env:VICTORY_JOIN_PARTS = "1"
    $env:VICTORY_BACK_OWN_VIEW = "1"
    $env:VICTORY_INDIRECT_GATHER = "1"
    $env:VICTORY_CARD_REHASH_RUNS = "1"
    $env:VICTORY_CARD_WINDOWS_MB = "2048"
    $env:VICTORY_CARD_BUFFERS_MIN_KB = "64"
    $env:VICTORY_CARD_BUFFERS_MB = "$CardMB"
    $env:VICTORY_TEXTURE_BACK = "1"
    $env:VICTORY_CARD_HASH_LEN = "1"
    # Not in that set.
    Remove-Item Env:VICTORY_RELAY, Env:VICTORY_WIDE_BACK, Env:VICTORY_CARD_BUFFERS_MIN_MB -ErrorAction SilentlyContinue
}
if ($Rapido) {
    # victory-fork's newer switches, each measured in its README (first rooms
    # 7-10 fps on an RTX 4070): draw windows with plain copies, fetches on a
    # copy queue, parts joined, copy-back through our own view, scattered
    # indirect records clamped once per list, card rehash runs joined, texture
    # refreshes that do not wait for remaps. Off by default: on this PC
    # victory-fork with all of them on flickered between the loading picture
    # and grey in the prologue (2026-11-25).
    $env:VICTORY_INDIRECT_WINDOW = "1"
    $env:VICTORY_COPY_PLAIN = "1"
    $env:VICTORY_QUEUE2 = "1"
    $env:VICTORY_JOIN_PARTS = "1"
    $env:VICTORY_BACK_OWN_VIEW = "1"
    $env:VICTORY_INDIRECT_GATHER = "1"
    $env:VICTORY_CARD_REHASH_RUNS = "1"
    $env:VICTORY_TEXTURE_NO_WAIT = "1"
    # Copies in a list that continue the one before become one (the frame
    # summary names it; 26-31 a frame here, 2026-11-26).
    $env:VICTORY_JOIN_COPIES = "1"
}
# -VentanasMB 1536: copies of windows of title memory outside buffers (the
# ray-tracing structures among them) in the card's memory, up to that many
# MB (VICTORY_CARD_WINDOWS_MB), instead of the PC's memory the card reads
# over the bus: with the structures' pages kept, the card read 12.6 GB of
# shared memory and took about 310 ms a frame (2026-11-26). The card needs
# the room: -Texturas baja leaves it.
if ($VentanasMB -gt 0) { $env:VICTORY_CARD_WINDOWS_MB = "$VentanasMB" } else { Remove-Item Env:VICTORY_CARD_WINDOWS_MB -ErrorAction SilentlyContinue }
# Console wave64 compute programs that share memory inside per-lane branches
# run as two 32-lane halves with their branches taken wave-wide
# (VICTORY_WAVE_WIDE=1): on a 32-lane card they were otherwise emitted for
# 64-lane waves and refused (shadow masks, screen-space reflections, culling,
# decals: 10 of the 14 refused in the prologue). The particle light-atlas
# programs that once hung a card in this form stay out (shader_cache.rs).
# -SinWide turns it off.
$env:VICTORY_WAVE_WIDE = if ($SinWide) { "0" } else { "1" }
# Ray tracing: the title's indirect light, bounce and lamp and sun shadows
# walk structures that the 52 builder programs of XboxBvhBuilder.dll build
# and refit on the GPU every frame. The layer runs those stages only all
# together, and only with VICTORY_BVH_BUILDER=1; on a 32-lane card all 52
# now translate and are made (victory translate --pipelines on the dll).
# Without them whatever the sun does not reach directly stays black.
# What the builder stores stays in the host's copies (quarantine): copied
# back into title memory it crashed the title within seconds, with
# empty-box bounds (+-infinity) in its pointers, and comparing 1 GB a frame
# for it took the run to 0.1 fps. The programs that walk the structures
# read the host's copies anyway.
# The cause was found since (after Flamingo): the builder dispatches by
# thread counts, which the layer took as groups, and stored past its
# arrays. With that fixed, -RTCompleto copies its results back (no
# quarantine), so the title also gets the sizes and counters it reads.
# -SinRT turns the builder off (if the title crashes with it).
$env:VICTORY_BVH_BUILDER = if ($SinRT) { "0" } else { "1" }
# By default only the stores through title memory by address (the
# structures themselves) stay in the host's copies; what the builder writes
# into the title buffers it was given (sizes, counters) comes back
# (VICTORY_BVH_QUARANTINE=2, after VIctory-7900XTX, where rooms get their
# indirect light that way at about a third less frame rate). Everything kept
# out (=1, -RTEncierro) left the picture dark; everything back (-RTCompleto)
# showed the light at 0.1 fps.
$env:VICTORY_BVH_QUARANTINE = if ($SinRT -or $RTCompleto) { "0" } elseif ($RTEncierro) { "1" } else { "2" }
# Copies of title memory the ray-tracing programs reach by address (4 MiB
# windows mostly): 1023 ran out within a second of the builder starting and
# left about 40 pages a frame reading zeros. 2047 hold up to 8 GB of this
# PC's RAM; the log's "touched pages that had no copy" lines say how many
# still find no number (no_number) and how many MB the copies hold.
$env:VICTORY_MEMORY_COPIES = "$CopiasRT"
# By default (-RTRefrescar turns it off): the copies of title memory outside buffers (where the
# structures lie) are taken once and then left to the card, so title memory
# no longer goes over what the builder built there (VICTORY_BVH_KEEP=1, see
# memory.rs keep_windows). With it the cell and the corridors got their
# ray-traced light for the first time (2026-11-26).
$keep = -not ($RTRefrescar -or $SinRT)
if ($keep) { $env:VICTORY_BVH_KEEP = "1" } else { Remove-Item Env:VICTORY_BVH_KEEP -ErrorAction SilentlyContinue }
# With it, windows of 256 KiB instead of 4 MiB and up to 16,383 copies: on
# the 4 MiB grid all 2,047 copies (8.4 GB of RAM) were taken and 40 to 150
# pages a frame that rays walked had none, read as zeros (2026-11-26). The
# finer grid holds 16 times the pages in the same memory (16,383 copies at
# most 4 GB); VIctory-7900XTX keeps the structures in 64 KiB pages.
if ($keep) {
    $env:VICTORY_WINDOW_KB = "256"
    if (-not $PSBoundParameters.ContainsKey('CopiasRT')) { $env:VICTORY_MEMORY_COPIES = "16383" }
} else {
    Remove-Item Env:VICTORY_WINDOW_KB -ErrorAction SilentlyContinue
}
# -Luz 0.6: the least indirect light (Belobug's VIctory-Belobug-AMD, also in
# VIctory-7900XTX): right after the title's program that resolves indirect
# light, its output is raised to this level, tinted by the sky the title
# draws, and reflections are held to it. Interiors are readable without the
# ray-traced light; off by default (crate::ambient).
# -SinPelo: hair strands left out (VICTORY_HAIR=0): about 80 ms of the
# card's time a frame on this PC; characters keep their hair meshes.
if ($SinPelo) { $env:VICTORY_HAIR = "0" } else { Remove-Item Env:VICTORY_HAIR -ErrorAction SilentlyContinue }
if ($Luz) { $env:VICTORY_AMBIENT = $Luz } else { Remove-Item Env:VICTORY_AMBIENT -ErrorAction SilentlyContinue }
# Large tiled textures detiled on 4 CPU threads (victory-fork PR #271).
$env:VICTORY_TEXTURE_DETILE_WORKERS = "4"

$env:VICTORY_PRECOMPILED = if (-not $Precompiled) { "0" } elseif ($Calls) { $Calls } else { "1" }
$env:VICTORY_PRECOMPILED_SKIP = $Skip

Set-Location $PSScriptRoot
$exe = ".\target\release\victory.exe"
$log = "run-$Resolution.log"
if ($Diag) {
    # cargo build --release -p victory --features diagnostics --target-dir target\diag
    $exe = ".\target\diag\release\victory.exe"
    $log = "diag-$Resolution.log"
    $env:VICTORY_NATIVE_PSO_DUMP = Join-Path $PSScriptRoot "pipeline-dumps"
    # -Tiempos: the card's time of every command it runs, by program
    # (VICTORY_GPU_TIMING; the run is slower: each list is sent on its own).
    # The log's "the card's time by command (timing run)" lines.
    # Timestamps are read only where each list is submitted and waited for
    # on its own (`Host::submit`), not through the relay: the relay is off
    # for it (with it on, a run of 9 minutes logged no command, 2026-11-26).
    if ($Tiempos) {
        $env:VICTORY_GPU_TIMING = "1"
        # Commands are noted (and timed) only with the full device-removed
        # data, as victory-fork's timing runs have it; it also says which
        # command the card or the driver was in if the run dies (a run with
        # the timing alone logged no command and ended in the driver).
        $env:VICTORY_D3D12_DRED = "1"
        # Unset, the relay is on (shadow.rs `relay`).
        $env:VICTORY_RELAY = "0"
        Remove-Item Env:VICTORY_LATE_WAIT -ErrorAction SilentlyContinue
    } else {
        Remove-Item Env:VICTORY_GPU_TIMING -ErrorAction SilentlyContinue
        # Breadcrumbs and the faulting address, cheap (`light`): where a lost
        # card or a driver crash stopped.
        $env:VICTORY_D3D12_DRED = "light"
    }
}
& $exe run $Game --execute 2>&1 | Tee-Object -FilePath $log
