# Runs the cool_fox build (DoubleGhosty/cool_fox, "CGBJ") with its author's
# tuned switches, from our own clone in %USERPROFILE%\victory-coolfox.
# Built from source by a launcher, it ran at 3-4 fps on Series S
# on the RTX 2060 SUPER test PC with every switch at its default, which leaves
# most of the speed work off. These are the switches of cool_fox's own
# Play-VIctory.cmd, where its author measured 5-7 fps.
#
# Usage:  .\jugar-cf.ps1              (Series S, the tuned switches)
#         .\jugar-cf.ps1 -SeriesX     (Series X: its author says faces render
#                                      whole on 8 GB cards only there; slower)
#         .\jugar-cf.ps1 -SeriesX -Escala 0.6  (Series X rendered at 60% and
#                                      scaled up by the title: 0.5 to 1.0;
#                                      below 2/3 it is faster and softer)
#         .\jugar-cf.ps1 -Simple      (every switch at its default: as the
#                                      launcher runs it, to compare)
#         .\jugar-cf.ps1 -Luz 1.0     (the stand-in light's level; 0.8 default)
#         .\jugar-cf.ps1 -Exe <path>  (another cool_fox build)
# The title's executable is VICTORY_GAME_DIR if that names an .exe; if it
# names a folder, the largest .exe in it. Or pass it with -Game.
param(
    [switch]$SeriesX,
    [string]$Escala = "",
    [switch]$Simple,
    [string]$Luz = "0.8",
    [string]$Exe = "$env:USERPROFILE\victory-coolfox\target\release\victory.exe",
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
    Write-Error "cool_fox build not found at $Exe. Clone github.com/DoubleGhosty/cool_fox to $env:USERPROFILE\victory-coolfox and build it there (cargo build --release -p victory), or pass -Exe <path>."
    exit 1
}

# Nothing left over from jugar.ps1 in this window; the ones set for the user
# or the machine (cache and data folders) stay.
$kept = @([Environment]::GetEnvironmentVariables('User').Keys) + @([Environment]::GetEnvironmentVariables('Machine').Keys)
Get-ChildItem Env: | Where-Object { $_.Name -like "VICTORY_*" -and $kept -notcontains $_.Name } |
    ForEach-Object { Remove-Item "Env:$($_.Name)" }

if (-not $Simple) {
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
    # Off on NVIDIA (its author): THIN leaves heads undrawn, JOIN_PARTS
    # repeats frames of the opening video.
    $env:VICTORY_THIN = "0"
    $env:VICTORY_JOIN_PARTS = "0"
    # The stand-in light as a constant: the frame estimate washed the
    # whole picture out in a bright doorway.
    $env:VICTORY_AMBIENT = "constant:$Luz"
    $env:VICTORY_AMBIENT_AIR = "1.5"
}

$console = if ($SeriesX) { "series-x" } else { "series-s" }
if ($Escala) {
    if (-not $SeriesX) {
        Write-Warning "-Escala works on Series X only (on Series S the title's FidelityFX scale is not safe); ignored."
    } else {
        $env:VICTORY_RENDER_SCALE = $Escala
    }
}

$log = "cf-$console.log"
Write-Host "cool_fox: $console$(if ($Simple) { ', defaults' } else { ', tuned' }); log in $log"
& $Exe run $Game --execute --console $console 2>&1 | Tee-Object -FilePath $log
