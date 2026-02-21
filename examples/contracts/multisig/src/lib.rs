#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, Vec,
};

// -------------------------------------------------------------------
//  Storage Keys
// -------------------------------------------------------------------

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    /// Configuration: required number of approvals (M)
    RequiredApprovals,
    /// Configuration: list of authorized signers (N addresses)
    Signers,
    /// Transaction proposal counter
    ProposalCounter,
    /// Proposal details by ID
    Proposal(u64),
    /// Approvals for a proposal: Vec<Address>
    Approvals(u64),
    /// Whether a proposal has been executed
    Executed(u64),
}

// -------------------------------------------------------------------
//  Data Types
// -------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Proposal {
    pub id: u64,
    pub proposer: Address,
    pub target: Address,
    pub function_name: soroban_sdk::Symbol,
    pub description: soroban_sdk::String,
}

// -------------------------------------------------------------------
//  Errors
// -------------------------------------------------------------------

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum MultisigError {
    /// Contract has already been initialized
    AlreadyInitialized = 1,
    /// Caller is not an authorized signer
    NotASigner = 2,
    /// Proposal does not exist
    ProposalNotFound = 3,
    /// Proposal has already been executed
    AlreadyExecuted = 4,
    /// Not enough approvals to execute
    InsufficientApprovals = 5,
    /// Signer has already approved this proposal
    AlreadyApproved = 6,
    /// Invalid configuration (M must be > 0 and <= N)
    InvalidConfig = 7,
    /// Signer has not approved this proposal
    NotApproved = 8,
}

// -------------------------------------------------------------------
//  Contract
// -------------------------------------------------------------------

#[contract]
pub struct MultisigWallet;

#[contractimpl]
impl MultisigWallet {
    // ---------------------------------------------------------------
    //  Init
    // ---------------------------------------------------------------

    /// Initialize the multisig wallet with M-of-N configuration.
    /// `required_approvals`: M (minimum approvals needed)
    /// `signers`: N authorized addresses
    pub fn initialize(
        env: Env,
        required_approvals: u32,
        signers: Vec<Address>,
    ) -> Result<(), MultisigError> {
        if env.storage().instance().has(&DataKey::RequiredApprovals) {
            return Err(MultisigError::AlreadyInitialized);
        }

        let signer_count = signers.len();
        if required_approvals == 0 || required_approvals > signer_count {
            return Err(MultisigError::InvalidConfig);
        }

        env.storage()
            .instance()
            .set(&DataKey::RequiredApprovals, &required_approvals);
        env.storage().instance().set(&DataKey::Signers, &signers);
        env.storage().instance().set(&DataKey::ProposalCounter, &0u64);

        env.events().publish(
            (symbol_short!("init"),),
            (required_approvals, signer_count),
        );

        Ok(())
    }

    // ---------------------------------------------------------------
    //  Propose
    // ---------------------------------------------------------------

    /// Propose a new transaction. Caller must be an authorized signer.
    pub fn propose(
        env: Env,
        proposer: Address,
        target: Address,
        function_name: soroban_sdk::Symbol,
        description: soroban_sdk::String,
    ) -> Result<u64, MultisigError> {
        proposer.require_auth();

        // Verify proposer is a signer
        let signers: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::Signers)
            .unwrap();

        if !Self::is_signer(&signers, &proposer) {
            return Err(MultisigError::NotASigner);
        }

        // Increment proposal counter
        let mut counter: u64 = env
            .storage()
            .instance()
            .get(&DataKey::ProposalCounter)
            .unwrap_or(0);
        counter += 1;

        let proposal = Proposal {
            id: counter,
            proposer: proposer.clone(),
            target,
            function_name: function_name.clone(),
            description: description.clone(),
        };

        env.storage()
            .persistent()
            .set(&DataKey::Proposal(counter), &proposal);
        env.storage()
            .instance()
            .set(&DataKey::ProposalCounter, &counter);

        // Initialize empty approvals list
        let approvals: Vec<Address> = Vec::new(&env);
        env.storage()
            .persistent()
            .set(&DataKey::Approvals(counter), &approvals);

