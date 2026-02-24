#![no_std]
use soroban_sdk::{contract, contractimpl, symbol_short, Env, Vec};

#[contract]
pub struct BudgetHeavy;

#[contractimpl]
impl BudgetHeavy {
    pub fn heavy(env: Env, n: u32) -> u32 {
        let mut v = Vec::<u32>::new(&env);
        for i in 0..n {
            v.push_back(i);
            env.storage().instance().set(&symbol_short!("i"), &i);
        }
        v.len()
    }
}
