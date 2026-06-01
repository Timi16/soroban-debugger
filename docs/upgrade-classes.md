# Upgrade Compatibility Classes

When you run `soroban-debug upgrade-check`, the debugger compares the old and new WASM binaries and classifies the result based on exported function compatibility and, when provided, test-input execution diffs.

The current analyzer uses a simple decision rule:

- Any removed or signature-changed exported function is `Breaking`.
- Any execution mismatch from `--test-inputs` is `Breaking`.
- If there are no breaking changes but there is at least one added exported function, the result is `Caution`.
- If the exported surface and sampled execution are unchanged, the result is `Safe`.

## Safe
Use `Safe` when the contract change is effectively identical from the debugger's point of view:

- No exported functions were added.
- No exported functions were removed.
- No exported function signatures changed.
- No sampled executions differed when `--test-inputs` was used.

This means downstream callers can keep using the same contract interface without code changes.

## Caution
Use `Caution` when the upgrade is additive but still worth a review:

- One or more exported functions were added.
- Existing exported function names, parameter counts, parameter types, and return types stayed the same.
- No sampled executions differed when `--test-inputs` was used.

This class means the contract grew a new surface area, so callers and indexers may want to notice the new entry points even though the upgrade is still compatible.

## Breaking
Use `Breaking` when the upgrade changes how existing callers interact with the contract:

- An exported function was removed.
- An exported function's parameter count changed.
- An exported function's parameter types changed.
- An exported function's return types changed.
- A sampled execution produced a different result with `--test-inputs`.

This class means the old and new contract versions are not safely interchangeable without updating callers or reviewing the changed behavior.
