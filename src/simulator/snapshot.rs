//! Snapshot persistence and restoration
//!
//! This module provides functionality to save and restore network snapshots,
//! allowing users to capture the state of a ledger after debugging and
//! restore it later for continued work.

use super::state::{NetworkSnapshot, SimulatorError};
use crate::Result;
use std::fs;
use std::path::Path;
use tracing::info;

/// Manages snapshot persistence (save and restore operations)
pub struct SnapshotManager;

impl SnapshotManager {
    /// Save a snapshot to a JSON file
    pub fn save<P: AsRef<Path>>(snapshot: &NetworkSnapshot, path: P) -> Result<()> {
        let path = path.as_ref();
        info!("Saving snapshot to: {:?}", path);

        // Validate snapshot before saving
        snapshot.validate()?;

        // Serialize to pretty JSON
        let json = serde_json::to_string_pretty(snapshot).map_err(SimulatorError::JsonError)?;

        // Write to file
        fs::write(path, &json).map_err(SimulatorError::IoError)?;

        info!("Snapshot saved successfully ({} bytes)", json.len());

        Ok(())
    }

    /// Load a snapshot from a JSON file
    pub fn load<P: AsRef<Path>>(path: P) -> Result<NetworkSnapshot> {
        let path = path.as_ref();
        info!("Loading snapshot from: {:?}", path);

        // Read file
        let contents = fs::read_to_string(path).map_err(SimulatorError::IoError)?;

        // Parse JSON
        let snapshot: NetworkSnapshot =
            serde_json::from_str(&contents).map_err(SimulatorError::JsonError)?;

        // Validate loaded snapshot
        snapshot.validate()?;

        info!(
            "Snapshot loaded: {} accounts, {} contracts",
            snapshot.accounts.len(),
            snapshot.contracts.len()
        );

        Ok(snapshot)
    }

    /// Generate a summary of differences between two snapshots
    pub fn diff_snapshots(before: &NetworkSnapshot, after: &NetworkSnapshot) -> SnapshotDiff {
        SnapshotDiff::compute(before, after)
    }

    /// Validate that a snapshot file is valid and readable
    pub fn validate_file<P: AsRef<Path>>(path: P) -> Result<()> {
        let snapshot = Self::load(path.as_ref())?;
        snapshot.validate()?;
        Ok(())
    }
}

/// Represents the differences between two network snapshots
#[derive(Debug, Clone)]
pub struct SnapshotDiff {
    /// Ledger sequence changes
    pub ledger_sequence_changed: bool,
    pub old_sequence: Option<u32>,
    pub new_sequence: Option<u32>,

    /// Timestamp changes
    pub timestamp_changed: bool,
    pub old_timestamp: Option<u64>,
    pub new_timestamp: Option<u64>,

    /// Account changes
    pub accounts_added: Vec<String>,
    pub accounts_removed: Vec<String>,
    pub accounts_modified: Vec<AccountDiff>,

    /// Contract changes
    pub contracts_added: Vec<String>,
    pub contracts_removed: Vec<String>,
    pub contracts_modified: Vec<ContractDiff>,
}

