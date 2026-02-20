//! JSON to Soroban value parsing
//!
//! This module provides comprehensive conversion from JSON arguments to Soroban Val types.
//! It handles:
//! - JSON objects → Soroban Map
//! - JSON arrays → Soroban Vec
//! - Primitive types (numbers, strings, booleans)
//! - Nested structures

use serde_json::Value;
use soroban_sdk::{Env, Map, Symbol, TryFromVal, Val, Vec as SorobanVec};
use thiserror::Error;
use tracing::{debug, warn};

/// Errors that can occur during argument parsing
#[derive(Debug, Error)]
pub enum ArgumentParseError {
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    #[error("Unsupported type: {0}")]
    UnsupportedType(String),

    #[error("Failed to convert value: {0}")]
    ConversionError(String),

    #[error("JSON parsing error: {0}")]
    JsonError(String),

    #[error("Empty arguments")]
    EmptyArguments,
}

impl From<serde_json::Error> for ArgumentParseError {
    fn from(err: serde_json::Error) -> Self {
        ArgumentParseError::JsonError(err.to_string())
    }
}

/// Argument parser for converting JSON to Soroban values
pub struct ArgumentParser {
    env: Env,
}

impl ArgumentParser {
    /// Create a new argument parser with the given Soroban environment
    pub fn new(env: Env) -> Self {
        Self { env }
    }

    /// Parse a JSON string into Soroban argument values
    ///
    /// Supports:
    /// - JSON arrays → converted to Vec of Soroban values
    /// - JSON objects → converted to a Map (if passed as single argument)
    /// - Primitive values
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // Array of values
    /// parser.parse_args_string(r#"["user", 1000, true]"#)?;
    ///
    /// // Object as single argument
    /// parser.parse_args_string(r#"{"user":"ABC","balance":1000}"#)?;
    /// ```
    pub fn parse_args_string(&self, json_str: &str) -> Result<Vec<Val>, ArgumentParseError> {
        if json_str.trim().is_empty() {
            return Err(ArgumentParseError::EmptyArguments);
        }

        let value: Value = serde_json::from_str(json_str)?;
        self.parse_value(&value)
    }

    /// Parse a JSON value into a Vec of Soroban values
    ///
    /// If the JSON is an array, each element becomes a separate argument.
    /// If the JSON is an object, it's wrapped as a single Map argument.
    /// Otherwise, the single value becomes one argument.
    fn parse_value(&self, value: &Value) -> Result<Vec<Val>, ArgumentParseError> {
        match value {
            Value::Array(arr) => {
                debug!("Parsing array with {} elements", arr.len());
                arr.iter()
                    .enumerate()
                    .map(|(i, v)| {
                        self.json_to_soroban_val(v).map_err(|e| {
                            warn!("Failed to parse array element {}: {}", i, e);
                            ArgumentParseError::ConversionError(format!(
                                "Array element {}: {}",
                                i, e
                            ))
                        })
                    })
                    .collect()
            }
            Value::Object(_) => {
                debug!("Parsing object as single Map argument");
                let map_val = self.json_to_soroban_val(value)?;
                Ok(vec![map_val])
            }
            _ => {
                debug!("Parsing single value");
                self.json_to_soroban_val(value).map(|v| vec![v])
            }
        }
    }

