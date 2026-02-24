#![no_std]
use soroban_sdk::{contract, contractimpl, Env};

#[contract]
pub struct AlwaysPanic;

#[contractimpl]
impl AlwaysPanic {
    pub fn panic(_env: Env) {
        panic!("p")
    }
}
