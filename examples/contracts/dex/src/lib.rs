#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, Env};

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    ReserveA,
    ReserveB,
}

#[contract]
pub struct DexContract;

#[contractimpl]
impl DexContract {
    pub fn add_liquidity(env: Env, amount_a: i128, amount_b: i128) {
        let reserve_a: i128 = env
            .storage()
            .instance()
            .get(&DataKey::ReserveA)
            .unwrap_or(0);
        let reserve_b: i128 = env
            .storage()
            .instance()
            .get(&DataKey::ReserveB)
            .unwrap_or(0);

        env.storage()
            .instance()
            .set(&DataKey::ReserveA, &(reserve_a + amount_a));
        env.storage()
            .instance()
            .set(&DataKey::ReserveB, &(reserve_b + amount_b));
    }

    pub fn remove_liquidity(env: Env, amount_a: i128, amount_b: i128) {
        let reserve_a: i128 = env
            .storage()
            .instance()
            .get(&DataKey::ReserveA)
            .unwrap_or(0);
        let reserve_b: i128 = env
            .storage()
            .instance()
            .get(&DataKey::ReserveB)
            .unwrap_or(0);

        env.storage()
            .instance()
            .set(&DataKey::ReserveA, &(reserve_a - amount_a));
        env.storage()
            .instance()
            .set(&DataKey::ReserveB, &(reserve_b - amount_b));
    }

    pub fn swap(env: Env, token_in: bool, amount_in: i128) -> i128 {
        let reserve_a: i128 = env
            .storage()
            .instance()
            .get(&DataKey::ReserveA)
            .unwrap_or(0);
        let reserve_b: i128 = env
            .storage()
            .instance()
            .get(&DataKey::ReserveB)
            .unwrap_or(0);

        let (reserve_in, reserve_out) = if token_in {
            (reserve_a, reserve_b)
        } else {
            (reserve_b, reserve_a)
        };

        let amount_out = (reserve_out * amount_in) / (reserve_in + amount_in);

        if token_in {
            env.storage()
                .instance()
                .set(&DataKey::ReserveA, &(reserve_a + amount_in));
            env.storage()
                .instance()
                .set(&DataKey::ReserveB, &(reserve_b - amount_out));
        } else {
            env.storage()
                .instance()
                .set(&DataKey::ReserveA, &(reserve_a - amount_out));
            env.storage()
                .instance()
                .set(&DataKey::ReserveB, &(reserve_b + amount_in));
        }

        amount_out
    }

    pub fn get_price(env: Env, token_in: bool) -> (i128, i128) {
        let reserve_a: i128 = env
            .storage()
            .instance()
            .get(&DataKey::ReserveA)
            .unwrap_or(0);
        let reserve_b: i128 = env
            .storage()
            .instance()
            .get(&DataKey::ReserveB)
            .unwrap_or(0);

        if token_in {
            (reserve_b, reserve_a)
        } else {
            (reserve_a, reserve_b)
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::Env;

    #[test]
    fn test_liquidity_functions() {
        let env = Env::default();
        let contract_id = env.register(DexContract, ());
        let client = DexContractClient::new(&env, &contract_id);

        client.add_liquidity(&1000, &2000);
        let (price_num, price_denom) = client.get_price(&true);
        assert_eq!(price_num, 2000);
        assert_eq!(price_denom, 1000);

        client.remove_liquidity(&100, &200);
        let (price_num, price_denom) = client.get_price(&true);
        assert_eq!(price_num, 1800);
        assert_eq!(price_denom, 900);
    }

    #[test]
    fn test_swap_function() {
        let env = Env::default();
        let contract_id = env.register(DexContract, ());
        let client = DexContractClient::new(&env, &contract_id);

        client.add_liquidity(&1000, &2000);

        let amount_out = client.swap(&true, &100);
        assert_eq!(amount_out, 181);

        let (reserve_b, reserve_a) = client.get_price(&true);
        assert_eq!(reserve_a, 1100);
        assert_eq!(reserve_b, 1819);
    }
}
