use crossterm::style::{Color, Stylize};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Default TTL warning threshold in ledger sequence numbers.
const DEFAULT_TTL_WARNING_THRESHOLD: u32 = 1000;

/// Type of Soroban ledger storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StorageType {
    Instance,
    Persistent,
    Temporary,
}

impl fmt::Display for StorageType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageType::Instance => write!(f, "Instance"),
            StorageType::Persistent => write!(f, "Persistent"),
            StorageType::Temporary => write!(f, "Temporary"),
        }
    }
}

/// Information about a single ledger entry accessed during contract execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEntryInfo {
    /// The storage key
    pub key: String,
    /// The storage value (debug-formatted)
    pub value: String,
    /// Which storage type this entry belongs to
    pub storage_type: StorageType,
    /// Time-to-live in ledger sequence numbers
    pub ttl: u32,
    /// Whether this entry was read (vs written)
    pub is_read: bool,
    /// Whether this entry was written
    pub is_write: bool,
}

impl LedgerEntryInfo {
    /// Check if this entry is near expiration given a threshold.
    pub fn is_near_expiry(&self, threshold: u32) -> bool {
        self.ttl < threshold
    }
}

/// Inspects ledger entries accessed during contract execution.
///
/// Tracks all ledger entries (Instance, Persistent, Temporary) that were
/// read or written, including their TTLs, and provides near-expiry warnings.
pub struct LedgerEntryInspector {
    entries: Vec<LedgerEntryInfo>,
    ttl_warning_threshold: u32,
}

