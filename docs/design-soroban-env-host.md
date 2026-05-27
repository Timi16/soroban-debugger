# Design Note: Direct Use of `soroban-env-host`

> Backlog reference: `docs/issues/backlog-100-issues.md` — item **I-016**, Section B (Architecture and Design Docs).

## Context

`soroban-debugger` invokes Soroban contracts from a host process. There are
two practical entry points for doing so:

1. **`soroban-sdk` testutils** — the high-level testing harness exposed by the
   contract SDK (`soroban_sdk::testutils::*`, `Env::with_default_*`). It is
   intentionally ergonomic: it hides the underlying host, registers a contract
   under a generated id, and gives back typed return values.
2. **`soroban-env-host` directly** — the low-level host that `soroban-sdk`
   itself wraps. It exposes the budget, storage map, footprint, diagnostic
   events, and the raw `Val`/`HostObject` machinery as first-class API.

The debugger uses option 2.

## Decision

`soroban-debugger` depends on `soroban-env-host` directly and treats
`soroban-sdk` strictly as a build/test dependency for fixture contracts.

```toml
# Cargo.toml
soroban-sdk      = { version = "...", features = ["testutils"] }   # fixtures only
soroban-env-host = { version = "...", features = ["testutils"] }   # runtime
soroban-env-common = "..."
```

The choice was made deliberately, not by accident, and is enforced by the
module layout: `src/runtime/`, `src/inspector/`, and `src/repl/` import only
`soroban-env-host` / `soroban-env-common`. Application code that depends on
`soroban-sdk` lives under `examples/` and the fixture crates under
`tests/`.

## Why direct, not via `soroban-sdk`

A debugger has different goals from a contract author's test. We need:

1. **Step-level execution control.** The runtime must be able to invoke a
   single host function, observe the result, and pause — so that breakpoints,
   stepping, and time-travel inspection (`src/runtime/instrumentation.rs`,
   `src/runtime/instruction.rs`) can run between WASM operations.
   `soroban-sdk::Env` is designed to invoke a contract end-to-end and return a
   typed result. There is no public seam to inject between operations.

2. **Direct access to the budget.** The Inspector module
   (`src/inspector/budget.rs`) reads `host.budget_ref()` to render CPU and
   memory deltas per checkpoint. The SDK exposes only the aggregate after a
   call returns; we need to sample it mid-call.

3. **Direct access to storage and the footprint.**
   `src/inspector/storage.rs` walks the storage map and the
   read/write footprint to compare states. The SDK abstracts ledger storage
   behind typed accessors; the host exposes the raw `Storage` and
   `Footprint`, which is what `inspect`, `compare`, and `replay` actually
   need.

4. **Diagnostic events without a contract context.**
   `src/inspector/events.rs` reads `host.get_events()` directly, including
   the diagnostic level. The SDK's user-facing test API filters and
   transforms these.

5. **Replay determinism.** `src/runtime/loader.rs` and `src/runtime/parser.rs`
   need to recreate the *exact* host state used in a captured trace
   (`src/compare/trace.rs`). That means seeding the storage map, the budget
   limits, and the network passphrase on a fresh `Host`. The SDK constructs a
   `Host` internally with conventions of its own; bypassing it removes a
   layer of behavior we would otherwise have to mirror or work around.

6. **Stable types for IPC.** The remote debug server
   (`src/server/debug_server.rs`) serializes host values across a transport
   (`src/server/protocol.rs`). The host's `Val` / `ScVal` and the
   `soroban-env-common` types are the stable, versioned surface for that —
   the SDK's higher-level wrappers are not.

## Tradeoffs we accept

This choice is not free. The costs are:

- **Tighter version coupling.** When `soroban-env-host` releases a breaking
  change we have to react, even if `soroban-sdk` has not yet caught up. We
  manage this by pinning `soroban-env-host`, `soroban-env-common`, and
  `soroban-sdk` to compatible majors in `Cargo.toml` and gating updates in
  CI.

- **More boilerplate at the call site.** Things the SDK does for free
  (registering a contract, allocating an id, wrapping return values) are
  done explicitly in `src/runtime/executor.rs` and
  `src/runtime/invoker.rs`. We accept this in exchange for the visibility
  that the inspectors need.

- **Lower-level error surfaces.** Host errors come back as
  `HostError`/`Status` rather than the friendly SDK errors. The
  `src/cli/output.rs` layer normalises these into the diagnostic format used
  by the rest of the CLI.

## When to reach for `soroban-sdk` instead

There are still good reasons to depend on the SDK in *some* parts of this
repo:

- **Fixture contracts** (under `examples/` and `tests/fixtures/`) are normal
  Soroban contracts. They use `soroban-sdk` like any other contract,
  precisely because that is what the debugger is debugging.

- **Sample / integration tests** that want to exercise the public,
  user-facing surface (rather than reach into the host) may use
  `soroban-sdk::testutils`.

A future contributor who wants to add SDK-based assertions in test code
should not feel obligated to switch to the lower-level host API. The rule is
only that runtime/inspector/protocol code stays on the host directly.

## Out of scope

- We do not vendor or fork `soroban-env-host`. We track upstream majors.
- We do not provide a "use either" abstraction layer over the SDK and the
  host. The cost of maintaining that layer would be larger than the cost of
  reading the host API directly in the few modules that need it.

## Related

- `ARCHITECTURE.md` — overall module breakdown (refers back to this doc for
  the host-vs-SDK rationale).
- `src/runtime/executor.rs` — primary consumer of the direct host API.
- `src/inspector/` — modules that depend on host internals
  (`budget.rs`, `storage.rs`, `events.rs`).
- `src/compare/trace.rs` — trace schema that requires deterministic host
  state to replay.
