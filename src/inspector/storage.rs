use anyhow::{Context, Result};
use crossterm::style::{Color, Stylize};
use regex::Regex;
use serde::{Deserialize, Serialize};
use soroban_env_host::Host;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

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

/// Storage state snapshot for import/export
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StorageState {
    pub entries: HashMap<String, String>,
}

impl StorageState {
    /// Export storage state to JSON file
    pub fn export_to_file<P: AsRef<Path>>(
        entries: &HashMap<String, String>,
        path: P,
    ) -> Result<()> {
        let state = StorageState {
            entries: entries.clone(),
        };
        let json =
            serde_json::to_string_pretty(&state).context("Failed to serialize storage state")?;
        fs::write(path.as_ref(), json).context("Failed to write storage file")?;
        Ok(())
    }

    /// Import storage state from JSON file
    pub fn import_from_file<P: AsRef<Path>>(path: P) -> Result<HashMap<String, String>> {
        let contents = fs::read_to_string(path.as_ref()).context("Failed to read storage file")?;
        let state: StorageState =
            serde_json::from_str(&contents).context("Failed to parse storage JSON")?;
        Ok(state.entries)
    }
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
    // Tracks frequency of key reads
    reads: HashMap<String, usize>,
    // Tracks frequency of key writes
    writes: HashMap<String, usize>,
}

impl StorageInspector {
    pub fn new() -> Self {
        Self {
            storage: HashMap::new(),
            reads: HashMap::new(),
            writes: HashMap::new(),
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
            tracing::info!("Storage is empty");
            return;
        }

        tracing::info!(entries = self.storage.len(), "Storage entries");
        for (key, value) in &self.storage {
            tracing::debug!(key, value, "Storage entry");
        }
    }

    /// Display storage filtered by the given patterns.
    /// Prints a notice when filtering is active.
    pub fn display_filtered(&self, filter: &StorageFilter) {
        if self.storage.is_empty() {
            tracing::info!("Storage is empty");
            return;
        }

        let mut matched = 0;
        let keys: Vec<&String> = self.storage.keys().collect();

        for key in keys {
            if filter.matches(key) {
                tracing::debug!(key, value = self.storage[key], "Filtered storage entry");
                matched += 1;
            }
        }

        if matched == 0 && !filter.is_empty() {
            tracing::info!("No storage entries matched the filter");
        }

        let total = self.storage.len();
        tracing::info!(
            matched = matched,
            total = total,
            filter = filter.summary(),
            "Storage filtering complete"
        );
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
        let k = key.into();
        let v = value.into();
        self.storage.insert(k.clone(), v.clone());
        self.track_write(&k);
    }

    /// Record a read access for a key
    pub fn track_read(&mut self, key: &str) {
        *self.reads.entry(key.to_string()).or_insert(0) += 1;
    }

    /// Record a write access for a key
    pub fn track_write(&mut self, key: &str) {
        *self.writes.entry(key.to_string()).or_insert(0) += 1;
    }

    /// Analyze access patterns
    pub fn analyze_access_patterns(&self) -> AccessPatternReport {
        let mut stats: HashMap<String, AccessStats> = HashMap::new();

        for (k, v) in &self.reads {
            stats.entry(k.clone()).or_default().reads = *v;
        }

        for (k, v) in &self.writes {
            stats.entry(k.clone()).or_default().writes = *v;
        }

        let mut hot_read_keys = Vec::new();
        let mut write_heavy_keys = Vec::new();
        let mut read_never_written = Vec::new();

        for (key, stat) in &stats {
            if stat.reads > 5 {
                hot_read_keys.push(key.clone());
            }
            if stat.writes > stat.reads {
                write_heavy_keys.push(key.clone());
            }
            if stat.reads > 0 && stat.writes == 0 {
                read_never_written.push(key.clone());
            }
        }

        AccessPatternReport {
            stats,
            hot_read_keys,
            write_heavy_keys,
            read_never_written,
        }
    }

