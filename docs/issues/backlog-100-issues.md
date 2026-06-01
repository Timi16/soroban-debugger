# Backlog: 100 Issues — Soroban Debugger (Epic J)

> **Epic J — Documentation, Contribution Workflow, and Release Operations**
>
> This file is intentionally broad and inventory-oriented rather than scheduled.
> It covers all known gaps across docs, contribution workflow, and release
> operations as observed in the current state of the repository. For prioritised
> delivery sequencing, see [roadmap-priorities.md](roadmap-priorities.md).

---

## Legend

| Tag | Meaning |
|-----|---------|
| `[DOC]` | Documentation content gap |
| `[IA]` | Information-architecture / navigation issue |
| `[CONTRIB]` | Contributor-workflow or process gap |
| `[RELEASE]` | Release operations or automation gap |
| `[DX]` | Developer-experience improvement |
| `[META]` | Repo-level housekeeping (badges, templates, etc.) |

---

## Planning Workflow

Use this file as the inventory, then move into the roadmap for execution decisions:

1. Read the relevant section here to understand the raw issue list and context.
2. Switch to [roadmap-priorities.md](roadmap-priorities.md#section-rollup) for the section rollup, then use the [Priority Table](roadmap-priorities.md#priority-table) and [Wave Plan](roadmap-priorities.md#wave-plan) to decide sequencing.
3. When editing this backlog, update the corresponding roadmap row in the same commit so the two files stay in sync.

---

## Section A — README and Landing Docs (Issues 1–12)

Roadmap view: [Section A priorities](roadmap-priorities.md#section-a--readme-and-landing-docs)

- **I-001** `[IA]` README table of contents is missing; readers must scroll through 420 lines to locate sections.
- **I-002** `[DOC]` README "Troubleshooting" table has only 3 rows; the full list lives in `docs/remote-troubleshooting.md` without a cross-link from the table.
- **I-003** `[DOC]` README "Storage Filtering" section duplicates the same pattern table twice (lines 200–209) — consolidate.
- **I-004** `[DOC]` README does not document the `.soroban-debug.toml` config file format beyond a 4-line snippet; no schema reference exists.
- **I-005** `[DOC]` `DRAFT.md` (40 bytes, root-level) contains no content; either populate or delete.
- **I-006** `[DOC]` `docs/landing.html` is a standalone HTML page not referenced from the README or any other doc — its purpose and audience are unclear.
- **I-007** `[IA]` `docs/` has 28 files but no index file; new readers cannot discover what's available without browsing the directory tree.
- **I-008** `[DOC]` FAQ items are numbered 1–27 but item 12 is missing (numbering jumps from 11 to 13) — fix the numbering gap.
- **I-009** `[DOC]` FAQ item 6 "Contract panics with Unknown error" has no answer body — the entry is a header with no content below it.
- **I-010** `[DOC]` README quick-start installs via Cargo but doesn't mention the `--locked` flag, which is recommended for reproducible installs.
- **I-011** `[DOC]` There is no changelog (`CHANGELOG.md`) in the repo, yet `docs/release-checklist.md` line 79 requires one to be updated before tagging.
- **I-012** `[META]` README badges reference a hardcoded branch (`main`) in badge URLs; the codecov badge will silently break if the default branch is renamed.

---

## Section B — Architecture and Design Docs (Issues 13–22)

Roadmap view: [Section B priorities](roadmap-priorities.md#section-b--architecture-and-design-docs)

- **I-013** `[DOC]` `ARCHITECTURE.md` describes `Stepper` as "(Planned)" with no follow-up issue or tracking link; its current implementation status is unknown.
- **I-014** `[DOC]` `ARCHITECTURE.md` "Extension Points" section lists four items but omits the plugin system, remote server, and batch executor — all of which now exist.
- ~**I-015** `[DOC]` No architecture-level doc covers the VS Code extension / DAP adapter; `ARCHITECTURE.md` only covers the Rust CLI.~
- **I-016** `[DOC]` No design document explains the decision to use `soroban-env-host` directly rather than higher-level Soroban SDK abstractions.
- **I-017** `[DOC]` The plugin ABI stability contract (what breaks a plugin across debugger versions) is described only in `docs/plugin-api.md`; a shorter summary should appear in `ARCHITECTURE.md`.
- **I-018** `[DOC]` The batch execution design (rayon parallelism, result aggregation) is documented only in `BATCH_EXECUTION_SUMMARY.md` — an implementation summary rather than a design doc; a canonical design reference in `docs/` is missing.
- **I-019** `[DOC]` `docs/dependency-graph.md` is 612 bytes and only points to the `--dependency-graph` flag; it needs example DOT/Mermaid output and interpretation guidance.
- **I-020** `[DOC]` No document describes the trace file schema evolution policy (versioning, backward compatibility, migration).
- **I-021** `[DOC]` `docs/replay-artifacts.md` (820 bytes) mentions the replay feature but provides no example workflow from capture to replay.
- **I-022** `[DOC]` `docs/resource-timeline.md` (579 bytes) is a stub; its content does not match the depth of other feature docs.

---

## Section C — Feature Reference Docs (Issues 23–40)

Roadmap view: [Section C priorities](roadmap-priorities.md#section-c--feature-reference-docs)

- **I-023** `[DOC]` `docs/instruction-stepping.md` (11 KB) covers the feature thoroughly but has no link back to the feature matrix — readers don't know which surfaces support it.
- **I-024** `[DOC]` `docs/remote-debugging.md` covers TLS setup but doesn't show a complete `launch.json` snippet for the VS Code attach flow.
- **I-025** `[DOC]` `docs/remote-troubleshooting.md` references a "Local and CI Sandbox Failures" section that exists, but the FAQ (question 27) links to it using anchor syntax that doesn't match the actual heading casing.
- **I-026** `[DOC]` `docs/plugin-api.md` trust-policy section documents environment variables but doesn't cross-link to the plugin failure-handling doc.
- **I-027** `[DOC]` `docs/plugin-failure-handling.md` (1956 bytes) covers session-level circuit-breaker behavior introduced in PR #902 but doesn't describe what happens to in-flight events when a plugin trips the breaker.
- **I-028** `[DOC]` `docs/plugin-command-namespaces.md` (1435 bytes) is not linked from `docs/plugin-api.md`, so readers learning about custom commands won't discover the namespace conflict rules.
- **I-029** `[DOC]` `docs/analyzer-suppressions.md` (822 bytes) describes the suppression file format but gives no example of a suppression that covers a whole rule vs. a specific finding.
- **I-030** `[DOC]` `docs/security-rules.md` (854 bytes) lists rule codes but doesn't map each code to its description, severity, or remediation guidance.
- **I-031** `[DOC]` `docs/wasm-artifact-metadata.md` (1716 bytes) documents the metadata fields but doesn't show an example `inspect --format json` output that contains them.
- **I-032** `[DOC]` `docs/watch-mode.md` (744 bytes) mentions the watch command but doesn't document the polling interval, file-glob support, or debounce behavior.
- **I-033** `[DOC]` `docs/storage-snapshot.md` (1761 bytes) references `--import-storage` but the feature matrix shows this is `PARTIAL` in the extension (use `snapshotPath` instead) — the doc doesn't mention the distinction.
- ~**I-034** `[DOC]` `docs/upgrade-classes.md` (1071 bytes) lists upgrade classes (Safe / Caution / Breaking) but doesn't explain what contract changes map to each class.~
- **I-035** `[DOC]` `docs/batch-execution.md` references JSON format but doesn't document the full set of batch result fields (e.g., `duration_ms`, `error`) that appear in JSON output mode.
- **I-036** `[DOC]` `docs/optimization-guide.md` (16 KB) is the longest doc in the set and has no internal TOC.
- **I-037** `[DOC]` `docs/source-level-debugging.md` (3393 bytes) mentions DWARF but doesn't explain what happens when DWARF info is absent (heuristic fallback) — covered in the FAQ but not here.
- **I-038** `[DOC]` `docs/debug-cross-contract.md` (4608 bytes) doesn't cover the `--mock` flag, which is specifically useful for cross-contract call isolation.
- **I-039** `[DOC]` `docs/scenario-cookbook.md` (2938 bytes) provides only TOML snippets; a worked end-to-end scenario showing TOML authoring → execution → trace review is missing.
- **I-040** `[DOC]` `docs/performance-regressions.md` (1680 bytes) references `scripts/check_benchmark_regressions.sh` but doesn't explain the selftest mode (`selftest-coverage-missing-field`) mentioned in `feature-matrix.md`.

---

## Section D — Tutorials (Issues 41–52)

Roadmap view: [Section D priorities](roadmap-priorities.md#section-d--tutorials)

- **I-041** `[DOC]` `docs/tutorials/first-debug.md` doesn't reference the `.soroban-debug.toml` config file, which new users would benefit from knowing about early.
- **I-042** `[DOC]` `docs/tutorials/scenario-runner.md` shows TOML structure but doesn't document all TOML keys (e.g., `timeout`, `expected_events`, `skip`).
- **I-043** `[DOC]` `docs/tutorials/debug-auth-errors.md` has empty checkbox items that suggest the tutorial is incomplete.
- **I-044** ~`[DOC]` `docs/tutorials/symbolic-analysis-budgets.md` doesn't explain how to interpret the exploration report or act on findings.~
- **I-045** ~`[DOC]` `docs/tutorials/understanding-budget.md` covers CPU/memory budget but doesn't mention the `--budget-trend` flag or history-based regression detection.~
- **I-046** `[DOC]` `docs/doc/tutorials/video-token-transfer.md` lives under `docs/doc/tutorials/` rather than `docs/tutorials/` — inconsistent nesting that breaks the docs IA.
- **I-047** `[IA]` No tutorial covers plugin development end-to-end; `docs/plugin-api.md` is a reference, not a tutorial.
- **I-048** `[DOC]` No tutorial covers the VS Code extension setup (installing the extension, writing a `launch.json`, setting breakpoints).
- **I-049** `[DOC]` No tutorial covers using the TUI (`soroban-debug tui`) — the feature is mentioned in the command index but has no guide.
- **I-050** `[DOC]` No tutorial covers the upgrade-check workflow (building two WASM versions, running the check, interpreting Safe/Caution/Breaking output).
- **I-051** `[DOC]` No tutorial covers the REPL (`soroban-debug repl`) — how to enter it, issue commands, and exit.
- **I-052** ~`[DOC]` No tutorial covers remote debugging in a CI environment (the typical DevOps use case beyond the local SSH-tunnel workaround).~

---

## Section E — Contributor Workflow (Issues 53–70)

Roadmap view: [Section E priorities](roadmap-priorities.md#section-e--contributor-workflow)

- **I-053** `[CONTRIB]` `CONTRIBUTING.md` says "Check the issue tracker for open issues and labels like `good first issue`" but doesn't link to the actual filtered GitHub URL.
- **I-054** `[CONTRIB]` `CONTRIBUTING.md` "Areas for Contribution" lists items in free text; it should link to concrete open GitHub issues or project board columns.
- **I-055** `[CONTRIB]` `CONTRIBUTING.md` describes the PR checklist but does not explain what "CI/test behavior changes" means or give examples of what N/A covers.
- **I-056** `[CONTRIB]` `CONTRIBUTING.md` doesn't document the expected turnaround time for PR reviews — first response SLA, review escalation path, etc.
- **I-057** `[CONTRIB]` `CONTRIBUTING.md` doesn't describe the branch-naming convention for bugfixes (`fix/`), releases (`release/`), or docs (`docs/`); only `feat/` is shown.
- **I-058** `[CONTRIB]` `CONTRIBUTING.md` references `CODE_OF_CONDUCT.md` but no such file exists in the repository.
- **I-059** `[CONTRIB]` `CONTRIBUTING.md` documents fuzzing under "Fuzzing" but doesn't describe how to add a new fuzz target or contribute findings back.
- **I-060** `[CONTRIB]` There is no issue template (`.github/ISSUE_TEMPLATE/`) — bug reports and feature requests arrive in freeform text.
- **I-061** `[CONTRIB]` There is no PR template (`.github/pull_request_template.md`) — the checklist in `CONTRIBUTING.md` is not enforced at PR creation time.
- **I-062** `[CONTRIB]` `CONTRIBUTING.md` describes `cliff.toml` for changelog generation but the file's location or invocation command is not documented.
- **I-063** `[CONTRIB]` No documented escalation path exists for a stale PR (e.g., re-request review after N days, ping maintainer label).
- **I-064** `[CONTRIB]` No `CODEOWNERS` file exists to automatically assign reviewers by component area.
- **I-065** `[CONTRIB]` The pre-commit hook config (`.pre-commit-config.yaml`) is documented in `CONTRIBUTING.md` but the "skip for docs-only commits" tip uses an ad-hoc env var; a canonical `SKIP` variable list is missing.
- **I-066** `[CONTRIB]` No guidance on how to run the VS Code extension tests locally (`extensions/vscode/`) — the only mention is in the release checklist.
- **I-067** `[CONTRIB]` `CONTRIBUTING.md` "Project Structure" lists `src/` subdirs but doesn't include `extensions/`, `fuzz/`, `benches/`, `scripts/`, or `man/`.
- **I-068** `[CONTRIB]` No issue label taxonomy is documented; contributors don't know what labels exist or how to self-label.
- **I-069** `[CONTRIB]` No contribution guide exists for documentation-only changes (e.g., how to preview docs locally, link-checking).
- **I-070** `[CONTRIB]` No guidance exists on how to run `make regen-man` on Windows (the `Makefile` uses Unix tools that aren't available by default on Windows).

---

## Section F — Release Operations (Issues 71–82)

Roadmap view: [Section F priorities](roadmap-priorities.md#section-f--release-operations)

- **I-071** `[RELEASE]` `docs/release-checklist.md` requires `CHANGELOG.md` to be updated but provides no example entry format, no link to `cliff.toml`, and no `git-cliff` invocation command.
- **I-072** `[RELEASE]` The release checklist sign-off section uses `@____` placeholder syntax; there is no documented process for assigning owners before a release cycle begins.
- **I-073** `[RELEASE]` The benchmark threshold (10%/20%) is hardcoded in the release checklist but the actual values are also set in CI scripts — the two can drift without detection.
- **I-074** `[RELEASE]` The waiver process requires creating an issue or PR comment but the issue label to use (e.g., `release-waiver`) is not specified.
- **I-075** `[RELEASE]` The release checklist doesn't document the crates.io publish step: which crate(s) to publish, in what order (given workspace dependencies), and how to handle a failed publish mid-sequence.
- **I-076** `[RELEASE]` No post-release checklist exists (announce on channels, update docs site, close milestone, etc.).
- **I-077** `[RELEASE]` `docs/release-checklist.md` lists the VS Code extension check but doesn't note that the extension must also be published to the VS Code Marketplace separately from crates.io.
- **I-078** `[RELEASE]` No documented rollback procedure exists if a published release is found defective.
- **I-079** `[RELEASE]` `scripts/check_benchmark_regressions.sh` is referenced in both `CONTRIBUTING.md` and the release checklist but the script's flags, output format, and failure modes are not documented.
- **I-080** `[RELEASE]` No `.github/workflows/release.yml` or equivalent automation exists to enforce the release checklist gates in CI at tag-push time.
- **I-081** `[RELEASE]` Man page drift is enforced by `make check-man` in CI, but the exact CI job name and the workflow file that runs it are not documented for contributors.
- **I-082** `[RELEASE]` No SemVer policy document exists defining what constitutes a major/minor/patch release for this project (e.g., is a new CLI flag a minor or patch bump?).

---

## Section G — Repo Health and Meta (Issues 83–93)

Roadmap view: [Section G priorities](roadmap-priorities.md#section-g--repo-health-and-meta)

- **I-083** `[META]` Several implementation-summary files (`BATCH_EXECUTION_SUMMARY.md`, `IMPLEMENTATION_SUMMARY.md`, `PLUGIN_RELOAD_DIFF_IMPLEMENTATION.md`, `FLAMEGRAPH_IMPLEMENTATION.md`) live in the root and appear to be one-off delivery notes rather than living documentation; a policy for these files is needed.
- **I-084** `[META]` `PR_DESCRIPTION.md` lives in the repo root — it appears to be a leftover from a PR and should be removed or archived.
- **I-085** `[META]` No `SECURITY.md` file exists; GitHub displays a warning and security researchers have no disclosed contact point.
- **I-086** `[META]` No `SUPPORT.md` file exists; GitHub will not surface community support links.
- **I-087** `[META]` `.github/` directory exists (inferred from CI badge) but its contents are undocumented — no list of workflows, their triggers, or their gates is available to contributors.
- **I-088** `[META]` The repo has dual licenses (Apache 2.0 and MIT) but the `Cargo.toml` license field should be `"MIT OR Apache-2.0"`; this should be verified.
- **I-089** `[META]` `docs/issues/` contains only `2026-03-29-main-regressions.md` — no index or naming convention for issue-notes files is documented.
- **I-090** `[META]` `scratch/` directory exists in the repo root; its purpose and gitignore status are undocumented.
- **I-091** `[DX]` The `Makefile` is 3149 bytes but has no `help` target; contributors must read the file to discover available targets.
- **I-092** `[DX]` No `docs/doc/` index exists; `docs/doc/compare.md` and `docs/doc/tutorials/video-token-transfer.md` are orphaned from the main docs tree.
- **I-093** `[META]` `ci.sh` and `run_local_ci.sh` are both present; the difference between them is undocumented.

---

## Section H — DX and Tooling Quality (Issues 94–100)

Roadmap view: [Section H priorities](roadmap-priorities.md#section-h--dx-and-tooling-quality)

- **I-094** `[DX]` `docs/getting-started.md` (3001 bytes) is the natural entry point for new users but is not linked from the README "Quick Start" section.
- **I-095** `[DX]` The `feature-matrix.md` "Maintaining This Document" section tells editors which source files to check but doesn't describe a process to detect drift (e.g., a CI check that flags undocumented flags).
- **I-096** `[DX]` Man pages in `man/man1/` are generated but there's no published HTML equivalent — the `cargo doc` output and man pages are the only machine-generated references.
- **I-097** `[DX]` `docs/cli-command-groups.md` (2682 bytes) lists command groups but doesn't link to the per-command reference docs or man pages.
- **I-098** `[DX]` `docs/faq.md` has no feedback mechanism (e.g., "Was this helpful?" link, GitHub Discussions link, or issue link for corrections).
- **I-099** `[DX]` The `examples/` directory is referenced in the README but no `examples/README.md` or index exists to describe what each example demonstrates.
- **I-100** `[DX]` No documented process exists for keeping third-party dependency documentation in sync when a crate dependency is upgraded (e.g., `soroban-env-host` version bumps that change host API behavior).