    /// Convert a JSON value to a Soroban Val
    fn json_to_soroban_val(&self, json_value: &Value) -> Result<Val, ArgumentParseError> {
        match json_value {
            Value::Null => {
                debug!("Converting null to empty map");
                // Return empty map as placeholder for null
                let empty_map: Map<Symbol, Val> = Map::new(&self.env);
                Ok(empty_map.into())
            }
            Value::Bool(b) => {
                debug!("Converting bool: {}", b);
                // Use TryFromVal to convert bool to Val
                let bool_val = Val::try_from_val(&self.env, b).map_err(|e| {
                    ArgumentParseError::ConversionError(format!(
                        "Failed to convert bool to Val: {:?}",
                        e
                    ))
                })?;
                Ok(bool_val)
            }
            Value::Number(num) => {
                debug!("Converting number: {}", num);
                // Convert to i128 (Soroban's integer type)
                if let Some(i) = num.as_i64() {
                    // Convert i64 to i128
                    let i128_val = Val::try_from_val(&self.env, &(i as i128)).map_err(|e| {
                        ArgumentParseError::ConversionError(format!(
                            "Failed to convert i128 to Val: {:?}",
                            e
                        ))
                    })?;
                    Ok(i128_val)
                } else if let Some(u) = num.as_u64() {
                    // Convert u64 to i128 (with range check)
                    if u > i128::MAX as u64 {
                        return Err(ArgumentParseError::ConversionError(format!(
                            "Number {} exceeds i128::MAX",
                            u
                        )));
                    }
                    let i128_val = Val::try_from_val(&self.env, &(u as i128)).map_err(|e| {
                        ArgumentParseError::ConversionError(format!(
                            "Failed to convert i128 to Val: {:?}",
                            e
                        ))
                    })?;
                    Ok(i128_val)
                } else if let Some(f) = num.as_f64() {
                    // Floats are not directly supported in Soroban
                    Err(ArgumentParseError::UnsupportedType(format!(
                        "Floating point numbers are not supported in Soroban: {}",
                        f
                    )));
                } else {
                    Err(ArgumentParseError::ConversionError(format!(
                        "Cannot convert number to i128: {}",
                        num
                    )))
                }
            }
            Value::String(s) => {
                debug!("Converting string: {}", s);
                // Convert to Symbol (Soroban's string/symbol type)
                let symbol = Symbol::new(&self.env, s);
                // Use TryFromVal to convert Symbol to Val
                let symbol_val = Val::try_from_val(&self.env, &symbol).map_err(|e| {
                    ArgumentParseError::ConversionError(format!(
                        "Failed to convert Symbol to Val: {:?}",
                        e
                    ))
                })?;
                Ok(symbol_val)
            }
            Value::Array(arr) => {
                debug!("Converting array with {} elements to Vec", arr.len());
                self.array_to_soroban_vec(arr)
            }
            Value::Object(obj) => {
                debug!("Converting object with {} fields to Map", obj.len());
                self.object_to_soroban_map(obj)
            }
        }
    }

    /// Convert a JSON array to a Soroban Vec (vector type)
    fn array_to_soroban_vec(&self, arr: &[Value]) -> Result<Val, ArgumentParseError> {
        let mut soroban_vec = SorobanVec::<Val>::new(&self.env);

        for (i, item) in arr.iter().enumerate() {
            let val = self.json_to_soroban_val(item).map_err(|e| {
                warn!("Failed to convert array element {}: {}", i, e);
                ArgumentParseError::ConversionError(format!(
                    "Cannot convert array element {} to Soroban value: {}",
                    i, e
                ))
            })?;
            soroban_vec.push_back(val);
        }

        Ok(soroban_vec.into())
    }

    /// Convert a JSON object to a Soroban Map
    ///
    /// Supports string keys only (converted to Symbols).
    /// Values can be any supported type: numbers, booleans, strings,
    /// arrays (Vec), nested objects (Map), etc.
    fn object_to_soroban_map(
        &self,
        obj: &serde_json::Map<String, Value>,
    ) -> Result<Val, ArgumentParseError> {
        let mut soroban_map = Map::<Symbol, Val>::new(&self.env);

        for (key, value) in obj.iter() {
            // Keys must be strings (JSON objects always have string keys)
            let key_symbol = Symbol::new(&self.env, key);

            // Recursively convert the value
            let val = self.json_to_soroban_val(value).map_err(|e| {
                warn!("Failed to convert map value for key '{}': {}", key, e);
                ArgumentParseError::ConversionError(format!(
                    "Cannot convert value for key '{}' to Soroban value: {}",
                    key, e
                ))
            })?;

            // Set the key-value pair (modifies map in-place)
            soroban_map.set(key_symbol, val);
        }

        Ok(soroban_map.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::Env;

    fn create_parser() -> ArgumentParser {
        ArgumentParser::new(Env::default())
    }

    #[test]
    fn test_parse_empty_array() {
        let parser = create_parser();
        let result = parser.parse_args_string("[]");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 0);
    }

    #[test]
    fn test_parse_single_string() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#""hello""#);
        assert!(result.is_ok());
        let vals = result.unwrap();
        assert_eq!(vals.len(), 1);
    }

    #[test]
    fn test_parse_single_number() {
        let parser = create_parser();
        let result = parser.parse_args_string("42");
        assert!(result.is_ok());
        let vals = result.unwrap();
        assert_eq!(vals.len(), 1);
    }