    /// Display visually sorted table of access patterns
    pub fn display_access_report(&self) {
        let report = self.analyze_access_patterns();
        if report.stats.is_empty() {
            println!("No storage access patterns recorded.");
            return;
        }

        println!("\nStorage Access Pattern Report");
        println!(
            "{:<30} | {:<10} | {:<10} | {:<20}",
            "Key", "Reads", "Writes", "Notes"
        );
        println!("{:-<30}-+-{:-<10}-+-{:-<10}-+-{:-<20}", "", "", "", "");

        let mut entries: Vec<_> = report.stats.into_iter().collect();
        // Sort primarily by highest reads, then highest writes, then alphabetically
        entries.sort_by(|a, b| {
            b.1.reads
                .cmp(&a.1.reads)
                .then_with(|| b.1.writes.cmp(&a.1.writes))
                .then_with(|| a.0.cmp(&b.0))
        });

        for (key, stat) in entries {
            let mut notes = Vec::new();
            if stat.reads > 5 {
                notes.push("HOT READ (Suggest Caching)");
            } else if stat.reads > 0 && stat.writes == 0 {
                notes.push("READ-ONLY");
            }
            if stat.writes > stat.reads {
                notes.push("WRITE-HEAVY");
            }

            let display_notes = if notes.is_empty() {
                "".to_string()
            } else {
                notes.join(", ")
            };

            let key_display = if key.len() > 30 {
                format!("{}...", &key[0..27])
            } else {
                key.clone()
            };

            println!(
                "{:<30} | {:<10} | {:<10} | {}",
                key_display.with(Color::Cyan),
                stat.reads.to_string().with(if stat.reads > 5 {
                    Color::Red
                } else {
                    Color::White
                }),
                stat.writes.to_string().with(if stat.writes > stat.reads {
                    Color::Yellow
                } else {
                    Color::White
                }),
                display_notes.with(Color::DarkGrey)
            );
        }
        println!();
    }

    /// Capture a snapshot of all storage entries from the host
    pub fn capture_snapshot(_host: &Host) -> HashMap<String, String> {
        // In a real implementation, we would iterate through host.get_ledger_entries()
        // or track changes via a custom Storage instance.
        // For this debugger, we'll try to extract what's available.
        // Since Host doesn't easily expose all entries without XDR iteration,
        // we'll use a placeholder logic that would be backed by actual storage tracking
        // in a production environment.

        // NOTE: In Soroban host, entries are typically accessed by key.
        // To show "everything", we'd need to have tracked access during execution.

        HashMap::new()
    }

    /// Compute the difference between two storage snapshots
    pub fn compute_diff(
        before: &HashMap<String, String>,
        after: &HashMap<String, String>,
        alerts: &[String],
    ) -> StorageDiff {
        let mut added = HashMap::new();
        let mut modified = HashMap::new();
        let mut deleted = Vec::new();
        let mut triggered_alerts = Vec::new();

        let alert_filter = StorageFilter::new(alerts).unwrap_or_else(|e| {
            tracing::warn!("Invalid alert pattern: {}", e);
            StorageFilter::new(&[]).unwrap()
        });

        for (key, val_after) in after {
            match before.get(key) {
                Some(val_before) => {
                    if val_before != val_after {
                        modified.insert(key.clone(), (val_before.clone(), val_after.clone()));
                        if alert_filter.matches(key) {
                            triggered_alerts.push(key.clone());
                        }
                    }
                }
                None => {
                    added.insert(key.clone(), val_after.clone());
                    if alert_filter.matches(key) {
                        triggered_alerts.push(key.clone());
                    }
                }
            }
        }

        for key in before.keys() {
            if !after.contains_key(key) {
                deleted.push(key.clone());
                if alert_filter.matches(key) {
                    triggered_alerts.push(key.clone());
                }
            }
        }

        StorageDiff {
            added,
            modified,
            deleted,
            triggered_alerts,
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
            println!(
                "  {} {} = {}",
                "+".with(Color::Green),
                key,
                diff.added[key].clone().with(Color::Green)
            );
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

        if !diff.triggered_alerts.is_empty() {
            println!(
                "\n{}",
                "!!! CRITICAL STORAGE ALERT !!!".with(Color::Red).bold()
            );
            let mut alerts = diff.triggered_alerts.clone();
            alerts.sort();
            for key in alerts {
                println!("  {} was modified!", key.with(Color::Red).bold());
            }
        }
    }
}

/// Represents the differences between two storage states
#[derive(Debug, Clone, Default)]
pub struct StorageDiff {
    pub added: HashMap<String, String>,
    pub modified: HashMap<String, (String, String)>,
    pub deleted: Vec<String>,
    pub triggered_alerts: Vec<String>,
}

impl StorageDiff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.modified.is_empty() && self.deleted.is_empty()
    }
}

/// Statistics for a single storage access key
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccessStats {
    pub reads: usize,
    pub writes: usize,
}

/// Report containing an analysis of storage access patterns
#[derive(Debug, Clone)]
pub struct AccessPatternReport {
    pub stats: HashMap<String, AccessStats>,
    pub hot_read_keys: Vec<String>,
    pub write_heavy_keys: Vec<String>,
    pub read_never_written: Vec<String>,
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

