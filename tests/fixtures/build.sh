#!/bin/bash
# Build script to compile all test fixture contracts to WASM
#
# Usage: ./build.sh
#
# Prerequisites:
#   - Rust toolchain installed
#   - wasm32-unknown-unknown target: rustup target add wasm32-unknown-unknown
#
# This script builds all contracts in tests/fixtures/contracts/ and
# places the compiled WASM files in tests/fixtures/wasm/

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONTRACTS_DIR="${SCRIPT_DIR}/contracts"
WASM_DIR="${SCRIPT_DIR}/wasm"

# Check if wasm32 target is installed
if ! rustup target list --installed | grep -q "wasm32-unknown-unknown"; then
    echo "Error: wasm32-unknown-unknown target not installed."
    echo "Install it with: rustup target add wasm32-unknown-unknown"
    exit 1
fi

# Create wasm output directory
mkdir -p "${WASM_DIR}"

echo "Building test fixture contracts..."

# Build each contract
for contract_dir in "${CONTRACTS_DIR}"/*/; do
    if [ -f "${contract_dir}Cargo.toml" ]; then
        contract_name=$(basename "${contract_dir}")
        echo "  Building ${contract_name}..."
        
        (
            cd "${contract_dir}"
            cargo build --release --target wasm32-unknown-unknown
            
            # Find the generated WASM file
            wasm_file="target/wasm32-unknown-unknown/release/${contract_name//-/_}.wasm"
            
            if [ -f "${wasm_file}" ]; then
                cp "${wasm_file}" "${WASM_DIR}/${contract_name}.wasm"
                echo "    ✓ Built ${contract_name}.wasm"
            else
                echo "    ✗ Failed to find WASM output for ${contract_name}"
                exit 1
            fi
        )
    fi
done

echo ""
echo "All contracts built successfully!"
echo "WASM files are in: ${WASM_DIR}"
