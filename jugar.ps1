# Launches Grand Theft Auto VI with the settings that matter on a weaker GPU.
# Usage:  .\jugar.ps1            (1920x1080)
#         .\jugar.ps1 1280x720   (another resolution)
#         .\jugar.ps1 -Diag      (the diagnostics build, target\diag: logs why
#                                 draws are dropped and dumps the arguments of
#                                 unknown pipeline methods to pipeline-dumps\)
#         .\jugar.ps1 -Precompiled  (makes device method 0x218's compute
#                                 pipelines, VICTORY_PRECOMPILED=1: faster, but
#                                 the picture can go grey; work in progress)
# The game's executable is taken from VICTORY_GAME_DIR (the folder holding
# game_scarlett_master.exe, or that file itself), or pass it with -Game.
param(
    [string]$Resolution = "1920x1080",
    [switch]$Diag,
    [switch]$Precompiled,
    [string]$Game = $(
        if ($env:VICTORY_GAME_DIR -like "*.exe") { $env:VICTORY_GAME_DIR }
        elseif ($env:VICTORY_GAME_DIR) { Join-Path $env:VICTORY_GAME_DIR "game_scarlett_master.exe" }
    )
)

if (-not $Game -or -not (Test-Path $Game)) {
    Write-Error "Game executable not found. Set VICTORY_GAME_DIR to the game's folder or pass -Game <path to game_scarlett_master.exe>."
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

$env:VICTORY_PRECOMPILED = if ($Precompiled) { "1" } else { "0" }

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
