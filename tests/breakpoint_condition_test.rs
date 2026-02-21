#![cfg(any())]
use soroban_debugger::debugger::breakpoint::{BreakpointManager, Condition, Operator};
use std::collections::HashMap;

#[test]
fn test_parse_storage_condition() {
    let cond = BreakpointManager::parse_condition("storage[balance] > 100").unwrap();
    match cond {
        Condition::Storage {
            key,
            operator,
            value,
        } => {
            assert_eq!(key, "balance");
            assert_eq!(operator, Operator::Gt);
            assert_eq!(value, "100");
        }
        _ => panic!("Expected storage condition"),
    }
}

#[test]
fn test_parse_argument_condition() {
    let cond = BreakpointManager::parse_condition("amount >= 500").unwrap();
    match cond {
        Condition::Argument {
            name,
            operator,
            value,
        } => {
            assert_eq!(name, "amount");
            assert_eq!(operator, Operator::Ge);
            assert_eq!(value, "500");
        }
        _ => panic!("Expected argument condition"),
    }
}

#[test]
fn test_evaluate_storage_condition() {
    let mut manager = BreakpointManager::new();
    let cond = BreakpointManager::parse_condition("storage[user1] == Bob").unwrap();
    manager.add("test_func", Some(cond));

    let mut storage = HashMap::new();
    storage.insert("user1".to_string(), "Bob".to_string());

    assert!(manager.should_break("test_func", &storage, None));

    storage.insert("user1".to_string(), "Alice".to_string());
    assert!(!manager.should_break("test_func", &storage, None));
}

#[test]
fn test_evaluate_numeric_condition() {
    let mut manager = BreakpointManager::new();
    let cond = BreakpointManager::parse_condition("amount > 1000").unwrap();
    manager.add("test_func", Some(cond));

    let storage = HashMap::new();

    // Test with JSON args
    assert!(manager.should_break("test_func", &storage, Some("{\"amount\": 1500}")));
    assert!(!manager.should_break("test_func", &storage, Some("{\"amount\": 500}")));
    assert!(!manager.should_break("test_func", &storage, Some("{\"amount\": 1000}")));
    // > 1000 is false for 1000
}

#[test]
fn test_invalid_condition_syntax() {
    assert!(BreakpointManager::parse_condition("amount").is_err());
    assert!(BreakpointManager::parse_condition("storage[balance]").is_err());
}
