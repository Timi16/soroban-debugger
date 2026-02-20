#![no_std]

use soroban_sdk::{contract, contractimpl, symbol_short, Address, Env, Val};

#[contract]
pub struct CrossContractCaller;

#[contractimpl]
impl CrossContractCaller {
    /// Call another contract's function
    /// 
    /// # Arguments
    /// * `callee` - Address of the contract to call
    /// * `function` - Function name to call (as symbol)
    /// * `args` - Arguments to pass to the function
    pub fn call_contract(
        env: Env,
        callee: Address,
        function: soroban_sdk::Symbol,
        args: Vec<Val>,
    ) -> Val {
        env.invoke_contract(&callee, &function, &args)
    }

    /// Call another contract's echo function with a value
    pub fn call_echo(env: Env, callee: Address, value: Val) -> Val {
        let args = soroban_sdk::vec![&env, value];
        env.invoke_contract(&callee, &symbol_short!("echo"), &args)
    }

    /// Chain call: call a contract that calls another contract
    pub fn chain_call(
        env: Env,
        first: Address,
        second: Address,
        value: Val,
    ) -> Val {
        // Call first contract
        let first_result = env.invoke_contract(
            &first,
            &symbol_short!("echo"),
            &soroban_sdk::vec![&env, value],
        );
        
        // Call second contract with first result
        env.invoke_contract(
            &second,
            &symbol_short!("echo"),
            &soroban_sdk::vec![&env, first_result],
        )
    }
}

#[cfg(test)]
mod test;
