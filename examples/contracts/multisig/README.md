# Multisig Wallet Contract Example

A multi-signature wallet contract that demonstrates how the Soroban Debugger
captures **cross-address authorization flows** and **approval state changes** —
essential patterns for debugging governance and treasury contracts.

## What this contract does

This contract implements a classic M-of-N multisig wallet where M signers must
approve a transaction before it can be executed.

| Function | Description |
|---|---|
| `initialize(required_approvals, signers)` | Set up M-of-N configuration (e.g., 2-of-3) |
| `propose(proposer, target, function_name, description)` | Create a new transaction proposal |
| `approve(approver, proposal_id)` | Add approval from an authorized signer |
| `revoke(revoker, proposal_id)` | Remove a previous approval |
| `execute(executor, proposal_id)` | Execute proposal if threshold is met |
| `get_proposal(proposal_id)` | Read proposal details |
| `get_approval_count(proposal_id)` | Read current approval count |
| `get_approvals(proposal_id)` | Read list of approvers |
| `is_executed(proposal_id)` | Check if proposal was executed |
| `get_required_approvals()` | Read M (required approvals) |
| `get_signers()` | Read N (authorized signers) |

## Build
```bash
cd examples/contracts/multisig
cargo build --target wasm32-unknown-unknown --release
```

The WASM output will be at:
`target/wasm32-unknown-unknown/release/soroban_multisig_example.wasm`

## Debugger Walkthrough

### 1. Initialize 2-of-3 multisig — watch configuration storage
```bash
soroban-debug inspect \
  --wasm target/wasm32-unknown-unknown/release/soroban_multisig_example.wasm \
  --function initialize \
  --args '[2, ["GSIGNER1", "GSIGNER2", "GSIGNER3"]]'
```

Expected storage diff:
```
+ RequiredApprovals  →  2
+ Signers            →  [GSIGNER1, GSIGNER2, GSIGNER3]
+ ProposalCounter    →  0
```

### 2. Create proposal — watch proposal storage populate
```bash
soroban-debug inspect \
  --wasm target/wasm32-unknown-unknown/release/soroban_multisig_example.wasm \
  --function propose \
  --args '["GSIGNER1", "GTARGET", "transfer", "Send 100 tokens"]'
```

Expected storage diff:
```
+ Proposal(1)  →  {
    id: 1,
    proposer: GSIGNER1,
    target: GTARGET,
    function_name: "transfer",
    description: "Send 100 tokens"
  }
+ Approvals(1)  →  []
~ ProposalCounter  :  0  →  1
```

### 3. First approval — watch approval list grow
```bash
soroban-debug inspect \
  --wasm target/wasm32-unknown-unknown/release/soroban_multisig_example.wasm \
  --function approve \
  --args '["GSIGNER1", 1]'
```

Expected storage diff:
```
~ Approvals(1)  :  []  →  [GSIGNER1]
```

Key debugging insight: The `require_auth()` call verifies GSIGNER1's signature.
Use `soroban-debug step` to trace the authorization check.

### 4. Second approval — threshold reached
```bash
soroban-debug inspect \
  --wasm target/wasm32-unknown-unknown/release/soroban_multisig_example.wasm \
  --function approve \
  --args '["GSIGNER2", 1]'
```

Expected storage diff:
```
~ Approvals(1)  :  [GSIGNER1]  →  [GSIGNER1, GSIGNER2]
```

Now the proposal has 2 approvals, meeting the 2-of-3 threshold.

### 5. Execute proposal — watch execution flag set
```bash
soroban-debug inspect \
  --wasm target/wasm32-unknown-unknown/release/soroban_multisig_example.wasm \
  --function execute \
  --args '["GSIGNER3", 1]'
```

Expected storage diff:
```
+ Executed(1)  →  true
```

Note: Any signer can trigger execution once the threshold is met.

### 6. Revoke approval — watch approval list shrink
```bash
soroban-debug inspect \
  --wasm target/wasm32-unknown-unknown/release/soroban_multisig_example.wasm \
  --function revoke \
  --args '["GSIGNER1", 1]'
```

This will fail with `AlreadyExecuted` error since we executed in step 5.
To test revocation, create a new proposal and revoke before execution:

