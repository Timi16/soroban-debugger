use crate::inspector::budget::{MemorySummary, MemoryTracker};
use crate::runtime::mocking::MockRegistry;
use crate::utils::ArgumentParser;
use crate::{runtime::mocking::MockCallLogEntry, runtime::mocking::MockContractDispatcher};
use crate::{DebuggerError, Result};

use indicatif::{ProgressBar, ProgressStyle};
use soroban_env_host::xdr::ScVal;
use soroban_env_host::{DiagnosticLevel, Host, TryFromVal};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{Address, Env, InvokeError, Symbol, Val, Vec as SorobanVec};

use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex};
use tracing::{info, warn};

/// Represents a captured execution trace.
#[derive(Debug, Clone)]
pub struct ExecutionRecord {
    pub function: String,
    pub args: Vec<ScVal>,
    pub result: std::result::Result<ScVal, String>,
    pub storage_before: HashMap<String, String>,
    pub storage_after: HashMap<String, String>,
}

/// Storage snapshot for dry-run rollback.
#[derive(Clone)]
pub struct StorageSnapshot {
    pub storage: soroban_env_host::storage::Storage,
}

/// Re-export for convenience
pub use crate::runtime::mocking::MockCallLogEntry as MockCallEntry;

/// Structure to hold instruction counts per function
#[derive(Debug, Clone, serde::Serialize)]
pub struct InstructionCounts {
    pub function_counts: Vec<(String, u64)>,
    pub total: u64,
}

use crate::debugger::error_db::ErrorDatabase;

/// Executes Soroban contracts in a test environment.
pub struct ContractExecutor {
    env: Env,
    contract_address: Address,
    last_execution: Option<ExecutionRecord>,
    last_memory_summary: Option<MemorySummary>,
    mock_registry: Arc<Mutex<MockRegistry>>,
    wasm_bytes: Vec<u8>,
    timeout_secs: u64,
    error_db: ErrorDatabase,
    storage_inspector: crate::inspector::storage::StorageInspector,
}

impl ContractExecutor {
    /// Create a new contract executor.
    #[tracing::instrument(skip_all)]
    pub fn new(wasm: Vec<u8>) -> Result<Self> {
        info!("Initializing contract executor");

        // Create progress bar for WASM loading
        let pb = ProgressBar::new(100);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {msg}")
                .unwrap()
                .progress_chars("#>-"),
        );
        pb.set_message("Loading WASM contract...");

        // Use a guard to ensure progress bar is always cleared
        struct ProgressGuard(ProgressBar);
        impl Drop for ProgressGuard {
            fn drop(&mut self) {
                self.0.finish_and_clear();
            }
        }
        let _guard = ProgressGuard(pb);

        let env = Env::default();
        env.host()
            .set_diagnostic_level(DiagnosticLevel::Debug)
            .expect("Failed to set diagnostic level");

        // Simulate progress during WASM registration
        _guard.0.set_position(50);
        _guard.0.set_message("Registering contract...");

        let contract_address = env.register(wasm.as_slice(), ());

        let mut error_db = ErrorDatabase::new();
        if let Err(e) = error_db.load_custom_errors_from_wasm(&wasm) {
            warn!("Failed to load custom errors from spec: {}", e);
        }
        // Complete the progress bar
        _guard.0.set_position(100);
        _guard.0.set_message("Contract loaded successfully");