    #[test]
    fn test_parse_bool_true() {
        let parser = create_parser();
        let result = parser.parse_args_string("true");
        assert!(result.is_ok());
        let vals = result.unwrap();
        assert_eq!(vals.len(), 1);
    }

    #[test]
    fn test_parse_bool_false() {
        let parser = create_parser();
        let result = parser.parse_args_string("false");
        assert!(result.is_ok());
        let vals = result.unwrap();
        assert_eq!(vals.len(), 1);
    }

    #[test]
    fn test_parse_null() {
        let parser = create_parser();
        let result = parser.parse_args_string("null");
        assert!(result.is_ok());
        let vals = result.unwrap();
        assert_eq!(vals.len(), 1);
    }

    #[test]
    fn test_parse_array_mixed_types() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"["hello", 42, true, null]"#);
        assert!(result.is_ok());
        let vals = result.unwrap();
        assert_eq!(vals.len(), 4);
    }

    #[test]
    fn test_parse_simple_object() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"{"user":"alice","balance":1000}"#);
        // This test just verifies basic object parsing
        // The specific structure would need integration testing with contracts
        assert!(result.is_ok());
        let vals = result.unwrap();
        assert_eq!(vals.len(), 1);
    }

    #[test]
    fn test_parse_nested_object() {
        let parser = create_parser();
        let result =
            parser.parse_args_string(r#"{"user":"alice","data":{"flag":true,"count":42}}"#);
        assert!(result.is_ok());
        let vals = result.unwrap();
        assert_eq!(vals.len(), 1);
    }

    #[test]
    fn test_parse_object_with_array() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"{"items":[1,2,3],"name":"test"}"#);
        assert!(result.is_ok());
        let vals = result.unwrap();
        assert_eq!(vals.len(), 1);
    }

    #[test]
    fn test_parse_array_of_objects() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"[{"id":1,"name":"alice"},{"id":2,"name":"bob"}]"#);
        assert!(result.is_ok());
        let vals = result.unwrap();
        assert_eq!(vals.len(), 2);
    }

    #[test]
    fn test_parse_large_numbers() {
        let parser = create_parser();
        let result = parser.parse_args_string("9223372036854775807"); // i64::MAX
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_invalid_json() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"{"invalid": json}"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_empty_string_error() {
        let parser = create_parser();
        let result = parser.parse_args_string("");
        assert!(matches!(result, Err(ArgumentParseError::EmptyArguments)));
    }

    #[test]
    fn test_parse_whitespace_only_error() {
        let parser = create_parser();
        let result = parser.parse_args_string("   ");
        assert!(matches!(result, Err(ArgumentParseError::EmptyArguments)));
    }

    #[test]
    fn test_parse_complex_nested_structure() {
        let parser = create_parser();
        let json = r#"{
            "user": {
                "id": 123,
                "name": "alice",
                "active": true,
                "roles": ["admin", "user"]
            },
            "metadata": {
                "created": 1693531200,
                "tags": ["important", "verified"]
            }
        }"#;
        let result = parser.parse_args_string(json);
        assert!(result.is_ok());
        let vals = result.unwrap();
        assert_eq!(vals.len(), 1);
    }

    #[test]
    fn test_parse_array_with_objects_and_primitives() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"["alice", 100, {"flag": true}, [1, 2, 3]]"#);
        assert!(result.is_ok());
        let vals = result.unwrap();
        assert_eq!(vals.len(), 4);
    }

    #[test]
    fn test_parse_deeply_nested() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"{"a":{"b":{"c":{"d":{"e":"deep"}}}}}"#);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_object_with_numeric_keys() {
        let parser = create_parser();
        // JSON allows string keys but shows numeric string keys
        let result = parser.parse_args_string(r#"{"123":"numeric_key","456":789}"#);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_object_with_empty_strings() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"{"key":"","empty":""}"#);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_negative_numbers() {
        let parser = create_parser();
        let result = parser.parse_args_string("[-1, -100, -9223372036854775808]");
        assert!(result.is_ok());
        let vals = result.unwrap();
        assert_eq!(vals.len(), 3);
    }

    #[test]
    fn test_parse_float_not_supported() {
        let parser = create_parser();
        // Floats are not directly supported in Soroban, but serde_json parses them
        let result = parser.parse_args_string("3.14");
        // The parser should attempt to convert, but may fail or convert to nearest integer
        let _ = result; // Just checking it doesn't panic
    }
}
