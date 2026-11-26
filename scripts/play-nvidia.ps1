# Runs the title with the switches measured fastest on an RTX 5070 12 GB
# (Windows 11, i7-14700KF, 32 GB; runs r05/r10 of 2026-11-25: about 5.5 fps
# in the first gameplay scene, menus at 30 fps).
#
# Usage: scripts\play-nvidia.ps1 [-Exe <victory.exe>] [-Title <title exe>]
#   Defaults: target\release\victory.exe, game_dir from victory.toml.
# Cards with 8 GB: pass -CardMb 512.
param(
    [string]$Exe = "$PSScriptRoot\..\target\release\victory.exe",
    [string]$Title = '',
    [int]$CardMb = 1024
)
if (-not (Test-Path $Exe)) {
    Write-Error "$Exe not found: cargo build --release -p victory"
    exit 1
}
if (-not $Title) {
    $config = Join-Path $PSScriptRoot '..\victory.toml'
    $dir = (Select-String -Path $config -Pattern "^\s*game_dir\s*=\s*'([^']+)'" | Select-Object -First 1).Matches.Groups[1].Value
    $Title = Get-ChildItem $dir -Filter '*.exe' | Where-Object Name -notmatch '^(victory)' | Select-Object -First 1 -ExpandProperty FullName
}
# Submissions sent ahead with a relay thread that lands them (VICTORY_RELAY=2).
$env:VICTORY_RELAY = '2'
# Big title buffers' copies in the card's memory, compared there.
$env:VICTORY_CARD_BUFFERS_MIN_MB = '1'
$env:VICTORY_CARD_BUFFERS_MB = "$CardMb"
# Textures read through the kernel while the runtime remaps title memory.
$env:VICTORY_TEXTURE_NO_WAIT = '1'
# Shader loops stop after 2^14 iterations a lane (a runaway loop otherwise
# outlasts the driver's timeout and can freeze the desktop, #1819).
$env:VICTORY_SHADER_LOOP_LIMIT = '16384'
& $Exe run $Title --execute --console series-x
