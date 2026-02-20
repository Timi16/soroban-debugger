# NFT Example Contract

A simple NFT (Non-Fungible Token) contract for the Stellar Soroban platform.
Use this contract to explore the **soroban-debugger** features: breakpoints,
storage inspection, budget tracking, and event tracing.

## Contract Functions

| Function       | Description                                      |
| -------------- | ------------------------------------------------ |
| `initialize`   | Set the admin address (call once)                |
| `mint`         | Mint a new NFT with name & description metadata  |
| `transfer`     | Transfer ownership of a token                    |
| `burn`         | Destroy a token and its metadata                 |
| `owner_of`     | Query the current owner of a token               |
| `metadata`     | Query the metadata map for a token               |
| `total_supply` | Query the current number of tokens in existence  |

## Build

```bash
cd examples/contracts/nft
cargo build --target wasm32-unknown-unknown --release
```

The compiled WASM will be at:
```
target/wasm32-unknown-unknown/release/soroban_nft_example.wasm
```

## Debugger Walkthrough

Below are example commands using **soroban-debug** to explore this contract.

### 1. Inspect the Contract

View exported functions without executing:

```bash
soroban-debug inspect \
  --contract target/wasm32-unknown-unknown/release/soroban_nft_example.wasm \
  --functions
```

### 2. Debug a Mint Call

Step through a `mint` invocation with breakpoints and storage inspection:

```bash
soroban-debug run \
  --contract target/wasm32-unknown-unknown/release/soroban_nft_example.wasm \
  --function mint \
  --args '["GABC...OWNER", "Cool NFT", "A very cool NFT"]' \
  --breakpoint mint \
  --show-events \
  --verbose
```

During execution the debugger will:
- Pause at the `mint` function entry (breakpoint)
- Show the token ID allocation from storage
- Display the metadata map being written
- Print the `mint` event at the end

### 3. Debug a Transfer with Storage Inspection

```bash
soroban-debug run \
  --contract target/wasm32-unknown-unknown/release/soroban_nft_example.wasm \
  --function transfer \
  --args '[1, "GXYZ...NEWOWNER"]' \
  --storage '{"Owner(1)": "GABC...OWNER", "NextId": 2, "Supply": 1}' \
  --breakpoint transfer \
  --show-events
```

Use the interactive `storage` command to verify the owner changed:

```
(debug) storage
Storage:
  Owner(1) = GXYZ...NEWOWNER
  NextId   = 2
  Supply   = 1
```

### 4. Interactive Debugging Session

Start a full interactive session for exploratory debugging:

```bash
soroban-debug interactive \
  --contract target/wasm32-unknown-unknown/release/soroban_nft_example.wasm
```

Useful commands during the session:

```
(debug) step          # Execute next instruction
(debug) storage       # Inspect all storage entries
(debug) budget        # Show CPU and memory usage
(debug) stack         # View the call stack
(debug) continue      # Run to next breakpoint
```

### 5. Budget Analysis with --repeat

Stress-test the mint function to analyze resource usage:

```bash
soroban-debug run \
  --contract target/wasm32-unknown-unknown/release/soroban_nft_example.wasm \
  --function mint \
  --args '["GABC...OWNER", "Stress Test NFT", "Testing budget"]' \
  --repeat 10
```

This will execute 10 mints and display aggregate stats:

```
╔══════════════════════════════════════════╗
║       Repeat Run Summary (10 runs)       ║
╠══════════════════════════════════════════╣
║  Execution Time                          ║
║    Min:      1.234ms                     ║
║    Max:      2.567ms                     ║
║    Avg:      1.890ms                     ║
╠══════════════════════════════════════════╣
║  CPU Budget (instructions)               ║
║    Min:        45000                     ║
║    Max:        45200                     ║
║    Avg:        45100                     ║
╚══════════════════════════════════════════╝
```

## Running Tests

```bash
cd examples/contracts/nft
cargo test
```
