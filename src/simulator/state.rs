//! Network and ledger state snapshot schema
//!
//! This module defines the strongly-typed schema for network snapshots.
//! Snapshots represent the complete state of the Soroban ledger at a specific point,
//! including ledger metadata, accounts, and deployed contracts.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

/// Error type for simulator operations
#[derive(Debug, Error)]
pub enum SimulatorError {
    #[error("Invalid account address: {0}")]
    InvalidAddress(String),

    #[error("Invalid contract ID: {0}")]
    InvalidContractId(String),

    #[error("Invalid balance format: {0}")]
    InvalidBalance(String),

    #[error("Invalid ledger sequence: {0}")]
    InvalidLedgerSequence(String),

    #[error("Snapshot validation failed: {0}")]
    ValidationError(String),

    #[error("Contract not found: {0}")]
    ContractNotFound(String),

    #[error("Account not found: {0}")]
    AccountNotFound(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("JSON serialization error: {0}")]
    JsonError(#[from] serde_json::Error),
}

/// Complete network snapshot
///
/// Represents the entire state of a Soroban ledger instance, including
/// metadata, accounts, and deployed contracts. This can be persisted to
/// JSON and restored to recreate a specific ledger state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkSnapshot {
    /// Ledger metadata
    pub ledger: LedgerMetadata,

    /// Account states
    pub accounts: Vec<AccountState>,

    /// Deployed contracts
    pub contracts: Vec<ContractState>,
}

impl NetworkSnapshot {
    /// Create a new empty snapshot with default metadata
    pub fn new(sequence: u32, network_passphrase: impl Into<String>, timestamp: u64) -> Self {
        Self {
            ledger: LedgerMetadata {
                sequence,
                timestamp,
                network_passphrase: network_passphrase.into(),
            },
            accounts: Vec::new(),
            contracts: Vec::new(),
        }
    }

    /// Validate the snapshot for semantic correctness
    pub fn validate(&self) -> crate::Result<()> {
        // Validate ledger metadata
        self.ledger.validate()?;

        // Validate all accounts
        let mut addresses = std::collections::HashSet::new();
        for account in &self.accounts {
            account.validate()?;

            if !addresses.insert(&account.address) {
                return Err(SimulatorError::ValidationError(format!(
                    "Duplicate account address: {}",
                    account.address
                ))
                .into());
            }
        }

        // Validate all contracts
        let mut contract_ids = std::collections::HashSet::new();
        for contract in &self.contracts {
            contract.validate()?;

            if !contract_ids.insert(&contract.contract_id) {
                return Err(SimulatorError::ValidationError(format!(
                    "Duplicate contract ID: {}",
                    contract.contract_id
                ))
                .into());
            }
        }

        Ok(())
    }

    /// Find account by address
    pub fn get_account(&self, address: &str) -> Option<&AccountState> {
        self.accounts.iter().find(|a| a.address == address)
    }

    /// Find account by address (mutable)
    pub fn get_account_mut(&mut self, address: &str) -> Option<&mut AccountState> {
        self.accounts.iter_mut().find(|a| a.address == address)
    }

    /// Find contract by ID
    pub fn get_contract(&self, contract_id: &str) -> Option<&ContractState> {
        self.contracts.iter().find(|c| c.contract_id == contract_id)
    }

    /// Find contract by ID (mutable)
    pub fn get_contract_mut(&mut self, contract_id: &str) -> Option<&mut ContractState> {
        self.contracts
            .iter_mut()
            .find(|c| c.contract_id == contract_id)
    }

    /// Add or update an account
    pub fn add_account(&mut self, account: AccountState) -> crate::Result<()> {
        account.validate()?;

        // Remove existing account with same address if it exists
        self.accounts.retain(|a| a.address != account.address);

        self.accounts.push(account);
        Ok(())
    }

