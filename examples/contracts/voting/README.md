# Voting Contract Example

This example demonstrates a complex storage pattern in a Soroban smart contract, designed to showcase the capabilities of the `soroban-debugger`.

For information on how to contribute to this project, please see the [CONTRIBUTING.md](../../../CONTRIBUTION.md).

The contract implements a simple voting system where proposals can be created, voted on, and tallied. It uses persistent storage to track proposals and a map of votes for each proposal.

## Functions

- `create_proposal(creator: Address, id: u32, title: String)`: Creates a new proposal.
- `vote(voter: Address, proposal_id: u32, support: bool)`: Casts a vote (yay or nay).
- `tally(proposal_id: u32) -> (u32, u32)`: Returns the current vote count.
- `close(creator: Address, proposal_id: u32)`: Closes the proposal for further voting.

## How to Debug

This section provides a step-by-step guide on using `soroban-debug` to inspect the contract's execution and state.

### 1. Compile the Contract

First, compile the contract to WASM:

```bash
cargo build --target wasm32-unknown-unknown --release
```

The WASM file will be located at `target/wasm32-unknown-unknown/release/voting_contract.wasm`.

### 2. Run a Function with Debugging

Execute the `create_proposal` function and observe the initial state setup:

```bash
soroban-debug run \
  --contract target/wasm32-unknown-unknown/release/voting_contract.wasm \
  --function create_proposal \
  --args '["GDG7...123", 1, "Upgrade Protocol"]' \
  --verbose
```

### 3. Inspect Storage Diffs

After voting, use the debugger to see how the `Votes` map changes in storage. This is particularly useful for debugging complex data structures.

```bash
soroban-debug run \
  --contract target/wasm32-unknown-unknown/release/voting_contract.wasm \
  --function vote \
  --args '["GBK2...456", 1, true]' \
  --storage '{"Proposal(1)": {...}, "Votes(1)": {}}'
```

### 4. Interactive Tallying

Use the interactive mode to step through the `tally` function and watch the vote count increment:

```bash
soroban-debug interactive --contract target/wasm32-unknown-unknown/release/voting_contract.wasm
```

Once in the interactive TUI:
1. Use `break tally` to set a breakpoint at the start of the tally function.
2. Use `step` to execute instructions line by line.
3. Use `inspect` to view the values of `yays` and `nays` as they update.

## Storage Patterns to Observe

- **Persistent Storage**: Proposals and vote maps are stored persistently.
- **Nested Maps**: The `Votes` key points to a `Map<Address, bool>`, demonstrating how the debugger handles nested data structures.
- **Access Control**: The `close` function demonstrates how `require_auth()` works and how to debug authentication failures.
