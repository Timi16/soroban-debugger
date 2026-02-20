# Building Test Fixture WASM Files

## Prerequisites

1. **Rust toolchain** - Ensure Rust is installed via rustup
2. **WASM target** - Install the wasm32-unknown-unknown target:
   ```bash
   rustup target add wasm32-unknown-unknown
   ```

## Building

### Linux/macOS
```bash
cd tests/fixtures
chmod +x build.sh
./build.sh
```

### Windows (PowerShell)
```powershell
cd tests/fixtures
.\build.ps1
```

### Manual Build

If you prefer to build contracts individually:

```bash
cd tests/fixtures/contracts/counter
cargo build --release --target wasm32-unknown-unknown
cp target/wasm32-unknown-unknown/release/counter_fixture.wasm ../../wasm/counter.wasm
```

Repeat for each contract:
- `echo` → `echo.wasm`
- `panic` → `panic.wasm`
- `budget_heavy` → `budget_heavy.wasm`
- `cross_contract` → `cross_contract.wasm`

## Verification

After building, verify the WASM files exist:

```bash
ls -lh tests/fixtures/wasm/*.wasm
```

You should see 5 WASM files:
- `counter.wasm`
- `echo.wasm`
- `panic.wasm`
- `budget_heavy.wasm`
- `cross_contract.wasm`

## Troubleshooting

### "wasm32-unknown-unknown target not installed"
Run: `rustup target add wasm32-unknown-unknown`

### "No such file or directory" errors
Ensure you're running the build script from the `tests/fixtures/` directory, or adjust paths accordingly.

### Build failures
Check that:
1. All dependencies in `Cargo.toml` files are correct
2. The Soroban SDK version matches the main project (22.0.0)
3. You have write permissions in the `wasm/` directory
