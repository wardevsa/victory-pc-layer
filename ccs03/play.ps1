# CCS03: runs the title with the switches the README recommends for an 8 GB
# NVIDIA card (Series S profile) and writes the run's log to a file outside
# the repository, with a short summary next to it.
#
# Usage, from anywhere:
#   powershell -ExecutionPolicy Bypass -File ccs03\play.ps1 -Title <game folder>\game_scarlett_master.exe
#   (without -Title: the game_dir of victory.toml)
#
# Options:
#   -Smooth   texture pass held to 25 ms and 48 MB a frame (fewer stalls,
#             textures come in over more frames)
#   -Fast     refreshes of host copies on several threads (VICTORY_WIDE_REFRESH=1)
#   -Low      a 1280x720 display
#   -Steady   a frame counts at most 33 ms for the title (VICTORY_GAME_STEP_MS):
#             at 3-4 fps the game runs slower than real time instead of in
#             250-300 ms jumps, so boats, cars and the people in them keep to
#             the steps the game was made for and steering keeps up.
#             -Step <ms> sets another step.
#   -Diag     the diagnostics build (cargo build --profile diag -p victory
#             --features diagnostics), which logs why draws are dropped
#   -Console series-x   the Series X profile (needs more than 8 GB)
#   -LogDir <folder>    where the logs go (default: a folder next to the
#             repository, never inside it)
#
# After the run, in the log folder:
#   run-<time>.log          the whole log
#   run-<time>-short.txt    frame rate, why draws were dropped, warnings, end
#   run-<time>-crashes.log  copy of crashes.log, if the run added to it
param(
    [string]$Title = '',
    [string]$LogDir = '',
    [switch]$Diag,
    [switch]$Smooth,
    [switch]$Fast,
    [switch]$Low,
    [switch]$Steady,
    [double]$Step = 33,
    [ValidateSet('series-s', 'series-x')][string]$Console = 'series-s'
)
$repo = Split-Path -Parent $PSScriptRoot
$Exe = if ($Diag) { "$repo\target\diag\victory.exe" } else { "$repo\target\release\victory.exe" }
if (-not (Test-Path $Exe)) {
    $how = if ($Diag) { 'cargo build --profile diag -p victory --features diagnostics' } else { 'cargo build --release -p victory' }
    Write-Host "Not found: $Exe. Build it first: $how" -ForegroundColor Red
    exit 1
}
if (-not $Title) {
    $config = Join-Path $repo 'victory.toml'
    if (Test-Path $config) {
        $dir = (Select-String -Path $config -Pattern "^\s*game_dir\s*=\s*['""]([^'""]+)['""]" | Select-Object -First 1).Matches.Groups[1].Value
        if ($dir) { $Title = Join-Path $dir 'game_scarlett_master.exe' }
    }
}
if (-not $Title -or -not (Test-Path $Title)) {
    Write-Host "Title not found. Pass -Title <game folder>\game_scarlett_master.exe or set game_dir in victory.toml." -ForegroundColor Red
    exit 1
}
if (-not $LogDir) { $LogDir = Join-Path (Split-Path -Parent $repo) 'CCS03-logs' }
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null

# README, "Running".
$env:VICTORY_DISPLAY_MODE = '1920x1080'
$env:VICTORY_RELAY = '2'
$env:VICTORY_ASYNC_SUBMIT = '1'
$env:VICTORY_QUEUE2 = '1'
$env:VICTORY_INDIRECT_WINDOW = '1'
$env:VICTORY_COPY_PLAIN = '1'
$env:VICTORY_TEXTURE_NO_WAIT = '1'
$env:VICTORY_WIDE_TEXTURES = '1'
$env:VICTORY_TEXTURE_DROP_MIPS = '1'
$env:VICTORY_TEXTURE_ROOM_PERCENT = '86'
$env:VICTORY_CARD_ROOM_FOR_TARGETS_MB = '1024'
$env:VICTORY_CARD_BUFFERS_MB = '768'
$env:VICTORY_AMBIENT = '0.9'
$env:VICTORY_JOIN_PARTS = '1'
$env:VICTORY_BACK_OWN_VIEW = '1'
$env:VICTORY_INDIRECT_GATHER = '1'
$env:VICTORY_CARD_REHASH_RUNS = '1'
# victory-fork PR 999: water textures the title reads on the CPU are copied
# back (boats float); a hash as long as the title's buffer (the water right).
$env:VICTORY_TEXTURE_BACK = '1'
$env:VICTORY_CARD_HASH_LEN = '1'
$env:NO_COLOR = '1'
if ($Fast) { $env:VICTORY_WIDE_REFRESH = '1' }
if ($Low) { $env:VICTORY_DISPLAY_MODE = '1280x720' }
if ($Steady) { $env:VICTORY_GAME_STEP_MS = "$Step" }
if ($Smooth) {
    $env:VICTORY_TEXTURE_STAGE_MS = '25'
    $env:VICTORY_TEXTURE_STAGE_MB = '48'
    $env:VICTORY_TEXTURE_CATCHUP = '0'
}

