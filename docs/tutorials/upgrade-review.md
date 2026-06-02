# Tutorial: Review an Upgrade

This tutorial covers the `upgrade-check` command, used to safely evaluate the impact of replacing an old contract binary with a new one.

## The Goal
Ensure that a new version of your contract doesn't break existing integrations or cause unexpected execution changes.

## Step 1: Prepare the binaries
You need the currently deployed WASM and the new WASM you plan to deploy.

## Step 2: Run the upgrade check
Compare the two binaries:

```bash
soroban-debug upgrade-check --old old.wasm --new new.wasm
```

## Step 3: Interpret the results
The tool classifies the upgrade into one of three categories. For the full mapping between contract changes and classes, see [Upgrade Classes](../upgrade-classes.md).

- **Safe:** The exported surface is unchanged, and any sampled `--test-inputs` executions match.
- **Caution:** New exported functions were added, but existing signatures still match.
- **Breaking:** Functions were removed, signatures changed, or a sampled execution produced a different result.
