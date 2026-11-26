# CCS03: what has to pass before a pull request into ccs03.
# Run from anywhere:  powershell -ExecutionPolicy Bypass -File ccs03\test.ps1
# Writes the full output to target\ccs03-test.log and prints a summary.
$ErrorActionPreference = 'Continue'
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo
$env:PATH = (Join-Path $env:USERPROFILE '.cargo\bin') + ";$env:PATH"
foreach ($v in 'VICTORY_DXIL', 'VICTORY_PC_DXC') {
    if (-not (Get-Item "env:$v" -ErrorAction SilentlyContinue)) {
        $u = [Environment]::GetEnvironmentVariable($v, 'User'); if ($u) { Set-Item "env:$v" $u }
    }
}
New-Item -ItemType Directory -Force target | Out-Null
$log = Join-Path $repo 'target\ccs03-test.log'
$failed = @()
"=== $(Get-Date -Format s)  $(git log -1 --format='%h %s')" | Out-File -Encoding utf8 $log
foreach ($step in @(
        @('tests', 'cargo test --workspace --locked --no-fail-fast'),
        @('clippy', 'cargo clippy --workspace --all-targets --locked -- -D warnings'),
        @('clippy diagnostics', 'cargo clippy --workspace --all-targets --all-features --locked -- -D warnings'))) {
    Write-Host "--- $($step[0])" -ForegroundColor Cyan
    "=== $($step[0]): $($step[1])" | Out-File -Append -Encoding utf8 $log
    cmd /c "$($step[1]) 2>&1" | Tee-Object -Append -FilePath $log | Select-String -Pattern 'test result|^error|FAILED|^warning: ' | ForEach-Object { $_.Line }
    if ($LASTEXITCODE -ne 0) { $failed += $step[0] }
}
if ($failed) { Write-Host "`nNOT OK: $($failed -join ', ')  (see $log)" -ForegroundColor Red; exit 1 }
Write-Host "`nAll OK: ready for a pull request into ccs03." -ForegroundColor Green
