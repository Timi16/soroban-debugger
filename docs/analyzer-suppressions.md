# Analyzer Suppressions

The Soroban Debugger's security analyzer allows you to suppress specific findings that are deemed false positives or accepted risks for your project.

## Suppression File Format

Suppressions are defined in a TOML file. The default configuration file `.soroban-debug.toml` can point to your suppressions file:

```toml
[output]
suppressions_file = "suppressions.toml"
```

### Format

```toml
[[suppressions]]
rule_id = "missing-auth"
contract_path = "test_data/contracts"
location = "Dynamic trace"
reason = "Intentional risk in test environments"
```

- `rule_id`: ID of the rule being suppressed (see [security-rules.md](security-rules.md) for a list of IDs)
- `contract_path`: Substring of the contract path. Use `""` to match all contracts.
- `location`: Optional substring matching the location of the finding. If omitted, all findings for the rule/contract are suppressed.
- `reason`: Justification for ignoring the finding

## Examples

### Suppress a rule globally
To ignore a rule across the entire project, use an empty `contract_path` and omit the `location`.

```toml
[[suppressions]]
rule_id = "arithmetic"
contract_path = ""
reason = "Project uses a custom checked arithmetic wrapper that the analyzer doesn't yet recognize"
```

### Suppress a rule for a specific contract
To ignore a rule only for certain files (e.g., test mocks), provide a substring of the path.

```toml
[[suppressions]]
rule_id = "missing-auth"
contract_path = "contracts/mocks/"
reason = "Mock contracts intentionally bypass authorization for testing"
```

### Suppress a specific finding
To ignore a specific finding within a contract, provide both the `contract_path` and the `location` string reported by the analyzer.

```toml
[[suppressions]]
rule_id = "hardcoded-address"
contract_path = "contracts/governance"
location = "src/lib.rs:124"
reason = "This is the hardcoded DAO treasury address, which is intentional"
```
