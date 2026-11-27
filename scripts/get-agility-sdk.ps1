# Fetches Microsoft's D3D12 Agility SDK runtime (D3D12Core.dll, and the debug
# layer d3d12SDKLayers.dll) from NuGet into D3D12\ beside victory.exe (and beside
# the test programs in target\debug\deps). For a build with `--features agility-sdk`,
# which exports D3D12SDKVersion and D3D12SDKPath for it (apps/victory/src/agility.rs,
# crates/victory-shader/src/agility_tests.rs, crates/victory-gpu/src/agility_tests.rs):
# needed on Windows 10, whose own D3D12 has no shader model 6.6 bindless heaps.
# Such a build needs the DLL there: without it D3D12 does not start
# (D3D12_ERROR_INVALID_REDIST). Not for Wine or Proton. The DLLs are Microsoft's,
# under the package's license, and are never committed (docs/asset-policy.md).
#
#   powershell -ExecutionPolicy Bypass -File scripts\get-agility-sdk.ps1 [-Build release|diag]
param(
    [ValidateSet('release', 'diag', 'debug')]
    [string[]]$Build = @('release', 'diag', 'debug')
)
$ErrorActionPreference = 'Stop'
# Keep the middle number equal to D3D12SDKVersion in apps/victory/src/agility.rs.
$version = '1.619.6'
$targets = $Build | ForEach-Object {
    $dir = Join-Path $PSScriptRoot "..\target\$_"
    $dir
    Join-Path $dir 'deps'  # the test programs
} | Where-Object { Test-Path $_ }
if (-not $targets) {
    Write-Error "No target\$($Build -join ', target\') yet: build victory first (cargo build --release --locked -p victory --features agility-sdk)."
}
$work = Join-Path ([IO.Path]::GetTempPath()) "victory-agility-$version"
New-Item -ItemType Directory -Force $work | Out-Null
$package = Join-Path $work 'package.zip'
if (-not (Test-Path $package)) {
    $url = "https://api.nuget.org/v3-flatcontainer/microsoft.direct3d.d3d12/$version/microsoft.direct3d.d3d12.$version.nupkg"
    Write-Host "Downloading the Agility SDK $version from nuget.org"
    Invoke-WebRequest -Uri $url -OutFile $package -UseBasicParsing
}
$unpacked = Join-Path $work 'unpacked'
if (-not (Test-Path (Join-Path $unpacked 'build\native\bin\x64\D3D12Core.dll'))) {
    Expand-Archive -Path $package -DestinationPath $unpacked -Force
}
foreach ($target in $targets) {
    $into = Join-Path $target 'D3D12'
    New-Item -ItemType Directory -Force $into | Out-Null
    foreach ($dll in 'D3D12Core.dll', 'd3d12SDKLayers.dll') {
        Copy-Item -Force (Join-Path $unpacked "build\native\bin\x64\$dll") $into
    }
    Write-Host "Agility SDK $version in $((Resolve-Path $into).Path)"
}
