#![no_std]

use soroban_sdk::{contract, contractimpl, symbol_short, Env};

#[contract]
pub struct Counter;

#[contractimpl]
impl Counter {
    /// Initialize the counter with a starting value
    pub fn init(env: Env, value: i64) {
        env.storage().instance().set(&symbol_short!("count"), &value);
    }

    /// Increment the counter by 1
    pub fn increment(env: Env) -> i64 {
        let current: i64 = env
            .storage()
            .instance()
            .get(&symbol_short!("count"))
            .unwrap_or(0);
        let new_value = current + 1;
        env.storage().instance().set(&symbol_short!("count"), &new_value);
        new_value
    }

    /// Decrement the counter by 1
    pub fn decrement(env: Env) -> i64 {
        let current: i64 = env
            .storage()
            .instance()
            .get(&symbol_short!("count"))
            .unwrap_or(0);
        let new_value = current - 1;
        env.storage().instance().set(&symbol_short!("count"), &new_value);
        new_value
    }

    /// Get the current counter value
    pub fn get(env: Env) -> i64 {
        env.storage()
            .instance()
            .get(&symbol_short!("count"))
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod test;