        Ok(Self {
            env,
            contract_address,
            last_execution: None,
            last_memory_summary: None,
            mock_registry: Arc::new(Mutex::new(MockRegistry::default())),
            wasm_bytes: wasm,
            timeout_secs: 30,
            error_db,
            storage_inspector: crate::inspector::storage::StorageInspector::new(),
        })
    }

    pub fn set_timeout(&mut self, secs: u64) {
        self.timeout_secs = secs;
    }

    /// Enable auth mocking for interactive/test-like execution flows (e.g. REPL).
    pub fn enable_mock_all_auths(&self) {
        self.env.mock_all_auths();
    }

    /// Generate a test account address (StrKey) for REPL shorthand aliases.
    pub fn generate_repl_account_strkey(&self) -> Result<String> {
        use soroban_sdk::testutils::Address as _;

        let addr = Address::generate(&self.env);
        let debug = format!("{:?}", addr);
        for token in debug
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .filter(|s| !s.is_empty())
        {
            if (token.starts_with('G') || token.starts_with('C')) && token.len() >= 10 {
                return Ok(token.to_string());
            }
        }

        Err(DebuggerError::ExecutionError(format!(
            "Failed to format generated REPL address alias (debug={debug})"
        ))
        .into())
    }

    /// Execute a contract function.
    #[tracing::instrument(skip(self), fields(function = function))]
    pub fn execute(&mut self, function: &str, args: Option<&str>) -> Result<String> {
        info!("Executing function: {}", function);
        let mut memory_tracker = MemoryTracker::new(
            self.env
                .host()
                .budget_cloned()
                .get_mem_bytes_consumed()
                .unwrap_or(0),
        );
        memory_tracker.record_snapshot(self.env.host(), "execute:start");

        // Create spinner for contract execution
        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.green} {msg}")
                .unwrap()
                .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ "),
        );
        spinner.set_message(format!("Executing function: {}...", function));
        spinner.enable_steady_tick(std::time::Duration::from_millis(100));

        // Validate function existence
        let exported_functions = match crate::utils::wasm::parse_functions(&self.wasm_bytes) {
            Ok(funcs) => funcs,
            Err(e) => {
                spinner.finish_and_clear();
                return Err(e);
            }
        };
        if !exported_functions.contains(&function.to_string()) {
            spinner.finish_and_clear();
            return Err(DebuggerError::InvalidFunction(function.to_string()).into());
        }

        // Convert function name to Symbol
        let func_symbol = Symbol::new(&self.env, function);

        let parsed_args = if let Some(args_json) = args {
            match self.parse_args(function, args_json) {
                Ok(args) => args,
                Err(e) => {
                    spinner.finish_and_clear();
                    return Err(e);
                }
            }
        } else {
            vec![]
        };
        memory_tracker.record_snapshot(self.env.host(), "execute:parse_args");

        let args_vec = if parsed_args.is_empty() {
            SorobanVec::<Val>::new(&self.env)
        } else {
            SorobanVec::from_slice(&self.env, &parsed_args)
        };
        memory_tracker.record_snapshot(self.env.host(), "execute:build_args_vec");

        // Capture storage before
        let storage_before = match self.get_storage_snapshot() {
            Ok(snapshot) => snapshot,
            Err(e) => {
                spinner.finish_and_clear();
                return Err(e);
            }
        };
        memory_tracker.record_snapshot(self.env.host(), "execute:storage_before");

        // Convert args to ScVal for record
        let sc_args: Vec<ScVal> = match parsed_args
            .iter()
            .map(|v| ScVal::try_from_val(self.env.host(), v))
            .collect::<std::result::Result<Vec<_>, _>>()
        {
            Ok(args) => args,
            Err(e) => {
                spinner.finish_and_clear();
                return Err(DebuggerError::ExecutionError(format!(
                    "Failed to convert arguments to ScVal: {:?}",
                    e
                ))
                .into());
            }
        };
        memory_tracker.record_snapshot(self.env.host(), "execute:convert_args");

        let (tx, rx) = std::sync::mpsc::channel();
        if self.timeout_secs > 0 {
            let timeout_secs = self.timeout_secs;
            std::thread::spawn(move || {
                match rx.recv_timeout(std::time::Duration::from_secs(timeout_secs)) {
                    Ok(_) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        tracing::error!(
                            "Contract execution timed out after {} seconds.",
                            timeout_secs
                        );
                        std::process::exit(124);
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {}
                }
            });
        }

        // Call the contract
        let invocation_result = self.env.try_invoke_contract::<Val, InvokeError>(
            &self.contract_address,
            &func_symbol,
            args_vec,
        );
        memory_tracker.record_snapshot(self.env.host(), "execute:invoke");

        // Clear spinner after execution
        spinner.finish_and_clear();

        // Capture storage after
        let storage_after = match self.get_storage_snapshot() {
            Ok(snapshot) => snapshot,
            Err(e) => {
                // Spinner already cleared, just return error
                return Err(e);
            }
        };
        memory_tracker.record_snapshot(self.env.host(), "execute:storage_after");

        let (display_result, record_result) = match &invocation_result {
            Ok(Ok(val)) => {
                info!("Function executed successfully");
                let sc_val = ScVal::try_from_val(self.env.host(), val).map_err(|e| {
                    DebuggerError::ExecutionError(format!("Result conversion failed: {:?}", e))
                })?;
                (Ok(format!("{:?}", val)), Ok(sc_val))
            }
            Ok(Err(conv_err)) => {
                warn!("Return value conversion failed: {:?}", conv_err);
                let err_msg = format!("Return value conversion failed: {:?}", conv_err);
                (
                    Err(DebuggerError::ExecutionError(err_msg.clone()).into()),
                    Err(err_msg),
                )
            }
            Err(Ok(inv_err)) => {
                let err_msg = match inv_err {
                    InvokeError::Contract(code) => {
                        warn!("Contract returned error code: {}", code);
                        self.error_db.display_error(*code);
                        format!("The contract returned an error code: {}. This typically indicates a business logic failure (e.g. `panic!` or `require!`).", code)
                    }
                    InvokeError::Abort => {
                        warn!("Contract execution aborted");
                        "Contract execution was aborted. This could be due to a trap, budget exhaustion, or an explicit abort call.".to_string()
                    }
                };
                (
                    Err(DebuggerError::ExecutionError(err_msg.clone()).into()),
                    Err(err_msg),
                )
            }
            Err(Err(inv_err)) => {
                warn!("Invocation error conversion failed: {:?}", inv_err);
                let err_msg = format!("Invocation failed with internal error: {:?}", inv_err);
                (
                    Err(DebuggerError::ExecutionError(err_msg.clone()).into()),
                    Err(err_msg),
                )
            }
        };
        memory_tracker.record_snapshot(self.env.host(), "execute:result_convert");

        let _ = tx.send(());

        // Display budget usage and warnings
        crate::inspector::BudgetInspector::display(self.env.host());
        let memory_summary = memory_tracker.finalize(self.env.host());
        memory_summary.display();

        // Update storage access patterns from diagnostic events
        if let Ok(diagnostic_events) = self.get_diagnostic_events() {
            self.storage_inspector.track_from_diagnostic_events(diagnostic_events);
        }

        self.last_execution = Some(ExecutionRecord {
            function: function.to_string(),
            args: sc_args,
            result: record_result,
            storage_before,
            storage_after,
        });
        self.last_memory_summary = Some(memory_summary);

        display_result
    }

    /// Get the last execution record, if any.
    pub fn last_execution(&self) -> Option<&ExecutionRecord> {
        self.last_execution.as_ref()
    }

    /// Get the last memory summary captured during execution, if any.
    pub fn last_memory_summary(&self) -> Option<&MemorySummary> {
        self.last_memory_summary.as_ref()
    }

    /// Set initial storage state.
    pub fn set_initial_storage(&mut self, _storage_json: String) -> Result<()> {
        info!("Setting initial storage (not yet implemented)");
        Ok(())
    }

    pub fn set_mock_specs(&mut self, specs: &[String]) -> Result<()> {
        let registry = MockRegistry::from_cli_specs(&self.env, specs)?;
        self.set_mock_registry(registry)
    }

    pub fn set_mock_registry(&mut self, registry: MockRegistry) -> Result<()> {
        self.mock_registry = Arc::new(Mutex::new(registry));
        self.install_mock_dispatchers()
    }

    pub fn get_mock_call_log(&self) -> Vec<MockCallLogEntry> {
        match self.mock_registry.lock() {
            Ok(registry) => registry.calls().to_vec(),
            Err(_) => Vec::new(),
        }
    }

    /// Get instruction counts per function (stub - not yet implemented)
    pub fn get_instruction_counts(&self) -> Result<InstructionCounts> {
        // Placeholder implementation - instruction counting not yet implemented
        Ok(InstructionCounts {
            function_counts: Vec::new(),
            total: 0,
        })
    }

    /// Get the host instance.
    pub fn host(&self) -> &Host {
        self.env.host()
    }

    /// Get the storage inspector.
    pub fn storage_inspector(&self) -> &crate::inspector::storage::StorageInspector {
        &self.storage_inspector
    }

    /// Get the authorization tree from the environment.
    pub fn get_auth_tree(&self) -> Result<Vec<crate::inspector::auth::AuthNode>> {
        crate::inspector::auth::AuthInspector::get_auth_tree(&self.env)
    }

    /// Get events captured during execution.
    pub fn get_events(&self) -> Result<Vec<crate::inspector::events::ContractEvent>> {
        crate::inspector::events::EventInspector::get_events(self.env.host())
    }

    pub fn get_storage_snapshot(&self) -> Result<HashMap<String, String>> {
        Ok(crate::inspector::storage::StorageInspector::capture_snapshot(self.env.host()))
    }

    /// Retrieve the entire ledger snapshot representing the environment's current state.
    pub fn get_ledger_snapshot(&self) -> Result<soroban_ledger_snapshot::LedgerSnapshot> {
        Ok(self.env.to_ledger_snapshot())
    }

    /// Finish the execution session, consuming the environment to extract the underlying storage footprint.
    /// This removes all internal references to the host and then extracts its tracking state.
    pub fn finish(
        &mut self,
    ) -> Result<(
        soroban_env_host::storage::Footprint,
        soroban_env_host::storage::Storage,
    )> {
        let dummy_env = Env::default();
        let dummy_addr = Address::generate(&dummy_env);

        let old_env = std::mem::replace(&mut self.env, dummy_env);
        self.contract_address = dummy_addr;

        let host = old_env.host().clone();
        drop(old_env);

        let (storage, _events) = host.try_finish().map_err(|e| {
            crate::DebuggerError::ExecutionError(format!(
                "Failed to finalize host execution tracking: {:?}",
                e
            ))
        })?;

        Ok((storage.footprint.clone(), storage))
    }

    /// Snapshot current storage state for dry-run rollback.
    pub fn snapshot_storage(&self) -> Result<StorageSnapshot> {
        let storage = self
            .env
            .host()
            .with_mut_storage(|storage| Ok(storage.clone()))
            .map_err(|e| {
                DebuggerError::ExecutionError(format!("Failed to snapshot storage: {:?}", e))
            })?;
        Ok(StorageSnapshot { storage })
    }

    /// Restore storage state from snapshot (dry-run rollback).
    pub fn restore_storage(&mut self, snapshot: &StorageSnapshot) -> Result<()> {
        self.env
            .host()
            .with_mut_storage(|storage| {
                *storage = snapshot.storage.clone();
                Ok(())
            })
            .map_err(|e| {
                DebuggerError::ExecutionError(format!("Failed to restore storage: {:?}", e))
            })?;
        info!("Storage state restored (dry-run rollback)");
        Ok(())
    }

    /// Get diagnostic events from the host.
    pub fn get_diagnostic_events(&self) -> Result<Vec<soroban_env_host::xdr::ContractEvent>> {
        Ok(self
            .env
            .host()
            .get_diagnostic_events()
            .map_err(|e| {
                DebuggerError::ExecutionError(format!("Failed to get diagnostic events: {}", e))
            })?
            .0
            .into_iter()
            .map(|he| he.event)
            .collect())
    }

    fn parse_args(&self, function: &str, args_json: &str) -> Result<Vec<Val>> {
        let parser = ArgumentParser::new(self.env.clone());
        let normalized_args_json = self.normalize_args_for_function(function, args_json)?;

        parser
            .parse_args_string(&normalized_args_json)
            .map_err(|e| {
                warn!("Failed to parse arguments: {}", e);
                DebuggerError::InvalidArguments(e.to_string()).into()
            })
    }

    fn normalize_args_for_function(&self, function: &str, args_json: &str) -> Result<String> {
        let signatures = crate::utils::wasm::parse_function_signatures(&self.wasm_bytes)?;
        let Some(signature) = signatures.into_iter().find(|sig| sig.name == function) else {
            return Ok(args_json.to_string());
        };

        let mut args_value: JsonValue = serde_json::from_str(args_json).map_err(|e| {
            DebuggerError::InvalidArguments(format!("Invalid JSON in --args: {}", e))
        })?;

        let JsonValue::Array(args) = &mut args_value else {
            return Ok(args_json.to_string());
        };

        for (arg, param) in args.iter_mut().zip(signature.params.iter()) {
            if param.type_name.starts_with("Option<") {
                if is_typed_annotation(arg) {
                    continue;
                }
                *arg = serde_json::json!({"type": "option", "value": arg.clone()});
                continue;
            }

            if param.type_name.starts_with("Tuple<") {
                let arity = tuple_arity_from_type_name(&param.type_name).ok_or_else(|| {
                    DebuggerError::InvalidArguments(format!(
                        "Invalid tuple type in function spec for '{}': {}",
                        param.name, param.type_name
                    ))
                })?;

                let JsonValue::Array(actual_arr) = arg else {
                    return Err(DebuggerError::InvalidArguments(format!(
                        "Argument '{}' expects tuple with {} elements, got {}",
                        param.name,
                        arity,
                        json_type_name(arg)
                    ))
                    .into());
                };

                if actual_arr.len() != arity {
                    return Err(DebuggerError::InvalidArguments(format!(
                        "Tuple arity mismatch: expected {}, got {}",
                        arity,
                        actual_arr.len()
                    ))
                    .into());
                }

                *arg = serde_json::json!({"type": "tuple", "arity": arity, "value": actual_arr.clone()});
            }
        }

        serde_json::to_string(&args_value).map_err(|e| {
            DebuggerError::ExecutionError(format!("Failed to normalize arguments JSON: {}", e))
                .into()
        })
    }

    fn install_mock_dispatchers(&self) -> Result<()> {
        let ids = match self.mock_registry.lock() {
            Ok(registry) => registry.mocked_contract_ids(),
            Err(_) => {
                return Err(DebuggerError::ExecutionError(
                    "Mock registry lock poisoned".to_string(),
                )
                .into())
            }
        };

        for contract_id in ids {
            let address = self.parse_contract_address(&contract_id)?;
            let dispatcher =
                MockContractDispatcher::new(contract_id.clone(), Arc::clone(&self.mock_registry))
                    .boxed();
            self.env
                .host()
                .register_test_contract(address.to_object(), dispatcher)
                .map_err(|e| {
                    DebuggerError::ExecutionError(format!(
                        "Failed to register test contract: {}",
                        e
                    ))
                })?;
        }

        Ok(())
    }
    fn parse_contract_address(&self, contract_id: &str) -> Result<Address> {
        let parsed = catch_unwind(AssertUnwindSafe(|| {
            Address::from_str(&self.env, contract_id)
        }));
        match parsed {
            Ok(addr) => Ok(addr),
            Err(_) => Err(DebuggerError::InvalidArguments(format!(
                "Invalid contract id in --mock: {contract_id}"
            ))
            .into()),
        }
    }
}

