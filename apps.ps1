Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass


$ErrorActionPreference = "Stop"

$ProjectRoot = $PSScriptRoot

Write-Host "========================================"
Write-Host " Building RustyOS Userspace Applications"
Write-Host "========================================"
Write-Host ""

# ============================================================
# Build Terminal first
# ============================================================

Write-Host "[1/2] Building Terminal..."
Write-Host ""

& powershell `
    -ExecutionPolicy Bypass `
    -File (Join-Path $ProjectRoot "user\terminal\build.ps1")

if ($LASTEXITCODE -ne 0) {
    throw "Terminal build failed"
}

Write-Host ""
Write-Host "[1/2] Terminal build complete."
Write-Host ""

# ============================================================
# Build Launcher second
# ============================================================

Write-Host "[2/2] Building Launcher..."
Write-Host ""

& powershell `
    -ExecutionPolicy Bypass `
    -File (Join-Path $ProjectRoot "user\launcher\build.ps1")

if ($LASTEXITCODE -ne 0) {
    throw "Launcher build failed"
}

Write-Host ""
Write-Host "========================================"
Write-Host " Userspace build complete!"
Write-Host "========================================"
Write-Host ""
Write-Host "Terminal:"
Write-Host "  $ProjectRoot\user\terminal\Terminal"
Write-Host ""
Write-Host "Launcher:"
Write-Host "  $ProjectRoot\user\launcher\Launcher"
Write-Host ""
