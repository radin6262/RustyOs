$ErrorActionPreference = "Stop"

$AppDir = $PSScriptRoot
$TargetDir = Join-Path $AppDir "target"
$TerminalDir = Join-Path `
    (Split-Path $AppDir -Parent) `
    "terminal"

$TerminalBuildScript = Join-Path `
    $TerminalDir `
    "build.ps1"

$TerminalElf = Join-Path `
    $TerminalDir `
    "Terminal"

Write-Host "========================================"
Write-Host " Building RustyOS Launcher"
Write-Host "========================================"

# ============================================================
# Make sure Terminal exists first.
#
# Launcher embeds Terminal with:
#
# include_bytes!("../../terminal/Terminal")
#
# so Terminal must already be built.
# ============================================================

if (-not (Test-Path $TerminalElf)) {

    Write-Host ""
    Write-Host "Terminal ELF not found."
    Write-Host "Building Terminal first..."
    Write-Host ""

    if (-not (Test-Path $TerminalBuildScript)) {
        throw "Terminal build script was not found at: $TerminalBuildScript"
    }

    & $TerminalBuildScript

    if ($LASTEXITCODE -ne 0) {
        throw "Terminal build failed"
    }
}

if (-not (Test-Path $TerminalElf)) {
    throw "Terminal ELF was not found at: $TerminalElf"
}

Write-Host ""
Write-Host "Terminal ELF found:"
Write-Host $TerminalElf
Write-Host ""

# ============================================================
# Go to Launcher project
# ============================================================

Set-Location $AppDir

# ============================================================
# Clean Launcher only
# ============================================================

Write-Host "Cleaning Launcher..."
Write-Host ""

cargo clean `
    --target-dir $TargetDir

if ($LASTEXITCODE -ne 0) {
    throw "cargo clean failed for Launcher"
}

# ============================================================
# Build Launcher
# ============================================================

Write-Host ""
Write-Host "Building Launcher..."
Write-Host ""

cargo build `
    --release `
    --target x86_64-unknown-none `
    --target-dir $TargetDir

if ($LASTEXITCODE -ne 0) {
    throw "cargo build failed for Launcher"
}

# ============================================================
# Locate generated ELF
# ============================================================

$BuiltLauncher = Join-Path `
    $TargetDir `
    "x86_64-unknown-none\release\Launcher"

$OutputLauncher = Join-Path `
    $AppDir `
    "Launcher"

if (-not (Test-Path $BuiltLauncher)) {
    throw "Launcher ELF was not found at: $BuiltLauncher"
}

# ============================================================
# Copy final ELF beside the source
# ============================================================

Copy-Item `
    -Path $BuiltLauncher `
    -Destination $OutputLauncher `
    -Force

# ============================================================
# Verify
# ============================================================

if (-not (Test-Path $OutputLauncher)) {
    throw "Failed to copy Launcher ELF to: $OutputLauncher"
}

Write-Host ""
Write-Host "========================================"
Write-Host " Launcher build complete!"
Write-Host "========================================"
Write-Host ""
Write-Host "Output:"
Write-Host $OutputLauncher
Write-Host ""