        let diff = StorageInspector::compute_diff(&before, &after, &[]);
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

        let diff = StorageInspector::compute_diff(&before, &after, &[]);
        assert!(diff.added.is_empty());
        assert_eq!(
            diff.modified.get("key1"),
            Some(&("val_old".to_string(), "val_new".to_string()))
        );
        assert!(diff.deleted.is_empty());
    }

    #[test]
    fn test_storage_diff_deleted() {
        let mut before = HashMap::new();
        before.insert("key1".to_string(), "val1".to_string());
        let after = HashMap::new();

        let diff = StorageInspector::compute_diff(&before, &after, &[]);
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

        let diff = StorageInspector::compute_diff(&before, &after, &[]);
        assert_eq!(diff.added.len(), 1);
        assert_eq!(diff.modified.len(), 1);
        assert_eq!(diff.deleted.len(), 1);
        assert!(diff.added.contains_key("added"));
        assert!(diff.modified.contains_key("modified"));
        assert!(diff.deleted.contains(&"deleted".to_string()));
    }

    // ── StorageState import/export tests ─────────────────────────────

    #[test]
    fn test_storage_export_import() {
        use tempfile::NamedTempFile;

        let mut entries = HashMap::new();
        entries.insert("key1".to_string(), "value1".to_string());
        entries.insert("key2".to_string(), "value2".to_string());

        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        StorageState::export_to_file(&entries, path).unwrap();
        let imported = StorageState::import_from_file(path).unwrap();

        assert_eq!(imported.len(), 2);
        assert_eq!(imported.get("key1"), Some(&"value1".to_string()));
        assert_eq!(imported.get("key2"), Some(&"value2".to_string()));
    }

    #[test]
    fn test_storage_export_empty() {
        use tempfile::NamedTempFile;

        let entries = HashMap::new();
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path();

        StorageState::export_to_file(&entries, path).unwrap();
        let imported = StorageState::import_from_file(path).unwrap();

        assert_eq!(imported.len(), 0);
    }

    #[test]
    fn test_storage_import_invalid_json() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut temp_file = NamedTempFile::new().unwrap();
        write!(temp_file, "invalid json").unwrap();

        let result = StorageState::import_from_file(temp_file.path());
        assert!(result.is_err());
    }

    // ── Storage Access Pattern Analyzer tests ────────────────────────

    #[test]
    fn test_track_read_and_write() {
        let mut inspector = StorageInspector::new();
        inspector.track_read("key1");
        inspector.track_read("key1");
        inspector.track_write("key1");
        inspector.track_write("key2");

        let report = inspector.analyze_access_patterns();
        assert_eq!(report.stats.get("key1").unwrap().reads, 2);
        assert_eq!(report.stats.get("key1").unwrap().writes, 1);
        assert_eq!(report.stats.get("key2").unwrap().reads, 0);
        assert_eq!(report.stats.get("key2").unwrap().writes, 1);

        // testing via set method
        inspector.set("key3", "val3");
        let report = inspector.analyze_access_patterns();
        assert_eq!(report.stats.get("key3").unwrap().writes, 1);
    }

    #[test]
    fn test_analyze_hot_keys_and_suggestions() {
        let mut inspector = StorageInspector::new();

        // Hot read key
        for _ in 0..6 {
            inspector.track_read("hot_key");
        }

        // Write heavy key
        inspector.track_write("write_heavy");
        inspector.track_write("write_heavy");
        inspector.track_read("write_heavy");

        // Read but never written
        inspector.track_read("read_only");
        inspector.track_read("read_only");

        let report = inspector.analyze_access_patterns();

        assert!(report.hot_read_keys.contains(&"hot_key".to_string()));
        assert!(!report.hot_read_keys.contains(&"read_only".to_string()));

        assert!(report.write_heavy_keys.contains(&"write_heavy".to_string()));
        assert!(!report.write_heavy_keys.contains(&"hot_key".to_string()));

        assert!(report.read_never_written.contains(&"read_only".to_string()));
        // hot_key had 0 writes so it should actually be in read_never_written too
        assert!(report.read_never_written.contains(&"hot_key".to_string()));
    }

    #[test]
    fn test_display_access_report() {
        // Just verify it runs without panicking with sorted items
        let mut inspector = StorageInspector::new();

        for _ in 0..10 {
            inspector.track_read("config:global");
        }
        for _ in 0..3 {
            inspector.track_write("user:alice:balance");
        }
        inspector.track_read("user:alice:balance");
        inspector.track_read("user:bob:balance");

        inspector.display_access_report();
    }
}
