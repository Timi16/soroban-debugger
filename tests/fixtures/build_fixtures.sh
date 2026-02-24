#!/bin/bash
set -e
WASM_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/wasm"
mkdir -p "$WASM_DIR"
for dir in "$(dirname "${BASH_SOURCE[0]}")"/contracts/*/; do
    if [ -f "${dir}Cargo.toml" ]; then
        name=$(basename "$dir")
        (cd "$dir" && cargo build --release --target wasm32-unknown-unknown)
        cp "${dir}target/wasm32-unknown-unknown/release/${name//-/_}.wasm" "$WASM_DIR/${name}.wasm"
    fi
done
