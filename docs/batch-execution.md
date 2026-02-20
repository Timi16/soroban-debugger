# Batch Execution

The batch execution feature allows you to run the same contract function with multiple different argument sets in parallel, which is useful for batch regression testing and performance benchmarking.

## Usage

```bash
soroban-debug run \
  --contract path/to/contract.wasm \
  --function function_name \
  --batch-args path/to/batch_args.json
```

## Batch Args File Format

The batch args file should be a JSON array containing test cases. Each test case can have:

- `args` (required): Function arguments as a JSON string
- `expected` (optional): Expected result for assertion
- `label` (optional): Human-readable label for the test case

### Example

```json
[
  {
    "args": "[1, 2]",
    "expected": "3",
    "label": "Add 1 + 2"
  },
  {
    "args": "[10, 20]",
    "expected": "30",
    "label": "Add 10 + 20"
  },
  {
    "args": "[100, 200]",
    "label": "Add 100 + 200 (no assertion)"
  }
]
```

## Features

### Parallel Execution

All test cases are executed in parallel using Rayon, which significantly speeds up batch testing for contracts with multiple test scenarios.

### Result Assertions

When you provide an `expected` value, the tool will compare the actual result with the expected value and mark the test as passed or failed accordingly.

### Pass/Fail Summary

After execution, you'll see:

- Individual results for each test case
- Pass/fail status with visual indicators (✓/✗)
- Execution duration for each test
- Overall summary with counts and total duration

### JSON Output

Use `--json` or `--format json` to get machine-readable output:

```bash
soroban-debug run \
  --contract contract.wasm \
  --function add \
  --batch-args batch.json \
  --json
```

## Example Output

```
================================================================================
  Batch Execution Results
================================================================================

✓ PASS Add 1 + 2
  Args: [1, 2]
  Result: 3
  Expected: 3
  Duration: 12ms

✓ PASS Add 10 + 20
  Args: [10, 20]
  Result: 30
  Expected: 30
  Duration: 10ms

✗ FAIL Add 100 + 200
  Args: [100, 200]
  Result: 299
  Expected: 300
  Result does not match expected value
  Duration: 11ms

================================================================================
  Summary
================================================================================
  Total:    3
  Passed:   2
  Failed:   1
  Errors:   0
  Duration: 33ms
================================================================================
```

## Integration with Other Features

Batch execution works with:

- `--network-snapshot`: Load network state before batch execution
- `--json`: Output results in JSON format
- `--format json`: Alternative way to request JSON output

## Exit Codes

- `0`: All tests passed
- Non-zero: One or more tests failed or encountered errors

## Performance

The parallel execution model provides significant performance improvements:

- 10 test cases: ~10x faster than sequential
- 100 test cases: ~50x faster than sequential (depending on CPU cores)

## Use Cases

1. **Regression Testing**: Run a suite of test cases to ensure contract behavior hasn't changed
2. **Edge Case Testing**: Test boundary conditions and edge cases in parallel
3. **Performance Benchmarking**: Measure execution time across different input scenarios
4. **Upgrade Validation**: Verify that a new contract version produces the same results as the old version
