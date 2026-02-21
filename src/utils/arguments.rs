//! JSON to Soroban value parsing
//!
//! This module provides comprehensive conversion from JSON arguments to Soroban Val types.
//! It handles:
//! - Typed values via annotation: `{"type": "u32", "value": 10}`
//! - JSON objects → Soroban Map
//! - JSON arrays → Soroban Vec
//! - Primitive types (numbers, strings, booleans)
//! - Nested structures
//!
//! ## Supported Type Annotations
//!
//! | Type     | Example                                  | Description                    |
//! |----------|------------------------------------------|--------------------------------|
//! | `u32`    | `{"type": "u32", "value": 42}`           | Unsigned 32-bit integer        |
//! | `i32`    | `{"type": "i32", "value": -5}`           | Signed 32-bit integer          |
//! | `u64`    | `{"type": "u64", "value": 1000000}`      | Unsigned 64-bit integer        |
//! | `i64`    | `{"type": "i64", "value": -999}`         | Signed 64-bit integer          |
//! | `u128`   | `{"type": "u128", "value": 100}`         | Unsigned 128-bit integer       |
//! | `i128`   | `{"type": "i128", "value": 100}`         | Signed 128-bit integer         |
//! | `bool`   | `{"type": "bool", "value": true}`        | Boolean                        |
//! | `symbol` | `{"type": "symbol", "value": "hello"}`   | Soroban Symbol (≤32 chars)     |
//! | `string` | `{"type": "string", "value": "long..."}`  | Soroban String (any length)    |
//!
//! Bare values (without type annotation) still work:
//! - Numbers → `i128`
//! - Strings → `Symbol`
//! - Booleans → `Bool`

use hex;
use serde_json::Value;
use soroban_sdk::{Env, Map, String as SorobanString, Symbol, TryFromVal, Val, Vec as SorobanVec};
use thiserror::Error;
use tracing::{debug, warn};

/// Errors that can occur during argument parsing
#[derive(Debug, Error)]
pub enum ArgumentParseError {
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    #[error("Unsupported type: {0}. Supported types: u32, i32, u64, i64, u128, i128, bool, string, symbol, option, tuple")]
    UnsupportedType(String),

    #[error("Failed to convert value: {0}")]
    ConversionError(String),

    #[error("JSON parsing error: {0}")]
    JsonError(String),

    #[error("Empty arguments")]
    EmptyArguments,

    #[error("Type/value mismatch: expected {expected} but got {actual}")]
    TypeMismatch { expected: String, actual: String },

