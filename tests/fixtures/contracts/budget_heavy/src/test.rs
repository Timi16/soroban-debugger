#![cfg(test)]

use super::*;
use soroban_sdk::Env;

#[test]
fn test_budget_heavy() {
    let env = Env::default();
    let contract_id = env.register_contract(None, BudgetHeavy);
    let client = BudgetHeavyClient::new(&env, &contract_id);

    // Test with small values to avoid timeout in tests
    assert_eq!(client.heavy_computation(&10), 45);
    assert_eq!(client.heavy_memory(&5), 5);
    assert_eq!(client.nested_loops(&5), 100);
}
