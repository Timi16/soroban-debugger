# Security Rules Metadata

The `soroban-debug` CLI analyzer runs static and dynamic rules to evaluate security constraints.

## Rule Output
Rules supply robust metadata explicitly defined via the `SecurityRule` trait:
- `id`: Static string identifier for the rule.
- `severity`: Low, Medium, High, or Critical.
- `rationale`: Why the rule exists and what threat it prevents.
- `remediation`: Explicit direction to fix the offense securely.

## Built-in Rules
1. **hardcoded-address**: Flags hardcoded addresses that limit portability.
2. **missing-auth**: Checks dynamic trace paths for missed `.require_auth()`.
3. **arithmetic**: Validates potential unbounded operation panics.
4. **reentrancy**: Defends against nested external calls to untrusted code.
5. **unbounded-iteration**: Defends against `Vec` bounds scaling out of control.

## Suppressing Rules
If a rule flags a finding that is an accepted risk or a false positive, you can suppress it using a suppression file. See [analyzer-suppressions.md](analyzer-suppressions.md) for details on the format and examples.