$stamp = Get-Date -Format 'yyyy-MM-dd_HH-mm'
$log = Join-Path $LogDir "run-$stamp.log"
$short = Join-Path $LogDir "run-$stamp-short.txt"
$crashes = Join-Path $env:LOCALAPPDATA 'victory\data\crashes.log'
$crashesBefore = if (Test-Path $crashes) { (Get-Item $crashes).Length } else { 0 }

Write-Host "Starting. The log goes to: $log" -ForegroundColor Green
Write-Host 'Close the game window when done; this window then writes the summary.' -ForegroundColor Green
$started = Get-Date
$p = Start-Process -FilePath $Exe `
    -ArgumentList @('run', "`"$Title`"", '--execute', '--console', $Console) `
    -RedirectStandardError $log -RedirectStandardOutput "$log.out" `
    -NoNewWindow -PassThru -Wait
$ended = Get-Date

$lines = Get-Content $log
$out = New-Object System.Collections.Generic.List[string]
$out.Add("Run $stamp, console $Console, smooth $Smooth, fast $Fast, low $Low, steady $Steady ($Step ms), $([int]($ended - $started).TotalSeconds) s, exit code $($p.ExitCode)")
$out.Add('')
$out.Add('== Frame rate (one line every ~5 s) ==')
$lines | Where-Object { $_ -match 'fps, [0-9.]+ ms/frame' } | ForEach-Object {
    if ($_ -match 'T(\d\d:\d\d:\d\d).*?: (.{0,230})') { $out.Add("$($Matches[1]) $($Matches[2])") }
}
$out.Add('')
$out.Add('== Why draws are dropped (diagnostics build; per-frame averages summed over the run) ==')
$drops = @{}
$lines | Where-Object { $_ -match 'dropped ([0-9.]+)/frame without a host pipeline: (.*)$' } | ForEach-Object {
    $why = $Matches[2]; $drops[$why] = [double]($drops[$why]) + [double]$Matches[1]
}
$drops.GetEnumerator() | Sort-Object Value -Descending | Select-Object -First 40 |
    ForEach-Object { $out.Add(('{0,8:N1} {1}' -f $_.Value, $_.Key)) }
$out.Add('')
$out.Add('== Warnings and errors (grouped) ==')
$lines | Where-Object { $_ -match ' (WARN|ERROR) ' } |
    ForEach-Object {
        $k = $_ -replace '^\S+\s+', '' -replace '0x[0-9a-f]+', 'X' -replace '\d+', 'N'
        $k.Substring(0, [Math]::Min(220, $k.Length))
    } |
    Group-Object | Sort-Object Count -Descending | Select-Object -First 30 |
    ForEach-Object { $out.Add("$($_.Count)x $($_.Name)") }
$out.Add('')
$out.Add('== Last 40 lines ==')
$lines | Select-Object -Last 40 | ForEach-Object { $out.Add($_.Substring(0, [Math]::Min(300, $_.Length))) }
$out | Set-Content -Encoding UTF8 $short

if ((Test-Path $crashes) -and (Get-Item $crashes).Length -gt $crashesBefore) {
    Copy-Item $crashes (Join-Path $LogDir "run-$stamp-crashes.log")
    Write-Host "crashes.log has something new: copied to $LogDir" -ForegroundColor Yellow
}
Write-Host ''
Write-Host "Done. Summary: $short" -ForegroundColor Green
Write-Host "(Whole log: $log)"
