# Roadmap Priorities — Epic J

> **Companion to:** [backlog-100-issues.md](backlog-100-issues.md)
>
> **Purpose:** Assign priority, effort, owner area, and dependency notes to every
> backlog issue so maintainers can move from backlog review to delivery planning
> without reclassifying the full set from scratch.
>
> **Done when:** Every issue has a row in this table, and the wave ordering gives
> any contributor a clear first item to pick up.

---

## Reading the Table

| Column | Values | Meaning |
|--------|--------|---------|
| **P** | P0 / P1 / P2 / P3 | P0 = blocker (ship-gate or broken UX). P1 = high value, do this release cycle. P2 = next cycle. P3 = nice-to-have / deferred. |
| **Effort** | XS / S / M / L / XL | XS ≤ 1 hr · S ≤ half-day · M ≤ 2 days · L ≤ 1 week · XL > 1 week |
| **Owner area** | Docs / DX / Contrib / Release / Meta | The team or role best placed to drive this issue. |
| **Depends on** | Issue IDs or "—" | Issues that should land first, or which provide the foundation for this one. |
| **Wave** | 1 / 2 / 3 / 4 | Delivery wave (see [Wave Plan](#wave-plan) below). |

---

## Section Rollup

Use this table before you dive into the 100-row priority matrix. It gives each
backlog section a recommended entry point so maintainers can open issues in a
useful order instead of re-triaging the whole epic every time.

| Section | Backlog slice | Planning focus | Start with | Why this is the entry point |
|---------|---------------|----------------|------------|-----------------------------|
| **A** | [README and Landing Docs](backlog-100-issues.md#section-a--readme-and-landing-docs) | New-user discoverability and release-facing repo hygiene | `I-009`, then `I-007` | `I-009` is a P0 FAQ gap; `I-007` unlocks several later navigation fixes. |
| **B** | [Architecture and Design Docs](backlog-100-issues.md#section-b--architecture-and-design-docs) | Fill missing system-shape docs before dependent tutorials land | `I-015` | The VS Code / DAP architecture doc unblocks `I-024` and `I-048`. |
| **C** | [Feature Reference Docs](backlog-100-issues.md#section-c--feature-reference-docs) | Close reference gaps that affect discoverability and advanced workflows | `I-023`, then `I-030` | `I-023` is a fast cross-link win; `I-030` unlocks plugin tutorial work. |
| **D** | [Tutorials](backlog-100-issues.md#section-d--tutorials) | Repair broken learning paths, then add missing workflows | `I-043`, then `I-046` | `I-043` is the only P0 in this section; `I-046` improves tutorial discoverability immediately. |
| **E** | [Contributor Workflow](backlog-100-issues.md#section-e--contributor-workflow) | Remove contributor blockers, then formalize process | `I-058`, then `I-053` | `I-058` fixes a missing referenced file; `I-053` is an easy on-ramp for contribution flow. |
| **F** | [Release Operations](backlog-100-issues.md#section-f--release-operations) | Establish release-critical documentation before automation depth | `I-071` | It is P0 and also unblocks follow-on `git-cliff` docs in contributor guidance. |
| **G** | [Repo Health and Meta](backlog-100-issues.md#section-g--repo-health-and-meta) | Clean repo-level trust and navigation issues | `I-085`, then `I-083` | `I-085` is security-critical; `I-083` prevents future root-level doc sprawl. |
| **H** | [DX and Tooling Quality](backlog-100-issues.md#section-h--dx-and-tooling-quality) | Improve entry points first, then automate drift detection | `I-094` | It is a P0 fix on the primary onboarding path and is independent of later tooling work. |

---

## Priority Table

### Section A — README and Landing Docs

| ID | Title (short) | P | Effort | Owner | Depends on | Wave |
|----|--------------|---|--------|-------|------------|------|
| I-001 | Add README table of contents | P1 | XS | Docs | — | 1 |
| I-002 | Add cross-link from README troubleshooting table to full guide | P1 | XS | Docs | — | 1 |
| I-003 | Deduplicate README storage-filter pattern table | P1 | XS | Docs | — | 1 |
| I-004 | Document `.soroban-debug.toml` config schema | P1 | M | Docs | — | 2 |
| I-005 | Remove or populate `DRAFT.md` | P2 | XS | Meta | — | 1 |
| I-006 | Clarify purpose of `docs/landing.html` or link it from README | P2 | S | Docs | I-007 | 2 |
| I-007 | Create `docs/index.md` navigation index | P1 | M | Docs | — | 1 |
| I-008 | Fix FAQ numbering gap (missing item 12) | P1 | XS | Docs | — | 1 |
| I-009 | Complete FAQ item 6 (contract panics answer) | P0 | S | Docs | — | 1 |
| I-010 | Add `--locked` flag to README quick-start install | P2 | XS | Docs | — | 2 |
| I-011 | Create `CHANGELOG.md` (required by release checklist) | P0 | M | Release | I-071 | 1 |
| I-012 | Harden README badge URLs against branch rename | P2 | XS | Meta | — | 3 |

### Section B — Architecture and Design Docs

| ID | Title (short) | P | Effort | Owner | Depends on | Wave |
|----|--------------|---|--------|-------|------------|------|
| I-013 | Update `ARCHITECTURE.md` — clarify `Stepper` status | P2 | XS | Docs | — | 2 |
| I-014 | Expand `ARCHITECTURE.md` Extension Points with plugin/remote/batch | P2 | S | Docs | I-015, I-017 | 2 |
| I-015 | ~Add VS Code extension / DAP adapter architecture section~ | P2 | M | Docs | — | 2 |
| I-016 | Document rationale for direct `soroban-env-host` integration | P3 | S | Docs | — | 3 |
| I-017 | Add plugin ABI stability summary to `ARCHITECTURE.md` | P2 | S | Docs | — | 2 |
| I-018 | Promote batch execution to a proper design doc in `docs/` | P2 | S | Docs | — | 2 |
| I-019 | Expand `docs/dependency-graph.md` with example DOT/Mermaid output | P2 | M | Docs | — | 2 |
| I-020 | Document trace file schema versioning and migration policy | P1 | M | Docs | — | 2 |
| I-021 | Expand `docs/replay-artifacts.md` with capture-to-replay workflow | P2 | M | Docs | — | 2 |
| I-022 | Expand `docs/resource-timeline.md` from stub to full reference | P2 | M | Docs | — | 2 |

### Section C — Feature Reference Docs

| ID | Title (short) | P | Effort | Owner | Depends on | Wave |
|----|--------------|---|--------|-------|------------|------|
| I-023 | Add feature-matrix back-link to `docs/instruction-stepping.md` | P1 | XS | Docs | — | 1 |
| I-024 | Add complete `launch.json` snippet for VS Code attach flow | P1 | S | Docs | I-015 | 2 |
| I-025 | Fix broken anchor in FAQ item 27 → `remote-troubleshooting.md` | P1 | XS | Docs | — | 1 |
| I-026 | Cross-link plugin-api.md trust section → plugin-failure-handling.md | P2 | XS | Docs | — | 2 |
| I-027 | Document in-flight event behavior when plugin circuit-breaker trips | P1 | S | Docs | — | 2 |
| I-028 | Link `plugin-command-namespaces.md` from `plugin-api.md` | P1 | XS | Docs | — | 1 |
| I-029 | Add suppression examples (rule-level vs. finding-level) | P2 | S | Docs | — | 2 |
| I-030 | Expand `docs/security-rules.md` with severity and remediation guide | P2 | M | Docs | — | 2 |
| I-031 | Add `inspect --format json` example output to `wasm-artifact-metadata.md` | P2 | S | Docs | — | 2 |
| I-032 | Expand `docs/watch-mode.md` with polling interval and debounce details | P2 | S | Docs | — | 2 |
| I-033 | Clarify CLI vs. extension behavior in `docs/storage-snapshot.md` | P1 | S | Docs | — | 2 |
| ~I-034~ | ~Map contract change types to upgrade classes in `upgrade-classes.md`~ | P1 | M | Docs | — | 2 |
| I-035 | Document full batch result JSON fields in `docs/batch-execution.md` | P2 | S | Docs | — | 2 |
| I-036 | Add internal TOC to `docs/optimization-guide.md` | P2 | XS | Docs | — | 2 |
| I-037 | Add DWARF-absent heuristic fallback to `source-level-debugging.md` | P1 | S | Docs | — | 2 |
| I-038 | Add `--mock` flag coverage to `debug-cross-contract.md` | P1 | S | Docs | — | 2 |
| I-039 | Add end-to-end worked example to `scenario-cookbook.md` | P2 | M | Docs | — | 2 |
| I-040 | Document `selftest-coverage-missing-field` in `performance-regressions.md` | P2 | S | Docs | — | 3 |

### Section D — Tutorials

| ID | Title (short) | P | Effort | Owner | Depends on | Wave |
|----|--------------|---|--------|-------|------------|------|
| I-041 | Reference `.soroban-debug.toml` in `first-debug.md` | P1 | XS | Docs | I-004 | 2 |
| I-042 | Document all TOML keys in `scenario-runner.md` | P1 | M | Docs | — | 2 |
| I-043 | Complete empty checklist items in `debug-auth-errors.md` | P0 | S | Docs | — | 1 |
| I-044 | ~Add report interpretation to `symbolic-analysis-budgets.md`~ | P2 | M | Docs | — | 2 |
| I-045 | ~Add `--budget-trend` flag coverage to `understanding-budget.md`~ | P1 | S | Docs | — | 2 |
| I-046 | Relocate `docs/doc/tutorials/video-token-transfer.md` to `docs/tutorials/` | P1 | XS | Docs | I-007 | 1 |
| I-047 | Write end-to-end plugin development tutorial | P2 | L | Docs | I-030, I-026 | 3 |
| I-048 | Write VS Code extension setup tutorial | P1 | M | Docs | I-015, I-024 | 2 |
| I-049 | Write TUI (`soroban-debug tui`) tutorial | P2 | M | Docs | — | 3 |
| I-050 | Write upgrade-check workflow tutorial | P2 | M | Docs | I-034 | 3 |
| I-051 | Write REPL tutorial | P2 | M | Docs | — | 3 |
| I-052 | ~Write remote debugging in CI tutorial~ | P2 | L | Docs | I-024, I-048 | 3 |

### Section E — Contributor Workflow

| ID | Title (short) | P | Effort | Owner | Depends on | Wave |
|----|--------------|---|--------|-------|------------|------|
| I-053 | Add filtered GitHub issue URL to `CONTRIBUTING.md` | P1 | XS | Contrib | — | 1 |
| I-054 | Link "Areas for Contribution" to open GitHub issues / project board | P1 | S | Contrib | I-068 | 2 |
| I-055 | Clarify "CI/test behavior changes" definition and N/A examples | P1 | S | Contrib | — | 1 |
| I-056 | Document PR review SLA and escalation path | P2 | S | Contrib | — | 2 |
| I-057 | Extend branch-naming convention to `fix/`, `release/`, `docs/` | P1 | XS | Contrib | — | 1 |
| I-058 | Create `CODE_OF_CONDUCT.md` (currently referenced but missing) | P0 | S | Meta | — | 1 |
| I-059 | Document how to add a fuzz target and contribute findings | P2 | M | Contrib | — | 3 |
| I-060 | Create GitHub issue templates (bug + feature request) | P1 | M | Contrib | — | 1 |
| I-061 | Create GitHub PR template that embeds the contributor checklist | P1 | M | Contrib | I-060 | 1 |
| I-062 | Document `cliff.toml` location and `git-cliff` invocation in `CONTRIBUTING.md` | P1 | S | Contrib | I-071 | 2 |
| I-063 | Document stale-PR escalation process | P2 | S | Contrib | — | 3 |
| I-064 | Create `CODEOWNERS` file | P1 | S | Contrib | — | 2 |
| I-065 | Document canonical `SKIP` variable list for pre-commit hooks | P2 | S | Contrib | — | 2 |
| I-066 | Add VS Code extension local test instructions to `CONTRIBUTING.md` | P1 | S | Contrib | I-048 | 2 |
| I-067 | Expand `CONTRIBUTING.md` project structure to include all top-level dirs | P1 | S | Contrib | — | 1 |
| I-068 | Document issue label taxonomy | P1 | S | Contrib | — | 2 |
| I-069 | Write docs-only contribution guide (preview, link-check) | P2 | M | Contrib | — | 2 |
| I-070 | Document `make regen-man` on Windows | P2 | M | DX | — | 3 |

### Section F — Release Operations

| ID | Title (short) | P | Effort | Owner | Depends on | Wave |
|----|--------------|---|--------|-------|------------|------|
| I-071 | Add `CHANGELOG.md` format example and `git-cliff` command to release checklist | P0 | S | Release | — | 1 |
| I-072 | Define owner-assignment process for release checklist sign-off | P1 | S | Release | — | 2 |
| I-073 | Sync benchmark thresholds between release checklist and CI scripts | P1 | S | Release | — | 2 |
| I-074 | Specify issue label for release waivers (`release-waiver`) | P1 | XS | Release | I-068 | 2 |
| I-075 | Document crates.io publish order and failure recovery steps | P1 | M | Release | — | 2 |
| I-076 | Write post-release checklist (announce, docs-site, close milestone) | P2 | M | Release | — | 2 |
| I-077 | Add VS Code Marketplace publish step to release checklist | P1 | S | Release | — | 2 |
| I-078 | Document rollback procedure for a defective published release | P2 | M | Release | — | 3 |
| I-079 | Document `check_benchmark_regressions.sh` flags and output format | P1 | M | Release | — | 2 |
| I-080 | Create `.github/workflows/release.yml` to gate release checklist at tag-push | P2 | L | Release | I-073 | 3 |
| I-081 | Document the CI job name and workflow file for man-page drift check | P1 | S | Contrib | — | 2 |
| I-082 | Write SemVer policy document for this project | P1 | M | Release | — | 2 |

### Section G — Repo Health and Meta

| ID | Title (short) | P | Effort | Owner | Depends on | Wave |
|----|--------------|---|--------|-------|------------|------|
| I-083 | Define policy for root-level implementation-summary files | P1 | S | Meta | — | 1 |
| I-084 | Remove or archive `PR_DESCRIPTION.md` from repo root | P1 | XS | Meta | — | 1 |
| I-085 | Create `SECURITY.md` with responsible disclosure contact | P0 | S | Meta | — | 1 |
| I-086 | Create `SUPPORT.md` with community support links | P2 | XS | Meta | — | 2 |
| I-087 | Document `.github/` workflows: triggers, gates, owner | P1 | M | Meta | — | 2 |
| I-088 | Verify `Cargo.toml` license field matches dual-license files | P1 | XS | Meta | — | 1 |
| I-089 | Define naming convention and add index for `docs/issues/` | P2 | S | Docs | I-007 | 2 |
| I-090 | Document `scratch/` directory purpose and `.gitignore` status | P2 | XS | Meta | — | 2 |
| I-091 | Add `help` target to `Makefile` | P2 | S | DX | — | 2 |
| I-092 | Create `docs/doc/` index; fix orphaned files under that path | P1 | S | Docs | I-007, I-046 | 1 |
| I-093 | Document difference between `ci.sh` and `run_local_ci.sh` | P1 | XS | DX | — | 1 |

### Section H — DX and Tooling Quality

| ID | Title (short) | P | Effort | Owner | Depends on | Wave |
|----|--------------|---|--------|-------|------------|------|
| I-094 | Link `docs/getting-started.md` from README "Quick Start" | P0 | XS | DX | — | 1 |
| I-095 | Define process for detecting feature-matrix drift from source | P2 | M | DX | — | 3 |
| I-096 | Publish HTML equivalent of man pages (e.g., via GitHub Pages) | P3 | L | DX | I-096 (self) | 4 |
| I-097 | Add per-command doc and man-page links to `cli-command-groups.md` | P2 | M | Docs | — | 2 |
| I-098 | Add feedback / correction link to FAQ | P3 | XS | DX | — | 4 |
| I-099 | Add `examples/README.md` describing each example | P2 | M | Docs | — | 2 |
| I-100 | Document process for syncing docs when `soroban-env-host` is upgraded | P2 | M | Docs | — | 3 |

---

## Wave Plan

Waves are sequential delivery batches. Each wave is designed to be completeable
in approximately one sprint (two weeks) by a small team focused on docs/process
work. Items within a wave can be parallelised freely.

### Wave 1 — Blockers and Fast Wins (target: ≤ 2 weeks)

**Goal:** Eliminate broken-UX items, missing required files, and quick copy-edits
that unblock everything else. These are the issues that actively embarrass the
project or prevent contributors from starting.

| IDs | Theme |
|-----|-------|
| I-009, I-043 | Incomplete / broken content that misleads readers |
| I-011, I-071 | Missing `CHANGELOG.md` (required by release gate) |
| I-058 | Missing `CODE_OF_CONDUCT.md` (referenced but absent) |
| I-085 | Missing `SECURITY.md` |
| I-094 | `getting-started.md` not linked from README Quick Start |
| I-001, I-002, I-003, I-007, I-008 | README + docs navigation quick-fixes |
| I-023, I-025, I-028 | Broken/missing cross-links in feature docs |
| I-053, I-055, I-057, I-067 | CONTRIBUTING.md copy-edits (no new pages needed) |
| I-060, I-061 | GitHub issue + PR templates |
| I-005, I-083, I-084 | Root-level file housekeeping |
| I-046, I-092 | Fix misplaced / orphaned tutorial and doc index |
| I-088, I-093 | Licence field check; ci.sh vs run_local_ci.sh note |

**Entry criteria for Wave 1:** None — these can start immediately.  
**Exit criteria:** All P0 issues closed; all items in this wave have PRs merged.

---

### Wave 2 — Depth and Process (target: 2–4 weeks after Wave 1)

**Goal:** Fill the substantive content gaps in feature docs, tutorials, and
contributor process that Wave 1 made navigable.

| IDs | Theme |
|-----|-------|
| I-004, I-041 | Config file schema (`.soroban-debug.toml`) |
| I-013–I-022 | Architecture doc expansion |
| I-024, I-033, I-037, I-038, I-045 | Feature reference accuracy fixes |
| I-026–I-032, I-034–I-036 | Plugin, security, storage, batch doc depth |
| I-039, I-042, I-044 | Tutorial content depth |
| I-048, I-066 | VS Code extension setup tutorial + contrib guidance |
| I-020 | Trace schema versioning policy |
| I-027 | Plugin circuit-breaker in-flight event docs |
| I-054, I-056, I-062, I-064, I-065, I-068, I-069 | Contributor process formalization |
| I-072–I-082 | Release operations depth (except I-080) |
| I-086, I-087, I-089–I-091 | Repo meta and DX quick-wins |
| I-097, I-099 | CLI groups cross-links; examples index |

**Entry criteria for Wave 2:** Wave 1 complete; `docs/index.md` (I-007) merged.  
**Exit criteria:** All P1 issues closed; no Wave 2 item older than 3 weeks open.

---

### Wave 3 — Polish and Automation (target: 4–6 weeks after Wave 1)

**Goal:** Convert process knowledge into automation and tooling; close P2 items
that need research or cross-team input.

| IDs | Theme |
|-----|-------|
| I-016, I-040 | Architecture rationale docs |
| I-047, I-049, I-050, I-051, I-052 | Remaining tutorials (plugin, TUI, upgrade-check, REPL, CI remote) |
| I-059, I-063, I-070 | Contributor edge cases (fuzz targets, stale PRs, Windows man-regen) |
| I-078, I-080 | Rollback procedure; release.yml automation |
| I-012 | Badge hardening |
| I-095 | Feature-matrix drift detection |
| I-100 | Dependency upgrade doc process |

**Entry criteria for Wave 3:** Wave 2 complete.  
**Exit criteria:** All P2 issues closed or explicitly deferred to P3 with rationale.

---

### Wave 4 — Continuous Improvement (ongoing / backlog)

**Goal:** Deferred P3 items and long-horizon investments. Pick up opportunistically
or when a related engineering change creates a natural opening.

| IDs | Theme |
|-----|-------|
| I-096 | HTML man-page publishing (GitHub Pages) |
| I-098 | FAQ feedback link |
| Any items demoted from earlier waves with explicit owner + ETA |

---

## Dependency Graph (Key Chains)

The following chains contain the highest-risk blocking relationships. Resolve
blockers within each chain before starting dependants.

```
I-007 (docs index)
  └─► I-006, I-046, I-089, I-092

I-011 (CHANGELOG.md)
  └─► I-071 (git-cliff docs)
        └─► I-062 (CONTRIBUTING reference)

I-015 (VSCode arch doc)
  └─► I-024 (launch.json snippet)
        └─► I-048 (VSCode tutorial)
              └─► I-052 (CI remote tutorial)

I-034 (upgrade classes map)
  └─► I-050 (upgrade-check tutorial)

I-068 (label taxonomy)
  └─► I-054 (issue links in CONTRIBUTING)
  └─► I-074 (release-waiver label)
```

---

## Owner Area Definitions

| Owner | Who this maps to |
|-------|-----------------|
| **Docs** | Anyone comfortable writing Markdown; no Rust required |
| **DX** | Tooling-oriented contributor; light scripting / CI YAML |
| **Contrib** | Maintainer or senior contributor with process knowledge |
| **Release** | Release manager or designated release owner |
| **Meta** | Any maintainer with repo admin rights (for GitHub settings) |

Issues labelled **Docs** or **DX** in Wave 1 and Wave 2 are ideal `good-first-issue`
candidates for new contributors — they require no Rust knowledge and have clear
acceptance criteria.

---

## Quick-Start for Maintainers

1. **Open Wave 1** — filter this table by `Wave = 1`, sort by `P`, and open
   GitHub issues for each item using the naming convention `[Epic J][I-NNN] <Title>`.
2. **Label** each issue with the tag from the backlog (`DOC`, `CONTRIB`, `RELEASE`,
   etc.) plus the wave label (`wave-1`, `wave-2`, …).
3. **Assign** Docs/DX items without assignees as `good-first-issue` so the
   community can self-select.
4. **Gate Wave 2** on Wave 1 exit criteria — do not open Wave 2 issues until all
   P0 Wave 1 items are merged.
5. **Review this file** at the end of each wave — demote any issues that turn out
   to be lower value than expected, and promote anything the wave work revealed
   as higher priority.

---

## Maintenance Rules

1. When adding or removing an issue in [backlog-100-issues.md](backlog-100-issues.md), update the matching roadmap row in the same commit.
2. Keep the short title in this file close to the backlog wording so maintainers can grep for an ID and compare the two documents quickly.
3. When a dependency changes, update both the `Depends on` column and the relevant wave summary so the wave plan stays executable rather than historical.