fn tuple_arity_from_type_name(type_name: &str) -> Option<usize> {
    let inner = type_name.strip_prefix("Tuple<")?.strip_suffix('>')?;
    if inner.trim().is_empty() {
        return Some(0);
    }

    let mut depth = 0usize;
    let mut arity = 1usize;
    for ch in inner.chars() {
        match ch {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => arity += 1,
            _ => {}
        }
    }

    Some(arity)
}

fn is_typed_annotation(value: &JsonValue) -> bool {
    matches!(
        value,
        JsonValue::Object(obj) if obj.get("type").is_some() && obj.get("value").is_some()
    )
}

fn json_type_name(value: &JsonValue) -> &'static str {
    match value {
        JsonValue::Null => "null",
        JsonValue::Bool(_) => "boolean",
        JsonValue::Number(_) => "number",
        JsonValue::String(_) => "string",
        JsonValue::Array(_) => "array",
        JsonValue::Object(_) => "object",
    }
}

#[cfg(test)]
mod tests {
    use super::tuple_arity_from_type_name;

    #[test]
    fn tuple_arity_counts_top_level_types() {
        assert_eq!(tuple_arity_from_type_name("Tuple<U32, Symbol>"), Some(2));
        assert_eq!(
            tuple_arity_from_type_name("Tuple<U32, Option<Vec<Symbol>>, Map<U32, String>>"),
            Some(3)
        );
    }
}