```bash
# Create new proposal (ID 2)
soroban-debug inspect \
  --wasm target/wasm32-unknown-unknown/release/soroban_multisig_example.wasm \
  --function propose \
  --args '["GSIGNER1", "GTARGET", "mint", "Mint 50 tokens"]'

# Approve from two signers
soroban-debug inspect \
  --wasm target/wasm32-unknown-unknown/release/soroban_multisig_example.wasm \
  --function approve \
  --args '["GSIGNER1", 2]'

soroban-debug inspect \
  --wasm target/wasm32-unknown-unknown/release/soroban_multisig_example.wasm \
  --function approve \
  --args '["GSIGNER2", 2]'

# Revoke first approval
soroban-debug inspect \
  --wasm target/wasm32-unknown-unknown/release/soroban_multisig_example.wasm \
  --function revoke \
  --args '["GSIGNER1", 2]'
```

Expected storage diff for revoke:
```
~ Approvals(2)  :  [GSIGNER1, GSIGNER2]  →  [GSIGNER2]
```

Now the proposal only has 1 approval, below the threshold.

## Debugging Authorization Flows

### Step through approval with breakpoints
```bash
soroban-debug step \
  --wasm target/wasm32-unknown-unknown/release/soroban_multisig_example.wasm \
  --function approve \
  --args '["GSIGNER1", 1]' \
  --breakpoint approve
```

Key points to observe:
1. `require_auth()` validates the signer's authorization
2. `is_signer()` checks if address is in authorized list
3. `has_approved()` prevents duplicate approvals
4. Storage write adds address to approval vector

### Trace execution threshold check
```bash
soroban-debug step \
  --wasm target/wasm32-unknown-unknown/release/soroban_multisig_example.wasm \
  --function execute \
  --args '["GSIGNER3", 1]' \
  --breakpoint execute
```

Watch the debugger step through:
1. Authorization check for executor
2. Proposal existence verification
3. Execution status check
4. Approval count vs. required threshold comparison
5. Execution flag storage write

### Compare approval states
```bash
# Save state after first approval
soroban-debug inspect \
  --wasm target/wasm32-unknown-unknown/release/soroban_multisig_example.wasm \
  --function approve \
  --args '["GSIGNER1", 1]' \
  --save-snapshot approval_1.json

# Save state after second approval
soroban-debug inspect \
  --wasm target/wasm32-unknown-unknown/release/soroban_multisig_example.wasm \
  --function approve \
  --args '["GSIGNER2", 1]' \
  --save-snapshot approval_2.json

# Compare the two states
soroban-debug compare approval_1.json approval_2.json
```

This shows exactly how the approval list changes between states.

## Common Debugging Scenarios

### Scenario 1: "Why can't I execute?"
Use `inspect` to check:
- Current approval count: `get_approval_count(proposal_id)`
- Required threshold: `get_required_approvals()`
- Execution status: `is_executed(proposal_id)`

### Scenario 2: "Who approved this proposal?"
```bash
soroban-debug inspect \
  --wasm target/wasm32-unknown-unknown/release/soroban_multisig_example.wasm \
  --function get_approvals \
  --args '[1]'
```

### Scenario 3: "Why did my approval fail?"
Common errors:
- `NotASigner`: Address not in authorized signers list
- `AlreadyApproved`: This signer already approved
- `AlreadyExecuted`: Proposal was already executed
- `ProposalNotFound`: Invalid proposal ID

Use `step` mode to trace exactly where the error occurs.

## Running Tests
```bash
cargo test
```

The test suite includes:
- Basic initialization and configuration
- Proposal creation and counting
- Approval accumulation
- Execution with sufficient approvals
- Execution failure without enough approvals
- Approval revocation
- Duplicate approval prevention
- Double execution prevention
- Multiple concurrent proposals

## Architecture Notes

This contract demonstrates several key Soroban patterns:

1. **Multi-address authorization**: Each function validates the caller using
   `require_auth()`, showing how to debug cross-signer workflows.

2. **State machine**: Proposals move through states (proposed → approved →
   executed), with storage diffs showing each transition.

3. **Threshold logic**: The M-of-N approval check is a common pattern in
   governance contracts.

4. **Vector manipulation**: Approval lists grow and shrink, demonstrating
   dynamic storage patterns.

5. **Idempotency checks**: Prevents duplicate approvals and double execution,
   showing defensive programming patterns.

## Integration with Other Contracts

In a real deployment, the `execute()` function would invoke the target contract:

```rust
// Pseudo-code for actual execution
env.invoke_contract(
    &proposal.target,
    &proposal.function_name,
    proposal.args
);
```

This example focuses on the approval flow for debugger demonstration purposes.