impl LedgerEntryInspector {
    /// Create a new inspector with default TTL warning threshold.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            ttl_warning_threshold: DEFAULT_TTL_WARNING_THRESHOLD,
        }
    }

    /// Set the TTL warning threshold (entries with TTL below this are flagged).
    pub fn set_ttl_warning_threshold(&mut self, threshold: u32) {
        self.ttl_warning_threshold = threshold;
    }

    /// Get the current TTL warning threshold.
    pub fn ttl_warning_threshold(&self) -> u32 {
        self.ttl_warning_threshold
    }

    /// Add a tracked ledger entry.
    pub fn add_entry(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
        storage_type: StorageType,
        ttl: u32,
        is_read: bool,
        is_write: bool,
    ) {
        self.entries.push(LedgerEntryInfo {
            key: key.into(),
            value: value.into(),
            storage_type,
            ttl,
            is_read,
            is_write,
        });
    }

    /// Get all tracked entries.
    pub fn get_entries(&self) -> &[LedgerEntryInfo] {
        &self.entries
    }

    /// Get entries filtered by storage type.
    pub fn get_entries_by_type(&self, storage_type: StorageType) -> Vec<&LedgerEntryInfo> {
        self.entries
            .iter()
            .filter(|e| e.storage_type == storage_type)
            .collect()
    }

    /// Get entries that are near expiration.
    pub fn get_near_expiry_entries(&self) -> Vec<&LedgerEntryInfo> {
        self.entries
            .iter()
            .filter(|e| e.is_near_expiry(self.ttl_warning_threshold))
            .collect()
    }

    /// Returns true if no entries have been tracked.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Display all ledger entries grouped by storage type in a formatted table.
    pub fn display(&self) {
        if self.entries.is_empty() {
            println!("  (No ledger entries accessed)");
            return;
        }

        println!(
            "\n  {} ledger entries accessed during execution:\n",
            self.entries.len()
        );

        // Display entries grouped by type
        for storage_type in &[
            StorageType::Instance,
            StorageType::Persistent,
            StorageType::Temporary,
        ] {
            let entries = self.get_entries_by_type(*storage_type);
            if entries.is_empty() {
                continue;
            }

            let type_color = match storage_type {
                StorageType::Instance => Color::Cyan,
                StorageType::Persistent => Color::Blue,
                StorageType::Temporary => Color::Magenta,
            };

            println!(
                "  {} ({} entries)",
                format!("[{}]", storage_type).with(type_color).bold(),
                entries.len()
            );

            // Table header
            println!(
                "  {:<30} | {:<8} | {:<10} | {}",
                "Key", "Access", "TTL", "Value"
            );
            println!("  {:-<30}-+-{:-<8}-+-{:-<10}-+-{:-<30}", "", "", "", "");

            for entry in &entries {
                let access = match (entry.is_read, entry.is_write) {
                    (true, true) => "R/W",
                    (true, false) => "READ",
                    (false, true) => "WRITE",
                    (false, false) => "-",
                };

                let key_display = if entry.key.len() > 30 {
                    format!("{}...", &entry.key[0..27])
                } else {
                    entry.key.clone()
                };

                let value_display = if entry.value.len() > 30 {
                    format!("{}...", &entry.value[0..27])
                } else {
                    entry.value.clone()
                };

                let ttl_color = if entry.is_near_expiry(self.ttl_warning_threshold) {
                    Color::Red
                } else {
                    Color::Green
                };

                println!(
                    "  {:<30} | {:<8} | {:<10} | {}",
                    key_display.with(Color::White),
                    access.with(Color::Yellow),
                    entry.ttl.to_string().with(ttl_color),
                    value_display.with(Color::DarkGrey)
                );
            }
            println!();
        }
    }

    /// Display near-expiry warnings for entries with TTL below the threshold.
    pub fn display_warnings(&self) {
        let near_expiry = self.get_near_expiry_entries();
        if near_expiry.is_empty() {
            return;
        }

        println!(
            "  {}",
            format!(
                "⚠ {} ledger entries near expiration (TTL < {}):",
                near_expiry.len(),
                self.ttl_warning_threshold
            )
            .with(Color::Yellow)
            .bold()
        );

        for entry in &near_expiry {
            let urgency = if entry.ttl == 0 {
                "EXPIRED".with(Color::Red).bold()
            } else if entry.ttl < self.ttl_warning_threshold / 4 {
                "CRITICAL".with(Color::Red).bold()
            } else {
                "WARNING".with(Color::Yellow).bold()
            };

            println!(
                "    {} [{}] {} — TTL: {}",
                urgency,
                entry.storage_type,
                entry.key.clone().with(Color::White),
                entry.ttl.to_string().with(Color::Red)
            );
        }
        println!();
    }

    /// Convert all entries to a JSON-serializable value.
    pub fn to_json(&self) -> serde_json::Value {
        let grouped: std::collections::HashMap<String, Vec<&LedgerEntryInfo>> = self
            .entries
            .iter()
            .fold(std::collections::HashMap::new(), |mut acc, entry| {
                acc.entry(entry.storage_type.to_string())
                    .or_default()
                    .push(entry);
                acc
            });

        let mut result = serde_json::Map::new();
        result.insert(
            "total_entries".to_string(),
            serde_json::Value::Number(self.entries.len().into()),
        );
        result.insert(
            "ttl_warning_threshold".to_string(),
            serde_json::Value::Number(self.ttl_warning_threshold.into()),
        );

        let near_expiry_count = self.get_near_expiry_entries().len();
        result.insert(
            "near_expiry_count".to_string(),
            serde_json::Value::Number(near_expiry_count.into()),
        );

        let entries_json: Vec<serde_json::Value> = self
            .entries
            .iter()
            .map(|e| {
                serde_json::json!({
                    "key": e.key,
                    "value": e.value,
                    "storage_type": e.storage_type.to_string(),
                    "ttl": e.ttl,
                    "is_read": e.is_read,
                    "is_write": e.is_write,
                    "near_expiry": e.is_near_expiry(self.ttl_warning_threshold),
                })
            })
            .collect();

        result.insert(
            "entries".to_string(),
            serde_json::Value::Array(entries_json),
        );

        let by_type: serde_json::Map<String, serde_json::Value> = grouped
            .iter()
            .map(|(k, v)| (k.clone(), serde_json::Value::Number(v.len().into())))
            .collect();

        result.insert("by_type".to_string(), serde_json::Value::Object(by_type));

        serde_json::Value::Object(result)
    }
}

