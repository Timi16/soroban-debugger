#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, Map, String,
    Symbol,
};

/// Storage keys for the NFT contract.
#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    /// Contract administrator
    Admin,
    /// Owner of a given token ID
    Owner(u64),
    /// Metadata map for a given token ID
    Meta(u64),
    /// Next token ID to mint
    NextId,
    /// Total supply of tokens currently in existence
    Supply,
}

/// Errors returned by the NFT contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum NftError {
    /// Token ID does not exist
    NotFound = 1,
    /// Caller is not authorized for this action
    NotAuthorized = 2,
    /// Caller is not the contract admin
    NotAdmin = 3,
    /// Cannot transfer to the current owner
    SelfTransfer = 4,
}

#[contract]
pub struct NftContract;

#[contractimpl]
impl NftContract {
    // ---------------------------------------------------------------
    //  Admin
    // ---------------------------------------------------------------

    /// Initialize the contract, setting the admin address.
    /// Must be called once before any other function.
    pub fn initialize(env: Env, admin: Address) {
        if env
            .storage()
            .instance()
            .has(&DataKey::Admin)
        {
            panic!("already initialized");
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::NextId, &1u64);
        env.storage().instance().set(&DataKey::Supply, &0u64);
    }

    // ---------------------------------------------------------------
    //  Mint
    // ---------------------------------------------------------------

    /// Mint a new NFT. Only the admin may call this.
    /// `to`   – the address that will own the token.
    /// `name` – human-readable name stored in metadata.
    /// `desc` – description stored in metadata.
    ///
    /// Returns the newly minted token ID.
    pub fn mint(env: Env, to: Address, name: String, desc: String) -> u64 {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("not initialized");
        admin.require_auth();

        // Allocate a new token ID
        let token_id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::NextId)
            .unwrap_or(1);

        // Set owner
        env.storage()
            .persistent()
            .set(&DataKey::Owner(token_id), &to);

        // Build metadata map
        let mut meta = Map::<Symbol, String>::new(&env);
        meta.set(symbol_short!("name"), name);
        meta.set(symbol_short!("desc"), desc);
        env.storage()
            .persistent()
            .set(&DataKey::Meta(token_id), &meta);

        // Bump counters
        env.storage()
            .instance()
            .set(&DataKey::NextId, &(token_id + 1));

        let supply: u64 = env
            .storage()
            .instance()
            .get(&DataKey::Supply)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::Supply, &(supply + 1));

        // Emit event
        env.events()
            .publish((symbol_short!("mint"),), (token_id, &to));

        token_id
    }

    // ---------------------------------------------------------------
    //  Transfer
    // ---------------------------------------------------------------

    /// Transfer a token from the current owner to a new address.
    /// The current owner must authorize the call.
    pub fn transfer(env: Env, token_id: u64, to: Address) -> Result<(), NftError> {
        let owner: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Owner(token_id))
            .ok_or(NftError::NotFound)?;

        owner.require_auth();

        if owner == to {
            return Err(NftError::SelfTransfer);
        }

        env.storage()
            .persistent()
            .set(&DataKey::Owner(token_id), &to);

        env.events()
            .publish((symbol_short!("xfer"),), (token_id, &owner, &to));

        Ok(())
    }

    // ---------------------------------------------------------------
    //  Burn
    // ---------------------------------------------------------------

    /// Burn (destroy) a token. The current owner must authorize the call.
    pub fn burn(env: Env, token_id: u64) -> Result<(), NftError> {
        let owner: Address = env
            .storage()
            .persistent()
            .get(&DataKey::Owner(token_id))
            .ok_or(NftError::NotFound)?;

        owner.require_auth();

        env.storage().persistent().remove(&DataKey::Owner(token_id));
        env.storage().persistent().remove(&DataKey::Meta(token_id));

        let supply: u64 = env
            .storage()
            .instance()
            .get(&DataKey::Supply)
            .unwrap_or(1);
        env.storage()
            .instance()
            .set(&DataKey::Supply, &(supply.saturating_sub(1)));

        env.events()
            .publish((symbol_short!("burn"),), (token_id, &owner));

        Ok(())
    }

    // ---------------------------------------------------------------
    //  Read-only queries
    // ---------------------------------------------------------------

    /// Return the owner of the given token.
    pub fn owner_of(env: Env, token_id: u64) -> Result<Address, NftError> {
        env.storage()
            .persistent()
            .get(&DataKey::Owner(token_id))
            .ok_or(NftError::NotFound)
    }

    /// Return the metadata map for the given token.
    pub fn metadata(env: Env, token_id: u64) -> Result<Map<Symbol, String>, NftError> {
        env.storage()
            .persistent()
            .get(&DataKey::Meta(token_id))
            .ok_or(NftError::NotFound)
    }

    /// Return the current total supply.
    pub fn total_supply(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::Supply)
            .unwrap_or(0)
    }
}

// ===================================================================
//  Tests
// ===================================================================
#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Env, String};

    fn setup() -> (Env, NftContractClient<'static>, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(NftContract, ());
        let client = NftContractClient::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let user = Address::generate(&env);

        client.initialize(&admin);
        (env, client, admin, user)
    }

    #[test]
    fn test_mint_and_query() {
        let (env, client, _admin, user) = setup();
        let name = String::from_str(&env, "Cool NFT");
        let desc = String::from_str(&env, "A very cool NFT");

        let id = client.mint(&user, &name, &desc);
        assert_eq!(id, 1);
        assert_eq!(client.owner_of(&id), user);
        assert_eq!(client.total_supply(), 1);

        let meta = client.metadata(&id);
        assert_eq!(meta.get(symbol_short!("name")).unwrap(), name);
        assert_eq!(meta.get(symbol_short!("desc")).unwrap(), desc);
    }

    #[test]
    fn test_transfer() {
        let (env, client, _admin, user) = setup();
        let name = String::from_str(&env, "Transfer NFT");
        let desc = String::from_str(&env, "Will be transferred");

        let id = client.mint(&user, &name, &desc);

        let new_owner = Address::generate(&env);
        client.transfer(&id, &new_owner);

        assert_eq!(client.owner_of(&id), new_owner);
        assert_eq!(client.total_supply(), 1);
    }

    #[test]
    fn test_burn() {
        let (env, client, _admin, user) = setup();
        let name = String::from_str(&env, "Burn NFT");
        let desc = String::from_str(&env, "Will be burned");

        let id = client.mint(&user, &name, &desc);
        assert_eq!(client.total_supply(), 1);

        client.burn(&id);
        assert_eq!(client.total_supply(), 0);
    }

    #[test]
    fn test_mint_multiple() {
        let (env, client, _admin, user) = setup();
        let id1 = client.mint(
            &user,
            &String::from_str(&env, "NFT 1"),
            &String::from_str(&env, "First"),
        );
        let id2 = client.mint(
            &user,
            &String::from_str(&env, "NFT 2"),
            &String::from_str(&env, "Second"),
        );

        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        assert_eq!(client.total_supply(), 2);
    }
}