    /// Add or update a contract
    pub fn add_contract(&mut self, contract: ContractState) -> crate::Result<()> {
        contract.validate()?;

        // Remove existing contract with same ID if it exists
        self.contracts
            .retain(|c| c.contract_id != contract.contract_id);

        self.contracts.push(contract);
        Ok(())
    }

    /// Update ledger metadata
    pub fn update_ledger_metadata(&mut self, sequence: u32, timestamp: u64) -> crate::Result<()> {
        self.ledger.sequence = sequence;
        self.ledger.timestamp = timestamp;
        self.ledger.validate()?;
        Ok(())
    }
}

/// Ledger metadata (sequence, timestamp, network info)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerMetadata {
    /// Current ledger sequence number
    pub sequence: u32,

    /// Ledger close timestamp (Unix seconds)
    pub timestamp: u64,

    /// Network passphrase (e.g., "Test SDF Network ; September 2015")
    pub network_passphrase: String,
}

impl LedgerMetadata {
    /// Validate ledger metadata
    fn validate(&self) -> crate::Result<()> {
        if self.sequence == 0 {
            return Err(
                SimulatorError::InvalidLedgerSequence("Sequence must be > 0".to_string()).into(),
            );
        }

        if self.network_passphrase.is_empty() {
            return Err(SimulatorError::ValidationError(
                "Network passphrase cannot be empty".to_string(),
            )
            .into());
        }

        Ok(())
    }
}

/// Account state snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountState {
    /// Stellar account address (e.g., "GBRPYHIL2CI3FV4BMSXIUVQTQA7VLMVROCJ2QC543OUNXHTIPTOUR47")
    pub address: String,

    /// Account balance in stroops (as string to handle large numbers)
    pub balance: String,

    /// Account sequence number for transaction ordering
    pub sequence: u64,

    /// Optional account flags (standard Stellar flags)
    #[serde(default)]
    pub flags: Option<u32>,

    /// Optional account data (string key-value pairs)
    #[serde(default)]
    pub data: Option<BTreeMap<String, String>>,
}

impl AccountState {
    /// Create a new account state
    pub fn new(address: impl Into<String>, balance: impl Into<String>, sequence: u64) -> Self {
        Self {
            address: address.into(),
            balance: balance.into(),
            sequence,
            flags: None,
            data: None,
        }
    }

    /// Validate account state
    fn validate(&self) -> crate::Result<()> {
        // Validate address format (basic check for Stellar addresses)
        if self.address.is_empty() {
            return Err(
                SimulatorError::InvalidAddress("Address cannot be empty".to_string()).into(),
            );
        }

        if !self.address.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(SimulatorError::InvalidAddress(format!(
                "Invalid address format: {}",
                self.address
            ))
            .into());
        }

        // Validate balance is a valid number
        self.balance.parse::<u128>().map_err(|_| {
            anyhow::anyhow!(SimulatorError::InvalidBalance(format!(
                "Balance must be a valid unsigned integer: {}",
                self.balance
            )))
        })?;

        Ok(())
    }

    /// Set account data entry
    pub fn set_data(&mut self, key: String, value: String) {
        if self.data.is_none() {
            self.data = Some(BTreeMap::new());
        }
        if let Some(ref mut data) = self.data {
            data.insert(key, value);
        }
    }

    /// Get account data entry
    pub fn get_data(&self, key: &str) -> Option<&String> {
        self.data.as_ref().and_then(|d| d.get(key))
    }
}

/// Contract state snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractState {
    /// Contract ID (hex string or address)
    pub contract_id: String,

    /// WASM bytecode hash (hex string)
    pub wasm_hash: String,

    /// Contract source reference (path or identifier for WASM)
    #[serde(default)]
    pub wasm_ref: Option<String>,

    /// Contract instance storage (key-value pairs as JSON values)
    #[serde(default)]
    pub storage: BTreeMap<String, serde_json::Value>,
}