impl Default for LedgerEntryInspector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_inspector() -> LedgerEntryInspector {
        let mut inspector = LedgerEntryInspector::new();
        inspector.add_entry(
            "balance:alice",
            "1000",
            StorageType::Persistent,
            5000,
            true,
            false,
        );
        inspector.add_entry(
            "balance:bob",
            "500",
            StorageType::Persistent,
            200,
            true,
            true,
        );
        inspector.add_entry("config", "v1", StorageType::Instance, 999999, true, false);
        inspector.add_entry(
            "session:xyz",
            "active",
            StorageType::Temporary,
            50,
            false,
            true,
        );
        inspector.add_entry("nonce:alice", "3", StorageType::Temporary, 800, true, false);
        inspector
    }

    #[test]
    fn test_new_inspector_is_empty() {
        let inspector = LedgerEntryInspector::new();
        assert!(inspector.is_empty());
        assert_eq!(inspector.get_entries().len(), 0);
        assert_eq!(
            inspector.ttl_warning_threshold(),
            DEFAULT_TTL_WARNING_THRESHOLD
        );
    }

    #[test]
    fn test_add_and_get_entries() {
        let inspector = sample_inspector();
        assert_eq!(inspector.get_entries().len(), 5);
        assert!(!inspector.is_empty());
    }

    #[test]
    fn test_get_entries_by_type() {
        let inspector = sample_inspector();

        let persistent = inspector.get_entries_by_type(StorageType::Persistent);
        assert_eq!(persistent.len(), 2);
        assert!(persistent
            .iter()
            .all(|e| e.storage_type == StorageType::Persistent));

        let instance = inspector.get_entries_by_type(StorageType::Instance);
        assert_eq!(instance.len(), 1);
        assert_eq!(instance[0].key, "config");

        let temporary = inspector.get_entries_by_type(StorageType::Temporary);
        assert_eq!(temporary.len(), 2);
    }

    #[test]
    fn test_near_expiry_default_threshold() {
        let inspector = sample_inspector();
        // Default threshold is 1000
        let near_expiry = inspector.get_near_expiry_entries();
        // balance:bob (200), session:xyz (50), nonce:alice (800) are all < 1000
        assert_eq!(near_expiry.len(), 3);
    }

    #[test]
    fn test_near_expiry_custom_threshold() {
        let mut inspector = sample_inspector();
        inspector.set_ttl_warning_threshold(100);
        let near_expiry = inspector.get_near_expiry_entries();
        // Only session:xyz (50) is < 100
        assert_eq!(near_expiry.len(), 1);
        assert_eq!(near_expiry[0].key, "session:xyz");
    }

    #[test]
    fn test_near_expiry_zero_threshold() {
        let mut inspector = sample_inspector();
        inspector.set_ttl_warning_threshold(0);
        let near_expiry = inspector.get_near_expiry_entries();
        assert_eq!(near_expiry.len(), 0);
    }

    #[test]
    fn test_entry_is_near_expiry() {
        let entry = LedgerEntryInfo {
            key: "test".to_string(),
            value: "val".to_string(),
            storage_type: StorageType::Persistent,
            ttl: 500,
            is_read: true,
            is_write: false,
        };

        assert!(entry.is_near_expiry(1000));
        assert!(!entry.is_near_expiry(500));
        assert!(!entry.is_near_expiry(100));
    }

    #[test]
    fn test_storage_type_display() {
        assert_eq!(format!("{}", StorageType::Instance), "Instance");
        assert_eq!(format!("{}", StorageType::Persistent), "Persistent");
        assert_eq!(format!("{}", StorageType::Temporary), "Temporary");
    }

    #[test]
    fn test_display_empty_inspector() {
        let inspector = LedgerEntryInspector::new();
        // Just verify it doesn't panic
        inspector.display();
        inspector.display_warnings();
    }

    #[test]
    fn test_display_with_entries() {
        let inspector = sample_inspector();
        // Just verify it doesn't panic
        inspector.display();
        inspector.display_warnings();
    }

    #[test]
    fn test_to_json() {
        let inspector = sample_inspector();
        let json = inspector.to_json();

        assert_eq!(json["total_entries"], 5);
        assert_eq!(json["ttl_warning_threshold"], DEFAULT_TTL_WARNING_THRESHOLD);
        assert_eq!(json["near_expiry_count"], 3);

        let entries = json["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 5);

        // Verify first entry structure
        let first = &entries[0];
        assert_eq!(first["key"], "balance:alice");
        assert_eq!(first["storage_type"], "Persistent");
        assert_eq!(first["ttl"], 5000);
        assert_eq!(first["is_read"], true);
        assert_eq!(first["is_write"], false);
        assert_eq!(first["near_expiry"], false);

        // Verify by_type counts
        let by_type = json["by_type"].as_object().unwrap();
        assert_eq!(by_type["Persistent"], 2);
        assert_eq!(by_type["Instance"], 1);
        assert_eq!(by_type["Temporary"], 2);
    }

    #[test]
    fn test_to_json_empty() {
        let inspector = LedgerEntryInspector::new();
        let json = inspector.to_json();
        assert_eq!(json["total_entries"], 0);
        assert_eq!(json["near_expiry_count"], 0);
        assert_eq!(json["entries"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_default_impl() {
        let inspector = LedgerEntryInspector::default();
        assert!(inspector.is_empty());
        assert_eq!(
            inspector.ttl_warning_threshold(),
            DEFAULT_TTL_WARNING_THRESHOLD
        );
    }
}
