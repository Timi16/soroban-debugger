#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env,
};

// -------------------------------------------------------------------
//  Storage Keys
// -------------------------------------------------------------------

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    /// Amount staked by a given address
    StakeAmount(Address),
    /// Ledger timestamp when the address last staked
    StakeTime(Address),
    /// Reward rate in basis points per ledger second (set at init)
    RewardRate,
    /// Total tokens staked across all stakers
    TotalStaked,
}

// -------------------------------------------------------------------
//  Errors
// -------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum StakeError {
    /// Caller has no active stake
    NoStake = 1,
    /// Stake amount must be greater than zero
    ZeroAmount = 2,
    /// Cannot unstake more than currently staked
    InsufficientStake = 3,
    /// Contract has already been initialized
    AlreadyInitialized = 4,
}

// -------------------------------------------------------------------
//  Contract
// -------------------------------------------------------------------

#[contract]
pub struct StakingContract;

#[contractimpl]
impl StakingContract {

    // ---------------------------------------------------------------
    //  Init
    // ---------------------------------------------------------------

    /// Initialize the contract with a reward rate (basis points per second).
    /// Example: reward_rate = 10 means 0.10% reward per second of staking.
    pub fn initialize(env: Env, reward_rate: u64) -> Result<(), StakeError> {
        if env.storage().instance().has(&DataKey::RewardRate) {
            return Err(StakeError::AlreadyInitialized);
        }
        env.storage()
            .instance()
            .set(&DataKey::RewardRate, &reward_rate);
        env.storage()
            .instance()
            .set(&DataKey::TotalStaked, &0u64);
        Ok(())
    }

    // ---------------------------------------------------------------
    //  Stake
    // ---------------------------------------------------------------

    /// Stake `amount` tokens. Caller must authorize.
    /// If the caller already has a stake, the existing stake is topped
    /// up and the timestamp is reset (simplifies reward math for this
    /// example — a production contract would checkpoint rewards first).
    pub fn stake(env: Env, staker: Address, amount: u64) -> Result<(), StakeError> {
        if amount == 0 {
            return Err(StakeError::ZeroAmount);
        }

        staker.require_auth();

        let current: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::StakeAmount(staker.clone()))
            .unwrap_or(0);

        let new_total = current + amount;

        env.storage()
            .persistent()
            .set(&DataKey::StakeAmount(staker.clone()), &new_total);

        // Record the timestamp so reward duration can be calculated later
        let now = env.ledger().timestamp();
        env.storage()
            .persistent()
            .set(&DataKey::StakeTime(staker.clone()), &now);

