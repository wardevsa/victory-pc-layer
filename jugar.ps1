# Runs the title on this tree (Vice-Tay's newest) the way it was measured on
# the RTX 2060 SUPER test PC (Ryzen 5 5600GT, 6 cores, 32 GB): Series S, in a
# window, the release build, Vice-Tay's six-core preset (scripts\play.ps1
# -Machine 3060: 6/4/3 worker threads, up to 3 GB of shader-read windows in
# the card's memory, pages mapped by several threads). There, 2026-11-27: the
# boat at a steady 12 fps (the cool_fox base: 8-10), the card about 80% busy.
# The picture is Vice-Tay's own defaults, drawn at half size and scaled to a
# 1080p output by the title's own upscaler (its FidelityFX, as on the
# console): the boat went from 12.0 to 14.5-14.9 fps steady, the cutscene
# before it from 8-12 to 15-20 at its start. With the frame at native size
# the card took about 69 of a 90 ms frame (the diagnostics panel's GPU EST).
#
# Usage:  .\jugar.ps1                   (as above)
#         .\jugar.ps1 -Nativo           (drawn at full size, no upscaling: sharper, slower)
#         .\jugar.ps1 -Guardia          (the texture write guard: about 2 fps more,
#                                        some textures in coloured noise)
#         .\jugar.ps1 -Release:$false   (target\diag, the build Vice-Tay measures with)
#         .\jugar.ps1 -Pantalla         (full screen instead of a window)
#         .\jugar.ps1 -Mas @{VICTORY_PANEL='0'}  (any other setting)
# The title's executable is VICTORY_GAME_DIR if that names an .exe; if it
# names a folder, the largest .exe in it. Or pass it with -Game.
# Build first: cargo build --locked --release -p victory
param(
    [switch]$Pantalla,
    [switch]$Nativo,
    [switch]$Guardia,
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
# Half size through the title's upscaler unless -Nativo; -Mas wins over both.
$scaled = @{}
if (-not $Nativo) { $scaled = @{ VICTORY_OUTPUT = '1920x1080'; VICTORY_UPSCALER = '6'; VICTORY_RENDER_SCALE = '0.5' } }
# The fill of 240 bytes of the title's constants a shader of a flight still
# in the air writes (once a frame, at 0x...38280 in the boat) waited 24-28 ms
# a frame for the card: card steps refused it ("a copy at its place in a list
# writes what it reads"). With the read-through it reads the placed copy's
# result on the card: that wait went to 1.2 ms a frame (diagnostics run and
# release run of 2026-11-27).
$scaled.VICTORY_PLACED_FILL_REDIRECT = '1'
# Textures hashed at every check, not left to the write guard (-Guardia: the
# guard). With the guard, textures the title writes some way it does not see
# stayed wrong: a shirt's print, forearms, legs and beards in squares of
# coloured noise in the boat cutscene. Without it they drew right in the same
# shots, at more CPU: the cutscene's first 45 frame lines at a median of 12.4
# fps against 14.2 with the guard (11.0 before the read-through above).
if (-not $Guardia) { $scaled.VICTORY_TEXTURE_GUARD = '0' }
foreach ($k in $Mas.Keys) { $scaled[$k] = $Mas[$k] }
$Mas = $scaled
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
