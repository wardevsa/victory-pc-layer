# CCS03: a text report on every shader program of the title: whether it translates, in which form,
# whether dxil.dll's validator and this machine's driver take it, and (with the log of a run) what
# happened to the shaders that run used. How to read it: ccs03/SHADERS.md.
#
#   powershell -ExecutionPolicy Bypass -File ccs03\shader-report.ps1 -Victory <kit>\victory.exe
#       [-Title <game folder>\game_scarlett_master.exe] [-RunLog <log of a run>] [-Out <folder>]
#
# The report holds the programs' names and hashes and our own messages, no shader code: it can be
# shared. A full pass takes 10 to 30 minutes (some 26,000 programs, each also given to the driver).
param(
    [Parameter(Mandatory = $true)][string] $Victory,
    [string] $Title,
    [string] $RunLog,
    [string] $Out = 'shader-report',
    [int] $Jobs = 0,
    # Only the run's log, against the programs.tsv of an earlier pass in -Out.
    [switch] $RunOnly,
    # The report again from the translate.txt of an earlier pass in -Out, without translating.
    [switch] $Reuse
)
$ErrorActionPreference = 'Stop'
$began = Get-Date
$Victory = (Resolve-Path $Victory).Path
New-Item -ItemType Directory -Force $Out | Out-Null
$Out = (Resolve-Path $Out).Path
$home_ = $env:USERPROFILE
function Clean([string] $s) { if ($home_) { $s.Replace($home_, '~') } else { $s } }
function Save([string] $name, $lines) {
    [IO.File]::WriteAllLines((Join-Path $Out $name), [string[]]@($lines | ForEach-Object { Clean "$_" }))
}

# The title: -Title, else GAME_EXE, else VICTORY_GAME_DIR, else game_dir in victory.toml.
if (-not $Title) { $Title = $env:GAME_EXE }
if (-not $Title -and $env:VICTORY_GAME_DIR) { $Title = Join-Path $env:VICTORY_GAME_DIR 'game_scarlett_master.exe' }
if (-not $Title) {
    $config = $env:VICTORY_CONFIG
    if (-not $config) { $config = Join-Path (Split-Path -Parent $Victory) 'victory.toml' }
    if (Test-Path $config) {
        $m = Select-String -Path $config -Pattern "^\s*game_dir\s*=\s*['""]([^'""]+)['""]" | Select-Object -First 1
        if ($m) { $Title = Join-Path $m.Matches[0].Groups[1].Value 'game_scarlett_master.exe' }
    }
}

# What the run's log says of the host, for translating as that run did.
$runText = $null
if ($RunLog) {
    $RunLog = (Resolve-Path $RunLog).Path
    $runText = [IO.File]::ReadAllLines($RunLog)
}
function FromLog([string] $pattern) {
    if (-not $runText) { return $null }
    $m = $runText | Select-String -Pattern $pattern | Select-Object -First 1
    if ($m) { $m.Matches[0].Groups[1].Value } else { $null }
}
$gpus = @(Get-CimInstance Win32_VideoController -ErrorAction SilentlyContinue | ForEach-Object { "$($_.Name) (driver $($_.DriverVersion))" })
if (-not $env:VICTORY_HOST_WAVE) {
    $wave = FromLog 'host wave lanes min=\d+ max=(\d+)'
    if (-not $wave -and ($gpus -match 'NVIDIA|Intel')) { $wave = '32' }
    if ($wave) { $env:VICTORY_HOST_WAVE = $wave }
}
if (-not $env:VICTORY_VIEW_LIMIT) {
    $views = FromLog 'host view heaps hold (\d+) views'
    if (-not $views) { $views = '1000000' }
    $env:VICTORY_VIEW_LIMIT = [string]([int]$views - 1)
}

