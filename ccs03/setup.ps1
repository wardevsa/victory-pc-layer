# CCS03 developer setup for Windows 10/11 x64.
# Run from the repository root:  powershell -ExecutionPolicy Bypass -File ccs03\setup.ps1
# Installs what is missing (Git, Rust, the MSVC build tools, DXC), sets this
# clone's Git identity, installs the asset-policy hook and runs the tests.
# Needs no game files.
param(
    [string]$Name,   # your GitHub user name
    [string]$Email   # your GitHub noreply address (Settings > Emails)
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo
function Step($text) { Write-Host "`n=== $text ===" -ForegroundColor Cyan }
function Have($cmd) { [bool](Get-Command $cmd -ErrorAction SilentlyContinue) }

Step 'Git'
if (-not (Have git)) {
    winget install --id Git.Git -e --accept-package-agreements --accept-source-agreements
    $env:PATH += ';C:\Program Files\Git\cmd'
}
git --version

Step 'Git identity for this clone only'
if (-not $Name)  { $Name  = Read-Host 'GitHub user name' }
if (-not $Email) { $Email = Read-Host 'GitHub noreply e-mail (Settings > Emails, e.g. 123+name@users.noreply.github.com)' }
git config user.name $Name
git config user.email $Email
git config core.autocrlf false
Write-Host "commits here: $(git config user.name) <$(git config user.email)>"

Step 'MSVC build tools'
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
$vc = if (Test-Path $vswhere) { & $vswhere -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property displayName } else { $null }
if (-not $vc) {
    Write-Host 'Installing Visual Studio Build Tools (C++ workload); Windows may ask for admin rights.'
    winget install --id Microsoft.VisualStudio.2022.BuildTools -e --accept-package-agreements --accept-source-agreements `
        --override '--quiet --wait --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended'
} else { Write-Host "found: $vc" }

Step 'Rust (the version pinned in rust-toolchain.toml)'
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if (-not (Have rustup)) {
    $init = Join-Path $env:TEMP 'rustup-init.exe'
    Invoke-WebRequest -UseBasicParsing 'https://win.rustup.rs/x86_64' -OutFile $init
    & $init -y --profile minimal --default-toolchain none
    $env:PATH = "$cargoBin;$env:PATH"
}
rustup toolchain install
cargo --version

Step 'DXC (shader compiler and validator for the tests)'
$dxcDir = Join-Path $env:LOCALAPPDATA 'ccs03\dxc'
if (-not (Test-Path "$dxcDir\bin\x64\dxil.dll")) {
    $rel = Invoke-RestMethod 'https://api.github.com/repos/microsoft/DirectXShaderCompiler/releases/latest'
    $asset = $rel.assets | Where-Object { $_.name -match '^dxc_.*\.zip$' } | Select-Object -First 1
    Write-Host "downloading $($rel.tag_name) / $($asset.name)"
    $zip = Join-Path $env:TEMP 'dxc.zip'
    Invoke-WebRequest -UseBasicParsing $asset.browser_download_url -OutFile $zip
    Expand-Archive -Force $zip $dxcDir
}
[Environment]::SetEnvironmentVariable('VICTORY_DXIL', "$dxcDir\bin\x64\dxil.dll", 'User')
[Environment]::SetEnvironmentVariable('VICTORY_PC_DXC', "$dxcDir\bin\x64\dxc.exe", 'User')
$env:VICTORY_DXIL = "$dxcDir\bin\x64\dxil.dll"
$env:VICTORY_PC_DXC = "$dxcDir\bin\x64\dxc.exe"
Write-Host "VICTORY_DXIL = $env:VICTORY_DXIL"

Step 'Remotes'
# upstream = victory-fork (the base of ccs03), victory = VIctory (its base).
foreach ($r in @(@('upstream', 'https://github.com/fxceless/victory-fork.git'),
                 @('victory',  'https://github.com/Marvisak/VIctory.git'))) {
    if (git remote | Select-String -Quiet "^$($r[0])$") { git remote set-url $r[0] $r[1] }
    else { git remote add $r[0] $r[1] }
    git remote set-url --push $r[0] no_push
    git fetch $r[0] main
}
git remote -v

Step 'Asset-policy hook'
cargo xtask install-hooks

Step 'Tests (no game files needed)'
& (Join-Path $PSScriptRoot 'test.ps1')
