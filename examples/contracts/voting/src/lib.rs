#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, String, Map};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Proposal(u32),
    Votes(u32), // Map<Address, bool>
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Proposal {
    pub id: u32,
    pub title: String,
    pub creator: Address,
    pub closed: bool,
}

#[contract]
pub struct VotingContract;

#[contractimpl]
impl VotingContract {
    /// Create a new proposal
    pub fn create_proposal(env: Env, creator: Address, id: u32, title: String) {
        creator.require_auth();
        
        let key = DataKey::Proposal(id);
        if env.storage().persistent().has(&key) {
            panic!("Proposal already exists");
        }

        let proposal = Proposal {
            id,
            title,
            creator,
            closed: false,
        };

        env.storage().persistent().set(&key, &proposal);
        
        // Initialize empty votes map
        let votes: Map<Address, bool> = Map::new(&env);
        env.storage().persistent().set(&DataKey::Votes(id), &votes);
    }

    /// Cast a vote for a proposal
    pub fn vote(env: Env, voter: Address, proposal_id: u32, support: bool) {
        voter.require_auth();

        let prop_key = DataKey::Proposal(proposal_id);
        let proposal: Proposal = env.storage().persistent().get(&prop_key).expect("Proposal not found");
        
        if proposal.closed {
            panic!("Proposal is closed");
        }

        let votes_key = DataKey::Votes(proposal_id);
        let mut votes: Map<Address, bool> = env.storage().persistent().get(&votes_key).expect("Votes map not found");
        
        votes.set(voter, support);
        env.storage().persistent().set(&votes_key, &votes);
    }

    /// Tally the votes for a proposal
    pub fn tally(env: Env, proposal_id: u32) -> (u32, u32) {
        let votes_key = DataKey::Votes(proposal_id);
        let votes: Map<Address, bool> = env.storage().persistent().get(&votes_key).expect("Votes map not found");
        
        let mut yays = 0;
        let mut nays = 0;

        for res in votes.iter() {
            let (_voter, support) = res;
            if support {
                yays += 1;
            } else {
                nays += 1;
            }
        }

        (yays, nays)
    }

    /// Close a proposal
    pub fn close(env: Env, creator: Address, proposal_id: u32) {
        creator.require_auth();

        let prop_key = DataKey::Proposal(proposal_id);
        let mut proposal: Proposal = env.storage().persistent().get(&prop_key).expect("Proposal not found");
        
        if proposal.creator != creator {
            panic!("Only creator can close proposal");
        }

        proposal.closed = true;
        env.storage().persistent().set(&prop_key, &proposal);
    }
}
