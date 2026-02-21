#![cfg(test)]

use super::*;
use soroban_sdk::{symbol_short, Env};

#[test]
fn test_counter() {
    let env = Env::default();
    let contract_id = env.register_contract(None, Counter);

    // Initialize
    CounterClient::new(&env, &contract_id).init(&0);

    // Increment
    assert_eq!(CounterClient::new(&env, &contract_id).increment(), 1);
    assert_eq!(CounterClient::new(&env, &contract_id).increment(), 2);

    // Decrement
    assert_eq!(CounterClient::new(&env, &contract_id).decrement(), 1);

    // Get
    assert_eq!(CounterClient::new(&env, &contract_id).get(), 1);
}
