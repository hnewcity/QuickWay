#Requires -Version 5.1
param([string]$NpmRegistry = 'https://registry.npmjs.org')

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($env:OS -ne 'Windows_NT') { throw 'Run this script on Windows with the MSVC C++ Build Tools installed.' }
foreach ($program in @('cargo', 'rustup', 'node', 'npm.cmd', 'npx.cmd')) {
    if (-not (Get-Command $program -ErrorAction SilentlyContinue)) { throw "Missing $program. Install Rust (MSVC) and Node.js first." }
}

function Invoke-Checked([string]$Program, [string[]]$Arguments) {
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Program failed with exit code $LASTEXITCODE. Existing release files were not replaced." }
}

$projectRoot = Split-Path -Parent $PSScriptRoot
$desktopRoot = Join-Path $projectRoot 'desktop'
$coreTarget = Join-Path $projectRoot 'target'
$desktopTarget = Join-Path $desktopRoot 'src-tauri\target'
$target = 'x86_64-pc-windows-msvc'
$savedCi = $env:CI
$savedTargetDir = $env:CARGO_TARGET_DIR
Push-Location $projectRoot
try {
    # Tauri accepts true/false for CI, not 1. Explicit target paths avoid Mac build caches.
    $env:CI = 'true'
    $env:CARGO_TARGET_DIR = $desktopTarget
    Invoke-Checked 'rustup' @('target', 'add', $target)
    Push-Location $desktopRoot
    try {
        # Reinstall platform-specific Vite/Tauri binaries when moving the shared disk from Mac.
        Invoke-Checked 'npm.cmd' @('ci', '--registry', $NpmRegistry)
        Invoke-Checked 'npm.cmd' @('run', 'build')
        Invoke-Checked 'npx.cmd' @('--no-install', 'playwright', 'install', 'chromium')
        Invoke-Checked 'npm.cmd' @('run', 'test:ui', '--', '--project=windows-chromium')
    } finally { Pop-Location }

    Invoke-Checked 'cargo' @('test', '--locked', '--target', $target, '--target-dir', $coreTarget)
    Invoke-Checked 'cargo' @('test', '--locked', '--manifest-path', 'desktop/src-tauri/Cargo.toml', '--target', $target, '--target-dir', $desktopTarget)
    Invoke-Checked 'cargo' @('clippy', '--all-targets', '--locked', '--target', $target, '--target-dir', $coreTarget, '--', '-D', 'warnings')
    Invoke-Checked 'cargo' @('clippy', '--all-targets', '--locked', '--manifest-path', 'desktop/src-tauri/Cargo.toml', '--target', $target, '--target-dir', $desktopTarget, '--', '-D', 'warnings')
    Invoke-Checked 'cargo' @('build', '--release', '--locked', '--target', $target, '--target-dir', $coreTarget)
    Push-Location $desktopRoot
    try {
        Invoke-Checked 'npm.cmd' @('run', 'desktop:build:windows', '--', '--target', $target, '--', '--locked')
    } finally { Pop-Location }

    # Windows PowerShell 5.1 reads BOM-less files as the ANSI code page, which corrupts the Chinese description.
    $config = Get-Content -Raw -Encoding UTF8 -LiteralPath (Join-Path $desktopRoot 'src-tauri\tauri.conf.json') | ConvertFrom-Json
    $installerName = "QuickWay_$($config.version)_x64-setup.exe"
    $desktopRelease = Join-Path $desktopTarget "$target\release"
    $cliSource = Join-Path $coreTarget "$target\release\quickway.exe"
    $artifacts = @(
        @{ Source = (Join-Path $desktopRelease 'quickway-desktop.exe'); Name = 'QuickWay.exe' },
        @{ Source = (Join-Path $desktopRelease "bundle\nsis\$installerName"); Name = $installerName },
        @{ Source = $cliSource; Name = 'quickway-cli.exe' }
    )
    # Check every build output before touching the previous delivery.
    foreach ($artifact in $artifacts) {
        if (-not (Test-Path -LiteralPath $artifact.Source -PathType Leaf)) { throw "Missing build output: $($artifact.Source)" }
        $artifact.Hash = (Get-FileHash -LiteralPath $artifact.Source -Algorithm SHA256).Hash
    }
    $releaseDir = Join-Path $projectRoot 'release'
    $backupDir = Join-Path $releaseDir ('windows-backups\' + (Get-Date -Format 'yyyyMMdd-HHmmss-ffff'))
    New-Item -ItemType Directory -Path $backupDir -Force | Out-Null
    foreach ($artifact in $artifacts) {
        $destination = Join-Path $releaseDir $artifact.Name
        if (Test-Path -LiteralPath $destination) { Copy-Item -LiteralPath $destination -Destination $backupDir }
    }
    foreach ($artifact in $artifacts) {
        $destination = Join-Path $releaseDir $artifact.Name
        Copy-Item -LiteralPath $artifact.Source -Destination $destination -Force
        if ((Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash -ne $artifact.Hash) { throw "Copy verification failed: $destination. Previous files are in $backupDir" }
    }
    # Keep the existing Start-QuickWay / Stop-QuickWay scripts on the freshly built CLI.
    $legacyCliDir = Join-Path $coreTarget 'release'
    New-Item -ItemType Directory -Path $legacyCliDir -Force | Out-Null
    Copy-Item -LiteralPath $cliSource -Destination (Join-Path $legacyCliDir 'quickway.exe') -Force
    $checksums = Get-ChildItem -LiteralPath $releaseDir -File |
        Where-Object { $_.Extension -in @('.exe', '.dmg') } | Sort-Object Name | ForEach-Object {
            ((Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant() + '  ' + $_.Name)
        }
    $checksums | Set-Content -LiteralPath (Join-Path $releaseDir 'SHA256SUMS.txt') -Encoding ASCII
    $marker = Join-Path $releaseDir 'WINDOWS-REBUILD-REQUIRED.txt'
    if (Test-Path -LiteralPath $marker) { Remove-Item -LiteralPath $marker }
    Write-Host "Windows tests and build passed. Delivery: $releaseDir"
    Write-Host "Previous Windows release files: $backupDir"
} finally {
    $env:CI = $savedCi
    $env:CARGO_TARGET_DIR = $savedTargetDir
    Pop-Location
}