        env.events().publish(
            (symbol_short!("propose"),),
            (counter, &proposer, &function_name),
        );

        Ok(counter)
    }

    // ---------------------------------------------------------------
    //  Approve
    // ---------------------------------------------------------------

    /// Approve a proposal. Caller must be an authorized signer.
    pub fn approve(env: Env, approver: Address, proposal_id: u64) -> Result<(), MultisigError> {
        approver.require_auth();

        // Verify approver is a signer
        let signers: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::Signers)
            .unwrap();

        if !Self::is_signer(&signers, &approver) {
            return Err(MultisigError::NotASigner);
        }

        // Check proposal exists
        if !env
            .storage()
            .persistent()
            .has(&DataKey::Proposal(proposal_id))
        {
            return Err(MultisigError::ProposalNotFound);
        }

        // Check not already executed
        let executed: bool = env
            .storage()
            .persistent()
            .get(&DataKey::Executed(proposal_id))
            .unwrap_or(false);

        if executed {
            return Err(MultisigError::AlreadyExecuted);
        }

        // Get current approvals
        let mut approvals: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Approvals(proposal_id))
            .unwrap();

        // Check if already approved
        if Self::has_approved(&approvals, &approver) {
            return Err(MultisigError::AlreadyApproved);
        }

        // Add approval
        approvals.push_back(approver.clone());
        env.storage()
            .persistent()
            .set(&DataKey::Approvals(proposal_id), &approvals);

        env.events().publish(
            (symbol_short!("approve"),),
            (proposal_id, &approver, approvals.len()),
        );

        Ok(())
    }

    // ---------------------------------------------------------------
    //  Revoke
    // ---------------------------------------------------------------

    /// Revoke a previous approval. Caller must be an authorized signer.
    pub fn revoke(env: Env, revoker: Address, proposal_id: u64) -> Result<(), MultisigError> {
        revoker.require_auth();

        // Verify revoker is a signer
        let signers: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::Signers)
            .unwrap();

        if !Self::is_signer(&signers, &revoker) {
            return Err(MultisigError::NotASigner);
        }

        // Check proposal exists
        if !env
            .storage()
            .persistent()
            .has(&DataKey::Proposal(proposal_id))
        {
            return Err(MultisigError::ProposalNotFound);
        }

        // Check not already executed
        let executed: bool = env
            .storage()
            .persistent()
            .get(&DataKey::Executed(proposal_id))
            .unwrap_or(false);

        if executed {
            return Err(MultisigError::AlreadyExecuted);
        }

        // Get current approvals
        let approvals: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Approvals(proposal_id))
            .unwrap();

        // Find and remove the approval
        let mut found = false;
        let mut new_approvals: Vec<Address> = Vec::new(&env);

        for i in 0..approvals.len() {
            let addr = approvals.get(i).unwrap();
            if addr == revoker {
                found = true;
            } else {
                new_approvals.push_back(addr);
            }
        }

        if !found {
            return Err(MultisigError::NotApproved);
        }

        env.storage()
            .persistent()
            .set(&DataKey::Approvals(proposal_id), &new_approvals);

        env.events().publish(
            (symbol_short!("revoke"),),
            (proposal_id, &revoker, new_approvals.len()),
        );

        Ok(())
    }

    // ---------------------------------------------------------------
    //  Execute
    // ---------------------------------------------------------------

    /// Execute a proposal if it has enough approvals.
    /// Any signer can trigger execution once threshold is met.
    pub fn execute(env: Env, executor: Address, proposal_id: u64) -> Result<(), MultisigError> {
        executor.require_auth();

        // Verify executor is a signer
        let signers: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::Signers)
            .unwrap();

        if !Self::is_signer(&signers, &executor) {
            return Err(MultisigError::NotASigner);
        }

        // Check proposal exists
        let proposal: Proposal = env
            .storage()
            .persistent()
            .get(&DataKey::Proposal(proposal_id))
            .ok_or(MultisigError::ProposalNotFound)?;

        // Check not already executed
        let executed: bool = env
            .storage()
            .persistent()
            .get(&DataKey::Executed(proposal_id))
            .unwrap_or(false);

        if executed {
            return Err(MultisigError::AlreadyExecuted);
        }

        // Check approval threshold
        let approvals: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Approvals(proposal_id))
            .unwrap();

        let required: u32 = env
            .storage()
            .instance()
            .get(&DataKey::RequiredApprovals)
            .unwrap();

        if approvals.len() < required {
            return Err(MultisigError::InsufficientApprovals);
        }

        // Mark as executed
        env.storage()
            .persistent()
            .set(&DataKey::Executed(proposal_id), &true);

        env.events().publish(
            (symbol_short!("execute"),),
            (proposal_id, &executor, &proposal.target),
        );

        Ok(())
    }

    // ---------------------------------------------------------------
    //  Read-only queries
    // ---------------------------------------------------------------

    /// Get proposal details
    pub fn get_proposal(env: Env, proposal_id: u64) -> Option<Proposal> {
        env.storage()
            .persistent()
            .get(&DataKey::Proposal(proposal_id))
    }

    /// Get approval count for a proposal
    pub fn get_approval_count(env: Env, proposal_id: u64) -> u32 {
        let approvals: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Approvals(proposal_id))
            .unwrap_or(Vec::new(&env));
        approvals.len()
    }

    /// Get list of approvers for a proposal
    pub fn get_approvals(env: Env, proposal_id: u64) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&DataKey::Approvals(proposal_id))
            .unwrap_or(Vec::new(&env))
    }

    /// Check if a proposal has been executed
    pub fn is_executed(env: Env, proposal_id: u64) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::Executed(proposal_id))
            .unwrap_or(false)
    }

    /// Get required approval count (M)
    pub fn get_required_approvals(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::RequiredApprovals)
            .unwrap_or(0)
    }

    /// Get list of authorized signers (N)
    pub fn get_signers(env: Env) -> Vec<Address> {
        env.storage()
            .instance()
            .get(&DataKey::Signers)
            .unwrap_or(Vec::new(&env))
    }

    /// Get total number of proposals created
    pub fn get_proposal_count(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::ProposalCounter)
            .unwrap_or(0)
    }

    // ---------------------------------------------------------------
    //  Helper functions
    // ---------------------------------------------------------------

    fn is_signer(signers: &Vec<Address>, address: &Address) -> bool {
        for i in 0..signers.len() {
            if signers.get(i).unwrap() == *address {
                return true;
            }
        }
        false
    }

    fn has_approved(approvals: &Vec<Address>, address: &Address) -> bool {
        for i in 0..approvals.len() {
            if approvals.get(i).unwrap() == *address {
                return true;
            }
        }
        false
    }
}

