# build-all.ps1
# Builds release binaries for Windows (native) and Linux (x86_64 musl, static),
# names them with the version tag, and packages portable zips into ./dist.
#
# Usage:
#   .\build-all.ps1          # build both platforms + package zips
#   .\build-all.ps1 -NoZip   # build both platforms, keep only raw binaries
#
# Requirements: cargo, cargo-zigbuild, zig (for the Linux target).

param(
    [switch]$NoZip
)

$ErrorActionPreference = "Stop"

# Read the version from Cargo.toml, e.g. version = "1.2.0" -> 1.2.0
$version = (Select-String -Path "Cargo.toml" -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
$tag = "rust-daemon-v$version"
$dist = "dist"
$winExe = "$tag-windows-x64.exe"
$linuxBin = "$tag-linux-x64"

Write-Host "Building $tag for Windows + Linux (x86_64)..."

# --- 1/3 Windows: native toolchain ---
Write-Host "[1/3] Building Windows (native)..."
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "Windows build failed" }

# --- 2/3 Linux: static musl via cargo-zigbuild ---
Write-Host "[2/3] Building Linux (x86_64-unknown-linux-musl)..."
cargo zigbuild --release --target x86_64-unknown-linux-musl
if ($LASTEXITCODE -ne 0) { throw "Linux build failed" }

# --- Stage the raw binaries ---
Write-Host "[3/3] Staging artifacts..."
New-Item -ItemType Directory -Force -Path $dist | Out-Null
Copy-Item "target\release\rust-daemon.exe" (Join-Path $dist $winExe) -Force
Copy-Item "target\x86_64-unknown-linux-musl\release\rust-daemon" (Join-Path $dist $linuxBin) -Force

if ($NoZip) {
    Write-Host "Done. Artifacts in ./$dist"
    exit 0
}

# --- Package portable zips: binary + .env.example + services/ + webui/ ---
foreach ($pkg in @(
    @{ Bin = $winExe; Zip = "$tag-windows-x64.zip" },
    @{ Bin = $linuxBin; Zip = "$tag-linux-x64.zip" }
)) {
    $stage = Join-Path $dist "stage-$($pkg.Zip)"
    Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force -Path (Join-Path $stage $tag) | Out-Null

    Copy-Item (Join-Path $dist $pkg.Bin) (Join-Path $stage "$tag\$($pkg.Bin)") -Force
    Copy-Item ".env.example" (Join-Path $stage "$tag\.env.example") -Force
    if (Test-Path "services") { Copy-Item "services" (Join-Path $stage $tag) -Recurse -Force }
    if (Test-Path "webui")    { Copy-Item "webui"    (Join-Path $stage $tag) -Recurse -Force }

    Compress-Archive -Path (Join-Path $stage "*") -DestinationPath (Join-Path $dist $pkg.Zip) -Force
    Remove-Item $stage -Recurse -Force
    Write-Host "  -> $dist\$($pkg.Zip)"
}

Write-Host "Done. Artifacts in ./$dist"