impl ContractState {
    /// Create a new contract state
    pub fn new(contract_id: impl Into<String>, wasm_hash: impl Into<String>) -> Self {
        Self {
            contract_id: contract_id.into(),
            wasm_hash: wasm_hash.into(),
            wasm_ref: None,
            storage: BTreeMap::new(),
        }
    }

    /// Validate contract state
    fn validate(&self) -> crate::Result<()> {
        if self.contract_id.is_empty() {
            return Err(SimulatorError::InvalidContractId(
                "Contract ID cannot be empty".to_string(),
            )
            .into());
        }

        if self.wasm_hash.is_empty() {
            return Err(
                SimulatorError::InvalidContractId("WASM hash cannot be empty".to_string()).into(),
            );
        }

        // Validate WASM hash is valid hex
        if !self
            .wasm_hash
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == 'x' || c == 'X')
        {
            return Err(SimulatorError::InvalidContractId(format!(
                "WASM hash must be valid hex: {}",
                self.wasm_hash
            ))
            .into());
        }

        Ok(())
    }

    /// Set a storage entry
    pub fn set_storage(&mut self, key: impl Into<String>, value: serde_json::Value) {
        self.storage.insert(key.into(), value);
    }

    /// Get a storage entry
    pub fn get_storage(&self, key: &str) -> Option<&serde_json::Value> {
        self.storage.get(key)
    }

    /// Set WASM reference
    pub fn set_wasm_ref(&mut self, wasm_ref: impl Into<String>) {
        self.wasm_ref = Some(wasm_ref.into());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_snapshot() {
        let snapshot = NetworkSnapshot::new(100, "Test Network", 1234567890);
        assert_eq!(snapshot.ledger.sequence, 100);
        assert_eq!(snapshot.ledger.network_passphrase, "Test Network");
        assert_eq!(snapshot.ledger.timestamp, 1234567890);
    }

    #[test]
    fn test_account_validation() {
        let mut account = AccountState::new("GABCD123", "1000000", 1);
        assert!(account.validate().is_ok());

        account.balance = "not_a_number".to_string();
        assert!(account.validate().is_err());
    }

    #[test]
    fn test_contract_validation() {
        let contract = ContractState::new(
            "CA7QYNF5GE5XEC4HALXWFVQQ5TQWQ5LF7WMXMEQG7BWHBQV26YCWL5",
            "aabbccdd",
        );
        assert!(contract.validate().is_ok());

        let invalid = ContractState::new(
            "CA7QYNF5GE5XEC4HALXWFVQQ5TQWQ5LF7WMXMEQG7BWHBQV26YCWL5",
            "notatex",
        );
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn test_snapshot_validation_duplicate_accounts() {
        let mut snapshot = NetworkSnapshot::new(100, "Test", 1234567890);
        let acc1 = AccountState::new("GABCD123", "1000000", 1);
        let acc2 = AccountState::new("GABCD123", "2000000", 2);

        snapshot.add_account(acc1).unwrap();
        snapshot.add_account(acc2).unwrap();

        // The second account should replace the first
        assert_eq!(snapshot.accounts.len(), 1);
        assert_eq!(snapshot.accounts[0].balance, "2000000");
    }

    #[test]
    fn test_account_data_operations() {
        let mut account = AccountState::new("GABCD123", "1000000", 1);
        account.set_data("key1".to_string(), "value1".to_string());

        assert_eq!(account.get_data("key1"), Some(&"value1".to_string()));
        assert_eq!(account.get_data("key2"), None);
    }

    #[test]
    fn test_contract_storage_operations() {
        let mut contract = ContractState::new(
            "CA7QYNF5GE5XEC4HALXWFVQQ5TQWQ5LF7WMXMEQG7BWHBQV26YCWL5",
            "aabbccdd",
        );
        contract.set_storage("balance", serde_json::json!(1000));

        assert_eq!(
            contract.get_storage("balance"),
            Some(&serde_json::json!(1000))
        );
    }
}
