<#
  build-installer.ps1
  -------------------
  Builds the GymPOS standalone NSIS installer locally.
  Run from ANY directory – the script resolves its own paths.

  Usage:
    .\scripts\build-installer.ps1
    .\scripts\build-installer.ps1 -Version 0.2.0

  Output:
    gympos-saas\bin\GymPOS-Setup-<version>-x64.exe
    gympos-saas\bin\latest.json           (updater manifest)

  Requirements:
    - Rust stable (https://rustup.rs/)
    - Tauri CLI  (cargo install tauri-cli --version "^2")
    - NSIS 3.x   (https://nsis.sourceforge.io/)  – Tauri bundles it automatically on Windows
#>

param (
    [string]$Version = ""   # Optionally override version; leave blank to read from Cargo.toml
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# ── Paths ─────────────────────────────────────────────────────────────────────
$RepoRoot   = Split-Path -Parent $PSScriptRoot
$TauriDir   = Join-Path $RepoRoot "desktop\src-tauri"
$ModelsDir  = Join-Path $RepoRoot "desktop\models"
$BinDir     = Join-Path $RepoRoot "bin"
$CargoToml  = Join-Path $TauriDir "Cargo.toml"
$TauriConf  = Join-Path $TauriDir "tauri.conf.json"
$SeedDb     = Join-Path $BinDir "gympos_local.sqlite"

# ── Determine version ─────────────────────────────────────────────────────────
if (-not $Version) {
    $match = Select-String -Path $CargoToml -Pattern '^version\s*=\s*"(.+)"' | Select-Object -First 1
    if ($match) {
        $Version = $match.Matches[0].Groups[1].Value
    } else {
        Write-Error "Could not parse version from Cargo.toml"
        exit 1
    }
}
Write-Host "⚙️  Building GymPOS v$Version installer..." -ForegroundColor Cyan

# ── Sanity checks ─────────────────────────────────────────────────────────────
foreach ($path in @($ModelsDir, $TauriDir, $SeedDb)) {
    if (-not (Test-Path $path)) {
        Write-Error "Required path not found: $path"
        exit 1
    }
}

# Confirm models are present
$OnnxFiles = Get-ChildItem -Path $ModelsDir -Filter "*.onnx"
if ($OnnxFiles.Count -eq 0) {
    Write-Error "No .onnx models found in $ModelsDir — cannot build."
    exit 1
}
Write-Host "  ✓ Models found ($($OnnxFiles.Count) .onnx files)" -ForegroundColor Green

# ── Build ─────────────────────────────────────────────────────────────────────
Push-Location $TauriDir
try {
    Write-Host "  → Running: cargo tauri build" -ForegroundColor DarkCyan
    cargo tauri build 2>&1 | Tee-Object -FilePath (Join-Path $BinDir "build.log")
    if ($LASTEXITCODE -ne 0) {
        Write-Error "Tauri build failed — see $BinDir\build.log"
        exit 1
    }
} finally {
    Pop-Location
}

# ── Collect artifacts ─────────────────────────────────────────────────────────
$BundleDir = Join-Path $TauriDir "target\release\bundle\nsis"
$Installer  = Get-ChildItem -Path $BundleDir -Filter "*-setup.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
$UpdaterJson = Get-ChildItem -Path $BundleDir -Filter "latest.json" -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1

if (-not $Installer) {
    Write-Error "Installer .exe not found in $BundleDir — build may have failed."
    exit 1
}

# Copy to bin/
$DestInstaller = Join-Path $BinDir "GymPOS-Setup-$Version-x64.exe"
Copy-Item -Path $Installer.FullName -Destination $DestInstaller -Force
Write-Host "  ✓ Installer → $DestInstaller" -ForegroundColor Green

if ($UpdaterJson) {
    $DestJson = Join-Path $BinDir "latest.json"
    Copy-Item -Path $UpdaterJson.FullName -Destination $DestJson -Force
    Write-Host "  ✓ Updater manifest → $DestJson" -ForegroundColor Green
}

# ── Summary ───────────────────────────────────────────────────────────────────
$SizeMB = [math]::Round((Get-Item $DestInstaller).Length / 1MB, 1)
Write-Host ""
Write-Host "✅  Build complete!" -ForegroundColor Green
Write-Host "   Installer : $DestInstaller ($SizeMB MB)" -ForegroundColor White
Write-Host "   To release: git tag v$Version && git push origin v$Version" -ForegroundColor Yellow
Write-Host ""
