#![no_std]
use soroban_sdk::{contract, contractimpl, Address, Env, Symbol, Val, Vec};

#[contract]
pub struct Caller;

#[contractimpl]
impl Caller {
    pub fn call(env: Env, c: Address, f: Symbol, a: Vec<Val>) -> Val {
        env.invoke_contract(&c, &f, a)
    }
}
