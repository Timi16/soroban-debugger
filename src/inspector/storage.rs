use regex::Regex;
use std::collections::HashMap;
use soroban_env_host::Host;
use crossterm::style::{Color, Stylize};

/// Represents a storage key filter pattern
#[derive(Debug, Clone)]
pub enum FilterPattern {
    /// Prefix match: `balance:*` matches keys starting with `balance:`
    Prefix(String),
    /// Regex match: `re:^balance_\d+$` matches keys via regex
    Regex(Regex),
    /// Exact match: `balance` matches the key exactly
    Exact(String),
}

impl FilterPattern {
    /// Parse a filter string into a FilterPattern
    ///
    /// - `re:<pattern>` → Regex filter
    /// - `<prefix>*` → Prefix filter (trailing `*`)
    /// - `<exact>` → Exact match
    pub fn parse(pattern: &str) -> Result<Self, String> {
        if let Some(regex_str) = pattern.strip_prefix("re:") {
            let regex = Regex::new(regex_str)
                .map_err(|e| format!("Invalid regex pattern '{}': {}", regex_str, e))?;
            Ok(FilterPattern::Regex(regex))
        } else if let Some(prefix) = pattern.strip_suffix('*') {
            Ok(FilterPattern::Prefix(prefix.to_string()))
        } else {
            Ok(FilterPattern::Exact(pattern.to_string()))
        }
    }

    /// Check if a key matches this filter pattern
    pub fn matches(&self, key: &str) -> bool {
        match self {
            FilterPattern::Prefix(prefix) => key.starts_with(prefix),
            FilterPattern::Regex(regex) => regex.is_match(key),
            FilterPattern::Exact(exact) => key == exact,
        }
    }
}

/// A collection of filter patterns combined with OR logic
#[derive(Debug, Clone)]
pub struct StorageFilter {
    patterns: Vec<FilterPattern>,
}

impl StorageFilter {
    /// Create a new storage filter from a list of pattern strings
    pub fn new(patterns: &[String]) -> Result<Self, String> {
        let parsed: Result<Vec<FilterPattern>, String> =
            patterns.iter().map(|p| FilterPattern::parse(p)).collect();
        Ok(Self { patterns: parsed? })
    }

    /// Check if any filter matches the given key.
    /// Returns true if no filters are set (show everything).
    pub fn matches(&self, key: &str) -> bool {
        if self.patterns.is_empty() {
            return true;
        }
        self.patterns.iter().any(|p| p.matches(key))
    }

