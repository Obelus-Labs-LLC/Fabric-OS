# V8 Setup Script for Windows
# This script prepares the V8 build environment using WSL2 or Docker

param(
    [string]$Method = "wsl",  # "wsl" or "docker"
    [switch]$SkipDeps = $false
)

$ErrorActionPreference = "Stop"

$ProjectRoot = Split-Path -Parent $PSScriptRoot
$VendorDir = Join-Path $ProjectRoot "vendor"
$BuildDir = Join-Path $ProjectRoot "build"

Write-Host "=== V8 Setup for FabricOS (Windows) ===" -ForegroundColor Cyan
Write-Host "Method: $Method" -ForegroundColor Gray
Write-Host "Project root: $ProjectRoot" -ForegroundColor Gray
Write-Host ""

# Create directories
New-Item -ItemType Directory -Force -Path $VendorDir | Out-Null
New-Item -ItemType Directory -Force -Path $BuildDir | Out-Null

# Check prerequisites
if ($Method -eq "wsl") {
    Write-Host "Checking WSL2..." -ForegroundColor Yellow
    $wslCheck = wsl --status 2>&1
    if ($LASTEXITCODE -ne 0) {
        Write-Error "WSL2 not installed. Please install WSL2 first: wsl --install"
        exit 1
    }
    Write-Host "WSL2 is available" -ForegroundColor Green
    
    # Check for required packages
    Write-Host "Checking build dependencies in WSL..." -ForegroundColor Yellow
    $depsCheck = wsl bash -c "which python3 git curl 2>/dev/null | wc -l"
    if ($depsCheck -lt 3) {
        Write-Host "Installing dependencies in WSL..." -ForegroundColor Yellow
        wsl bash -c "sudo apt-get update && sudo apt-get install -y python3 git curl ninja-build clang lld"
    }
} 
elseif ($Method -eq "docker") {
    Write-Host "Checking Docker..." -ForegroundColor Yellow
    $dockerCheck = docker version 2>&1
    if ($LASTEXITCODE -ne 0) {
        Write-Error "Docker not installed or not running"
        exit 1
    }
    Write-Host "Docker is available" -ForegroundColor Green
}

Write-Host ""
Write-Host "=== Fetching V8 Source ===" -ForegroundColor Cyan

# Clone depot_tools if needed
$DepotTools = Join-Path $VendorDir "depot_tools"
if (-not (Test-Path $DepotTools)) {
    Write-Host "Cloning depot_tools..." -ForegroundColor Yellow
    git clone https://chromium.googlesource.com/chromium/tools/depot_tools.git $DepotTools
}

# Clone V8 if needed
$V8Dir = Join-Path $VendorDir "v8"
if (-not (Test-Path $V8Dir)) {
    Write-Host "Fetching V8..." -ForegroundColor Yellow
    New-Item -ItemType Directory -Force -Path $V8Dir | Out-Null
    
    if ($Method -eq "wsl") {
        # Use WSL for fetch (more reliable)
        $WslProjectRoot = wsl wslpath -u "$ProjectRoot"
        wsl bash -c "cd $WslProjectRoot && export PATH=\"$WslProjectRoot/vendor/depot_tools:\$PATH\" && fetch v8"
    }
    else {
        # Docker approach
        docker run --rm -v "${ProjectRoot}:/workspace" -w /workspace `
            ubuntu:22.04 bash -c "
                apt-get update && apt-get install -y git curl python3
                export PATH=/workspace/vendor/depot_tools:\$PATH
                fetch v8
            "
    }
}

# Checkout stable version
Write-Host "Checking out stable V8 version..." -ForegroundColor Yellow
if ($Method -eq "wsl") {
    $WslV8Dir = wsl wslpath -u "$V8Dir/v8"
    wsl bash -c "cd $WslV8Dir && git checkout 12.4.254.19 && gclient sync"
}

Write-Host ""
Write-Host "=== Setup Complete ===" -ForegroundColor Green
Write-Host ""
Write-Host "Next steps:" -ForegroundColor Cyan
Write-Host "1. Run the build script in WSL:" -ForegroundColor White
Write-Host "   wsl bash scripts/build_v8.sh" -ForegroundColor Gray
Write-Host ""
Write-Host "2. Or use Docker:" -ForegroundColor White
Write-Host "   docker run --rm -it -v `${PWD}:/workspace ubuntu:22.04" -ForegroundColor Gray
Write-Host "   cd /workspace && bash scripts/build_v8.sh" -ForegroundColor Gray
Write-Host ""
Write-Host "Note: V8 compilation requires 16GB+ RAM and takes 30-60 minutes" -ForegroundColor Yellow
