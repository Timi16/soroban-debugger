#![no_std]

use soroban_sdk::{contract, contractimpl, symbol_short, Env, Vec};

#[contract]
pub struct BudgetHeavy;

#[contractimpl]
impl BudgetHeavy {
    /// Perform a budget-heavy operation with many iterations
    pub fn heavy_computation(env: Env, iterations: u32) -> u32 {
        let mut result = 0u32;
        for i in 0..iterations {
            // Perform some computation
            result = result.wrapping_add(i);
            
            // Store intermediate results (costs budget)
            env.storage()
                .instance()
                .set(&symbol_short!("iter"), &i);
        }
        result
    }

    /// Allocate and manipulate large vectors
    pub fn heavy_memory(env: Env, size: u32) -> u32 {
        let mut vec = Vec::<u32>::new(&env);
        for i in 0..size {
            vec.push_back(i);
        }
        vec.len()
    }

    /// Nested loops for CPU-intensive operations
    pub fn nested_loops(env: Env, n: u32) -> u32 {
        let mut sum = 0u32;
        for i in 0..n {
            for j in 0..n {
                sum = sum.wrapping_add(i.wrapping_mul(j));
            }
        }
        sum
    }
}

#[cfg(test)]
mod test;