$tsv = Join-Path $Out 'programs.tsv'
$programs = [ordered]@{}
$totals = @()
if (-not $RunOnly) {
    if (-not $Title -or -not (Test-Path $Title)) {
        throw "Title not found. Pass -Title <game folder>\game_scarlett_master.exe, or set GAME_EXE or game_dir in victory.toml."
    }
    $paths = @()
    $pack = Join-Path (Split-Path -Parent $Title) 'shaders.rpf'
    if (Test-Path $pack) { $paths += $pack }
    $paths += $Title
    $census = Join-Path $Out 'census.tsv'
    if (-not $Reuse) { Remove-Item -ErrorAction SilentlyContinue $census }
    $env:VICTORY_SHADER_CENSUS = $census
    $raw = Join-Path $Out 'translate.txt'
    $err = Join-Path $Out 'translate-stderr.txt'
    if ($Reuse -and (Test-Path $raw)) { Write-Host "Reading the earlier pass in $raw" } else {
        $arguments = @('translate', '--validate', '--pipelines', '--list')
        if ($Jobs -gt 0) { $arguments += @('--jobs', "$Jobs") }
        $arguments += $paths | ForEach-Object { '"' + $_ + '"' }
        Write-Host "Translating every shader program of $(($paths | ForEach-Object { Split-Path -Leaf $_ }) -join ' and ')"
        Write-Host "(host waves $($env:VICTORY_HOST_WAVE), view heap $([int]$env:VICTORY_VIEW_LIMIT + 1)); 10 to 30 minutes..."
        $p = Start-Process -FilePath $Victory -ArgumentList $arguments -WorkingDirectory (Split-Path -Parent $Victory) `
            -RedirectStandardOutput $raw -RedirectStandardError $err -NoNewWindow -PassThru
        $null = $p.Handle
        while (-not $p.WaitForExit(30000)) { Write-Host ("  {0:n0} s" -f ((Get-Date) - $began).TotalSeconds) }
        if ($p.ExitCode -ne 0) { Write-Host "translate ended with code $($p.ExitCode); see translate-stderr.txt" -ForegroundColor Yellow }
    }

    $wave64 = @{}
    if (Test-Path $census) {
        foreach ($l in [IO.File]::ReadLines($census)) {
            $f = $l.Split("`t")
            if ($f.Length -gt 1) { $wave64[$f[0]] = ($f[1] -eq 'wave64=1') }
        }
    }
    $listLine = '^(?:[A-Za-z]:\\|\\\\|/)'
    foreach ($l in [IO.File]::ReadLines($raw)) {
        if ($l -notmatch $listLine) {
            if ($l -match '^(containers|programs|structured|resources|I/O|DXIL|validator|\s{13}\d|\s+used-resource)') { $totals += $l }
            continue
        }
        $cut = $l.IndexOf('  ')
        if ($cut -lt 0) { continue }
        $label = $l.Substring(0, $cut)
        $outcome = $l.Substring($cut + 2)
        if ($outcome -eq 'duplicate') { continue }
        $key = $label -replace ' at 0x[0-9a-f]+$', ''
        if (-not $programs.Contains($key)) {
            # the record's name (a packed record's label), else the one in the container
            $name = ''
            if (($key -match '@0x[0-9a-f]+ (.+?) #[0-9a-f]{16}') -and ($Matches[1] -ne '?')) { $name = $Matches[1] }
            elseif ($key -match '~[0-9a-f]{8} (\S+) (Ls|Hs|Es|Gs|Vs|Ps|Ms|Cs)$') { $name = $Matches[1] }
            $hash = ''; if ($key -match '#([0-9a-f]{16})') { $hash = $Matches[1] }
            $stage = ''; if ($key -match ' (Ls|Hs|Es|Gs|Vs|Ps|Ms|Cs)$') { $stage = $Matches[1] }
            $source = Split-Path -Leaf (($key -split '@')[0])
            $programs[$key] = [pscustomobject]@{
                Stage = $stage; Name = $name; Hash = $hash; Source = $source
                Wave64 = [bool]$wave64[$key]; Validated = $false; Pipeline = $false; Form = ''; Problems = @()
            }
        }
        $r = $programs[$key]
        if ($outcome -match '^validated  \[(.*)\]$') { $r.Validated = $true; $r.Form = $Matches[1] }
        elseif ($outcome -match '^pipeline created  \[(.*)\]$') { $r.Pipeline = $true; $r.Form = $Matches[1] }
        else { $r.Problems += $outcome }
    }
    $rows = New-Object System.Collections.Generic.List[string]
    $rows.Add("stage`tname`thash`tresult`tform`twave64`tproblems`tfile")
    foreach ($r in $programs.Values) {
        $result = if ($r.Problems.Count -eq 0 -and $r.Validated -and $r.Pipeline) { 'ok' } else { 'problem' }
        $rows.Add("$($r.Stage)`t$($r.Name)`t$($r.Hash)`t$result`t$($r.Form)`t$([int]$r.Wave64)`t$($r.Problems -join ' | ')`t$($r.Source)")
    }
    Save 'programs.tsv' $rows
} elseif (Test-Path $tsv) {
    foreach ($l in ([IO.File]::ReadAllLines($tsv) | Select-Object -Skip 1)) {
        $f = $l.Split("`t")
        $probs = @(); if ($f[6]) { $probs = $f[6] -split ' \| ' }
        $programs["$($f[2]) $($f[0])"] = [pscustomobject]@{
            Stage = $f[0]; Name = $f[1]; Hash = $f[2]; Source = $f[7]; Wave64 = ($f[5] -eq '1')
            Validated = ($f[3] -eq 'ok'); Pipeline = ($f[3] -eq 'ok'); Form = $f[4]; Problems = $probs
        }
    }
}

