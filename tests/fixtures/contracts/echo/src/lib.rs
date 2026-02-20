#![no_std]

use soroban_sdk::{contract, contractimpl, Env, Val};

#[contract]
pub struct Echo;

#[contractimpl]
impl Echo {
    /// Echo function that returns its input unchanged
    pub fn echo(_env: Env, value: Val) -> Val {
        value
    }

    /// Echo a string
    pub fn echo_string(_env: Env, value: soroban_sdk::String) -> soroban_sdk::String {
        value
    }

    /// Echo an integer
    pub fn echo_i64(_env: Env, value: i64) -> i64 {
        value
    }

    /// Echo a boolean
    pub fn echo_bool(_env: Env, value: bool) -> bool {
        value
    }
}

#[cfg(test)]
mod test;
