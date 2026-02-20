#![cfg(test)]

use super::*;
use soroban_sdk::Env;

#[test]
#[should_panic(expected = "always panics")]
fn test_panic() {
    let env = Env::default();
    let contract_id = env.register_contract(None, PanicContract);
    PanicContractClient::new(&env, &contract_id).panic();
}