# The summary.
function Shown($r) { if ($r.Name) { "$($r.Name) #$($r.Hash)" } else { "(no name) #$($r.Hash)" } }
$s = New-Object System.Collections.Generic.List[string]
$s.Add("CCS03 shader report, $(Get-Date -Format 'yyyy-MM-dd HH:mm')")
$version = Join-Path (Split-Path -Parent $Victory) 'VERSIE.txt'
if (Test-Path $version) { $s.Add("Build: $((Get-Content $version -TotalCount 1).Trim())") }
foreach ($g in $gpus) { $s.Add("GPU: $g") }
$s.Add("Translated as for host waves of $($env:VICTORY_HOST_WAVE) lanes and a view heap of $([int]$env:VICTORY_VIEW_LIMIT + 1)")
$s.Add('')
$all = @($programs.Values)
if ($all.Count -gt 0) {
    $s.Add('== Every program of the title (victory translate --validate --pipelines)')
    foreach ($t in $totals) { $s.Add($t) }
    $s.Add('')
    $s.Add('stage   programs        ok  validator  driver  not translated')
    foreach ($g in ($all | Group-Object Stage | Sort-Object Name)) {
        $v = @($g.Group | Where-Object { $_.Problems -match '^validator:' }).Count
        $d = @($g.Group | Where-Object { $_.Problems -match '^pipeline:' }).Count
        $n = @($g.Group | Where-Object { $_.Problems -and -not ($_.Problems -match '^(validator|pipeline):') }).Count
        $ok = @($g.Group | Where-Object { $_.Problems.Count -eq 0 -and $_.Validated -and $_.Pipeline }).Count
        $s.Add(('{0,-5} {1,10:n0} {2,9:n0} {3,10:n0} {4,7:n0} {5,15:n0}' -f $(if ($g.Name) { $g.Name } else { '?' }), $g.Count, $ok, $v, $d, $n))
    }
    $s.Add('(validator: dxil.dll refuses our DXIL; driver: the driver refuses the pipeline; not translated: the translator stops)')
    $s.Add('')
    $w = @($all | Where-Object { $_.Stage -eq 'Cs' -and ($_.Wave64 -or ($_.Form -and $_.Form -ne 'as written')) })
    if ($w.Count -gt 0) {
        $s.Add("== Console wave64 compute programs on a host of $($env:VICTORY_HOST_WAVE)-lane waves: the form each was taken in")
        foreach ($g in ($w | Group-Object Form | Sort-Object Count -Descending)) {
            $form = if ($g.Name) { $g.Name } else { '(none: did not translate)' }
            $s.Add(('{0,7:n0}  {1}' -f $g.Count, $form))
            if ($form -match 'APPROXIMATE|wave64 kept') {
                foreach ($r in ($g.Group | Sort-Object Name)) { $s.Add("           $(Shown $r)") }
            }
        }
        $s.Add('(APPROXIMATE: each half of a console wave gives its own result; wave64 kept: the driver refuses it)')
        $s.Add('')
    }
    $bad = @($all | Where-Object { $_.Problems.Count -gt 0 })
    $s.Add("== Programs with a problem: $($bad.Count) of $($all.Count)")
    $byReason = @{}
    foreach ($r in $bad) {
        foreach ($p in $r.Problems) {
            if (-not $byReason.ContainsKey($p)) { $byReason[$p] = New-Object System.Collections.Generic.List[object] }
            $byReason[$p].Add($r)
        }
    }
    foreach ($k in ($byReason.Keys | Sort-Object { $byReason[$_].Count } -Descending)) {
        $s.Add(('{0,7:n0}  {1}' -f $byReason[$k].Count, $k))
        foreach ($r in ($byReason[$k] | Select-Object -First 12)) { $s.Add("           $($r.Stage) $(Shown $r)") }
        if ($byReason[$k].Count -gt 12) { $s.Add("           ... $($byReason[$k].Count - 12) more in programs.tsv") }
    }
    $s.Add('')
}

