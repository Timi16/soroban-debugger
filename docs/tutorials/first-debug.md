Debugging Your First Soroban Contract

This tutorial will take you from zero to successfully debugging a Soroban smart contract. You will learn how to build a contract with debug symbols, attach the debugger, set breakpoints, step through your Rust code, and inspect the contract's storage.

**Prerequisites:**
* Rust toolchain installed
* `wasm32-unknown-unknown` target installed (`rustup target add wasm32-unknown-unknown`)
* Soroban CLI installed

---

## 1. Installing the Debugger

To step through Soroban WebAssembly (WASM) execution, you need the Soroban debugger. 

Install it via Cargo by running the following command in your terminal:

```bash
cargo install soroban-debugger
```

Verify the installation was successful by checking the version:

```bash
soroban-debugger --version
```

*Expected Output:*
```text
soroban-debugger 1.0.0
```

## 2. Creating a Small Contract

Let's create a simple counter contract to debug. 

First, initialize a new Soroban project:

```bash
soroban contract init debug-tutorial
cd debug-tutorial
```

Open `contracts/hello_world/src/lib.rs` and replace its contents with the following counter implementation:

```rust
#![no_std]
use soroban_sdk::{contract, contractimpl, symbol_short, Env, Symbol};

const COUNTER: Symbol = symbol_short!("COUNTER");

#[contract]
pub struct CounterContract;

#[contractimpl]
impl CounterContract {
    pub fn increment(env: Env) -> u32 {
        // Breakpoint target 1
        let mut count: u32 = env.storage().instance().get(&COUNTER).unwrap_or(0);
        
        // Breakpoint target 2
        count += 1;
        
        env.storage().instance().set(&COUNTER, &count);
        count
    }
}
```

## 3. Building to WASM (With Debug Symbols)

By default, release builds strip away the debugging information (DWARF symbols) needed to map WASM instructions back to your Rust source code. We need to ensure debug symbols are included.

Open `contracts/hello_world/Cargo.toml` and ensure your `[profile.dev]` or `[profile.release]` includes `debug = true`:

```toml
[profile.dev]
debug = true
opt-level = 0
```

Now, build the contract:

```bash
soroban contract build
```

Verify that your WASM file was generated at `target/wasm32-unknown-unknown/release/hello_world.wasm`. Because debug symbols are included, the file size will be significantly larger than a heavily optimized production build.

## 4. Starting the Debugger

Launch the debugger and pass the path to your compiled WASM file.

```bash
soroban-debugger target/wasm32-unknown-unknown/release/hello_world.wasm
```

*Expected Output:*
```text
Loading WASM module...
Debug symbols loaded successfully.
(soroban-debug) 
```

You are now in the interactive debugging prompt. Type `help` to see all available basic debugger commands (`run`, `step`, `next`, `break`, `print`, `storage`).

## 5. Setting Breakpoints

Before running the contract, we need to tell the debugger where to pause execution. You can set breakpoints by function name or by file and line number.

Let's set a breakpoint on line 13 of `lib.rs` (the start of our `increment` function):

```bash
(soroban-debug) break src/lib.rs:13
```

*Expected Output:*
```text
Breakpoint 1 set at src/lib.rs:13
```

![Setting a Breakpoint](./images/debugger-breakpoint.png)
*(Screenshot: Terminal showing the breakpoint confirmation and the interactive prompt)*

## 6. Running and Stepping Through Code

To trigger the execution, we use the `invoke` command, simulating a call to the contract.

```bash
(soroban-debug) invoke increment
```

*Expected Output:*
```text
Executing 'increment'...
Hit Breakpoint 1 at src/lib.rs:13

12 |     pub fn increment(env: Env) -> u32 {
13 | >       let mut count: u32 = env.storage().instance().get(&COUNTER).unwrap_or(0);
14 |         count += 1;
```

Execute the current line and move to the next one using the `next` (or `n`) command:

```bash
(soroban-debug) next
```

## 7. Inspecting Storage and Variables

Now that we have stepped past line 13, the `count` variable has been initialized. Let's inspect it using the `print` command:

```bash
(soroban-debug) print count
```

*Expected Output:*
```text
count = 0
```

Step one more time to execute `count += 1;`:

```bash
(soroban-debug) next
(soroban-debug) print count
```

*Expected Output:*
```text
count = 1
```

**Inspecting Contract Storage:**
Soroban contracts interact heavily with host storage. You can inspect the current state of the mock ledger using the `storage` command:

```bash
(soroban-debug) storage get COUNTER
```

*Expected Output:*
```text
Key: Symbol("COUNTER")
Value: U32(1)
```

Type `continue` (or `c`) to let the function finish executing.

```bash
(soroban-debug) continue
```

*Expected Output:*
```text
Execution completed. 
Return value: U32(1)
```

## Common Workflows + Troubleshooting

### Issue: "No debug symbols found"
**Symptom:** The debugger loads the WASM but warns that symbols are missing, and breakpoints won't bind.
**Fix:** Ensure you added `debug = true` in your `Cargo.toml` profile and rebuilt the contract. Do not use `--release` if your release profile has `debug = false`. You can also try running `cargo clean` before rebuilding.

### Issue: Variables print as `<optimized out>`
**Symptom:** You type `print count` and the debugger returns `<optimized out>`.
**Fix:** The Rust compiler optimized the variable away. Change `opt-level = 0` in your `Cargo.toml` development profile to prevent the compiler from aggressively inlining and removing local variables.

### Issue: Breakpoint hits the wrong line
**Symptom:** You set a breakpoint at line 15, but execution halts at line 18.
**Fix:** This is a common artifact of Rust macros (like `#[contractimpl]`) generating boilerplate code under the hood. Try setting the breakpoint one line lower, or use function name breakpoints instead: `break increment`.

## Implementation Notes

* **WASM DWARF:** This debugger relies on DWARF debugging data embedded inside custom sections of the WebAssembly binary. Stripping your WASM for mainnet deployment using tools like `wasm-opt` will remove these sections. Always debug against unstripped development builds.
* **Host Environment:** The debugger runs a mock Soroban environment. State does not persist between `soroban-debugger` CLI sessions unless you export the ledger state to a JSON file. 

---
*Return to the [Docs Index](../../README.md) for more tutorials.*
