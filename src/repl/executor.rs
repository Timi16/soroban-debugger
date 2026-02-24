/// REPL command execution
///
/// Handles execution of function calls and storage inspection
/// against the loaded contract.
use super::ReplConfig;
use crate::inspector::StorageInspector;
use crate::runtime::executor::ContractExecutor;
use crate::utils::wasm::{parse_function_signatures, ContractFunctionSignature};
use crate::Result;
use serde_json::json;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;

/// Executor for REPL commands
pub struct ReplExecutor {
    executor: ContractExecutor,
    signatures: HashMap<String, ContractFunctionSignature>,
    address_aliases: HashMap<String, String>,
}

impl ReplExecutor {
    /// Create a new REPL executor
    pub fn new(config: &ReplConfig) -> Result<Self> {
        let wasm_bytes = fs::read(&config.contract_path).map_err(|_e| {
            miette::miette!(
                "Failed to read contract WASM file: {:?}",
                config.contract_path
            )
        })?;
        let signatures = parse_function_signatures(&wasm_bytes)?
            .into_iter()
            .map(|sig| (sig.name.clone(), sig))
            .collect();
        let mut executor = ContractExecutor::new(wasm_bytes)?;
        executor.enable_mock_all_auths();

        if let Some(storage_json) = &config.storage {
            // Best-effort for parity with the rest of the CLI. The underlying
            // executor currently treats this as a no-op placeholder.
            executor.set_initial_storage(storage_json.clone())?;
        }

        Ok(ReplExecutor {
            executor,
            signatures,
            address_aliases: HashMap::new(),
        })
    }

    /// Call a contract function
    pub async fn call_function(&mut self, function: &str, args: Vec<String>) -> Result<()> {
        let args_json = self.args_to_json_array_for(function, &args)?;
        let args_ref = if args_json == "[]" {
            None
        } else {
            Some(args_json.as_str())
        };

        let storage_before = self.executor.get_storage_snapshot()?;
        let result = self.executor.execute(function, args_ref)?;
        let storage_after = self.executor.get_storage_snapshot()?;

        crate::logging::log_display(
            format!("Result: {}", result),
            crate::logging::LogLevel::Info,
        );

        let diff = StorageInspector::compute_diff(&storage_before, &storage_after, &[]);
        if diff.is_empty() {
            crate::logging::log_display("Storage: (no changes)", crate::logging::LogLevel::Info);
        } else {
            StorageInspector::display_diff(&diff);
        }

        Ok(())
    }

    fn args_to_json_array_for(&mut self, function: &str, args: &[String]) -> Result<String> {
        let values = if let Some(sig) = self.signatures.get(function).cloned() {
            self.typed_repl_args(&sig, args)?
        } else {
            args.iter()
                .map(|arg| parse_repl_arg(arg))
                .collect::<std::result::Result<Vec<_>, _>>()?
        };

        serde_json::to_string(&values)
            .map_err(|e| miette::miette!("Failed to serialize REPL arguments: {}", e))
    }

    fn typed_repl_args(
        &mut self,
        signature: &ContractFunctionSignature,
        args: &[String],
    ) -> Result<Vec<Value>> {
        let mut values = Vec::with_capacity(args.len());

        for (idx, raw) in args.iter().enumerate() {
            let typed = signature.params.get(idx).map(|p| p.type_name.as_str());
            let value = match typed {
                Some("Address") => self.parse_address_arg(raw)?,
                Some("String") => parse_typed_string_arg(raw),
                Some("Symbol") => parse_typed_symbol_arg(raw),
                _ => parse_repl_arg(raw)?,
            };
            values.push(value);
        }

        Ok(values)
    }

    fn parse_address_arg(&mut self, raw: &str) -> Result<Value> {
        // Allow explicit JSON/typed annotations to pass through unchanged.
        if let Ok(v) = serde_json::from_str::<Value>(raw) {
            return Ok(v);
        }

        let address = if looks_like_strkey_address(raw) {
            raw.to_string()
        } else {
            if !self.address_aliases.contains_key(raw) {
                let generated = self.executor.generate_repl_account_strkey()?;
                crate::logging::log_display(
                    format!("Address alias '{}' -> {}", raw, generated),
                    crate::logging::LogLevel::Info,
                );
                self.address_aliases.insert(raw.to_string(), generated);
            }
            self.address_aliases
                .get(raw)
                .cloned()
                .ok_or_else(|| miette::miette!("Failed to resolve address alias: {}", raw))?
        };

        Ok(json!({
            "type": "address",
            "value": address,
        }))
    }

    /// Inspect and display contract storage
    pub fn inspect_storage(&self) -> Result<()> {
        let entries = self.executor.get_storage_snapshot()?;

        if entries.is_empty() {
            crate::logging::log_display("Storage is empty", crate::logging::LogLevel::Warn);
            return Ok(());
        }

        crate::logging::log_display("", crate::logging::LogLevel::Info);
        crate::logging::log_display("=== Contract Storage ===", crate::logging::LogLevel::Info);
        crate::logging::log_display("", crate::logging::LogLevel::Info);

        let mut items: Vec<_> = entries.iter().collect();
        items.sort_by(|(ka, _), (kb, _)| ka.cmp(kb));

        for (key, value) in items {
            crate::logging::log_display(
                format!("  {}: {}", key, value),
                crate::logging::LogLevel::Info,
            );
        }
        crate::logging::log_display("", crate::logging::LogLevel::Info);

        Ok(())
    }
}

fn parse_repl_arg(arg: &str) -> Result<Value> {
    match serde_json::from_str::<Value>(arg) {
        Ok(value) => Ok(value),
        Err(_) => Ok(Value::String(arg.to_string())),
    }
}

fn looks_like_strkey_address(s: &str) -> bool {
    let first = s.as_bytes().first().copied();
    matches!(first, Some(b'G') | Some(b'C')) && s.len() >= 10
}

fn parse_typed_string_arg(raw: &str) -> Value {
    if let Ok(v) = serde_json::from_str::<Value>(raw) {
        return v;
    }

    json!({
        "type": "string",
        "value": raw,
    })
}

fn parse_typed_symbol_arg(raw: &str) -> Value {
    if let Ok(v) = serde_json::from_str::<Value>(raw) {
        return v;
    }

    json!({
        "type": "symbol",
        "value": raw,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repl_args_default_to_strings() {
        let values: Vec<Value> = ["Alice", "Bob"]
            .iter()
            .map(|s| parse_repl_arg(s))
            .collect::<Result<_>>()
            .unwrap();
        let json = serde_json::to_string(&values).unwrap();
        assert_eq!(json, "[\"Alice\",\"Bob\"]");
    }

    #[test]
    fn repl_args_parse_json_literals() {
        let values: Vec<Value> = ["100", "true", "{\"type\":\"u32\",\"value\":7}"]
            .iter()
            .map(|s| parse_repl_arg(s))
            .collect::<Result<_>>()
            .unwrap();
        let json = serde_json::to_string(&values).unwrap();
        assert_eq!(json, "[100,true,{\"type\":\"u32\",\"value\":7}]");
    }

    #[test]
    fn typed_string_arg_uses_string_annotation() {
        let value = parse_typed_string_arg("MTK");
        assert_eq!(value, json!({"type":"string","value":"MTK"}));
    }
}
