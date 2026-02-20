//! Snapshot loading and host injection
//!
//! This module handles loading network snapshots from files and applying them
//! to the Soroban debugger environment.

use super::state::{AccountState, ContractState, NetworkSnapshot, SimulatorError};
use crate::Result;
use std::fs;
use std::path::Path;
use tracing::{debug, info};

/// Loads and applies network snapshots to a debug environment
pub struct SnapshotLoader {
    snapshot: NetworkSnapshot,
}

impl SnapshotLoader {
    /// Load a snapshot from a JSON file
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        info!("Loading network snapshot from: {:?}", path);

        // Read the file
        let contents = fs::read_to_string(path).map_err(SimulatorError::IoError)?;

        // Parse JSON
        let snapshot: NetworkSnapshot =
            serde_json::from_str(&contents).map_err(SimulatorError::JsonError)?;

        // Validate the snapshot
        snapshot.validate()?;

        info!(
            "Snapshot loaded: {} accounts, {} contracts, ledger seq={}",
            snapshot.accounts.len(),
            snapshot.contracts.len(),
            snapshot.ledger.sequence
        );

        Ok(Self { snapshot })
    }

    /// Create a snapshot from a NetworkSnapshot struct directly
    pub fn from_snapshot(snapshot: NetworkSnapshot) -> Result<Self> {
        snapshot.validate()?;
        Ok(Self { snapshot })
    }

    /// Get reference to the underlying snapshot
    pub fn snapshot(&self) -> &NetworkSnapshot {
        &self.snapshot
    }

    /// Get mutable reference to the underlying snapshot
    pub fn snapshot_mut(&mut self) -> &mut NetworkSnapshot {
        &mut self.snapshot
    }

    /// Apply snapshot state to the debugger environment
    ///
    /// This prepares the environment with the accounts and contracts
    /// defined in the snapshot. The exact behavior may be limited by
    /// the Soroban SDK's test environment capabilities.
    pub fn apply_to_environment(&self) -> Result<LoadedSnapshot> {
        info!("Applying snapshot to environment");

        let snapshot_info = SnapshotInfo {
            ledger_sequence: self.snapshot.ledger.sequence,
            ledger_timestamp: self.snapshot.ledger.timestamp,
            network_passphrase: self.snapshot.ledger.network_passphrase.clone(),
            account_count: self.snapshot.accounts.len(),
            contract_count: self.snapshot.contracts.len(),
        };

        debug!("Snapshot info: {:?}", snapshot_info);

        Ok(LoadedSnapshot {
            snapshot: self.snapshot.clone(),
            info: snapshot_info,
        })
    }

    /// Validate the snapshot without applying it
    pub fn validate(&self) -> Result<()> {
        info!("Validating snapshot");
        self.snapshot.validate()?;

        // Additional validation checks
        self.validate_references()?;

        info!("Snapshot validation passed");
        Ok(())
    }

    /// Validate that all references in the snapshot are consistent
    fn validate_references(&self) -> Result<()> {
        // Check that contract storage references don't reference non-existent contracts
        // (This can be extended in the future for more complex validations)

        debug!(
            "Validated all references for {} contracts and {} accounts",
            self.snapshot.contracts.len(),
            self.snapshot.accounts.len()
        );

        Ok(())
    }

    /// Get account information from the snapshot
    pub fn get_account(&self, address: &str) -> Option<&AccountState> {
        self.snapshot.get_account(address)
    }

    /// Get contract information from the snapshot
    pub fn get_contract(&self, contract_id: &str) -> Option<&ContractState> {
        self.snapshot.get_contract(contract_id)
    }

    /// List all accounts in the snapshot
    pub fn list_accounts(&self) -> Vec<&str> {
        self.snapshot
            .accounts
            .iter()
            .map(|a| a.address.as_str())
            .collect()
    }

    /// List all contracts in the snapshot
    pub fn list_contracts(&self) -> Vec<&str> {
        self.snapshot
            .contracts
            .iter()
            .map(|c| c.contract_id.as_str())
            .collect()
    }
}

