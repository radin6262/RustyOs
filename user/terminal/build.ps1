$ErrorActionPreference = "Stop"

$AppDir = $PSScriptRoot
$TargetDir = Join-Path $AppDir "target"

Write-Host "========================================"
Write-Host " Building RustyOS Terminal"
Write-Host "========================================"

# ============================================================
# Go to Terminal project
# ============================================================

Set-Location $AppDir

# ============================================================
# Clean Terminal only
# ============================================================

Write-Host ""
Write-Host "Cleaning Terminal..."
Write-Host ""

cargo clean `
    --target-dir $TargetDir

if ($LASTEXITCODE -ne 0) {
    throw "cargo clean failed for Terminal"
}

# ============================================================
# Build Terminal
# ============================================================

Write-Host ""
Write-Host "Building Terminal..."
Write-Host ""

cargo build `
    --release `
    --target x86_64-unknown-none `
    --target-dir $TargetDir

if ($LASTEXITCODE -ne 0) {
    throw "cargo build failed for Terminal"
}

# ============================================================
# Locate generated ELF
# ============================================================

$BuiltTerminal = Join-Path `
    $TargetDir `
    "x86_64-unknown-none\release\Terminal"

$OutputTerminal = Join-Path `
    $AppDir `
    "Terminal"

if (-not (Test-Path $BuiltTerminal)) {
    throw "Terminal ELF was not found at: $BuiltTerminal"
}

# ============================================================
# Copy final ELF beside the source
# ============================================================

Copy-Item `
    -Path $BuiltTerminal `
    -Destination $OutputTerminal `
    -Force

# ============================================================
# Verify
# ============================================================

if (-not (Test-Path $OutputTerminal)) {
    throw "Failed to copy Terminal ELF to: $OutputTerminal"
}

Write-Host ""
Write-Host "========================================"
Write-Host " Terminal build complete!"
Write-Host "========================================"
Write-Host ""
Write-Host "Output:"
Write-Host $OutputTerminal
Write-Host ""