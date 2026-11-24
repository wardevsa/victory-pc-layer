# Launches the title with the settings that matter on a weaker GPU.
# Usage:  .\jugar.ps1            (1920x1080)
#         .\jugar.ps1 1280x720   (another resolution)
#         .\jugar.ps1 -Diag      (the diagnostics build, target\diag: logs why
#                                 draws are dropped and dumps the arguments of
#                                 unknown pipeline methods to pipeline-dumps\)
#         .\jugar.ps1 -Precompiled  (makes device method 0x218's compute
#                                 pipelines, VICTORY_PRECOMPILED=1: faster, but
#                                 the picture can go grey; work in progress)
#         .\jugar.ps1 -Precompiled -Calls 1-500  (only those 0x218 calls)
# The title's executable is VICTORY_GAME_DIR if that names an .exe; if it
# names a folder, the largest .exe in it. Or pass it with -Game.
param(
    [string]$Resolution = "1920x1080",
    [switch]$Diag,
    [switch]$Precompiled,
    [string]$Calls = "",
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
# Title buffers of 1 MB or more keep a copy in the card's memory (up to 1 GB)
# instead of being copied every frame.
$env:VICTORY_CARD_BUFFERS_MIN_MB = "1"
$env:VICTORY_CARD_BUFFERS_MB = "1024"

$env:VICTORY_PRECOMPILED = if (-not $Precompiled) { "0" } elseif ($Calls) { $Calls } else { "1" }

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
