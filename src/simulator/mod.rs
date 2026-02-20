//! Network and ledger state simulator
//!
//! This module provides comprehensive network state simulation for Soroban debugging.
//! It allows users to:
//! - Load network snapshots from JSON files
//! - Configure mock ledger state (accounts, contracts, balances)
//! - Pre-deploy contract instances with populated storage
//! - Save and restore ledger state for iterative debugging

pub mod loader;
pub mod snapshot;
pub mod state;

pub use loader::{LoadedSnapshot, SnapshotLoader};
pub use snapshot::{AccountDiff, ContractDiff, SnapshotDiff, SnapshotManager};
pub use state::{AccountState, ContractState, LedgerMetadata, NetworkSnapshot, SimulatorError};