// ===================================================================
//  Tests
// ===================================================================
#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{
        symbol_short, testutils::Address as _, vec, Env, String as SorobanString,
    };

    fn setup() -> (
        Env,
        MultisigWalletClient<'static>,
        Address,
        Address,
        Address,
    ) {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register(MultisigWallet, ());
        let client = MultisigWalletClient::new(&env, &contract_id);

        let signer1 = Address::generate(&env);
        let signer2 = Address::generate(&env);
        let signer3 = Address::generate(&env);

        let signers = vec![&env, signer1.clone(), signer2.clone(), signer3.clone()];
        client.initialize(&2, &signers); // 2-of-3 multisig

        (env, client, signer1, signer2, signer3)
    }

    #[test]
    fn test_initialize() {
        let (_env, client, _signer1, _signer2, _signer3) = setup();
        assert_eq!(client.get_required_approvals(), 2);
        assert_eq!(client.get_signers().len(), 3);
    }

    #[test]
    fn test_propose() {
        let (env, client, signer1, _, _) = setup();
        let target = Address::generate(&env);
        let proposal_id = client.propose(
            &signer1,
            &target,
            &symbol_short!("transfer"),
            &SorobanString::from_str(&env, "Send 100 tokens"),
        );
        assert_eq!(proposal_id, 1);
        assert_eq!(client.get_proposal_count(), 1);
    }

    #[test]
    fn test_approve_and_count() {
        let (env, client, signer1, signer2, _) = setup();
        let target = Address::generate(&env);
        let proposal_id = client.propose(
            &signer1,
            &target,
            &symbol_short!("transfer"),
            &SorobanString::from_str(&env, "Send 100 tokens"),
        );

        client.approve(&signer1, &proposal_id);
        assert_eq!(client.get_approval_count(&proposal_id), 1);

        client.approve(&signer2, &proposal_id);
        assert_eq!(client.get_approval_count(&proposal_id), 2);
    }

    #[test]
    fn test_execute_with_sufficient_approvals() {
        let (env, client, signer1, signer2, _) = setup();
        let target = Address::generate(&env);
        let proposal_id = client.propose(
            &signer1,
            &target,
            &symbol_short!("transfer"),
            &SorobanString::from_str(&env, "Send 100 tokens"),
        );

        client.approve(&signer1, &proposal_id);
        client.approve(&signer2, &proposal_id);

        client.execute(&signer1, &proposal_id);
        assert!(client.is_executed(&proposal_id));
    }

    #[test]
    fn test_execute_fails_without_enough_approvals() {
        let (env, client, signer1, _, _) = setup();
        let target = Address::generate(&env);
        let proposal_id = client.propose(
            &signer1,
            &target,
            &symbol_short!("transfer"),
            &SorobanString::from_str(&env, "Send 100 tokens"),
        );

        client.approve(&signer1, &proposal_id);

        let result = client.try_execute(&signer1, &proposal_id);
        assert!(result.is_err());
    }

    #[test]
    fn test_revoke_approval() {
        let (env, client, signer1, signer2, _) = setup();
        let target = Address::generate(&env);
        let proposal_id = client.propose(
            &signer1,
            &target,
            &symbol_short!("transfer"),
            &SorobanString::from_str(&env, "Send 100 tokens"),
        );

        client.approve(&signer1, &proposal_id);
        client.approve(&signer2, &proposal_id);
        assert_eq!(client.get_approval_count(&proposal_id), 2);

        client.revoke(&signer1, &proposal_id);
        assert_eq!(client.get_approval_count(&proposal_id), 1);
    }

    #[test]
    fn test_cannot_approve_twice() {
        let (env, client, signer1, _, _) = setup();
        let target = Address::generate(&env);
        let proposal_id = client.propose(
            &signer1,
            &target,
            &symbol_short!("transfer"),
            &SorobanString::from_str(&env, "Send 100 tokens"),
        );

        client.approve(&signer1, &proposal_id);
        let result = client.try_approve(&signer1, &proposal_id);
        assert!(result.is_err());
    }

    #[test]
    fn test_cannot_execute_twice() {
        let (env, client, signer1, signer2, _) = setup();
        let target = Address::generate(&env);
        let proposal_id = client.propose(
            &signer1,
            &target,
            &symbol_short!("transfer"),
            &SorobanString::from_str(&env, "Send 100 tokens"),
        );

        client.approve(&signer1, &proposal_id);
        client.approve(&signer2, &proposal_id);
        client.execute(&signer1, &proposal_id);

        let result = client.try_execute(&signer1, &proposal_id);
        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_proposals() {
        let (env, client, signer1, signer2, signer3) = setup();
        let target = Address::generate(&env);

        let prop1 = client.propose(
            &signer1,
            &target,
            &symbol_short!("transfer"),
            &SorobanString::from_str(&env, "Proposal 1"),
        );
        let prop2 = client.propose(
            &signer2,
            &target,
            &symbol_short!("mint"),
            &SorobanString::from_str(&env, "Proposal 2"),
        );

        assert_eq!(prop1, 1);
        assert_eq!(prop2, 2);
        assert_eq!(client.get_proposal_count(), 2);

        // Approve and execute first proposal
        client.approve(&signer1, &prop1);
        client.approve(&signer2, &prop1);
        client.execute(&signer3, &prop1);

        // Second proposal still pending
        assert!(client.is_executed(&prop1));
        assert!(!client.is_executed(&prop2));
    }
}
