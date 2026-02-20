#![cfg(test)]

use super::*;
use soroban_sdk::Env;

#[test]
fn test_echo() {
    let env = Env::default();
    let contract_id = env.register_contract(None, Echo);

    let client = EchoClient::new(&env, &contract_id);

    assert_eq!(client.echo_i64(&42), 42);
    assert_eq!(client.echo_bool(&true), true);
    assert_eq!(client.echo_string(&soroban_sdk::String::from_str(&env, "hello")), soroban_sdk::String::from_str(&env, "hello"));
}
