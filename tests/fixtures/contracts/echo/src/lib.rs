#![no_std]
use soroban_sdk::{contract, contractimpl, Env, Val};

#[contract]
pub struct Echo;

#[contractimpl]
impl Echo {
    pub fn echo(_env: Env, v: Val) -> Val {
        v
    }
}
