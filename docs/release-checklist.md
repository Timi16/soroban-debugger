# Release Checklist

This checklist is the single release gate for the Soroban Debugger repo (CLI + analyzers + VS Code extension + benchmarks).

Use this for:
- Tag releases (`vX.Y.Z`) and crates.io publishes
- Hotfix releases

## Roles / Owners

- **Release Manager:** owns the go/no-go decision and waiver sign-off
- **Rust/CLI Owner:** owns core build/lint/test and packaging
- **VS Code Extension Owner:** owns extension build/test + DAP/protocol compatibility
- **Security/Analyzer Owner:** owns `analyze` sanity and any security-facing changes
- **Performance Owner:** owns benchmark sanity gates

## PR Quality Gates

- [ ] All merged PRs in this release window documented CI/test behavior changes or explicitly marked N/A

## Required Gates (no waivers by default)

### Rust (workspace)

- Format check: `cargo fmt --all -- --check`
  - Pass criteria: exit code 0
- Clippy: `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  - Pass criteria: exit code 0 (no warnings)
- Tests: `cargo test --workspace --all-features`
  - Pass criteria: exit code 0
- Man pages: `make check-man` (or `TMPDIR=/tmp make check-man` in restricted environments)
  - Pass criteria: exit code 0 (no drift between committed and generated man pages)
  - Notes:
    - Man pages are regenerated via `cargo build` during the check
    - If drift is detected, run `make regen-man` and commit the updated `.1` files
    - TMPDIR can be set to override temp directory location (useful in CI or sandbox environments)

### Security analyzer sanity

- Static analysis: `cargo run --quiet --bin soroban-debug -- analyze --contract tests/fixtures/wasm/echo.wasm --format json`
  - Pass criteria: exit code 0
- Optional dynamic analysis (when touching runtime/debug server):  
  `cargo run --quiet --bin soroban-debug -- analyze --contract tests/fixtures/wasm/echo.wasm --function echo --args '[7]' --timeout 30 --format json`
  - Pass criteria: exit code 0

### VS Code extension

From `extensions/vscode`:

- Install deps: `npm ci`
  - Pass criteria: exit code 0
- Compile: `npm run -s compile`
  - Pass criteria: exit code 0
- Tests: `npm test`
  - Pass criteria: exit code 0
  - Notes:
    - For best coverage, set `SOROBAN_DEBUG_BIN` to a locally-built debug binary path (e.g. `target/debug/soroban-debug`) so the smoke test exercises the real debugger server.

### Benchmarks (sanity thresholds)

Benchmarks must not regress beyond the configured thresholds:

- Thresholds (CI defaults):
  - Warn: 10%
  - Fail: 20%
- Command (CI-style):
  - `cargo bench --benches -- --noplot --sample-size 20 --measurement-time 5 --warm-up-time 2`
  - `cargo run --quiet --bin bench-regression -- record --criterion target/criterion --out .bench/current.json`
  - `cargo run --quiet --bin bench-regression -- compare --baseline .bench/baseline.json --current .bench/current.json --warn-pct 10 --fail-pct 20`
    - Pass criteria: compare exits 0 and reports no FAIL-level regressions

## Release Metadata Gates

- Version consistency:
  - Tag is `vX.Y.Z`
  - `Cargo.toml` version equals `X.Y.Z`
  - `extensions/vscode/package.json` version equals `X.Y.Z` (if publishing the extension as part of the release)
- Changelog:
  - `CHANGELOG.md` updated for `X.Y.Z`

## Waiver process (when absolutely necessary)

If any required gate is waived, the release must include a waiver record and explicit sign-off:

1. Create an issue or PR comment titled `Release waiver: vX.Y.Z`.
2. Include:
   - Which gate was waived
   - Why it failed / why it is safe to proceed
   - Scope/impact
   - Mitigation and follow-up owner + deadline
3. Release Manager signs off by linking the waiver record in the release notes under a `Waivers` section.

## Sign-off (fill before tagging)

- [ ] Release Manager: @____ (link to waiver record(s) if any)
- [ ] Rust/CLI Owner: @____
- [ ] VS Code Extension Owner: @____
- [ ] Security/Analyzer Owner: @____
- [ ] Performance Owner: @____

