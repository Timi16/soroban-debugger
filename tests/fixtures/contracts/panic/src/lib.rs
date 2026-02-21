#![no_std]

use soroban_sdk::{contract, contractimpl, Env};

#[contract]
pub struct PanicContract;

#[contractimpl]
impl PanicContract {
    /// Always panics - useful for error testing
    pub fn panic(_env: Env) {
        panic!("This contract always panics");
    }

    /// Panics with a custom message
    pub fn panic_with_message(_env: Env, message: soroban_sdk::String) {
        panic!("{}", message);
    }

    /// Panics when called with a specific value
    pub fn panic_if_true(_env: Env, should_panic: bool) {
        if should_panic {
            panic!("Panic triggered by condition");
        }
    }
}

#[cfg(test)]
mod test;
