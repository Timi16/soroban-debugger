# Test Fixture Contracts

This directory contains pre-compiled WASM test fixture contracts for use across all test suites.

## Contracts

- **counter** - Simple counter contract with increment, decrement, and get functions
- **echo** - Echo contract that returns its input unchanged
- **panic** - Contract that always panics, useful for error testing
- **budget_heavy** - Contract with budget-intensive operations for budget testing
- **cross_contract** - Contract that calls other contracts, for cross-contract call testing

## Building

To rebuild all WASM files from source:

### Linux/macOS:
```bash
./build.sh
```

### Windows:
```powershell
.\build.ps1
```

Or manually:
```bash
cd contracts/counter && cargo build --release --target wasm32-unknown-unknown
# Copy the resulting .wasm file to wasm/counter.wasm
```

## Usage in Tests

```rust
use std::path::PathBuf;

fn get_fixture_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("wasm")
        .join(format!("{}.wasm", name))
}

#[test]
fn test_with_counter() {
    let wasm_path = get_fixture_path("counter");
    let wasm_bytes = std::fs::read(&wasm_path).unwrap();
    // Use wasm_bytes in your test...
}
```

## Structure

```
tests/fixtures/
├── contracts/          # Source code for contracts
│   ├── counter/
│   ├── echo/
│   ├── panic/
│   ├── budget_heavy/
│   └── cross_contract/
├── wasm/               # Pre-compiled WASM files
│   ├── counter.wasm
│   ├── echo.wasm
│   ├── panic.wasm
│   ├── budget_heavy.wasm
│   └── cross_contract.wasm
├── build.sh            # Build script (Linux/macOS)
├── build.ps1           # Build script (Windows)
└── README.md           # This file
```
