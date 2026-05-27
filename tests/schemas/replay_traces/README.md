# Replay Trace Fixtures

Fixtures consumed by the replay-validation tests. See
`src/compare/trace.rs::validate_for_replay` and
`tests/replay_validation_tests.rs`.

| File | Purpose |
|---|---|
| `valid_current.json` | Well-formed trace stamped with the current schema version. Must validate. |
| `valid_legacy_no_version.json` | Pre-versioning trace (no `schema_version` field). Must validate. |
| `malformed_missing_function.json` | Required `function` field absent. Must fail validation. |
| `unsupported_future_schema.json` | `schema_version: "2.0.0"` (one major ahead of what this build supports). Must fail with a versioning error. |
