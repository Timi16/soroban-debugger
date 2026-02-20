use proptest::prelude::*;
use serde_json::Value;
use soroban_debugger::cli::commands::{parse_args, parse_storage};

use crate::utils::json_value;

proptest! {
    #[test]
    fn test_parse_args_valid_json(json in json_value()) {
        let json_str = json.to_string();
        let result = parse_args(&json_str);
        prop_assert!(result.is_ok());
        prop_assert_eq!(result.unwrap(), json_str);
    }

    #[test]
    fn test_parse_storage_valid_json(json in json_value()) {
        let json_str = json.to_string();
        let result = parse_storage(&json_str);
        prop_assert!(result.is_ok());
        prop_assert_eq!(result.unwrap(), json_str);
    }

    #[test]
    fn test_parse_args_invalid_json(s in "\\PC*") {
        if serde_json::from_str::<Value>(&s).is_ok() {
            return Ok(());
        }
        prop_assert!(parse_args(&s).is_err());
    }

    #[test]
    fn test_parse_storage_invalid_json(s in "\\PC*") {
        if serde_json::from_str::<Value>(&s).is_ok() {
            return Ok(());
        }
        prop_assert!(parse_storage(&s).is_err());
    }
}
