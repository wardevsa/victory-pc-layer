# Starts the title with the settings this fork is played with (README.md, "How to run it").
#   scripts\play.ps1 -Exe <path of your own copy's executable>
# -Safe         without the newer, faster ways (the first thing to try when something is wrong)
# -RayTracing   the title's own ray-traced indirect light: lit interiors, about a third slower
# -Build        target\diag (the default, the build all measurements are of) or target\release
# -More         other settings: -More @{VICTORY_PANEL='1'}
# -Record       writes down the pipelines the title creates (VICTORY_PIPELINE_BOOK=1), for -Warm
# -Warm         with the same settings, makes the pipelines earlier runs drew with (victory warm --used), no title
# -Shaders      with the same settings, translates these shader archives (victory shaders <paths>), no title
# Run it from a normal window, not as administrator: the driver then keeps its shader cache elsewhere.
[CmdletBinding(DefaultParameterSetName = 'Run')]
param(
    [Parameter(Mandatory, ParameterSetName = 'Run')][string]$Exe,
    [Parameter(Mandatory, ParameterSetName = 'Warm')][switch]$Warm,
    [Parameter(Mandatory, ParameterSetName = 'Shaders')][string[]]$Shaders,
    [switch]$Record,
    [switch]$Safe,
    [switch]$RayTracing,
    [ValidateSet('diag', 'release')][string]$Build = 'diag',
    [hashtable]$More = @{}
)
$settings = @{
    VICTORY_TEXTURE_NO_WAIT = '1'; VICTORY_RELAY = '2'; VICTORY_WIDE_BACK = '1'; VICTORY_CARD_REHASH_RUNS = '1'
    VICTORY_JOIN_COPIES = '1'; VICTORY_INDIRECT_WINDOW = '1'; VICTORY_RELAY_COPY = '1'; VICTORY_CARD_STEPS = '2'
    VICTORY_INDIRECT_BATCH = '2'; VICTORY_MAP_THREADS = '1'; VICTORY_TITLE_PRIORITIES = '0'; VICTORY_CARD_AWAKE = '1'
    VICTORY_SHOW_AFTER = '1'; VICTORY_AMBIENT = '0.8'; VICTORY_D3D12_DRED = 'light'; VICTORY_SKIP_PIPELINES = 'none'
    VICTORY_LEVELS_ON_DEMAND = '1'; VICTORY_CARD_BUFFERS_MIN_KB = '64'
    VICTORY_PORT_GUARD = '1'; VICTORY_PORT_JOURNAL = '1'
    VICTORY_LIBRARY_STORE_LATER = '1'; VICTORY_PIPELINE_WAIT_MS = '0'
}
if ($Safe) {
    $settings.VICTORY_UNDER_WIDE = '0'; $settings.VICTORY_LAND_ECHO = '0'
    $settings.VICTORY_PORT_GUARD = '0'; $settings.VICTORY_PORT_JOURNAL = '0'
} else {
    $settings.VICTORY_THIN = '1'; $settings.VICTORY_JOIN_PARTS = '8'; $settings.VICTORY_UPLOAD_AHEAD = '1'
}
if ($RayTracing) { $settings.VICTORY_BVH_BUILDER = '1' }
if ($Record) { $settings.VICTORY_PIPELINE_BOOK = '1' }
foreach ($k in $More.Keys) { $settings[$k] = $More[$k] }

$victory = Join-Path $PSScriptRoot "..\target\$Build\victory.exe"
if (-not (Test-Path -LiteralPath $victory)) { Write-Host "No $victory. Build it first (README.md)."; exit 1 }
if ($PSCmdlet.ParameterSetName -eq 'Run' -and -not (Test-Path -LiteralPath $Exe)) { Write-Host "No $Exe."; exit 1 }
# (Other settings left over in this window go; where your files are stays.)
$paths = 'VICTORY_CONFIG', 'VICTORY_GAME_DIR', 'VICTORY_CACHE_DIR', 'VICTORY_DATA_DIR'
Get-ChildItem Env:VICTORY_* | Where-Object { $_.Name -notin $paths } | ForEach-Object { Remove-Item "Env:$($_.Name)" }
foreach ($k in $settings.Keys) { Set-Item "Env:$k" $settings[$k] }
# An archive as written first (shader[1].rpf), else a pattern (stage*.rpf); one that matches nothing stops here.
if ($Shaders) {
    $Shaders = @(foreach ($s in $Shaders) {
        if (Test-Path -LiteralPath $s) { $found = @(Resolve-Path -LiteralPath $s) }
        else { $found = @(try { Resolve-Path $s -ErrorAction Stop } catch {}) }
        if (-not $found) { Write-Host "No $s."; exit 1 }
        $found.Path
    })
}
if ($Exe) { $Exe = (Resolve-Path -LiteralPath $Exe).Path }
Push-Location -LiteralPath (Join-Path $PSScriptRoot '..')   # victory.toml is looked for here
try {
    switch ($PSCmdlet.ParameterSetName) {
        'Warm' { & $victory warm --used; $code = $LASTEXITCODE }
        'Shaders' { & $victory shaders @Shaders; $code = $LASTEXITCODE }
        default { & $victory run $Exe --execute; $code = $LASTEXITCODE }
    }
} catch { Write-Host "$victory did not run: $_"; $code = 1 } finally { Pop-Location }
exit $code
