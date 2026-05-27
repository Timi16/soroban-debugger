# Writing Budget-Efficient Soroban Contracts

A practical guide to identifying and fixing the most common budget inefficiencies in Soroban smart contracts, using the Soroban Debugger's profiling and budget tools.

---

## Table of Contents

1. [Understanding the Soroban Budget Model](#1-understanding-the-soroban-budget-model)
2. [Using the Debugger Profiler](#2-using-the-debugger-profiler)
3. [Optimization Pattern 1: Redundant Storage Reads](#3-optimization-pattern-1-redundant-storage-reads)
4. [Optimization Pattern 2: Heavy Type Usage](#4-optimization-pattern-2-heavy-type-usage)
5. [Optimization Pattern 3: Unnecessary Computation](#5-optimization-pattern-3-unnecessary-computation)
6. [Optimization Pattern 4: Unbounded Iterations](#6-optimization-pattern-4-unbounded-iterations)
7. [Optimization Pattern 5: Inefficient Data Structures](#7-optimization-pattern-5-inefficient-data-structures)
8. [Summary: Budget Savings Cheatsheet](#8-summary-budget-savings-cheatsheet)

---

## 1. Understanding the Soroban Budget Model

Soroban enforces two resource budgets per transaction:

| Budget Type | What it measures |
|---|---|
| **CPU instructions** | Computational work (iterations, hashing, arithmetic) |
| **Memory bytes** | Heap allocations across the contract's lifetime |

Every host function call, storage operation, and type construction consumes from these budgets. Exceeding either budget causes the transaction to fail with `ExceededLimit`.

You can inspect current budget consumption at any point using the debugger:

```bash
soroban-debugger budget --contract <CONTRACT_ID> --fn <FUNCTION_NAME>
```

---

## 2. Using the Debugger Profiler

### Running a baseline profile

```bash
# Profile a single function invocation
soroban-debugger profile \
  --contract <CONTRACT_ID> \
  --fn create_invoice \
  --args '{"merchant": "GABC...", "amount": 1000}' \
  --output profile.json

# Print a human-readable budget summary
soroban-debugger budget-summary --input profile.json
```

### Reading the output

```
== Budget Summary: create_invoice ==
CPU Instructions:   142,800 / 100,000,000  (0.14%)
Memory Bytes:        18,432 / 40,000,000   (0.05%)

Top consumers (CPU):
  storage::get          x4   → 48,000 instructions
  map::insert           x3   → 36,200 instructions
  vec::push_back        x2   → 12,400 instructions
  arithmetic            x6   →  8,100 instructions
```

### Diffing before and after an optimization

```bash
soroban-debugger budget-diff --before baseline.json --after optimized.json
```

### Choosing a report format for `optimize`

The `optimize` command writes the same content in two formats; pick the one
your downstream consumer expects:

```bash
# Default: human-readable markdown (existing behavior)
soroban-debugger optimize --contract contract.wasm --function transfer

# Structured JSON for CI gates, IDE plugins, or dashboards
soroban-debugger optimize --contract contract.wasm --function transfer \
  --format json --output optimize.json
```

`--format` is independent of `--output`: `--format` chooses the content
(`pretty` markdown or `json`); `--output` chooses the destination (stdout
when omitted, a file when set). JSON output is keyed by `schema_version`,
`metadata`, `hotspots`, and `suggestions`, so consumers can pin to a
schema version and ignore fields they don't understand.

---

## 3. Optimization Pattern 1: Redundant Storage Reads

### The problem

Every `storage().persistent().get()` call costs CPU instructions and counts against your budget. Reading the same key multiple times in one function is the most common waste in Soroban contracts.

### Inefficient version

```rust
pub fn update_invoice_status(env: &Env, invoice_id: u64, new_status: InvoiceStatus) {
    // First read
    let invoice: Invoice = env.storage().persistent()
        .get(&DataKey::Invoice(invoice_id))
        .unwrap();

    if invoice.status == InvoiceStatus::Paid {
        panic_with_error!(env, ContractError::InvalidInvoiceStatus);
    }

    // Second read — fetches the same data again!
    let mut invoice: Invoice = env.storage().persistent()
        .get(&DataKey::Invoice(invoice_id))
        .unwrap();

    invoice.status = new_status;
    env.storage().persistent()
        .set(&DataKey::Invoice(invoice_id), &invoice);
}
```

**Profiler output (before):**
```
CPU Instructions:  38,400
  storage::get    x2  →  24,000 instructions   ← doubled cost
  storage::set    x1  →   8,200 instructions
```

### Efficient version

```rust
pub fn update_invoice_status(env: &Env, invoice_id: u64, new_status: InvoiceStatus) {
    // Single read — reuse the binding
    let mut invoice: Invoice = env.storage().persistent()
        .get(&DataKey::Invoice(invoice_id))
        .unwrap();

    if invoice.status == InvoiceStatus::Paid {
        panic_with_error!(env, ContractError::InvalidInvoiceStatus);
    }

    invoice.status = new_status;
    env.storage().persistent()
        .set(&DataKey::Invoice(invoice_id), &invoice);
}
```

**Profiler output (after):**
```
CPU Instructions:  26,200
  storage::get    x1  →  12,000 instructions   ✓ halved
  storage::set    x1  →   8,200 instructions
```

**Savings: ~32% CPU reduction (12,200 instructions)**

### Rule of thumb

Read once into a local variable at the top of your function, mutate in memory, write once at the end.

---

## 4. Optimization Pattern 2: Heavy Type Usage

### The problem

Soroban `Map<K, V>` and `Vec<T>` are host-managed types. Every construction, clone, or insertion involves a host call with associated CPU and memory costs. Using them for small, fixed-shape data is overkill.

### Inefficient version

```rust
// Using a Map to return structured data — expensive host object
pub fn get_merchant_summary(env: &Env, merchant_id: u64) -> Map<Symbol, Val> {
    let merchant: Merchant = env.storage().persistent()
        .get(&DataKey::Merchant(merchant_id))
        .unwrap();

    let mut result = Map::new(env);
    result.set(Symbol::new(env, "id"),      merchant.id.into_val(env));
    result.set(Symbol::new(env, "active"),  merchant.active.into_val(env));
    result.set(Symbol::new(env, "verified"), merchant.verified.into_val(env));
    result
}
```

**Profiler output (before):**
```
CPU Instructions:  51,600
Memory Bytes:       6,144
  map::new        x1  →  14,200 instructions
  map::set        x3  →  22,800 instructions
  symbol::new     x3  →   9,400 instructions
```

### Efficient version

```rust
// Use a typed struct — serialized once, no host map overhead
#[contracttype]
pub struct MerchantSummary {
    pub id: u64,
    pub active: bool,
    pub verified: bool,
}

pub fn get_merchant_summary(env: &Env, merchant_id: u64) -> MerchantSummary {
    let merchant: Merchant = env.storage().persistent()
        .get(&DataKey::Merchant(merchant_id))
        .unwrap();

    MerchantSummary {
        id: merchant.id,
        active: merchant.active,
        verified: merchant.verified,
    }
}
```

**Profiler output (after):**
```
CPU Instructions:  18,400
Memory Bytes:       1,024
  storage::get    x1  →  12,000 instructions
  struct::encode  x1  →   4,200 instructions
```

**Savings: ~64% CPU reduction (33,200 instructions), ~83% memory reduction**

### Rule of thumb

Prefer `#[contracttype]` structs over `Map` for structured return values. Reserve `Map` and `Vec` for genuinely variable-length data.

---

## 5. Optimization Pattern 3: Unnecessary Computation

### The problem

Performing calculations whose results are already stored, or which are not needed for the current execution path, burns CPU budget silently.

### Inefficient version

```rust
pub fn pay_invoice(env: &Env, payer: &Address, invoice_id: u64) {
    let invoice = get_invoice(env, invoice_id);

    // Fee is computed regardless of whether the invoice is payable
    let fee_bps = admin::get_fee(env, &invoice.token);         // storage read
    let fee_amount = (invoice.amount * fee_bps) / 10_000;      // arithmetic
    let merchant_amount = invoice.amount - fee_amount;          // arithmetic

    // Status check happens AFTER the computation
    if invoice.status != InvoiceStatus::Pending {
        panic_with_error!(env, ContractError::InvalidInvoiceStatus);
    }

    // ... transfer logic
}
```

**Profiler output (before):**
```
CPU Instructions:  44,800
  storage::get (fee)  x1  →  12,000 instructions   ← wasted on bad status
  arithmetic          x2  →   3,200 instructions    ← wasted on bad status
```

### Efficient version

```rust
pub fn pay_invoice(env: &Env, payer: &Address, invoice_id: u64) {
    let invoice = get_invoice(env, invoice_id);

    // Guard clause first — fail fast before any expensive work
    if invoice.status != InvoiceStatus::Pending {
        panic_with_error!(env, ContractError::InvalidInvoiceStatus);
    }

    // Computation only runs when we know we'll use the result
    let fee_bps = admin::get_fee(env, &invoice.token);
    let fee_amount = (invoice.amount * fee_bps) / 10_000;
    let merchant_amount = invoice.amount - fee_amount;

    // ... transfer logic
}
```

**Profiler output (after):**
```
CPU Instructions (happy path):   44,800   (unchanged — full path still runs)
CPU Instructions (error path):   19,200   ✓ saves 25,600 on invalid calls
```

**Savings: ~57% CPU reduction on the error path**

### Rule of thumb

Always validate inputs and state conditions before performing storage reads or heavy computation. Structure functions as: **validate → read → compute → write**.

---

## 6. Optimization Pattern 4: Unbounded Iterations

The security analyzer now keeps two separate signals here:

- `unbounded-iteration` for storage-read-heavy traversal patterns
- `storage-write-pressure` for write-heavy loops and repeated mutation of hot keys

That split matters because a contract can be safe from read-driven iteration risk and still burn budget by rewriting the same state over and over inside a loop.

### The problem

Iterating over a `Vec` or a range of storage keys with no upper bound makes your function's budget consumption proportional to data size. As state grows, the function eventually hits the CPU limit and becomes uncallable.

### Inefficient version

```rust
// Scans ALL invoices every time — O(n) with no cap
pub fn get_invoices(env: &Env, filter: InvoiceFilter) -> Vec<Invoice> {
    let invoice_count: u64 = env.storage().persistent()
        .get(&DataKey::InvoiceCount)
        .unwrap_or(0);

    let mut results = Vec::new(env);

    for i in 1..=invoice_count {                          // unbounded!
        if let Some(invoice) = env.storage().persistent()
            .get::<_, Invoice>(&DataKey::Invoice(i))
        {
            if matches_filter(&invoice, &filter) {
                results.push_back(invoice);
            }
        }
    }
    results
}
```

**Profiler output (before, 500 invoices):**
```
CPU Instructions:  6,240,000
  storage::get    x500  →  6,000,000 instructions   ← scales linearly
```

### Efficient version

```rust
pub fn get_invoices(
    env: &Env,
    filter: InvoiceFilter,
    page: u64,
    page_size: u64,           // caller controls the window
) -> Vec<Invoice> {
    let max_page_size: u64 = 20;                         // hard cap
    let page_size = page_size.min(max_page_size);

    let invoice_count: u64 = env.storage().persistent()
        .get(&DataKey::InvoiceCount)
        .unwrap_or(0);

    let start = (page * page_size) + 1;
    let end = (start + page_size).min(invoice_count + 1);

    let mut results = Vec::new(env);

    for i in start..end {                                // bounded window
        if let Some(invoice) = env.storage().persistent()
            .get::<_, Invoice>(&DataKey::Invoice(i))
        {
            if matches_filter(&invoice, &filter) {
                results.push_back(invoice);
            }
        }
    }
    results
}
```

**Profiler output (after, page_size=20):**
```
CPU Instructions:  242,000
  storage::get    x20   →  240,000 instructions   ✓ constant regardless of total
```

**Savings: ~96% CPU reduction at scale (6,000,000 → 240,000 instructions)**

### Measuring the budget cliff

```bash
# Simulate growth to find your breaking point
soroban-debugger budget-sweep \
  --contract <CONTRACT_ID> \
  --fn get_invoices \
  --sweep-param invoice_count \
  --range 1,1000,100
```

### Rule of thumb

Any function that reads from storage in a loop **must** have a hard upper bound on iterations. Add `page` + `page_size` parameters, and enforce a maximum page size in-contract.

---

## 7. Optimization Pattern 5: Inefficient Data Structures

### The problem

Storing multiple related values as separate storage keys multiplies the number of host calls needed to read or write them. Each `storage::get` and `storage::set` is expensive — batching related fields into one struct cuts costs proportionally.

### Inefficient version

```rust
// Each field stored and read separately
pub fn register_merchant(env: &Env, merchant: &Address) {
    let id = next_merchant_id(env);
    env.storage().persistent().set(&DataKey::MerchantAddress(id), merchant);
    env.storage().persistent().set(&DataKey::MerchantActive(id), &true);
    env.storage().persistent().set(&DataKey::MerchantVerified(id), &false);
    env.storage().persistent().set(&DataKey::MerchantDateRegistered(id), &env.ledger().timestamp());
}

pub fn get_merchant(env: &Env, id: u64) -> (Address, bool, bool, u64) {
    let address  = env.storage().persistent().get(&DataKey::MerchantAddress(id)).unwrap();
    let active   = env.storage().persistent().get(&DataKey::MerchantActive(id)).unwrap();
    let verified = env.storage().persistent().get(&DataKey::MerchantVerified(id)).unwrap();
    let date     = env.storage().persistent().get(&DataKey::MerchantDateRegistered(id)).unwrap();
    (address, active, verified, date)
}
```

**Profiler output (before):**
```
register_merchant:
  CPU Instructions:  49,600
  storage::set  x4  →  32,800 instructions

get_merchant:
  CPU Instructions:  48,000
  storage::get  x4  →  48,000 instructions
```

### Efficient version

```rust
#[contracttype]
#[derive(Clone)]
pub struct Merchant {
    pub id: u64,
    pub address: Address,
    pub active: bool,
    pub verified: bool,
    pub date_registered: u64,
}

pub fn register_merchant(env: &Env, merchant: &Address) {
    let id = next_merchant_id(env);
    let record = Merchant {
        id,
        address: merchant.clone(),
        active: true,
        verified: false,
        date_registered: env.ledger().timestamp(),
    };
    env.storage().persistent().set(&DataKey::Merchant(id), &record);  // one write
}

pub fn get_merchant(env: &Env, id: u64) -> Merchant {
    env.storage().persistent()
        .get(&DataKey::Merchant(id))
        .unwrap_or_else(|| panic_with_error!(env, ContractError::MerchantNotFound))
    // one read
}
```

**Profiler output (after):**
```
register_merchant:
  CPU Instructions:  20,400
  storage::set  x1  →   8,200 instructions   ✓ 4x fewer writes

get_merchant:
  CPU Instructions:  14,200
  storage::get  x1  →  12,000 instructions   ✓ 4x fewer reads
```

**Savings: ~59% CPU on writes, ~70% CPU on reads**

### Rule of thumb

Group fields that are always read or written together into a single `#[contracttype]` struct. Split structs only when subsets of fields are independently accessed in hot paths.

---

## 8. Summary: Budget Savings Cheatsheet

| Pattern | Typical CPU Saving | Technique |
|---|---|---|
| Redundant storage reads | 25–50% | Read once, mutate in memory, write once |
| Heavy type usage | 50–70% | Prefer `#[contracttype]` structs over `Map` |
| Unnecessary computation | 40–60% on error paths | Validate first, compute after |
| Unbounded iterations | 90–99% at scale | Paginate with hard `max_page_size` cap |
| Inefficient data structures | 50–75% | Co-locate related fields in one struct |

### Profiler quick reference

```bash
# Baseline a function
soroban-debugger profile --contract <ID> --fn <FN> --args '<JSON>' --output out.json

# Print budget summary
soroban-debugger budget-summary --input out.json

# Compare before/after
soroban-debugger budget-diff --before before.json --after after.json

# Find the budget cliff as data grows
soroban-debugger budget-sweep --contract <ID> --fn <FN> \
  --sweep-param <PARAM> --range <START,END,STEP>
```

---

*For more on the Soroban budget model, see the [official Soroban documentation](https://developers.stellar.org/docs/smart-contracts).*
