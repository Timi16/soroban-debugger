# Storage Snapshot Import/Export

This feature allows you to save and restore contract storage state between debug sessions.

## Usage

### Export Storage

Save the storage state after contract execution:

```bash
soroban-debug run \
  --contract contract.wasm \
  --function transfer \
  --args '["alice", "bob", 100]' \
  --export-storage storage.json
```

This creates a JSON file with the current storage state.

### Import Storage

Load a previously saved storage state before execution:

```bash
soroban-debug run \
  --contract contract.wasm \
  --function get_balance \
  --args '["alice"]' \
  --import-storage storage.json
```

### Combined Import and Export

You can import initial state and export the final state in one command:

```bash
soroban-debug run \
  --contract contract.wasm \
  --function transfer \
  --args '["alice", "bob", 100]' \
  --import-storage initial_state.json \
  --export-storage final_state.json
```

## JSON Format

The storage state file uses a simple, human-readable JSON format:

```json
{
  "entries": {
    "balance:alice": "1000",
    "balance:bob": "500",
    "total_supply": "1500",
    "admin": "alice"
  }
}
```

You can manually edit this file to set up specific test scenarios.

## Use Cases

1. **Reproducing Bugs**: Export storage when a bug occurs, then import it to reproduce the exact state
2. **Testing Edge Cases**: Manually create storage states that are difficult to reach through normal execution
3. **Regression Testing**: Save storage snapshots as test fixtures
4. **State Transitions**: Track how storage changes across multiple contract calls

## Example

See `examples/storage_state.json` for a sample storage state file.
