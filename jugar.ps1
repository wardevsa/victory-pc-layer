# Runs the title on this tree: TaynanGT's Vice-Tay (the cool_fox line, from
# f4b48ad) with our own changes on top (card steps for scratch fills,
# constants copied on the card behind a copy at its place, the warm-up that
# leaves out a shader the translator panics on).
#
# The settings are Vice-Tay's measured ones (scripts\play.ps1, its 4070 and
# 4090 profiles), sized for the test PC (Ryzen 5 5600GT, 6 cores; RTX 2060
# SUPER 8 GB), without the two that tore textures here (VICTORY_RELAY_COPY,
# VICTORY_TEXTURE_NO_WAIT), and with the picture as cool_fox 3277b15 drew it:
# hair strands drawn, no stand-in light.
#
# Usage:  .\jugar.ps1                 (Series S, the performance panel on)
#         .\jugar.ps1 -Preparar       (no game: every shader translated into
#                                      the cache, then the host pipelines of
#                                      earlier runs made; after each update)
#         .\jugar.ps1 -Diag           (the diagnostics build, target\diag)
# The title's executable is VICTORY_GAME_DIR if that names an .exe; if it
# names a folder, the largest .exe in it. Or pass it with -Game.
param(
    [switch]$Preparar,
    [switch]$Diag,
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
if ($Diag) { $Exe = "$PSScriptRoot\target\diag\victory.exe" }
if (-not (Test-Path $Exe)) {
    Write-Error "No build at $Exe. Build it first: cargo build --release -p victory"
    exit 1
}

# Nothing left over from an earlier run in this window; the ones set for the
# user or the machine (cache and data folders) stay.
$kept = @([Environment]::GetEnvironmentVariables('User').Keys) + @([Environment]::GetEnvironmentVariables('Machine').Keys)
Get-ChildItem Env: | Where-Object { $_.Name -like "VICTORY_*" -and $kept -notcontains $_.Name } |
    ForEach-Object { Remove-Item "Env:$($_.Name)" }

if ($Preparar) {
    $dir = Split-Path $Game
    # The shader cache's key holds the host's wave size and view heap: the
    # values the last run's log gives (NVIDIA: 32 lanes, 1,000,000 views).
    $wave = "32"; $views = "1000000"
    $last = Get-ChildItem (Join-Path $PSScriptRoot "run-*.log") -ErrorAction SilentlyContinue |
        Sort-Object LastWriteTime -Descending | Select-Object -First 1
    if ($last) {
        $text = [IO.File]::ReadAllText($last.FullName)
        if ($text -match 'host wave lanes min=\d+ max=(\d+)') { $wave = $Matches[1] }
        if ($text -match 'host view heaps hold (\d+) views') { $views = $Matches[1] }
    }
    $env:VICTORY_HOST_WAVE = $wave
    $env:VICTORY_VIEW_LIMIT = "$([int]$views - 1)"
    # The four programs Vice-Tay runs in the wave-wide form are part of the
    # cache's key: the same list as the runs below.
    $env:VICTORY_WAVE_WIDE_ONLY = '6f225b641c5b0708,5dc3b25986622949,369cbe906ea72764,2cc4c197ee29e291'
    $jobs = [Math]::Max(1, [Environment]::ProcessorCount - 2)
    $inputs = @("$dir\shaders.rpf", $Game, "$dir\XboxBvhBuilder.dll", "$dir\amd_fidelityfx_upscaler_xbox.dll") |
        Where-Object { Test-Path $_ }
    Write-Host "1/2: shaders into the cache ($wave-lane waves, $views views, $jobs threads)..."
    & $Exe translate --warm all --jobs $jobs @inputs
    Remove-Item Env:VICTORY_HOST_WAVE, Env:VICTORY_VIEW_LIMIT
    Write-Host "2/2: host pipelines of earlier runs, for the driver's cache..."
    & $Exe warm --used
    Write-Host "Ready: play with .\jugar.ps1"
    exit 0
}

$settings = @{
    # Vice-Tay's play set.
    VICTORY_RELAY = '2'; VICTORY_WIDE_BACK = '1'; VICTORY_CARD_REHASH_RUNS = '1'; VICTORY_JOIN_COPIES = '1'
    VICTORY_INDIRECT_WINDOW = '1'; VICTORY_CARD_STEPS = '2'; VICTORY_TITLE_PRIORITIES = '0'; VICTORY_SHOW_AFTER = '1'
    VICTORY_THIN = '1'; VICTORY_JOIN_PARTS = '8'; VICTORY_UPLOAD_AHEAD = '1'; VICTORY_INDIRECT_BATCH = '2'
    # A fill of the constant ring that a small copy at its place writes into
    # refused the whole submission to the card steps (Vice-Tay: 12.1 -> 14.3
    # fps on its RTX 4090; here 68-80 -> 0-35 ms of waiting a frame).
    VICTORY_PLACED_ORDER = '1'
    # The wave64 programs the 32-lane card takes only in the wave-wide form
    # (Vice-Tay: 30 minutes without a lost device).
    VICTORY_WAVE_WIDE_ONLY = '6f225b641c5b0708,5dc3b25986622949,369cbe906ea72764,2cc4c197ee29e291'
    # Vice-Tay's 4070 profile: shader buffers' copies in the card's memory.
    VICTORY_CARD_RESERVE_BUDGETS = '1'; VICTORY_CARD_BUFFERS_MIN_KB = '64'; VICTORY_CARD_BUFFERS_MB = '1024'
    VICTORY_CARD_WINDOWS_MB = '768'
    # A 6-core CPU: fewer helpers than Vice-Tay's 8 and 16 cores.
    VICTORY_THIN_THREADS = '5'; VICTORY_UNDER_THREADS = '3'; VICTORY_LAND_THREADS = '3'
    VICTORY_MAP_THREADS = '1'; VICTORY_PIPELINE_WORKERS = '3'
    # Ours, measured on the test PC: small fills of the title's own constants
    # go on without waiting; no texture guards (the page-mapping lock they
    # take made entering a new place wait 1-3 s a frame); the card's clocks
    # kept up through the CPU's gaps.
    VICTORY_FILL_TITLE = '1'; VICTORY_TEXTURE_GUARD = '0'; VICTORY_CARD_AWAKE = '1'
    # Vice-Tay's performance panel in the upper left corner.
    VICTORY_PANEL = '1'
    # Pipelines the title makes are written down for -Preparar.
    VICTORY_PIPELINE_BOOK = '1'
}
foreach ($k in $settings.Keys) { Set-Item "Env:$k" $settings[$k] }

$console = "series-s"
$log = if ($Diag) { "diag-$console.log" } else { "run-$console.log" }
Write-Host "VIctory (Vice-Tay base, ours on top): $console; log in $log"
# A writer of our own, not Tee-Object: Windows PowerShell 5.1's wraps long
# lines at the window's width and writes UTF-16.
$writer = [IO.StreamWriter]::new((Join-Path $PWD $log), $false, [Text.UTF8Encoding]::new($false))
Push-Location $PSScriptRoot   # victory.toml is looked for here
try {
    & $Exe run $Game --execute --console $console 2>&1 | ForEach-Object { $line = "$_"; $writer.WriteLine($line); $line }
} finally {
    $writer.Dispose()
    Pop-Location
}
