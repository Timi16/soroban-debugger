#![no_std]
use soroban_sdk::{contract, contractimpl, Env, Vec};

#[contract]
pub struct SampleContract;

#[contractimpl]
impl SampleContract {
    // CPU-ish: loop + repeated computation
    pub fn expensive(env: Env, n: u32) -> u64 {
        let mut acc: u64 = 0;
        let mut i: u32 = 0;
        while i < n {
            // some arithmetic to burn budget
            acc = acc.wrapping_add((i as u64).wrapping_mul(1234567) ^ 0xDEADBEEF);
            i += 1;
        }
        acc
    }

    // Memory-ish: build a vector
    pub fn alloc(env: Env, n: u32) -> Vec<u32> {
        let mut v = Vec::<u32>::new(&env);
        let mut i: u32 = 0;
        while i < n {
            v.push_back(i);
            i += 1;
        }
        v
    }
}