if ($runText) {
    $byHash = @{}
    foreach ($r in $all) { if ($r.Hash) { $byHash[$r.Hash] = $r } }
    function Named([string] $line) {
        [regex]::Replace($line, '\b[0-9a-f]{16}\b', {
                param($m) $r = $byHash[$m.Value]
                if ($r -and $r.Name) { "$($m.Value) ($($r.Name))" } else { $m.Value } })
    }
    $used = [ordered]@{}
    foreach ($l in $runText) {
        if ($l -match 'shader translated stage="(\w+)" hash=([0-9a-f]{16})') { $used[$Matches[2]] = $Matches[1] }
    }
    $s.Add("== The run: $(Split-Path -Leaf $RunLog)")
    $known = @($used.Keys | Where-Object { $byHash[$_] }).Count
    $s.Add("Shaders the run translated: $($used.Count) ($known of them found in the title's files above)")
    foreach ($g in ($used.Values | Group-Object | Sort-Object Name)) { $s.Add(('{0,9:n0}  {1}' -f $g.Count, $g.Name)) }
    $rows = New-Object System.Collections.Generic.List[string]
    $rows.Add("stage`thash`tname`toffline result`tform")
    foreach ($h in $used.Keys) {
        $r = $byHash[$h]
        if ($r) {
            $res = if ($r.Problems.Count -eq 0) { 'ok' } else { $r.Problems -join ' | ' }
            $rows.Add("$($used[$h])`t$h`t$($r.Name)`t$res`t$($r.Form)")
        } else { $rows.Add("$($used[$h])`t$h`t`tnot in the title's files`t") }
    }
    Save 'run-shaders.tsv' $rows
    $patterns = [ordered]@{
        'shader does not translate'         = 'shader does not translate'
        'host pipeline refused'             = 'host (mesh )?pipeline refused'
        'host root signature refused'       = 'host root signature refused'
        'no host pipeline'                  = 'no host pipeline:'
        'draws or dispatches dropped'       = 'dropped [\d.]+/frame without a host pipeline'
        'ray-tracing structure builder'     = 'structure builder (stays off|is turned off|runs)'
        'device removed or lost'            = 'device removed|DEVICE_REMOVED|device lost'
    }
    $problems = New-Object System.Collections.Generic.List[string]
    foreach ($k in $patterns.Keys) {
        $hits = @($runText | Where-Object { $_ -match $patterns[$k] })
        $s.Add(('{0,9:n0}  {1}' -f $hits.Count, $k))
        if ($hits.Count -gt 0) {
            $problems.Add("== $k ($($hits.Count))")
            foreach ($h in ($hits | Select-Object -First 200)) { $problems.Add((Named ($h -replace '^\S+\s+', ''))) }
            $problems.Add('')
        }
    }
    Save 'run-problems.txt' $problems
    $s.Add('(the lines themselves, with program names: run-problems.txt; every shader of the run: run-shaders.tsv)')
    $s.Add('')
}
$s.Add(("Made in {0:n0} s. Files: summary.txt, programs.tsv (one line per program), translate.txt (the raw list)" -f ((Get-Date) - $began).TotalSeconds))
Save 'summary.txt' $s
Write-Host ''
Get-Content (Join-Path $Out 'summary.txt') -TotalCount 100 | ForEach-Object { Write-Host $_ }
Write-Host ''
Write-Host "Report: $Out\summary.txt"