impl SnapshotDiff {
    /// Compute diff between two snapshots
    fn compute(before: &NetworkSnapshot, after: &NetworkSnapshot) -> Self {
        let mut diff = SnapshotDiff {
            ledger_sequence_changed: before.ledger.sequence != after.ledger.sequence,
            old_sequence: Some(before.ledger.sequence),
            new_sequence: Some(after.ledger.sequence),
            timestamp_changed: before.ledger.timestamp != after.ledger.timestamp,
            old_timestamp: Some(before.ledger.timestamp),
            new_timestamp: Some(after.ledger.timestamp),
            accounts_added: Vec::new(),
            accounts_removed: Vec::new(),
            accounts_modified: Vec::new(),
            contracts_added: Vec::new(),
            contracts_removed: Vec::new(),
            contracts_modified: Vec::new(),
        };

        // Compute account changes
        let before_addresses: std::collections::HashSet<_> =
            before.accounts.iter().map(|a| a.address.clone()).collect();
        let after_addresses: std::collections::HashSet<_> =
            after.accounts.iter().map(|a| a.address.clone()).collect();

        for addr in &before_addresses {
            if !after_addresses.contains(addr) {
                diff.accounts_removed.push(addr.clone());
            }
        }

        for addr in &after_addresses {
            if !before_addresses.contains(addr) {
                diff.accounts_added.push(addr.clone());
            }
        }

        for addr in before_addresses.intersection(&after_addresses) {
            let before_acc = before.get_account(addr).unwrap();
            let after_acc = after.get_account(addr).unwrap();

            let account_diff = AccountDiff {
                address: addr.clone(),
                balance_changed: before_acc.balance != after_acc.balance,
                old_balance: Some(before_acc.balance.clone()),
                new_balance: Some(after_acc.balance.clone()),
                sequence_changed: before_acc.sequence != after_acc.sequence,
                old_sequence: Some(before_acc.sequence),
                new_sequence: Some(after_acc.sequence),
            };

            if account_diff.balance_changed || account_diff.sequence_changed {
                diff.accounts_modified.push(account_diff);
            }
        }

        // Compute contract changes
        let before_contracts: std::collections::HashSet<_> = before
            .contracts
            .iter()
            .map(|c| c.contract_id.clone())
            .collect();
        let after_contracts: std::collections::HashSet<_> = after
            .contracts
            .iter()
            .map(|c| c.contract_id.clone())
            .collect();

        for id in &before_contracts {
            if !after_contracts.contains(id) {
                diff.contracts_removed.push(id.clone());
            }
        }

        for id in &after_contracts {
            if !before_contracts.contains(id) {
                diff.contracts_added.push(id.clone());
            }
        }

        for id in before_contracts.intersection(&after_contracts) {
            let before_contract = before.get_contract(id).unwrap();
            let after_contract = after.get_contract(id).unwrap();

            let storage_changed = before_contract.storage != after_contract.storage;

            if storage_changed {
                diff.contracts_modified.push(ContractDiff {
                    contract_id: id.clone(),
                    storage_changed,
                });
            }
        }

        diff
    }

