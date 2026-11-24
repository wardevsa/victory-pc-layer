# Launches the title with the settings that matter on a weaker GPU.
# Usage:  .\jugar.ps1            (1920x1080)
#         .\jugar.ps1 1280x720   (another resolution)
#         .\jugar.ps1 -Diag      (the diagnostics build, target\diag: logs why
#                                 draws are dropped and dumps the arguments of
#                                 unknown pipeline methods to pipeline-dumps\)
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
    $env:VICTORY_PIPELINE_DUMP = Join-Path $PSScriptRoot "pipeline-dumps"
}
& $exe run $Game --execute 2>&1 | Tee-Object -FilePath $log
