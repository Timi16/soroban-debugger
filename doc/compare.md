# Comparing Execution Traces

The `compare` subcommand lets you diff two execution trace JSON files
side-by-side. It is designed for **regression testing** — run your
contract, save the trace, make changes, run again, then compare the two
traces to spot any unintended differences.

## Quick-start

```bash
soroban-debug compare examples/trace_a.json examples/trace_b.json
```

Save the report to a file instead of stdout:

```bash
soroban-debug compare examples/trace_a.json examples/trace_b.json --output report.txt
```

## What is compared?

| Dimension         | Details                                               |
|-------------------|-------------------------------------------------------|
| **Storage**       | Keys added, removed, and modified with old/new values |
| **Budget**        | CPU instructions and memory deltas (absolute + %)     |
| **Return values** | Equality check with full value display                |
| **Execution flow**| LCS-based unified diff of the call sequence           |
| **Events**        | Side-by-side comparison of emitted events             |

## Trace JSON format

A trace file is a JSON object with the following fields (all optional
except where noted):

```json
{
  "label": "Human-readable name for the trace",
  "contract": "token.wasm",
  "function": "transfer",
  "args": "[\"Alice\", \"Bob\", 100]",
  "storage": {
    "balance:Alice": 900,
    "balance:Bob": 100,
    "total_supply": 1000
  },
  "budget": {
    "cpu_instructions": 45000,
    "memory_bytes": 15360,
    "cpu_limit": 100000,
    "memory_limit": 40960
  },
  "return_value": { "status": "ok" },
  "call_sequence": [
    { "function": "transfer", "depth": 0 },
    { "function": "get_balance", "args": "Alice", "depth": 1 },
    { "function": "set_balance", "args": "Alice, 900", "depth": 1 }
  ],
  "events": [
    {
      "contract_id": "CA7QYN...",
      "topics": ["transfer"],
      "data": "Alice→Bob 100"
    }
  ]
}
```

### Field reference

| Field            | Type            | Description                                     |
|------------------|-----------------|-------------------------------------------------|
| `label`          | `string?`       | Friendly name shown in the report header        |
| `contract`       | `string?`       | Contract WASM path or ID                        |
| `function`       | `string?`       | Invoked function name                           |
| `args`           | `string?`       | Function arguments (JSON-encoded)               |
| `storage`        | `object`        | Post-execution storage key→value map            |
| `budget`         | `object?`       | CPU and memory usage                            |
| `return_value`   | `any?`          | Return value (arbitrary JSON)                   |
| `call_sequence`  | `array`         | Ordered list of function calls                  |
| `events`         | `array`         | Events emitted during execution                 |

## Regression testing workflow

1. **Capture baseline trace** — run your contract and save the execution
   output as `baseline.json`.

2. **Make contract changes** — e.g., optimize gas usage, add fee logic, etc.

3. **Capture new trace** — run the modified contract and save as `new.json`.

4. **Compare** —
   ```bash
   soroban-debug compare baseline.json new.json
   ```

5. **Review the report** — look for:
   - Unexpected storage modifications (regressions)
   - Budget increases (performance regressions)
   - Changed return values (behavioural regressions)
   - New or missing function calls in the execution flow

### Example: Detecting a fee regression

Suppose `v1.0` of your token contract transfers the full amount, and
`v1.1` introduces a fee. The compare output will clearly show:

```
───────────────── Storage Changes ─────────────────

  Keys only in B (1):
    + fee_pool = 5

  Modified keys (1):
    ~ balance:Alice
        A: 900
        B: 895

───────────────── Budget Usage ────────────────────

                                            A               B          Delta
              CPU instructions          45000           38000          -7000

  CPU change: -15.56%
  Memory change: -8.85%

───────────────── Return Values ───────────────────

  A: {"status":"ok"}
  B: {"fee_charged":5,"status":"ok"}

───────────────── Execution Flow ──────────────────

  Unified diff (- = only in A, + = only in B):

    transfer()
  + check_allowance(Alice)
    get_balance(Alice)
  + compute_fee(100)
  ...
```

## Tips

- Keep trace files in version control alongside your contract code
  so you can compare across Git commits.
- Use `--output` to save the report, then `diff` two reports over time.
- Combine with CI: generate traces in your test suite and run
  `soroban-debug compare` as a CI step to catch regressions automatically.
