## Description

<!-- A clear and concise summary of the change and its motivation. -->

## Related Issue

<!-- Link the issue this PR addresses, e.g. Closes #123 -->

## Type of Change

- [ ] Bug fix (non-breaking change that fixes an issue)
- [ ] New feature (non-breaking change that adds functionality)
- [ ] Breaking change (fix or feature that would cause existing functionality to change)
- [ ] Documentation update
- [ ] Refactor / code cleanup
- [ ] Performance improvement
- [ ] Test / CI improvement

---

## CI/Test Behavior Changes

<!-- REQUIRED — fill in or mark N/A. -->

**What changed in CI or test behavior?**

<!-- Describe any changes to CI jobs, test configuration, test infrastructure, new or removed tests,
     changed test commands, updated thresholds, or any other shift in how tests run or what they check.
     If nothing changed, write: N/A — no CI/test behavior changes -->

N/A — no CI/test behavior changes

**Migration notes**

<!-- If this change requires contributors or CI environments to take action (e.g. install a new tool,
     update a local cache, re-run a setup script, change a local command), document those steps here.
     If no migration is needed, write: N/A -->

N/A

---

## Checklist

- [ ] All tests pass locally (`cargo test --workspace --all-features`)
- [ ] Code is formatted (`cargo fmt --all -- --check`)
- [ ] Clippy is clean (`cargo clippy --workspace --all-targets --all-features -- -D warnings`)
- [ ] Commit message follows [Conventional Commits](https://www.conventionalcommits.org/)
- [ ] PR description mentions the related issue(s)
- [ ] CI/test behavior changes documented above (or marked N/A)
- [ ] If CLI flags/subcommands/help text changed, man pages regenerated (`make regen-man`) and `.1` files committed
