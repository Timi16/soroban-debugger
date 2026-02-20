# Staking Contract Example

A time-based staking contract that demonstrates how the Soroban Debugger
captures **storage diffs across ledger timestamps** — one of the most
useful debugging patterns for DeFi contracts.

## What this contract does

| Function | Description |
|---|---|
| `initialize(reward_rate)` | Set up contract with a reward rate in basis points/second |
| `stake(staker, amount)` | Stake tokens; records amount + timestamp in persistent storage |
| `unstake(staker, amount)` | Withdraw tokens; clears storage entries when fully unstaked |
| `claim_rewards(staker)` | Compute rewards from elapsed time; resets stake timestamp |
| `get_balance(staker)` | Read current staked amount |
| `total_staked()` | Read global staked total |
| `stake_timestamp(staker)` | Read when staker last staked/claimed |

## Build
```bash
cd examples/contracts/staking
cargo build --target wasm32-unknown-unknown --release
```

The WASM output will be at:
`target/wasm32-unknown-unknown/release/soroban_staking_example.wasm`

## Debugger Walkthrough

### 1. Inspect initial stake — watch storage populate
```bash
soroban-debug inspect \
  --wasm target/wasm32-unknown-unknown/release/soroban_staking_example.wasm \
  --function stake \
  --args '["GSTAKER123", 10000]'
```

Expected storage diff:
```
+ StakeAmount(GSTAKER123)  →  10000
+ StakeTime(GSTAKER123)    →  1700000000   ← ledger timestamp
~ TotalStaked              :  0  →  10000
```

### 2. Advance time and claim rewards — watch timestamp reset
```bash
soroban-debug inspect \
  --wasm target/wasm32-unknown-unknown/release/soroban_staking_example.wasm \
  --function claim_rewards \
  --args '["GSTAKER123"]' \
  --ledger-time 1700000100   # 100 seconds later
```

Expected storage diff:
```
~ StakeTime(GSTAKER123)  :  1700000000  →  1700000100
```
Return value: `1000`  (= 10000 × 10 × 100 / 10000)

### 3. Full unstake — watch storage entries disappear
```bash
soroban-debug inspect \
  --wasm target/wasm32-unknown-unknown/release/soroban_staking_example.wasm \
  --function unstake \
  --args '["GSTAKER123", 10000]'
```

Expected storage diff:
```
- StakeAmount(GSTAKER123)  (removed)
- StakeTime(GSTAKER123)    (removed)
~ TotalStaked              :  10000  →  0
```

### 4. Step through stake execution
```bash
soroban-debug step \
  --wasm target/wasm32-unknown-unknown/release/soroban_staking_example.wasm \
  --function stake \
  --args '["GSTAKER123", 5000]' \
  --breakpoint stake
```

## Reward Rate Formula
```
reward = staked_amount × reward_rate × elapsed_seconds / 10_000
```

A `reward_rate` of `10` means 0.10% per second. Adjust at `initialize` time.

## Running Tests
```bash
cargo test
```