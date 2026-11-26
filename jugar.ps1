# Runs the title on this tree (cool_fox, DoubleGhosty). By default with
# cool_fox's own defaults: on the RTX 2060 SUPER test PC they ran at 3.2-4 fps
# on Series S, 5 at the boat, up to 8 in cutscenes, while the switch set of
# cool_fox's Play-VIctory.cmd (-Rapido) ran at about 2 fps (the card 250-380
# ms a frame against about 150). Since cool_fox 64b8967 its hair-strand
# passes are skipped unless VICTORY_HAIR is set (-Pelo).
#
# Usage:  .\jugar.ps1                 (Series S, cool_fox's defaults)
#         .\jugar.ps1 -Pelo           (hair strands drawn: the full hair, slower)
#         .\jugar.ps1 -Rapido         (the switch set of Play-VIctory.cmd)
#         .\jugar.ps1 -Salida 1664x936  (a smaller output: less work for the
#                                      card; 1280x720, 1664x936, 1920x1080)
#         .\jugar.ps1 -SeriesX        (Series X: its author says faces render
#                                      whole on 8 GB cards only there; slower)
#         .\jugar.ps1 -SeriesX -Escala 0.6  (Series X rendered at 60% and
#                                      scaled up by the title: 0.5 to 1.0)
#         .\jugar.ps1 -Rapido -Luz 1.0  (with -Rapido: the stand-in light's
#                                      constant level; 0.8 default)
# The title's executable is VICTORY_GAME_DIR if that names an .exe; if it
# names a folder, the largest .exe in it. Or pass it with -Game.
param(
    [switch]$SeriesX,
    [string]$Escala = "",
    [ValidateSet("", "1280x720", "1664x936", "1920x1080", "2560x1440")]
    [string]$Salida = "",
    [switch]$Rapido,
    [switch]$Pelo,
    [string]$Luz = "0.8",
    [string]$Exe = "$PSScriptRoot\target\release\victory.exe",
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
if (-not (Test-Path $Exe)) {
    Write-Error "No build at $Exe. Build it first: cargo build --release -p victory"
    exit 1
}

# Nothing left over from an earlier run in this window; the ones set for the
# user or the machine (cache and data folders) stay.
$kept = @([Environment]::GetEnvironmentVariables('User').Keys) + @([Environment]::GetEnvironmentVariables('Machine').Keys)
Get-ChildItem Env: | Where-Object { $_.Name -like "VICTORY_*" -and $kept -notcontains $_.Name } |
    ForEach-Object { Remove-Item "Env:$($_.Name)" }

if ($Pelo) { $env:VICTORY_HAIR = "1" }
if ($Rapido) {
    # Copies and the copy-back: sent without waiting for each submission,
    # found on several threads, joined, over the card's copy queue.
    $env:VICTORY_RELAY = "2"
    $env:VICTORY_WIDE_BACK = "1"
    $env:VICTORY_RELAY_COPY = "1"
    $env:VICTORY_JOIN_COPIES = "1"
    $env:VICTORY_CARD_REHASH_RUNS = "1"
    $env:VICTORY_CARD_STEPS = "2"
    $env:VICTORY_CARD_BUFFERS_MIN_KB = "64"
    # Indirect draws clamped a window and a batch at a time.
    $env:VICTORY_INDIRECT_WINDOW = "1"
    $env:VICTORY_INDIRECT_BATCH = "2"
    # Textures: no waiting for title memory to hold still, uploads ahead of
    # the draws, levels only when read, shown after their upload finished.
    $env:VICTORY_TEXTURE_NO_WAIT = "1"
    $env:VICTORY_UPLOAD_AHEAD = "1"
    $env:VICTORY_LEVELS_ON_DEMAND = "1"
    $env:VICTORY_SHOW_AFTER = "1"
    # The write guard misses some of the title's writes: a guard's arms and
    # the phone read colour noise. Every check hashes the texture again.
    $env:VICTORY_TEXTURE_GUARD = "0"
    # Constants read straight from title memory.
    $env:VICTORY_PORT_CONSTANTS = "1"
    $env:VICTORY_PORT_CBV = "1"
    # Pages mapped on one thread; the card's clocks kept up.
    $env:VICTORY_MAP_THREADS = "1"
    $env:VICTORY_CARD_AWAKE = "1"
    # Off on NVIDIA (cool_fox's author): THIN leaves heads undrawn,
    # JOIN_PARTS repeats frames of the opening video.
    $env:VICTORY_THIN = "0"
    $env:VICTORY_JOIN_PARTS = "0"
    # The stand-in light as a constant: the frame estimate washed the
    # whole picture out in a bright doorway.
    $env:VICTORY_AMBIENT = "constant:$Luz"
    $env:VICTORY_AMBIENT_AIR = "1.5"
}

$console = if ($SeriesX) { "series-x" } else { "series-s" }
if ($Salida) { $env:VICTORY_OUTPUT = $Salida }
if ($Escala) {
    if (-not $SeriesX) {
        Write-Warning "-Escala works on Series X only (on Series S the title's FidelityFX scale is not safe); ignored."
    } else {
        $env:VICTORY_RENDER_SCALE = $Escala
    }
}

$log = "run-$console.log"
Write-Host "VIctory (cool_fox base): $console$(if ($Rapido) { ', Play-VIctory.cmd switches' } else { ', defaults' })$(if ($Pelo) { ', hair strands' }); log in $log"
# A writer of our own, not Tee-Object: Windows PowerShell 5.1's wraps long
# lines at the window's width and writes UTF-16.
$writer = [IO.StreamWriter]::new((Join-Path $PWD $log), $false, [Text.UTF8Encoding]::new($false))
try {
    & $Exe run $Game --execute --console $console 2>&1 | ForEach-Object { $line = "$_"; $writer.WriteLine($line); $line }
} finally {
    $writer.Dispose()
}
