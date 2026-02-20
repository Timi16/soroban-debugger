# Video Tutorial Script: Debugging Token Transfer Contracts with Soroban Debugger

This guide is a **production-ready script + runbook** for recording a video tutorial that demonstrates how to debug a token transfer flow with `soroban-debug`.

> Goal: show viewers how to load a contract, set breakpoints on `transfer`, and inspect storage changes before/after execution.

## Audience and prerequisites

- Audience: Rust/Soroban developers who want practical debugger workflows.
- Prerequisites:
  - Rust toolchain installed.
  - `soroban-debug` available (installed or run with `cargo run -- ...`).
  - A token-style WASM contract exposing a `transfer` function.

---

## Timestamped recording script

## 00:00 — Intro (What viewers will learn)

**Narration**

“Welcome! In this tutorial we’ll debug a token transfer contract with the Soroban debugger. We’ll load a WASM contract, place breakpoints at `transfer`, step through execution, and inspect storage updates so we can verify balances and ownership-like state changes safely.”

**On-screen actions**

- Show repo root and explain the final workflow at a high level.
- Mention expected artifacts: contract path, transfer args, optional initial storage JSON.

---

## 00:45 — Environment check

**Narration**

“Before debugging contract logic, verify your debugger command works. This avoids burning time on tool setup while recording.”

**On-screen actions**

Run:

```bash
cargo run -- --help
cargo run -- run --help
cargo run -- interactive --help
```

Explain that if `soroban-debug` is installed globally, the same commands can be run as `soroban-debug ...`.

---

## 02:00 — Setup and contract loading

**Narration**

“Now let’s point the debugger at our token transfer contract. I’ll use a local WASM artifact with a `transfer` function. The key idea is that this command proves the contract is loadable and lists what we can invoke.”

**On-screen actions**

```bash
# Set your contract path once for easier copy/paste
export CONTRACT_WASM="./artifacts/token_contract.wasm"

# Inspect available functions and signatures
cargo run -- inspect --contract "$CONTRACT_WASM" --functions
```

Call out the `transfer` function in the output and explain expected argument order.

---

## 03:30 — First transfer debug run with breakpoint

**Narration**

“Next, we’ll execute `transfer` and stop exactly at the function boundary. This gives us a stable breakpoint where we can inspect state before the write occurs.”

**On-screen actions**

```bash
cargo run -- run \
  --contract "$CONTRACT_WASM" \
  --function transfer \
  --args '["alice", "bob", 100]' \
  --breakpoint transfer \
  --show-events \
  --verbose
```

Explain what to watch for:

- breakpoint hit confirmation,
- argument decode (`from`, `to`, `amount`),
- event traces during execution.

---

## 05:00 — Inspect storage and storage diff workflow

**Narration**

“Now we verify the transfer changed only what we expect. The best habit is snapshot-before, execute, snapshot-after, then compare.”

**On-screen actions**

1. Start interactive session:

```bash
cargo run -- interactive --contract "$CONTRACT_WASM"
```

2. In interactive prompt, use:

```text
(debug) storage
(debug) break transfer
(debug) continue
(debug) step
(debug) storage
(debug) budget
(debug) continue
```

3. Explain a practical diff method:
   - Save pre-transfer storage output to `before.json`.
   - Save post-transfer storage output to `after.json`.
   - Compare with your preferred diff tool (`diff`, IDE compare, or JSON diff extension).

Example CLI compare:

```bash
diff -u before.json after.json
```

Narrate expected difference:
- sender balance decreases,
- receiver balance increases,
- no unrelated keys mutate.

---

## 07:30 — Repeat run for confidence

**Narration**

“Once logic looks correct, run repeated executions to catch flaky behavior and monitor resource usage.”

**On-screen actions**

```bash
cargo run -- run \
  --contract "$CONTRACT_WASM" \
  --function transfer \
  --args '["alice", "bob", 1]' \
  --repeat 5
```

Point out summary stats and explain why repeat mode is useful for regression checks.

---

## 08:30 — Common mistakes and how to fix them

### 1) `contract file not found`

**Cause**: wrong path or missing build artifact.

**Fix**:
- `pwd` and verify location.
- `ls` the artifact directory.
- update `CONTRACT_WASM` to an absolute path.

### 2) `function not found: transfer`

**Cause**: wrong export name or contract doesn’t expose `transfer`.

**Fix**:
- run inspect with `--functions` and copy exact function name.
- ensure you compiled the correct contract version.

### 3) JSON argument parse errors

**Cause**: malformed JSON or wrong argument order/types.

**Fix**:
- validate JSON with `jq .`.
- switch to typed arguments when needed:
  `[{"type":"symbol","value":"alice"},{"type":"symbol","value":"bob"},{"type":"u64","value":100}]`

### 4) Breakpoint never hits

**Cause**: breakpoint name mismatch or execution path never calls that function.

**Fix**:
- set breakpoint to exact exported name.
- add `--verbose` and step manually to confirm path.

### 5) Storage appears unchanged

**Cause**: auth failure/revert, failed preconditions, or reading wrong keys.

**Fix**:
- inspect command output for errors.
- verify initial storage and auth assumptions.
- inspect both before and after snapshots and compare only relevant keys.

### 6) Network blocked while building example contracts

**Cause**: restricted environment cannot fetch crates.

**Fix**:
- use a prebuilt WASM artifact.
- run in an environment with crates access.
- vendor dependencies if required by CI policy.

---

## 09:45 — Outro

**Narration**

“Today we loaded a Soroban contract, set transfer breakpoints, stepped through execution, and validated storage changes with a before/after diff. Reuse this exact flow as your default token debugging checklist.”

---

## All commands used (copy/paste reference)

```bash
# Tooling checks
cargo run -- --help
cargo run -- run --help
cargo run -- interactive --help

# Contract loading / function discovery
export CONTRACT_WASM="./artifacts/token_contract.wasm"
cargo run -- inspect --contract "$CONTRACT_WASM" --functions

# Breakpointed transfer debug run
cargo run -- run \
  --contract "$CONTRACT_WASM" \
  --function transfer \
  --args '["alice", "bob", 100]' \
  --breakpoint transfer \
  --show-events \
  --verbose

# Interactive storage inspection
cargo run -- interactive --contract "$CONTRACT_WASM"
# then inside debugger:
# (debug) storage
# (debug) break transfer
# (debug) continue
# (debug) step
# (debug) storage
# (debug) budget
# (debug) continue

# Manual storage diff
# Save storage output to before.json and after.json, then:
diff -u before.json after.json

# Repeat run / stress validation
cargo run -- run \
  --contract "$CONTRACT_WASM" \
  --function transfer \
  --args '["alice", "bob", 1]' \
  --repeat 5
```

---

## Validation log (tested by following instructions)

I validated the tutorial flow in this repository by running the command discovery and CLI help steps exactly as written (`cargo run -- --help`, `cargo run -- run --help`, `cargo run -- interactive --help`).

For contract-specific execution (`inspect/run/interactive` against `token_contract.wasm`), this guide assumes you provide a local prebuilt token contract artifact at `./artifacts/token_contract.wasm`.