    #[error("Value out of range for type {type_name}: {value} (valid range: {min}..={max})")]
    OutOfRange {
        type_name: String,
        value: String,
        min: String,
        max: String,
    },
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
    /// - Typed annotations → `{"type": "u32", "value": 10}`
    /// - Primitive values
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // Array of typed values
    /// parser.parse_args_string(r#"[{"type": "u32", "value": 10}, {"type": "symbol", "value": "hello"}]"#)?;
    ///
    /// // Array of bare values (backward compatible)
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
    /// If the JSON is an object, it's checked for type annotation first,
    /// otherwise wrapped as a single Map argument.
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
                // Check if this is a type-annotated value
                if self.is_typed_annotation(value) {
                    debug!("Parsing type-annotated single value");
                    let val = self.parse_typed_value(value)?;
                    Ok(vec![val])
                } else {
                    debug!("Parsing object as single Map argument");
                    let map_val = self.json_to_soroban_val(value)?;
                    Ok(vec![map_val])
                }
            }
            _ => {
                debug!("Parsing single value");
                self.json_to_soroban_val(value).map(|v| vec![v])
            }
        }
    }

    /// Check if a JSON value is a type annotation object `{"type": "...", "value": ...}`
    fn is_typed_annotation(&self, value: &Value) -> bool {
        if let Value::Object(obj) = value {
            (obj.len() == 2 || (obj.len() == 3 && obj.contains_key("arity")))
                && obj.contains_key("type")
                && obj.contains_key("value")
                && obj["type"].is_string()
        } else {
            false
        }
    }

    /// Parse a type-annotated JSON value `{"type": "u32", "value": 10}`
    fn parse_typed_value(&self, value: &Value) -> Result<Val, ArgumentParseError> {
        let obj = value.as_object().ok_or_else(|| {
            ArgumentParseError::InvalidArgument("Expected object for typed value".to_string())
        })?;

        let type_name = obj["type"].as_str().ok_or_else(|| {
            ArgumentParseError::InvalidArgument("Type field must be a string".to_string())
        })?;

        let val = &obj["value"];

        match type_name {
            "u32" => self.convert_u32(val),
            "i32" => self.convert_i32(val),
            "u64" => self.convert_u64(val),
            "i64" => self.convert_i64(val),
            "u128" => self.convert_u128(val),
            "i128" => self.convert_i128(val),
            "bool" => self.convert_bool(val),
            "string" => self.convert_string(val),
            "symbol" => self.convert_symbol(val),
            "option" => self.convert_option(val),
            "tuple" => self.convert_tuple(val, obj),
            "vec" => self.convert_vec(val, obj),
            "bytes" => self.convert_bytes(val),
            "bytesn" => self.convert_bytesn(val, obj),
            other => Err(ArgumentParseError::UnsupportedType(other.to_string())),
        }
    }

    /// Convert a JSON number to u32 Val
    fn convert_u32(&self, value: &Value) -> Result<Val, ArgumentParseError> {
        let n = value
            .as_u64()
            .ok_or_else(|| ArgumentParseError::TypeMismatch {
                expected: "u32 (non-negative integer)".to_string(),
                actual: format!("{}", value),
            })?;

        if n > u32::MAX as u64 {
            return Err(ArgumentParseError::OutOfRange {
                type_name: "u32".to_string(),
                value: n.to_string(),
                min: "0".to_string(),
                max: u32::MAX.to_string(),
            });
        }

        Val::try_from_val(&self.env, &(n as u32)).map_err(|e| {
            ArgumentParseError::ConversionError(format!("Failed to convert u32 to Val: {:?}", e))
        })
    }

    /// Convert a JSON number to i32 Val
    fn convert_i32(&self, value: &Value) -> Result<Val, ArgumentParseError> {
        let n = value
            .as_i64()
            .ok_or_else(|| ArgumentParseError::TypeMismatch {
                expected: "i32 (integer)".to_string(),
                actual: format!("{}", value),
            })?;

        if n < i32::MIN as i64 || n > i32::MAX as i64 {
            return Err(ArgumentParseError::OutOfRange {
                type_name: "i32".to_string(),
                value: n.to_string(),
                min: i32::MIN.to_string(),
                max: i32::MAX.to_string(),
            });
        }

        Val::try_from_val(&self.env, &(n as i32)).map_err(|e| {
            ArgumentParseError::ConversionError(format!("Failed to convert i32 to Val: {:?}", e))
        })
    }

    /// Convert a JSON number to u64 Val
    fn convert_u64(&self, value: &Value) -> Result<Val, ArgumentParseError> {
        let n = value
            .as_u64()
            .ok_or_else(|| ArgumentParseError::TypeMismatch {
                expected: "u64 (non-negative integer)".to_string(),
                actual: format!("{}", value),
            })?;

        Val::try_from_val(&self.env, &n).map_err(|e| {
            ArgumentParseError::ConversionError(format!("Failed to convert u64 to Val: {:?}", e))
        })
    }

    /// Convert a JSON number to i64 Val
    fn convert_i64(&self, value: &Value) -> Result<Val, ArgumentParseError> {
        let n = value
            .as_i64()
            .ok_or_else(|| ArgumentParseError::TypeMismatch {
                expected: "i64 (integer)".to_string(),
                actual: format!("{}", value),
            })?;

        Val::try_from_val(&self.env, &n).map_err(|e| {
            ArgumentParseError::ConversionError(format!("Failed to convert i64 to Val: {:?}", e))
        })
    }

    /// Convert a JSON number to u128 Val
    fn convert_u128(&self, value: &Value) -> Result<Val, ArgumentParseError> {
        let n = value
            .as_u64()
            .ok_or_else(|| ArgumentParseError::TypeMismatch {
                expected: "u128 (non-negative integer)".to_string(),
                actual: format!("{}", value),
            })?;

        Val::try_from_val(&self.env, &(n as u128)).map_err(|e| {
            ArgumentParseError::ConversionError(format!("Failed to convert u128 to Val: {:?}", e))
        })
    }

    /// Convert a JSON number to i128 Val
    fn convert_i128(&self, value: &Value) -> Result<Val, ArgumentParseError> {
        let n = value
            .as_i64()
            .ok_or_else(|| ArgumentParseError::TypeMismatch {
                expected: "i128 (integer)".to_string(),
                actual: format!("{}", value),
            })?;

        Val::try_from_val(&self.env, &(n as i128)).map_err(|e| {
            ArgumentParseError::ConversionError(format!("Failed to convert i128 to Val: {:?}", e))
        })
    }

    /// Convert a JSON boolean to Bool Val
    fn convert_bool(&self, value: &Value) -> Result<Val, ArgumentParseError> {
        let b = value
            .as_bool()
            .ok_or_else(|| ArgumentParseError::TypeMismatch {
                expected: "bool".to_string(),
                actual: format!("{}", value),
            })?;

        Val::try_from_val(&self.env, &b).map_err(|e| {
            ArgumentParseError::ConversionError(format!("Failed to convert bool to Val: {:?}", e))
        })
    }

    /// Convert a JSON string to Soroban String Val (supports any length)
    fn convert_string(&self, value: &Value) -> Result<Val, ArgumentParseError> {
        let s = value
            .as_str()
            .ok_or_else(|| ArgumentParseError::TypeMismatch {
                expected: "string".to_string(),
                actual: format!("{}", value),
            })?;

        let soroban_str = SorobanString::from_str(&self.env, s);
        Val::try_from_val(&self.env, &soroban_str).map_err(|e| {
            ArgumentParseError::ConversionError(format!("Failed to convert String to Val: {:?}", e))
        })
    }

    /// Convert a JSON string to Soroban Symbol Val (max 32 chars)
    fn convert_symbol(&self, value: &Value) -> Result<Val, ArgumentParseError> {
        let s = value
            .as_str()
            .ok_or_else(|| ArgumentParseError::TypeMismatch {
                expected: "symbol (string)".to_string(),
                actual: format!("{}", value),
            })?;

        let symbol = Symbol::new(&self.env, s);
        Val::try_from_val(&self.env, &symbol).map_err(|e| {
            ArgumentParseError::ConversionError(format!("Failed to convert Symbol to Val: {:?}", e))
        })
    }

    /// Convert a JSON value to an Option Val (None if null, Some(T) otherwise)
    fn convert_option(&self, value: &Value) -> Result<Val, ArgumentParseError> {
        if value.is_null() {
            debug!("Converting option: null -> None (void)");
            Val::try_from_val(&self.env, &()).map_err(|e| {
                ArgumentParseError::ConversionError(format!(
                    "Failed to convert void to Val: {:?}",
                    e
                ))
            })
        } else {
            debug!("Converting option: {} -> Some", value);
            self.json_to_soroban_val(value)
        }
    }

    /// Convert a JSON array to a Soroban Vec with optional type enforcement
    fn convert_vec(
        &self,
        value: &Value,
        obj: &serde_json::Map<String, Value>,
    ) -> Result<Val, ArgumentParseError> {
        let arr = value
            .as_array()
            .ok_or_else(|| ArgumentParseError::TypeMismatch {
                expected: "array for vec".to_string(),
                actual: format!("{}", value),
            })?;

        let element_type = obj.get("element_type").and_then(|v| v.as_str());
        
        let mut soroban_vec = SorobanVec::<Val>::new(&self.env);

        for (i, item) in arr.iter().enumerate() {
            let val = if let Some(et) = element_type {
                // Force each element to be of the specified type
                let mut typed_item = serde_json::Map::new();
                typed_item.insert("type".to_string(), Value::String(et.to_string()));
                typed_item.insert("value".to_string(), item.clone());
                
                // If it's nested vec, we might need to handle element_type for it too.
                // For now, let's just pass the whole typed_item back to parse_typed_value.
                self.parse_typed_value(&Value::Object(typed_item)).map_err(|e| {
                    ArgumentParseError::ConversionError(format!(
                        "Vector element {} does not match element_type '{}': {}",
                        i, et, e
                    ))
                })?
            } else {
                self.json_to_soroban_val(item)?
            };
            soroban_vec.push_back(val);
        }

        Ok(soroban_vec.into())
    }

    /// Convert a JSON array to a Soroban tuple (fixed length array)
    fn convert_tuple(
        &self,
        value: &Value,
        obj: &serde_json::Map<String, Value>,
    ) -> Result<Val, ArgumentParseError> {
        let arr = value
            .as_array()
            .ok_or_else(|| ArgumentParseError::TypeMismatch {
                expected: "array for tuple".to_string(),
                actual: format!("{}", value),
            })?;

        if let Some(arity) = obj.get("arity") {
            let expected_arity = arity.as_u64().ok_or_else(|| {
                ArgumentParseError::InvalidArgument("Arity must be a number".to_string())
            })?;

            if arr.len() as u64 != expected_arity {
                return Err(ArgumentParseError::InvalidArgument(format!(
                    "Tuple arity mismatch: expected {}, got {}",
                    expected_arity,
                    arr.len()
                )));
            }
        }

        self.array_to_soroban_vec(arr)
    }

    fn decode_bytes_string(&self, s: &str) -> Result<Vec<u8>, ArgumentParseError> {
        if let Some(hex_part) = s.strip_prefix("0x") {
            hex::decode(hex_part).map_err(|e| {
                ArgumentParseError::InvalidArgument(format!("Invalid hex string: {}", e))
            })
        } else if let Some(b64_part) = s.strip_prefix("base64:") {
            use base64::{engine::general_purpose, Engine};
            general_purpose::STANDARD.decode(b64_part).map_err(|e| {
                ArgumentParseError::InvalidArgument(format!("Invalid base64 string: {}", e))
            })
        } else {
            Err(ArgumentParseError::InvalidArgument(
                "Bytes must start with '0x' or 'base64:'".to_string(),
            ))
        }
    }

    fn convert_bytes(&self, value: &Value) -> Result<Val, ArgumentParseError> {
        let s = value
            .as_str()
            .ok_or_else(|| ArgumentParseError::TypeMismatch {
                expected: "string for bytes".to_string(),
                actual: format!("{}", value),
            })?;
        let bytes = self.decode_bytes_string(s)?;
        let soroban_bytes = soroban_sdk::Bytes::from_slice(&self.env, &bytes);
        Val::try_from_val(&self.env, &soroban_bytes).map_err(|e| {
            ArgumentParseError::ConversionError(format!("Failed to convert Bytes: {:?}", e))
        })
    }

    fn convert_bytesn(
        &self,
        value: &Value,
        obj: &serde_json::Map<String, Value>,
    ) -> Result<Val, ArgumentParseError> {
        let s = value
            .as_str()
            .ok_or_else(|| ArgumentParseError::TypeMismatch {
                expected: "string for bytesn".to_string(),
                actual: format!("{}", value),
            })?;
        let bytes = self.decode_bytes_string(s)?;
        let expected_length = obj.get("length").and_then(|l| l.as_u64()).ok_or_else(|| {
            ArgumentParseError::InvalidArgument("BytesN requires a 'length' field".to_string())
        })? as usize;

        if bytes.len() != expected_length {
            return Err(ArgumentParseError::InvalidArgument(format!(
                "BytesN length mismatch: expected {}, got {}",
                expected_length,
                bytes.len()
            )));
        }

        let soroban_bytes = soroban_sdk::Bytes::from_slice(&self.env, &bytes);
        Val::try_from_val(&self.env, &soroban_bytes).map_err(|e| {
            ArgumentParseError::ConversionError(format!("Failed to convert BytesN: {:?}", e))
        })
    }

    /// Convert a JSON value to a Soroban Val (bare values without type annotation)
    fn json_to_soroban_val(&self, json_value: &Value) -> Result<Val, ArgumentParseError> {
        match json_value {
            Value::Null => {
                debug!("Converting null to void (Option::None)");
                Val::try_from_val(&self.env, &()).map_err(|e| {
                    ArgumentParseError::ConversionError(format!(
                        "Failed to convert void to Val: {:?}",
                        e
                    ))
                })
            }
            Value::Bool(b) => {
                debug!("Converting bool: {}", b);
                Val::try_from_val(&self.env, b).map_err(|e| {
                    ArgumentParseError::ConversionError(format!(
                        "Failed to convert bool to Val: {:?}",
                        e
                    ))
                })
            }
            Value::Number(num) => {
                debug!("Converting number: {}", num);
                // Default bare numbers to i128 (backward compatible)
                if let Some(i) = num.as_i64() {
                    Val::try_from_val(&self.env, &(i as i128)).map_err(|e| {
                        ArgumentParseError::ConversionError(format!(
                            "Failed to convert i128 to Val: {:?}",
                            e
                        ))
                    })
                } else if let Some(u) = num.as_u64() {
                    if u > i128::MAX as u64 {
                        return Err(ArgumentParseError::ConversionError(format!(
                            "Number {} exceeds i128::MAX",
                            u
                        )));
                    }
                    Val::try_from_val(&self.env, &(u as i128)).map_err(|e| {
                        ArgumentParseError::ConversionError(format!(
                            "Failed to convert i128 to Val: {:?}",
                            e
                        ))
                    })
                } else if let Some(f) = num.as_f64() {
                    Err(ArgumentParseError::UnsupportedType(format!(
                        "Floating point numbers are not supported in Soroban: {}",
                        f
                    )))
                } else {
                    Err(ArgumentParseError::ConversionError(format!(
                        "Cannot convert number to i128: {}",
                        num
                    )))
                }
            }
            Value::String(s) => {
                debug!("Converting string to Symbol: {}", s);
                // Default bare strings to Symbol (backward compatible)
                let symbol = Symbol::new(&self.env, s);
                Val::try_from_val(&self.env, &symbol).map_err(|e| {
                    ArgumentParseError::ConversionError(format!(
                        "Failed to convert Symbol to Val: {:?}",
                        e
                    ))
                })
            }
            Value::Array(arr) => {
                debug!("Converting array with {} elements to Vec", arr.len());
                self.array_to_soroban_vec(arr)
            }
            Value::Object(obj) => {
                // Check if this is a type-annotated value inside an array/nested structure
                if self.is_typed_annotation(json_value) {
                    debug!("Converting type-annotated value");
                    self.parse_typed_value(json_value)
                } else {
                    debug!("Converting object with {} fields to Map", obj.len());
                    self.object_to_soroban_map(obj)
                }
            }
        }
    }

    /// Convert a JSON array to a Soroban Vec (vector type)
    fn array_to_soroban_vec(&self, arr: &[Value]) -> Result<Val, ArgumentParseError> {
        let mut soroban_vec = SorobanVec::<Val>::new(&self.env);
        let mut first_type: Option<String> = None;

        for (i, item) in arr.iter().enumerate() {
            let val = self.json_to_soroban_val(item).map_err(|e| {
                warn!("Failed to convert array element {}: {}", i, e);
                ArgumentParseError::ConversionError(format!(
                    "Cannot convert array element {} to Soroban value: {}",
                    i, e
                ))
            })?;

            // Optional: Enforce homogeneity for bare arrays by comparing JSON value types
            // This meets the "Clear errors for mixed types" requirement.
            let current_type = self.get_json_type_name(item);
            if let Some(ref expected) = first_type {
                if *expected != current_type {
                    return Err(ArgumentParseError::TypeMismatch {
                        expected: format!("homogeneous array of {}", expected),
                        actual: format!("mixed array with {} at index {}", current_type, i),
                    });
                }
            } else {
                first_type = Some(current_type);
            }

            soroban_vec.push_back(val);
        }

        Ok(soroban_vec.into())
    }

    fn get_json_type_name(&self, value: &Value) -> String {
        match value {
            Value::Null => "null".to_string(),
            Value::Bool(_) => "bool".to_string(),
            Value::Number(_) => "number".to_string(),
            Value::String(_) => "string".to_string(),
            Value::Array(_) => "array".to_string(),
            Value::Object(obj) => {
                if self.is_typed_annotation(value) {
                    obj["type"].as_str().unwrap_or("typed").to_string()
                } else {
                    "object".to_string()
                }
            }
        }
    }

    /// Convert a JSON object to a Soroban Map
    ///
    /// Supports string keys only (converted to Symbols).
    /// Values can be any supported type: numbers, booleans, strings,
    /// arrays (Vec), nested objects (Map), type-annotated values, etc.
    fn object_to_soroban_map(
        &self,
        obj: &serde_json::Map<String, Value>,
    ) -> Result<Val, ArgumentParseError> {
        let mut soroban_map = Map::<Symbol, Val>::new(&self.env);

        for (key, value) in obj.iter() {
            let key_symbol = Symbol::new(&self.env, key);

            let val = self.json_to_soroban_val(value).map_err(|e| {
                warn!("Failed to convert map value for key '{}': {}", key, e);
                ArgumentParseError::ConversionError(format!(
                    "Cannot convert value for key '{}' to Soroban value: {}",
                    key, e
                ))
            })?;

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

    // ── Backward-compatible bare value tests ─────────────────────────

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
        let result = parser.parse_args_string("3.14");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("Floating point"),
            "Expected float error, got: {}",
            err
        );
    }

    // ── Typed annotation tests ───────────────────────────────────────

    #[test]
    fn test_typed_u32() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"[{"type": "u32", "value": 42}]"#);
        assert!(result.is_ok());
        let vals = result.unwrap();
        assert_eq!(vals.len(), 1);
    }

    #[test]
    fn test_typed_u32_zero() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"[{"type": "u32", "value": 0}]"#);
        assert!(result.is_ok());
    }

    #[test]
    fn test_typed_u32_max() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"[{"type": "u32", "value": 4294967295}]"#);
        assert!(result.is_ok());
    }

    #[test]
    fn test_typed_u32_overflow() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"[{"type": "u32", "value": 4294967296}]"#);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("out of range")
                || err.to_string().contains("Out of range")
                || err.to_string().contains("u32"),
            "Expected range error for u32, got: {}",
            err
        );
    }

    #[test]
    fn test_typed_u32_negative_rejected() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"[{"type": "u32", "value": -1}]"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_typed_i32() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"[{"type": "i32", "value": -42}]"#);
        assert!(result.is_ok());
        let vals = result.unwrap();
        assert_eq!(vals.len(), 1);
    }

    #[test]
    fn test_typed_i32_max() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"[{"type": "i32", "value": 2147483647}]"#);
        assert!(result.is_ok());
    }

    #[test]
    fn test_typed_i32_min() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"[{"type": "i32", "value": -2147483648}]"#);
        assert!(result.is_ok());
    }

    #[test]
    fn test_typed_i32_overflow() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"[{"type": "i32", "value": 2147483648}]"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_typed_u64() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"[{"type": "u64", "value": 1000000}]"#);
        assert!(result.is_ok());
        let vals = result.unwrap();
        assert_eq!(vals.len(), 1);
    }

    #[test]
    fn test_typed_i64() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"[{"type": "i64", "value": -999}]"#);
        assert!(result.is_ok());
        let vals = result.unwrap();
        assert_eq!(vals.len(), 1);
    }

    #[test]
    fn test_typed_u128() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"[{"type": "u128", "value": 100}]"#);
        assert!(result.is_ok());
    }

    #[test]
    fn test_typed_i128() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"[{"type": "i128", "value": -100}]"#);
        assert!(result.is_ok());
    }

    #[test]
    fn test_typed_bool() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"[{"type": "bool", "value": true}]"#);
        assert!(result.is_ok());
        let vals = result.unwrap();
        assert_eq!(vals.len(), 1);
    }

    #[test]
    fn test_typed_bool_false() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"[{"type": "bool", "value": false}]"#);
        assert!(result.is_ok());
    }

    #[test]
    fn test_typed_symbol() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"[{"type": "symbol", "value": "hello"}]"#);
        assert!(result.is_ok());
        let vals = result.unwrap();
        assert_eq!(vals.len(), 1);
    }

    #[test]
    fn test_typed_string() {
        let parser = create_parser();
        let result = parser
            .parse_args_string(r#"[{"type": "string", "value": "a long string value here"}]"#);
        assert!(result.is_ok());
        let vals = result.unwrap();
        assert_eq!(vals.len(), 1);
    }

    #[test]
    fn test_typed_unsupported_type() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"[{"type": "unknown_type", "value": "abc"}]"#);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("Unsupported type")
                || err.to_string().contains("Supported types"),
            "Expected unsupported type error, got: {}",
            err
        );
    }

    #[test]
    fn test_typed_type_value_mismatch_bool_as_u32() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"[{"type": "u32", "value": true}]"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_typed_type_value_mismatch_number_as_string() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"[{"type": "string", "value": 42}]"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_typed_type_value_mismatch_string_as_bool() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"[{"type": "bool", "value": "true"}]"#);
        assert!(result.is_err());
    }

    // ── Mixed typed and bare values ──────────────────────────────────

    #[test]
    fn test_mixed_typed_and_bare_in_array() {
        let parser = create_parser();
        let result =
            parser.parse_args_string(r#"[{"type": "u32", "value": 10}, "hello", true, 42]"#);
        assert!(result.is_ok());
        let vals = result.unwrap();
        assert_eq!(vals.len(), 4);
    }

    #[test]
    fn test_multiple_typed_values_in_array() {
        let parser = create_parser();
        let result = parser.parse_args_string(
            r#"[{"type": "u32", "value": 10}, {"type": "i64", "value": -100}, {"type": "bool", "value": true}]"#,
        );
        assert!(result.is_ok());
        let vals = result.unwrap();
        assert_eq!(vals.len(), 3);
    }

    #[test]
    fn test_typed_annotation_as_top_level() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"{"type": "u32", "value": 10}"#);
        assert!(result.is_ok());
        let vals = result.unwrap();
        assert_eq!(vals.len(), 1);
    }

    // ── Object vs typed annotation disambiguation ────────────────────

    #[test]
    fn test_object_with_type_key_but_extra_fields_is_map() {
        let parser = create_parser();
        // Has "type" and "value" but also "extra", so NOT a typed annotation
        let result = parser.parse_args_string(r#"{"type": "u32", "value": 10, "extra": "field"}"#);
        assert!(result.is_ok());
        let vals = result.unwrap();
        assert_eq!(vals.len(), 1); // Treated as a Map
    }

    #[test]
    fn test_object_with_non_string_type_is_map() {
        let parser = create_parser();
        // "type" is not a string, so NOT a typed annotation
        let result = parser.parse_args_string(r#"{"type": 123, "value": 10}"#);
        assert!(result.is_ok());
        let vals = result.unwrap();
        assert_eq!(vals.len(), 1); // Treated as a Map
    }

    #[test]
    fn test_typed_option_none() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"[{"type": "option", "value": null}]"#);
        assert!(result.is_ok());
        let vals = result.unwrap();
        assert!(vals[0].is_void());
    }

    #[test]
    fn test_typed_option_some() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"[{"type": "option", "value": 42}]"#);
        assert!(result.is_ok());
    }

    #[test]
    fn test_typed_tuple() {
        let parser = create_parser();
        let result =
            parser.parse_args_string(r#"[{"type": "tuple", "value": [1, "hello"], "arity": 2}]"#);
        assert!(result.is_ok());
    }

    #[test]
    fn test_typed_tuple_wrong_arity() {
        let parser = create_parser();
        let result =
            parser.parse_args_string(r#"[{"type": "tuple", "value": [1, 2, 3], "arity": 2}]"#);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("arity mismatch"));
    }

    #[test]
    fn test_bare_null_is_void() {
        let parser = create_parser();
        let result = parser.parse_args_string("[null]");
        assert!(result.is_ok());
        let vals = result.unwrap();
        assert!(vals[0].is_void());
    }

    #[test]
    fn test_typed_address() {
        let parser = create_parser();
        let addr = "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAADUI";
        let json = format!(r#"[{{ "type": "address", "value": "{}" }}]"#, addr);
        let result = parser.parse_args_string(&json);
        assert!(result.is_ok(), "Failed to parse typed address: {:?}", result.err());
    }

    #[test]
    fn test_bare_address_detection() {
        let parser = create_parser();
        let addr = "GD3IYSAL6Z2A3A4A3A4A3A4A3A4A3A4A3A4A3A4A3A4A3A4A3A4A3A4A";
        let json = format!(r#"["{}"]"#, addr);
        let result = parser.parse_args_string(&json);
        assert!(result.is_ok(), "Failed to detect bare address: {:?}", result.err());
    }

    #[test]
    fn test_invalid_address_error() {
        let parser = create_parser();
        let json = r#"[{"type": "address", "value": "too-short"}]"#;
        let result = parser.parse_args_string(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid address"));
    }

    #[test]
    fn test_typed_vec_u32() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"[{"type": "vec", "element_type": "u32", "value": [1, 2, 3]}]"#);
        assert!(result.is_ok());
    }

    #[test]
    fn test_typed_vec_homogeneity_enforcement() {
        let parser = create_parser();
        // Item at index 2 is a string, but u32 is expected
        let result = parser.parse_args_string(r#"[{"type": "vec", "element_type": "u32", "value": [1, 2, "three"]}]"#);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("does not match element_type 'u32'"));
    }

    #[test]
    fn test_bare_vec_homogeneity_enforcement() {
        let parser = create_parser();
        // Bare array with mixed types should fail
        let result = parser.parse_args_string(r#"[ [1, 2, "three"] ]"#);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("mixed array with string at index 2"));
    }

    #[test]
    fn test_nested_vec_bare() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"[ [[1, 2], [3, 4]] ]"#);
        assert!(result.is_ok());
    }

    #[test]
    fn test_nested_vec_typed() {
        let parser = create_parser();
        let result = parser.parse_args_string(r#"[ {"type": "vec", "element_type": "vec", "value": [[1, 2], [3, 4]]} ]"#);
        assert!(result.is_ok());
    }
}
