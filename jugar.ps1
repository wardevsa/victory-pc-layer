# Runs the title on this tree (Vice-Tay's newest) the way it was measured on
# the RTX 2060 SUPER test PC (Ryzen 5 5600GT, 6 cores, 32 GB): Series S, in a
# window, the release build, Vice-Tay's six-core preset (scripts\play.ps1
# -Machine 3060: 6/4/3 worker threads, up to 3 GB of shader-read windows in
# the card's memory, pages mapped by several threads). There, 2026-11-27: the
# boat at a steady 12 fps (the cool_fox base: 8-10), the card about 80% busy.
# The picture is Vice-Tay's own defaults.
#
# Usage:  .\jugar.ps1                   (as above)
#         .\jugar.ps1 -Release:$false   (target\diag, the build Vice-Tay measures with)
#         .\jugar.ps1 -Pantalla         (full screen instead of a window)
#         .\jugar.ps1 -Mas @{VICTORY_PANEL='0'}  (any other setting)
# The title's executable is VICTORY_GAME_DIR if that names an .exe; if it
# names a folder, the largest .exe in it. Or pass it with -Game.
# Build first: cargo build --locked --release -p victory
param(
    [switch]$Pantalla,
    [bool]$Release = $true,
    [hashtable]$Mas = @{},
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
$build = if ($Release) { 'release' } else { 'diag' }
$log = Join-Path $PSScriptRoot "run-series-s.log"
Write-Host "Vice-Tay base: Series S, -Machine 3060, $build build; log in $log"
# -WarmPipelines: the host pipelines of earlier runs are made before the title
# starts. Without it the first start of this tree on the test PC drew black
# for minutes: about 1,000 pipelines were waiting to be made in the world and
# their draws were left out (the driver took up to 72 s for one; 4,123 took
# 600 s on 8 processes). Pipelines made before are not made again.
$play = @{ Exe = $Game; Console = 'series-s'; Build = $build; Machine = '3060'; More = $Mas; WarmPipelines = $true }
if (-not $Pantalla) { $play.Windowed = $true }
# A writer of our own, not Tee-Object: Windows PowerShell 5.1's wraps long
# lines at the window's width and writes UTF-16.
$writer = [IO.StreamWriter]::new($log, $false, [Text.UTF8Encoding]::new($false))
try {
    & (Join-Path $PSScriptRoot "scripts\play.ps1") @play 2>&1 | ForEach-Object { $line = "$_"; $writer.WriteLine($line); $line }
} finally {
    $writer.Dispose()
}
