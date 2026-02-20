# DEX Contract Example

A simplified Decentralized Exchange (DEX) contract demonstrating automated market maker (AMM) functionality with multi-token storage debugging.

## Contract Overview

This DEX implements a constant product AMM (x * y = k) with:
- Two token reserves (Token A and Token B)
- Liquidity management (add/remove)
- Token swapping
- Price queries

## Functions

### `add_liquidity(amount_a: i128, amount_b: i128)`
Adds liquidity to both token reserves.

### `remove_liquidity(amount_a: i128, amount_b: i128)`
Removes liquidity from both token reserves.

### `swap(token_in: bool, amount_in: i128) -> i128`
Swaps tokens using constant product formula.
- `token_in`: true for Token A → B, false for Token B → A
- Returns: amount of output tokens

### `get_price(token_in: bool) -> (i128, i128)`
Returns price ratio (numerator, denominator).
- `token_in`: true for A→B price, false for B→A price

## Building

```bash
cd examples/contracts/dex
cargo build --target wasm32-unknown-unknown --release
```

## Debugging Walkthrough

### Setup: Add Initial Liquidity

1. Deploy the contract and add liquidity:
```bash
# Add 1000 Token A and 2000 Token B
soroban contract invoke --id <CONTRACT_ID> -- add_liquidity --amount_a 1000 --amount_b 2000
```

2. Set breakpoint in debugger at `swap` function to inspect reserves before swap.

### Debug Scenario: Token Swap

**Goal**: Debug a swap operation and observe how reserves change.

1. **Check initial price**:
```bash
soroban contract invoke --id <CONTRACT_ID> -- get_price --token_in true
# Returns: (2000, 1000) meaning 1 Token A = 2 Token B
```

2. **Set breakpoints** in your debugger:
   - Line where `reserve_a` and `reserve_b` are loaded from storage
   - Line where `amount_out` is calculated
   - Lines where reserves are updated in storage

3. **Execute swap**:
```bash
# Swap 100 Token A for Token B
soroban contract invoke --id <CONTRACT_ID> -- swap --token_in true --amount_in 100
```

4. **Inspect storage during debugging**:
   - **Before swap**: 
     - `ReserveA`: 1000
     - `ReserveB`: 2000
   - **Calculation**: 
     - `reserve_in`: 1000, `reserve_out`: 2000
     - `amount_out = (2000 * 100) / (1000 + 100) = 181`
   - **After swap**:
     - `ReserveA`: 1100 (1000 + 100)
     - `ReserveB`: 1819 (2000 - 181)

5. **Verify price changed**:
```bash
soroban contract invoke --id <CONTRACT_ID> -- get_price --token_in true
# Returns: (1819, 1100) - price shifted due to swap
```

### Key Debugging Points

**Storage Inspection**:
- Watch `DataKey::ReserveA` and `DataKey::ReserveB` in storage
- Observe how `env.storage().instance().get()` retrieves values
- Track `env.storage().instance().set()` updates

**Price Impact**:
- Initial ratio: 2000/1000 = 2.0
- After swap ratio: 1819/1100 ≈ 1.65
- Demonstrates slippage in AMM model

**Multi-token State**:
- Both reserves update atomically in single transaction
- Constant product maintained: 1000 * 2000 = 2,000,000 → 1100 * 1819 ≈ 2,000,900

### Advanced Debugging

**Test multiple swaps**:
```bash
# Swap 1: A → B
soroban contract invoke --id <CONTRACT_ID> -- swap --token_in true --amount_in 50

# Check reserves
soroban contract invoke --id <CONTRACT_ID> -- get_price --token_in true

# Swap 2: B → A  
soroban contract invoke --id <CONTRACT_ID> -- swap --token_in false --amount_in 100

# Observe reserve changes in both directions
```

**Debug liquidity removal**:
```bash
# Remove liquidity and watch reserves decrease
soroban contract invoke --id <CONTRACT_ID> -- remove_liquidity --amount_a 100 --amount_b 200
```

## Debugging Tips

1. Use breakpoints at storage operations to see exact state changes
2. Step through the swap calculation to understand AMM math
3. Compare reserve values before/after each operation
4. Watch for integer division precision in `amount_out` calculation
5. Verify constant product formula holds (with rounding)
