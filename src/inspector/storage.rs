use regex::Regex;
use std::collections::HashMap;

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
        tracing::info!(matched = matched, total = total, filter = filter.summary(), "Storage filtering complete");
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
    fn test_get_filtered_empty_filter_returns_all() {
        let mut inspector = StorageInspector::new();
        inspector.set("balance:alice", "1000");
        inspector.set("total_supply", "1500");

        let filter = StorageFilter::new(&[]).unwrap();
        let filtered = inspector.get_filtered(&filter);

        assert_eq!(filtered.len(), 2);
    }
}
