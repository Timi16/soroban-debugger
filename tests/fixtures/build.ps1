# PowerShell build script to compile all test fixture contracts to WASM
#
# Usage: .\build.ps1
#
# Prerequisites:
#   - Rust toolchain installed
#   - wasm32-unknown-unknown target: rustup target add wasm32-unknown-unknown
#
# This script builds all contracts in tests/fixtures/contracts/ and
# places the compiled WASM files in tests/fixtures/wasm/

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ContractsDir = Join-Path $ScriptDir "contracts"
$WasmDir = Join-Path $ScriptDir "wasm"

# Check if wasm32 target is installed
$InstalledTargets = rustup target list --installed
if ($InstalledTargets -notcontains "wasm32-unknown-unknown") {
    Write-Host "Error: wasm32-unknown-unknown target not installed." -ForegroundColor Red
    Write-Host "Install it with: rustup target add wasm32-unknown-unknown" -ForegroundColor Yellow
    exit 1
}

# Create wasm output directory
New-Item -ItemType Directory -Force -Path $WasmDir | Out-Null

Write-Host "Building test fixture contracts..." -ForegroundColor Cyan

# Build each contract
Get-ChildItem -Path $ContractsDir -Directory | ForEach-Object {
    $ContractDir = $_.FullName
    $ContractName = $_.Name
    $CargoToml = Join-Path $ContractDir "Cargo.toml"
    
    if (Test-Path $CargoToml) {
        Write-Host "  Building $ContractName..." -ForegroundColor Yellow
        
        Push-Location $ContractDir
        try {
            cargo build --release --target wasm32-unknown-unknown
            
            # Find the generated WASM file
            $WasmFileName = $ContractName -replace "-", "_"
            $WasmFile = Join-Path $ContractDir "target\wasm32-unknown-unknown\release\${WasmFileName}.wasm"
            
            if (Test-Path $WasmFile) {
                Copy-Item $WasmFile (Join-Path $WasmDir "${ContractName}.wasm")
                Write-Host "    ✓ Built ${ContractName}.wasm" -ForegroundColor Green
            } else {
                Write-Host "    ✗ Failed to find WASM output for $ContractName" -ForegroundColor Red
                exit 1
            }
        } finally {
            Pop-Location
        }
    }
}

Write-Host ""
Write-Host "All contracts built successfully!" -ForegroundColor Green
Write-Host "WASM files are in: $WasmDir" -ForegroundColor Cyan
