#![no_std]
use soroban_sdk::{contract, contractimpl, symbol_short, Env};

#[contract]
pub struct Counter;

#[contractimpl]
impl Counter {
    pub fn increment(env: Env) -> i64 {
        let val: i64 = env
            .storage()
            .instance()
            .get(&symbol_short!("c"))
            .unwrap_or(0)
            + 1;
        env.storage().instance().set(&symbol_short!("c"), &val);
        val
    }
    pub fn get(env: Env) -> i64 {
        env.storage()
            .instance()
            .get(&symbol_short!("c"))
            .unwrap_or(0)
    }
}
