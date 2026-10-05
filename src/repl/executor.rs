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
    engine: crate::debugger::engine::DebuggerEngine,
    signatures: HashMap<String, ContractFunctionSignature>,
    address_aliases: HashMap<String, String>,
    alias_path: std::path::PathBuf,
    watch_keys: Vec<String>,
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
        let executor = ContractExecutor::new(wasm_bytes)?;
        let mut engine = crate::debugger::engine::DebuggerEngine::new(executor, Vec::new());
        engine.executor_mut().enable_mock_all_auths();

        if let Some(snapshot_path) = &config.network_snapshot {
            let loader =
                crate::simulator::SnapshotLoader::from_file(snapshot_path).map_err(|e| {
                    miette::miette!("Failed to load network snapshot {:?}: {}", snapshot_path, e)
                })?;
            let loaded = loader.apply_to_environment()?;
            engine.executor_mut().apply_snapshot_ledger(&loaded)?;
            crate::logging::log_display(loaded.format_summary(), crate::logging::LogLevel::Info);
        }

        if let Some(storage_json) = &config.storage {
            engine
                .executor_mut()
                .set_initial_storage(storage_json.clone())?;
        }

        let alias_path = dirs::home_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join(".soroban_repl_aliases.json");

        let address_aliases = if alias_path.exists() {
            fs::read_to_string(&alias_path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            HashMap::new()
        };

        Ok(ReplExecutor {
            engine,
            signatures,
            address_aliases,
            alias_path,
            watch_keys: config.watch_keys.clone(),
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

        // Check if we should break before starting
        if self.engine.breakpoints().should_break(function) {
            let storage = self.engine.executor().get_storage_snapshot()?;
            if let Some(hit) = self
                .engine
                .breakpoints_mut()
                .on_hit(function, &storage, args_ref)?
            {
                for message in hit.log_messages {
                    crate::logging::log_display(message, crate::logging::LogLevel::Info);
                }

                if hit.should_pause {
                    self.engine.prepare_breakpoint_stop(function, args_ref);
                    crate::logging::log_display(
                        format!("Execution paused at function: {}", function),
                        crate::logging::LogLevel::Warn,
                    );
                    return Ok(());
                }
            }
        }

        let storage_before = self.engine.executor().get_storage_snapshot()?;
        let result = self.engine.execute(function, args_ref)?;
        let storage_after = self.engine.executor().get_storage_snapshot()?;

        crate::logging::log_display(
            format!("Result: {}", result),
            crate::logging::LogLevel::Info,
        );

        let diff =
            StorageInspector::compute_diff(&storage_before, &storage_after, &self.watch_keys);
        if diff.is_empty() {
            crate::logging::log_display("Storage: (no changes)", crate::logging::LogLevel::Info);
        } else {
            StorageInspector::display_diff(&diff);
        }

        Ok(())
    }

    /// Return known exported function names for REPL completion.
    pub fn function_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.signatures.keys().cloned().collect();
        names.sort();
        names
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

        // If the string looks like it was meant to be a strkey (G/C prefix,
        // 56 chars) but fails full validation, surface a clear error now
        // rather than letting the host emit a confusing internal error.
        if (raw.starts_with('G') || raw.starts_with('C'))
            && raw.len() == 56
            && !crate::analyzer::security::is_valid_strkey(raw)
        {
            return Err(miette::miette!(
                "'{}' has the right length for a Stellar StrKey address but \
                 is not valid (bad base32 characters or checksum). \
                 Check for typos, or use an alias instead.",
                raw
            ));
        }

        let address = if crate::analyzer::security::is_valid_strkey(raw) {
            raw.to_string()
        } else {
            if !self.address_aliases.contains_key(raw) {
                let generated = self.engine.executor_mut().generate_repl_account_strkey()?;
                crate::logging::log_display(
                    format!("Address alias '{}' -> {}", raw, generated),
                    crate::logging::LogLevel::Info,
                );
                self.address_aliases.insert(raw.to_string(), generated);
                // Persist aliases to disk
                if let Ok(json) = serde_json::to_string_pretty(&self.address_aliases) {
                    let _ = fs::write(&self.alias_path, json);
                }
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
        let entries = self.engine.executor().get_storage_snapshot()?;

        if entries.is_empty() {
            crate::logging::log_display("Storage is empty", crate::logging::LogLevel::Warn);
            return Ok(());
        }

        crate::logging::log_display("", crate::logging::LogLevel::Info);
        crate::logging::log_display("=== Contract Storage ===", crate::logging::LogLevel::Info);
        crate::logging::log_display("", crate::logging::LogLevel::Info);

        let mut items: Vec<_> = entries.iter().collect();
        items.sort_by_key(|(ka, _)| *ka);

        for (key, value) in items {
            crate::logging::log_display(
                format!("  {}: {}", key, value),
                crate::logging::LogLevel::Info,
            );
        }
        crate::logging::log_display("", crate::logging::LogLevel::Info);

        Ok(())
    }
    pub fn add_breakpoint(&mut self, function: &str, condition: Option<&str>) -> Result<()> {
        if let Some(condition) = condition {
            self.engine.breakpoints_mut().set(
                crate::debugger::breakpoint::Breakpoint::with_condition(
                    function.to_string(),
                    condition.to_string(),
                ),
            );
        } else {
            self.engine.breakpoints_mut().add(function);
        }
        Ok(())
    }

    pub fn list_breakpoints(&self) -> Vec<crate::debugger::breakpoint::Breakpoint> {
        self.engine
            .breakpoints()
            .list_detailed()
            .into_iter()
            .cloned()
            .collect()
    }

    pub fn remove_breakpoint(&mut self, function: &str) -> bool {
        self.engine.breakpoints_mut().remove(function)
    }

    pub fn display_functions(&self) -> Result<()> {
        crate::logging::log_display("", crate::logging::LogLevel::Info);
        crate::logging::log_display("=== Contract Functions ===", crate::logging::LogLevel::Info);
        let id = self.engine.executor().contract_address();
        crate::logging::log_display(format!("Address: {:?}", id), crate::logging::LogLevel::Info);
        crate::logging::log_display("", crate::logging::LogLevel::Info);

        let mut sigs: Vec<_> = self.signatures.values().collect::<Vec<_>>();
        sigs.sort_by_key(|s| s.name.clone());

        for sig in sigs {
            let params: Vec<String> = sig
                .params
                .iter()
                .map(|p| format!("{}: {}", p.name, p.type_name))
                .collect();
            let ret = sig.return_type.as_deref().unwrap_or("()");
            crate::logging::log_display(
                format!("  {}({}) -> {}", sig.name, params.join(", "), ret),
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