    /// Returns true if no filters are configured
    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// Get a human-readable summary of active filters
    pub fn summary(&self) -> String {
        self.patterns
            .iter()
            .map(|p| match p {
                FilterPattern::Prefix(prefix) => format!("{}*", prefix),
                FilterPattern::Regex(regex) => format!("re:{}", regex.as_str()),
                FilterPattern::Exact(exact) => exact.clone(),
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Inspects and displays contract storage
pub struct StorageInspector {
    // Storage will be tracked here
    storage: HashMap<String, String>,
}

impl StorageInspector {
    pub fn new() -> Self {
        Self {
            storage: HashMap::new(),
        }
    }

    /// Get all storage entries
    pub fn get_all(&self) -> &HashMap<String, String> {
        &self.storage
    }

    /// Get a specific storage value
    pub fn get(&self, key: &str) -> Option<&String> {
        self.storage.get(key)
    }

    /// Display storage in a readable format (no filtering)
    pub fn display(&self) {
        if self.storage.is_empty() {
            println!("Storage: (empty)");
            return;
        }

        println!("Storage:");
        for (key, value) in &self.storage {
            println!("  {} = {}", key, value);
        }
    }

    /// Display storage filtered by the given patterns.
    /// Prints a notice when filtering is active.
    pub fn display_filtered(&self, filter: &StorageFilter) {
        if self.storage.is_empty() {
            println!("Storage: (empty)");
            return;
        }

        if !filter.is_empty() {
            println!("Storage (filtered by: {}):", filter.summary());
        } else {
            println!("Storage:");
        }

        let mut matched = 0;
        let mut keys: Vec<&String> = self.storage.keys().collect();
        keys.sort();

        for key in keys {
            if filter.matches(key) {
                println!("  {} = {}", key, self.storage[key]);
                matched += 1;
            }
        }

        if matched == 0 && !filter.is_empty() {
            println!("  (no entries matched the filter)");
        }

        if !filter.is_empty() {
            let total = self.storage.len();
            println!("\n  Showing {}/{} entries", matched, total);
        }
    }

    /// Get filtered storage entries as a new HashMap
    pub fn get_filtered(&self, filter: &StorageFilter) -> HashMap<String, String> {
        self.storage
            .iter()
            .filter(|(key, _)| filter.matches(key))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// Insert a storage entry (used for testing and state tracking)
    pub fn set(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.storage.insert(key.into(), value.into());
    }

    /// Capture a snapshot of all storage entries from the host
    pub fn capture_snapshot(host: &Host) -> HashMap<String, String> {
        let mut snapshot = HashMap::new();
        
        // In a real implementation, we would iterate through host.get_ledger_entries()
        // or track changes via a custom Storage instance.
        // For this debugger, we'll try to extract what's available.
        // Since Host doesn't easily expose all entries without XDR iteration,
        // we'll use a placeholder logic that would be backed by actual storage tracking
        // in a production environment.
        
        // NOTE: In Soroban host, entries are typically accessed by key.
        // To show "everything", we'd need to have tracked access during execution.
        
        snapshot
    }

    /// Compute the difference between two storage snapshots
    pub fn compute_diff(
        before: &HashMap<String, String>,
        after: &HashMap<String, String>,
    ) -> StorageDiff {
        let mut added = HashMap::new();
        let mut modified = HashMap::new();
        let mut deleted = Vec::new();

        for (key, val_after) in after {
            match before.get(key) {
                Some(val_before) => {
                    if val_before != val_after {
                        modified.insert(key.clone(), (val_before.clone(), val_after.clone()));
                    }
                }
                None => {
                    added.insert(key.clone(), val_after.clone());
                }
            }
        }

        for key in before.keys() {
            if !after.contains_key(key) {
                deleted.push(key.clone());
            }
        }

        StorageDiff {
            added,
            modified,
            deleted,
        }
    }

    /// Display a color-coded storage diff
    pub fn display_diff(diff: &StorageDiff) {
        if diff.is_empty() {
            println!("Storage: (no changes)");
            return;
        }

        println!("Storage Changes:");

        // Sort keys for deterministic output
        let mut added_keys: Vec<_> = diff.added.keys().collect();
        added_keys.sort();
        for key in added_keys {
            println!("  {} {} = {}", "+".with(Color::Green), key, diff.added[key].clone().with(Color::Green));
        }

        let mut modified_keys: Vec<_> = diff.modified.keys().collect();
        modified_keys.sort();
        for key in modified_keys {
            let (old, new) = &diff.modified[key];
            println!(
                "  {} {}: {} -> {}",
                "~".with(Color::Yellow),
                key,
                old.clone().with(Color::Red),
                new.clone().with(Color::Green)
            );
        }

        let mut deleted_keys = diff.deleted.clone();
        deleted_keys.sort();
        for key in deleted_keys {
            println!("  {} {}", "-".with(Color::Red), key.with(Color::Red));
        }
    }
}

/// Represents the differences between two storage states
#[derive(Debug, Clone, Default)]
pub struct StorageDiff {
    pub added: HashMap<String, String>,
    pub modified: HashMap<String, (String, String)>,
    pub deleted: Vec<String>,
}

impl StorageDiff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.modified.is_empty() && self.deleted.is_empty()
    }
}

impl Default for StorageInspector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── FilterPattern tests ──────────────────────────────────────────

    #[test]
    fn test_parse_prefix_pattern() {
        let pattern = FilterPattern::parse("balance:*").unwrap();
        assert!(matches!(pattern, FilterPattern::Prefix(_)));
        assert!(pattern.matches("balance:alice"));
        assert!(pattern.matches("balance:bob"));
        assert!(!pattern.matches("total_supply"));
    }

    #[test]
    fn test_parse_regex_pattern() {
        let pattern = FilterPattern::parse(r"re:^balance_\d+$").unwrap();
        assert!(matches!(pattern, FilterPattern::Regex(_)));
        assert!(pattern.matches("balance_123"));
        assert!(pattern.matches("balance_0"));
        assert!(!pattern.matches("balance_abc"));
        assert!(!pattern.matches("xbalance_123"));
    }

    #[test]
    fn test_parse_exact_pattern() {
        let pattern = FilterPattern::parse("total_supply").unwrap();
        assert!(matches!(pattern, FilterPattern::Exact(_)));
        assert!(pattern.matches("total_supply"));
        assert!(!pattern.matches("total_supply_v2"));
        assert!(!pattern.matches("total"));
    }

    #[test]
    fn test_parse_invalid_regex() {
        let result = FilterPattern::parse(r"re:[invalid");
        assert!(result.is_err());
    }

    // ── StorageFilter tests ──────────────────────────────────────────

    #[test]
    fn test_empty_filter_matches_everything() {
        let filter = StorageFilter::new(&[]).unwrap();
        assert!(filter.matches("anything"));
        assert!(filter.matches(""));
        assert!(filter.is_empty());
    }

    #[test]
    fn test_single_prefix_filter() {
        let filter = StorageFilter::new(&["balance:*".to_string()]).unwrap();
        assert!(filter.matches("balance:alice"));
        assert!(filter.matches("balance:bob"));
        assert!(!filter.matches("total_supply"));
    }

    #[test]
    fn test_single_regex_filter() {
        let filter = StorageFilter::new(&[r"re:^user_\d+$".to_string()]).unwrap();
        assert!(filter.matches("user_1"));
        assert!(filter.matches("user_999"));
        assert!(!filter.matches("admin"));
    }

    #[test]
    fn test_multiple_filters_or_logic() {
        let filter =
            StorageFilter::new(&["balance:*".to_string(), "total_supply".to_string()]).unwrap();
        assert!(filter.matches("balance:alice"));
        assert!(filter.matches("total_supply"));
        assert!(!filter.matches("admin"));
    }

    #[test]
    fn test_multiple_mixed_filters() {
        let filter = StorageFilter::new(&[
            "balance:*".to_string(),
            r"re:^config_\w+$".to_string(),
            "admin".to_string(),
        ])
        .unwrap();
        assert!(filter.matches("balance:alice"));
        assert!(filter.matches("config_timeout"));
        assert!(filter.matches("admin"));
        assert!(!filter.matches("user_data"));
    }

    #[test]
    fn test_filter_summary() {
        let filter = StorageFilter::new(&[
            "balance:*".to_string(),
            r"re:^\d+$".to_string(),
            "admin".to_string(),
        ])
        .unwrap();
        let summary = filter.summary();
        assert!(summary.contains("balance:*"));
        assert!(summary.contains("re:"));
        assert!(summary.contains("admin"));
    }

    // ── StorageInspector filtering tests ─────────────────────────────

    #[test]
    fn test_display_filtered_empty_storage() {
        let inspector = StorageInspector::new();
        let filter = StorageFilter::new(&["balance:*".to_string()]).unwrap();
        // Just verify it doesn't panic
        inspector.display_filtered(&filter);
    }

    #[test]
    fn test_get_filtered() {
        let mut inspector = StorageInspector::new();
        inspector.set("balance:alice", "1000");
        inspector.set("balance:bob", "500");
        inspector.set("total_supply", "1500");
        inspector.set("admin", "alice");

        let filter = StorageFilter::new(&["balance:*".to_string()]).unwrap();
        let filtered = inspector.get_filtered(&filter);

        assert_eq!(filtered.len(), 2);
        assert!(filtered.contains_key("balance:alice"));
        assert!(filtered.contains_key("balance:bob"));
        assert!(!filtered.contains_key("total_supply"));
    }

    #[test]
    fn test_get_filtered_no_match() {
        let mut inspector = StorageInspector::new();
        inspector.set("total_supply", "1500");
        inspector.set("admin", "alice");

        let filter = StorageFilter::new(&["balance:*".to_string()]).unwrap();
        let filtered = inspector.get_filtered(&filter);

        assert_eq!(filtered.len(), 0);
    }

    #[test]
    fn test_storage_diff_added() {
        let before = HashMap::new();
        let mut after = HashMap::new();
        after.insert("key1".to_string(), "val1".to_string());

        let diff = StorageInspector::compute_diff(&before, &after);
        assert_eq!(diff.added.get("key1"), Some(&"val1".to_string()));
        assert!(diff.modified.is_empty());
        assert!(diff.deleted.is_empty());
    }

    #[test]
    fn test_storage_diff_modified() {
        let mut before = HashMap::new();
        before.insert("key1".to_string(), "val_old".to_string());
        let mut after = HashMap::new();
        after.insert("key1".to_string(), "val_new".to_string());

        let diff = StorageInspector::compute_diff(&before, &after);
        assert!(diff.added.is_empty());
        assert_eq!(diff.modified.get("key1"), Some(&("val_old".to_string(), "val_new".to_string())));
        assert!(diff.deleted.is_empty());
    }

    #[test]
    fn test_storage_diff_deleted() {
        let mut before = HashMap::new();
        before.insert("key1".to_string(), "val1".to_string());
        let after = HashMap::new();

        let diff = StorageInspector::compute_diff(&before, &after);
        assert!(diff.added.is_empty());
        assert!(diff.modified.is_empty());
        assert_eq!(diff.deleted, vec!["key1".to_string()]);
    }

    #[test]
    fn test_storage_diff_multiple_changes() {
        let mut before = HashMap::new();
        before.insert("unchanged".to_string(), "same".to_string());
        before.insert("modified".to_string(), "old".to_string());
        before.insert("deleted".to_string(), "gone".to_string());

        let mut after = HashMap::new();
        after.insert("unchanged".to_string(), "same".to_string());
        after.insert("modified".to_string(), "new".to_string());
        after.insert("added".to_string(), "fresh".to_string());

        let diff = StorageInspector::compute_diff(&before, &after);
        assert_eq!(diff.added.len(), 1);
        assert_eq!(diff.modified.len(), 1);
        assert_eq!(diff.deleted.len(), 1);
        assert!(diff.added.contains_key("added"));
        assert!(diff.modified.contains_key("modified"));
        assert!(diff.deleted.contains("deleted"));
    }
}
