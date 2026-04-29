# Soroban Debugger Examples Index

This directory contains a variety of examples demonstrating the capabilities of the Soroban Debugger, including sample contracts, plugins, and specialized test cases.

## 📂 Categorized Index

### 🔐 Authentication & Security
*   **[Auth Example](./contracts/auth-example)**: Demonstrates common authentication pitfalls (`*_buggy` functions) and their secure counterparts. Perfect for seeing how unauthorized access attempts appear in debugger traces.
*   **[Multisig Wallet](./contracts/multisig)**: A complex example showing M-of-N authorization patterns and proposal execution.

### 💾 Storage & State Mutation
*   **[DEX](./contracts/dex)**: A simple decentralized exchange showing reserve management and price calculations through storage updates.
*   **[Escrow](./contracts/escrow)**: Demonstrates time-locked state transitions (Pending -> Released/Refunded) and event emission.
*   **[Simple Token](./contracts/simple-token)**: Standard token implementation showing balance tracking and allowance management.

### 🔄 Cross-Contract Interactions
*   **[Multisig proposals](./contracts/multisig)**: Shows how a contract can propose and eventually execute calls to other "target" contracts.
*   **[Voting](./contracts/voting)**: Demonstrates coordination between users and centralized state.

### 🔍 Symbolic Analysis & Auditing
*   **[Unbounded Iteration Test](./test_unbounded_iteration.rs)**: A standalone example showing how the `SecurityAnalyzer` detects gas-exhaustion loops and storage-write pressure with confidence scores.

### 🧩 Debugger Extensibility
*   **[Example Logger Plugin](./plugins/example_logger)**: A full implementation of a debugger plugin. Demonstrates how to hook into execution events, add custom CLI commands, and manage plugin state.

### 🧪 Mocking & External Integration
*   **[Mock Usage](./mock-usage)**: Shows how to use the `soroban-debug-mock` crate to pre-populate contract storage from JSON snapshots for unit testing.

---

## 🛠️ Use Case Mapping

| I want to... | Use this Example |
| :--- | :--- |
| **Debug auth failures** | `contracts/auth-example` |
| **Visualize storage changes** | `contracts/dex` or `contracts/escrow` |
| **Detect gas loops/vulnerabilities** | `test_unbounded_iteration.rs` |
| **Extend the debugger CLI** | `plugins/example_logger` |
| **Pre-populate state for tests** | `mock-usage` |
| **Replay a specific transaction** | Use `trace_a.json` or `trace_b.json` with the `--replay` flag |

---

## 📄 Support Files Reference

*   **`*.json` (Snapshots & Traces)**: Sample ledger states (`snapshot.json`) and execution traces (`trace_a.json`) that can be loaded directly into the debugger.
*   **`batch_args.json`**: Demonstrates how to run the debugger in batch mode for CI or automated auditing.
*   **`analyzer-suppressions.toml`**: Example configuration for filtering security analyzer findings. See the [Analyzer Suppressions documentation](../docs/analyzer-suppressions.md) for usage details.
