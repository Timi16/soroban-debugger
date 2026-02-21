//! Integration tests for argument parsing
//!
//! These tests validate the full argument parsing pipeline from JSON string
//! to Soroban Val values, testing the public API of ArgumentParser.

use soroban_debugger::utils::ArgumentParser;
use soroban_sdk::Env;

fn create_parser() -> ArgumentParser {
    ArgumentParser::new(Env::default())
}

// ── Typed numeric parsing ────────────────────────────────────────────

#[test]
fn test_parse_u32_typed_argument() {
    let parser = create_parser();
    let result = parser.parse_args_string(r#"[{"type": "u32", "value": 10}]"#);
    assert!(result.is_ok(), "Failed to parse u32: {:?}", result.err());
    assert_eq!(result.unwrap().len(), 1);
}

#[test]
fn test_parse_i32_typed_argument() {
    let parser = create_parser();
    let result = parser.parse_args_string(r#"[{"type": "i32", "value": -5}]"#);
    assert!(result.is_ok(), "Failed to parse i32: {:?}", result.err());
    assert_eq!(result.unwrap().len(), 1);
}

#[test]
fn test_parse_u64_typed_argument() {
    let parser = create_parser();
    let result = parser.parse_args_string(r#"[{"type": "u64", "value": 1000000}]"#);
    assert!(result.is_ok(), "Failed to parse u64: {:?}", result.err());
    assert_eq!(result.unwrap().len(), 1);
}

#[test]
fn test_parse_i64_typed_argument() {
    let parser = create_parser();
    let result = parser.parse_args_string(r#"[{"type": "i64", "value": -999}]"#);
    assert!(result.is_ok(), "Failed to parse i64: {:?}", result.err());
    assert_eq!(result.unwrap().len(), 1);
}

// ── Boolean parsing ──────────────────────────────────────────────────

#[test]
fn test_parse_bool_true_bare() {
    let parser = create_parser();
    let result = parser.parse_args_string("[true]");
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 1);
}

#[test]
fn test_parse_bool_false_bare() {
    let parser = create_parser();
    let result = parser.parse_args_string("[false]");
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 1);
}

#[test]
fn test_parse_bool_typed() {
    let parser = create_parser();
    let result = parser.parse_args_string(r#"[{"type": "bool", "value": true}]"#);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 1);
}

// ── String and Symbol parsing ────────────────────────────────────────

#[test]
fn test_parse_symbol_bare() {
    let parser = create_parser();
    let result = parser.parse_args_string(r#"["hello"]"#);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 1);
}

#[test]
fn test_parse_symbol_typed() {
    let parser = create_parser();
    let result = parser.parse_args_string(r#"[{"type": "symbol", "value": "hello"}]"#);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 1);
}

#[test]
fn test_parse_string_typed() {
    let parser = create_parser();
    let result =
        parser.parse_args_string(r#"[{"type": "string", "value": "a longer string value"}]"#);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 1);
}

// ── Mixed arguments (simulating real contract calls) ─────────────────

#[test]
fn test_parse_counter_add_args() {
    // Simulate: soroban-debug run --contract counter.wasm --function add --args '[10]'
    let parser = create_parser();
    let result = parser.parse_args_string("[10]");
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 1);
}

#[test]
fn test_parse_counter_add_typed_args() {
    // Simulate: counter add with explicit u32 type
    let parser = create_parser();
    let result = parser.parse_args_string(r#"[{"type": "u32", "value": 10}]"#);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 1);
}

#[test]
fn test_parse_transfer_args() {
    // Simulate: token transfer with mixed types
    let parser = create_parser();
    let result = parser.parse_args_string(
        r#"[{"type": "symbol", "value": "Alice"}, {"type": "symbol", "value": "Bob"}, {"type": "u64", "value": 100}]"#,
    );
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 3);
}

#[test]
fn test_parse_mixed_typed_and_bare() {
    let parser = create_parser();
    let result = parser.parse_args_string(r#"[{"type": "u32", "value": 42}, "hello", true, 100]"#);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 4);
}

// ── Error handling ───────────────────────────────────────────────────

#[test]
fn test_error_unsupported_type() {
    let parser = create_parser();
    let result = parser.parse_args_string(r#"[{"type": "unknown_type", "value": "abc"}]"#);
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("Unsupported type") || err_msg.contains("unknown_type"),
        "Expected unsupported type error, got: {}",
        err_msg
    );
}

#[test]
fn test_error_u32_out_of_range() {
    let parser = create_parser();
    let result = parser.parse_args_string(r#"[{"type": "u32", "value": 5000000000}]"#);
    assert!(result.is_err());
}

#[test]
fn test_error_i32_out_of_range() {
    let parser = create_parser();
    let result = parser.parse_args_string(r#"[{"type": "i32", "value": 3000000000}]"#);
    assert!(result.is_err());
}

#[test]
fn test_error_type_value_mismatch() {
    let parser = create_parser();
    // u32 expects a number, not a string
    let result = parser.parse_args_string(r#"[{"type": "u32", "value": "hello"}]"#);
    assert!(result.is_err());
}

#[test]
fn test_error_bool_type_mismatch() {
    let parser = create_parser();
    let result = parser.parse_args_string(r#"[{"type": "bool", "value": "yes"}]"#);
    assert!(result.is_err());
}

#[test]
fn test_error_invalid_json() {
    let parser = create_parser();
    let result = parser.parse_args_string("not valid json");
    assert!(result.is_err());
}

#[test]
fn test_error_empty_args() {
    let parser = create_parser();
    let result = parser.parse_args_string("");
    assert!(result.is_err());
}

#[test]
fn test_error_float_not_supported() {
    let parser = create_parser();
    let result = parser.parse_args_string("[3.14]");
    assert!(result.is_err());
}

// ── Edge cases ───────────────────────────────────────────────────────

#[test]
fn test_boundary_u32_max() {
    let parser = create_parser();
    let result = parser.parse_args_string(r#"[{"type": "u32", "value": 4294967295}]"#);
    assert!(result.is_ok());
}

#[test]
fn test_boundary_u32_zero() {
    let parser = create_parser();
    let result = parser.parse_args_string(r#"[{"type": "u32", "value": 0}]"#);
    assert!(result.is_ok());
}

#[test]
fn test_boundary_i32_min() {
    let parser = create_parser();
    let result = parser.parse_args_string(r#"[{"type": "i32", "value": -2147483648}]"#);
    assert!(result.is_ok());
}

#[test]
fn test_boundary_i32_max() {
    let parser = create_parser();
    let result = parser.parse_args_string(r#"[{"type": "i32", "value": 2147483647}]"#);
    assert!(result.is_ok());
}

#[test]
fn test_empty_array_returns_no_args() {
    let parser = create_parser();
    let result = parser.parse_args_string("[]");
    assert!(result.is_ok());
    assert_eq!(result.unwrap().len(), 0);
}

#[test]
fn test_empty_string_typed() {
    let parser = create_parser();
    let result = parser.parse_args_string(r#"[{"type": "string", "value": ""}]"#);
    assert!(result.is_ok());
}