        // Update global total
        let global: u64 = env
            .storage()
            .instance()
            .get(&DataKey::TotalStaked)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::TotalStaked, &(global + amount));

        env.events()
            .publish((symbol_short!("stake"),), (&staker, amount, now));

        Ok(())
    }

    // ---------------------------------------------------------------
    //  Unstake
    // ---------------------------------------------------------------

    /// Unstake `amount` tokens. Caller must authorize.
    pub fn unstake(env: Env, staker: Address, amount: u64) -> Result<(), StakeError> {
        if amount == 0 {
            return Err(StakeError::ZeroAmount);
        }

        staker.require_auth();

        let current: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::StakeAmount(staker.clone()))
            .ok_or(StakeError::NoStake)?;

        if amount > current {
            return Err(StakeError::InsufficientStake);
        }

        let remaining = current - amount;

        if remaining == 0 {
            env.storage()
                .persistent()
                .remove(&DataKey::StakeAmount(staker.clone()));
            env.storage()
                .persistent()
                .remove(&DataKey::StakeTime(staker.clone()));
        } else {
            env.storage()
                .persistent()
                .set(&DataKey::StakeAmount(staker.clone()), &remaining);
        }

        // Update global total
        let global: u64 = env
            .storage()
            .instance()
            .get(&DataKey::TotalStaked)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::TotalStaked, &(global.saturating_sub(amount)));

        env.events()
            .publish((symbol_short!("unstake"),), (&staker, amount));

        Ok(())
    }

    // ---------------------------------------------------------------
    //  Claim Rewards
    // ---------------------------------------------------------------

    /// Calculate and return the pending reward for `staker`.
    /// Formula: reward = staked_amount * reward_rate * elapsed_seconds / 10_000
    /// In a production contract this would transfer tokens; here it
    /// returns the computed value and resets the stake timestamp so the
    /// debugger can show the storage diff clearly.
    pub fn claim_rewards(env: Env, staker: Address) -> Result<u64, StakeError> {
        staker.require_auth();

        let staked: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::StakeAmount(staker.clone()))
            .ok_or(StakeError::NoStake)?;

        let stake_time: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::StakeTime(staker.clone()))
            .ok_or(StakeError::NoStake)?;

        let rate: u64 = env
            .storage()
            .instance()
            .get(&DataKey::RewardRate)
            .unwrap_or(0);

        let now = env.ledger().timestamp();
        let elapsed = now.saturating_sub(stake_time);
        let reward = staked.saturating_mul(rate).saturating_mul(elapsed) / 10_000;

        // Reset timestamp so next claim period starts now
        env.storage()
            .persistent()
            .set(&DataKey::StakeTime(staker.clone()), &now);

        env.events()
            .publish((symbol_short!("claim"),), (&staker, reward, now));

        Ok(reward)
    }

    // ---------------------------------------------------------------
    //  Read-only queries
    // ---------------------------------------------------------------

    /// Return the staked balance for `staker`.
    pub fn get_balance(env: Env, staker: Address) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::StakeAmount(staker))
            .unwrap_or(0)
    }

    /// Return the total tokens staked across all stakers.
    pub fn total_staked(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::TotalStaked)
            .unwrap_or(0)
    }

    /// Return the unix timestamp when `staker` last staked / claimed.
    pub fn stake_timestamp(env: Env, staker: Address) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::StakeTime(staker))
            .unwrap_or(0)
    }
}

// ===================================================================
//  Tests
// ===================================================================
#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{testutils::{Address as _, Ledger}, Env};

    fn setup() -> (Env, StakingContractClient<'static>, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(StakingContract, ());
        let client = StakingContractClient::new(&env, &contract_id);
        client.initialize(&10u64); // 10 bps / second reward rate
        let staker = Address::generate(&env);
        (env, client, staker)
    }

    #[test]
    fn test_stake_and_balance() {
        let (_env, client, staker) = setup();
        client.stake(&staker, &1000u64);
        assert_eq!(client.get_balance(&staker), 1000);
        assert_eq!(client.total_staked(), 1000);
    }

    #[test]
    fn test_stake_multiple_times() {
        let (_env, client, staker) = setup();
        client.stake(&staker, &500u64);
        client.stake(&staker, &300u64);
        assert_eq!(client.get_balance(&staker), 800);
        assert_eq!(client.total_staked(), 800);
    }

    #[test]
    fn test_unstake_partial() {
        let (_env, client, staker) = setup();
        client.stake(&staker, &1000u64);
        client.unstake(&staker, &400u64);
        assert_eq!(client.get_balance(&staker), 600);
        assert_eq!(client.total_staked(), 600);
    }

    #[test]
    fn test_unstake_full_removes_storage() {
        let (_env, client, staker) = setup();
        client.stake(&staker, &1000u64);
        client.unstake(&staker, &1000u64);
        assert_eq!(client.get_balance(&staker), 0);
        assert_eq!(client.total_staked(), 0);
    }

    #[test]
    fn test_claim_rewards() {
        let (env, client, staker) = setup();
        client.stake(&staker, &10_000u64);

        // Advance ledger time by 100 seconds
        env.ledger().with_mut(|l| l.timestamp = l.timestamp + 100);

        // reward = 10_000 * 10 * 100 / 10_000 = 1_000
        let reward = client.claim_rewards(&staker);
        assert_eq!(reward, 1_000);
    }

    #[test]
    fn test_zero_stake_errors() {
        let (_env, client, staker) = setup();
        let result = client.try_stake(&staker, &0u64);
        assert!(result.is_err());
    }

    #[test]
    fn test_unstake_more_than_staked_errors() {
        let (_env, client, staker) = setup();
        client.stake(&staker, &100u64);
        let result = client.try_unstake(&staker, &200u64);
        assert!(result.is_err());
    }
}