    /// Format diff as human-readable string
    pub fn format_summary(&self) -> String {
        let mut output = String::new();

        // Ledger changes
        if self.ledger_sequence_changed {
            output.push_str(&format!(
                "Ledger sequence: {} → {}\n",
                self.old_sequence.unwrap_or(0),
                self.new_sequence.unwrap_or(0)
            ));
        }

        if self.timestamp_changed {
            output.push_str(&format!(
                "Timestamp: {} → {}\n",
                self.old_timestamp.unwrap_or(0),
                self.new_timestamp.unwrap_or(0)
            ));
        }

        // Account changes
        if !self.accounts_added.is_empty() {
            output.push_str(&format!(
                "Accounts added: {}\n",
                self.accounts_added.join(", ")
            ));
        }

        if !self.accounts_removed.is_empty() {
            output.push_str(&format!(
                "Accounts removed: {}\n",
                self.accounts_removed.join(", ")
            ));
        }

        if !self.accounts_modified.is_empty() {
            output.push_str("Accounts modified:\n");
            for acc_diff in &self.accounts_modified {
                if acc_diff.balance_changed {
                    output.push_str(&format!(
                        "  {} balance: {} → {}\n",
                        acc_diff.address,
                        acc_diff.old_balance.as_ref().unwrap_or(&"?".to_string()),
                        acc_diff.new_balance.as_ref().unwrap_or(&"?".to_string())
                    ));
                }
                if acc_diff.sequence_changed {
                    output.push_str(&format!(
                        "  {} sequence: {} → {}\n",
                        acc_diff.address,
                        acc_diff.old_sequence.unwrap_or(0),
                        acc_diff.new_sequence.unwrap_or(0)
                    ));
                }
            }
        }

        // Contract changes
        if !self.contracts_added.is_empty() {
            output.push_str(&format!(
                "Contracts added: {}\n",
                self.contracts_added.join(", ")
            ));
        }

        if !self.contracts_removed.is_empty() {
            output.push_str(&format!(
                "Contracts removed: {}\n",
                self.contracts_removed.join(", ")
            ));
        }

        if !self.contracts_modified.is_empty() {
            output.push_str(&format!(
                "Contracts with storage changes: {}\n",
                self.contracts_modified
                    .iter()
                    .map(|c| c.contract_id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        if output.is_empty() {
            output.push_str("No changes detected between snapshots");
        }

        output
    }

    /// Check if any changes were detected
    pub fn has_changes(&self) -> bool {
        self.ledger_sequence_changed
            || self.timestamp_changed
            || !self.accounts_added.is_empty()
            || !self.accounts_removed.is_empty()
            || !self.accounts_modified.is_empty()
            || !self.contracts_added.is_empty()
            || !self.contracts_removed.is_empty()
            || !self.contracts_modified.is_empty()
    }
}

/// Differences in a single account between snapshots
#[derive(Debug, Clone)]
pub struct AccountDiff {
    pub address: String,
    pub balance_changed: bool,
    pub old_balance: Option<String>,
    pub new_balance: Option<String>,
    pub sequence_changed: bool,
    pub old_sequence: Option<u64>,
    pub new_sequence: Option<u64>,
}

/// Differences in a single contract between snapshots
#[derive(Debug, Clone)]
pub struct ContractDiff {
    pub contract_id: String,
    pub storage_changed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::simulator::state::AccountState;
    use tempfile::TempDir;

    #[test]
    fn test_save_and_load_snapshot() {
        let mut snapshot = NetworkSnapshot::new(100, "Test Network", 1234567890);
        snapshot
            .add_account(AccountState::new("GABCD123", "1000000", 1))
            .unwrap();

        let tmpdir = TempDir::new().unwrap();
        let path = tmpdir.path().join("snapshot.json");

        SnapshotManager::save(&snapshot, &path).unwrap();
        let loaded = SnapshotManager::load(&path).unwrap();

        assert_eq!(loaded.ledger.sequence, snapshot.ledger.sequence);
        assert_eq!(loaded.accounts.len(), 1);
    }

    #[test]
    fn test_compute_account_changes() {
        let mut before = NetworkSnapshot::new(100, "Test Network", 1234567890);
        before
            .add_account(AccountState::new("GABCD123", "1000000", 1))
            .unwrap();

        let mut after = NetworkSnapshot::new(101, "Test Network", 1234567890);
        after
            .add_account(AccountState::new("GABCD123", "2000000", 2))
            .unwrap();

        let diff = SnapshotManager::diff_snapshots(&before, &after);

        assert!(diff.ledger_sequence_changed);
        assert!(!diff.accounts_modified.is_empty());
        assert!(diff.accounts_modified[0].balance_changed);
    }

    #[test]
    fn test_compute_account_additions() {
        let before = NetworkSnapshot::new(100, "Test Network", 1234567890);

        let mut after = NetworkSnapshot::new(100, "Test Network", 1234567890);
        after
            .add_account(AccountState::new("GABCD123", "1000000", 1))
            .unwrap();

        let diff = SnapshotManager::diff_snapshots(&before, &after);

        assert!(!diff.accounts_added.is_empty());
        assert_eq!(diff.accounts_added[0], "GABCD123");
    }

    #[test]
    fn test_diff_has_changes() {
        let before = NetworkSnapshot::new(100, "Test Network", 1234567890);
        let mut after = NetworkSnapshot::new(100, "Test Network", 1234567890);
        after
            .add_account(AccountState::new("GABCD123", "1000000", 1))
            .unwrap();

        let diff = SnapshotManager::diff_snapshots(&before, &after);
        assert!(diff.has_changes());
    }

    #[test]
    fn test_diff_no_changes() {
        let before = NetworkSnapshot::new(100, "Test Network", 1234567890);
        let after = before.clone();

        let diff = SnapshotManager::diff_snapshots(&before, &after);
        assert!(!diff.has_changes());
    }
}
