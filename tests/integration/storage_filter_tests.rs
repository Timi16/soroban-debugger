//! Integration tests for storage key filtering

use soroban_debugger::inspector::storage::{StorageFilter, StorageInspector};

// ── Prefix filtering ────────────────────────────────────────────────

#[test]
fn test_filter_by_prefix() {
    let mut inspector = StorageInspector::new();
    inspector.set("balance:alice", "1000");
    inspector.set("balance:bob", "500");
    inspector.set("total_supply", "1500");

    let filter = StorageFilter::new(&["balance:*".to_string()]).unwrap();
    let filtered = inspector.get_filtered(&filter);

    assert_eq!(filtered.len(), 2);
    assert!(filtered.contains_key("balance:alice"));
    assert!(filtered.contains_key("balance:bob"));
}

// ── Regex filtering ─────────────────────────────────────────────────

#[test]
fn test_filter_by_regex() {
    let mut inspector = StorageInspector::new();
    inspector.set("user_1", "alice");
    inspector.set("user_2", "bob");
    inspector.set("user_abc", "charlie");
    inspector.set("admin", "root");

    let filter = StorageFilter::new(&[r"re:^user_\d+$".to_string()]).unwrap();
    let filtered = inspector.get_filtered(&filter);

    assert_eq!(filtered.len(), 2);
    assert!(filtered.contains_key("user_1"));
    assert!(filtered.contains_key("user_2"));
    assert!(!filtered.contains_key("user_abc"));
}

// ── Exact match ─────────────────────────────────────────────────────

#[test]
fn test_filter_by_exact() {
    let mut inspector = StorageInspector::new();
    inspector.set("admin", "root");
    inspector.set("admin_backup", "root2");

    let filter = StorageFilter::new(&["admin".to_string()]).unwrap();
    let filtered = inspector.get_filtered(&filter);

    assert_eq!(filtered.len(), 1);
    assert!(filtered.contains_key("admin"));
}

// ── Multiple filters (OR logic) ─────────────────────────────────────

#[test]
fn test_multiple_filters_combine_with_or() {
    let mut inspector = StorageInspector::new();
    inspector.set("balance:alice", "1000");
    inspector.set("balance:bob", "500");
    inspector.set("total_supply", "1500");
    inspector.set("admin", "root");
    inspector.set("config_timeout", "30");

    let filter =
        StorageFilter::new(&["balance:*".to_string(), "total_supply".to_string()]).unwrap();
    let filtered = inspector.get_filtered(&filter);

    assert_eq!(filtered.len(), 3);
    assert!(filtered.contains_key("balance:alice"));
    assert!(filtered.contains_key("balance:bob"));
    assert!(filtered.contains_key("total_supply"));
}

#[test]
fn test_multiple_mixed_filter_types() {
    let mut inspector = StorageInspector::new();
    inspector.set("balance:alice", "1000");
    inspector.set("config_timeout", "30");
    inspector.set("config_retries", "5");
    inspector.set("admin", "root");
    inspector.set("user_123", "charlie");

    let filter = StorageFilter::new(&[
        "balance:*".to_string(),
        r"re:^config_\w+$".to_string(),
        "admin".to_string(),
    ])
    .unwrap();
    let filtered = inspector.get_filtered(&filter);

    assert_eq!(filtered.len(), 4);
    assert!(filtered.contains_key("balance:alice"));
    assert!(filtered.contains_key("config_timeout"));
    assert!(filtered.contains_key("config_retries"));
    assert!(filtered.contains_key("admin"));
    assert!(!filtered.contains_key("user_123"));
}

// ── Edge cases ──────────────────────────────────────────────────────

#[test]
fn test_no_filters_returns_all() {
    let mut inspector = StorageInspector::new();
    inspector.set("a", "1");
    inspector.set("b", "2");

    let filter = StorageFilter::new(&[]).unwrap();
    let filtered = inspector.get_filtered(&filter);

    assert_eq!(filtered.len(), 2);
}

#[test]
fn test_filter_no_matches() {
    let mut inspector = StorageInspector::new();
    inspector.set("total_supply", "1500");

    let filter = StorageFilter::new(&["balance:*".to_string()]).unwrap();
    let filtered = inspector.get_filtered(&filter);

    assert_eq!(filtered.len(), 0);
}

#[test]
fn test_invalid_regex_returns_error() {
    let result = StorageFilter::new(&[r"re:[invalid".to_string()]);
    assert!(result.is_err());
}

#[test]
fn test_filter_display_does_not_panic() {
    let mut inspector = StorageInspector::new();
    inspector.set("balance:alice", "1000");
    inspector.set("total_supply", "1500");

    let filter = StorageFilter::new(&["balance:*".to_string()]).unwrap();
    // Just verify display_filtered doesn't panic
    inspector.display_filtered(&filter);
}