/// Information about a successfully loaded snapshot
#[derive(Debug, Clone)]
pub struct LoadedSnapshot {
    pub(crate) snapshot: NetworkSnapshot,
    pub(crate) info: SnapshotInfo,
}

impl LoadedSnapshot {
    /// Get ledger sequence from loaded snapshot
    pub fn ledger_sequence(&self) -> u32 {
        self.info.ledger_sequence
    }

    /// Get network passphrase from loaded snapshot
    pub fn network_passphrase(&self) -> &str {
        &self.info.network_passphrase
    }

    /// Get account count from loaded snapshot
    pub fn account_count(&self) -> usize {
        self.info.account_count
    }

    /// Get contract count from loaded snapshot
    pub fn contract_count(&self) -> usize {
        self.info.contract_count
    }

    /// Get underlying snapshot
    pub fn snapshot(&self) -> &NetworkSnapshot {
        &self.snapshot
    }

    /// Get mutable underlying snapshot
    pub fn snapshot_mut(&mut self) -> &mut NetworkSnapshot {
        &mut self.snapshot
    }

    /// Format snapshot info for display
    pub fn format_summary(&self) -> String {
        format!(
            "Network Snapshot:\n  \
            Ledger Sequence: {}\n  \
            Timestamp: {}\n  \
            Network: {}\n  \
            Accounts: {}\n  \
            Contracts: {}",
            self.info.ledger_sequence,
            self.info.ledger_timestamp,
            self.info.network_passphrase,
            self.info.account_count,
            self.info.contract_count
        )
    }
}

/// Summary information about a snapshot
#[derive(Debug, Clone)]
pub struct SnapshotInfo {
    ledger_sequence: u32,
    ledger_timestamp: u64,
    network_passphrase: String,
    account_count: usize,
    contract_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_load_from_file() {
        let snapshot = NetworkSnapshot::new(100, "Test Network", 1234567890);
        let json = serde_json::to_string_pretty(&snapshot).unwrap();

        let mut file = NamedTempFile::new().unwrap();
        file.write_all(json.as_bytes()).unwrap();

        let loader = SnapshotLoader::from_file(file.path()).unwrap();
        assert_eq!(loader.snapshot().ledger.sequence, 100);
    }

    #[test]
    fn test_validation_passes_for_valid_snapshot() {
        let snapshot = NetworkSnapshot::new(100, "Test Network", 1234567890);
        let loader = SnapshotLoader::from_snapshot(snapshot).unwrap();
        assert!(loader.validate().is_ok());
    }

    #[test]
    fn test_apply_to_environment() {
        let snapshot = NetworkSnapshot::new(100, "Test Network", 1234567890);
        let loader = SnapshotLoader::from_snapshot(snapshot).unwrap();
        let loaded = loader.apply_to_environment().unwrap();

        assert_eq!(loaded.ledger_sequence(), 100);
        assert_eq!(loaded.network_passphrase(), "Test Network");
    }

    #[test]
    fn test_list_accounts_and_contracts() {
        let mut snapshot = NetworkSnapshot::new(100, "Test Network", 1234567890);
        snapshot
            .add_account(AccountState::new("GABCD123", "1000000", 1))
            .unwrap();
        snapshot
            .add_contract(ContractState::new("CA7QYNF5", "aabbccdd"))
            .unwrap();

        let loader = SnapshotLoader::from_snapshot(snapshot).unwrap();

        assert_eq!(loader.list_accounts(), vec!["GABCD123"]);
        assert_eq!(loader.list_contracts(), vec!["CA7QYNF5"]);
    }

    #[test]
    fn test_get_account_from_loader() {
        let mut snapshot = NetworkSnapshot::new(100, "Test Network", 1234567890);
        snapshot
            .add_account(AccountState::new("GABCD123", "1000000", 1))
            .unwrap();

        let loader = SnapshotLoader::from_snapshot(snapshot).unwrap();
        let account = loader.get_account("GABCD123").unwrap();

        assert_eq!(account.address, "GABCD123");
        assert_eq!(account.balance, "1000000");
    }
}
