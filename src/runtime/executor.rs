//! Soroban contract executor â€” public faÃ§ade for the runtime sub-modules.
//!
//! [`ContractExecutor`] is the main entry-point for all contract execution.
//! Internally it delegates to four focused sub-modules:
//!
//! - [`super::loader`]  â€” WASM loading and environment bootstrap.
//! - [`super::parser`]  â€” Argument parsing and type-aware normalisation.
//! - [`super::invoker`] â€” Function invocation with timeout protection.
//! - [`super::result`]  â€” Result types and formatting helpers.

use crate::inspector::budget::MemorySummary;
use crate::runtime::env::DebugEnv;
use crate::runtime::mocking::{MockCallLogEntry, MockContractDispatcher, MockRegistry};
use crate::server::protocol::{DynamicTraceEvent, DynamicTraceEventKind};
use crate::utils::arguments::ArgumentParser;
use crate::{DebuggerError, Result};

use soroban_env_host::Host;
use soroban_sdk::testutils::Address as _;
use soroban_sdk::testutils::Ledger as _;
use soroban_sdk::{Address, Env};
use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tracing::info;

// â”€â”€ re-exports so callers never need to import sub-modules directly â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
pub use crate::runtime::mocking::MockCallLogEntry as MockCallEntry;
pub use crate::runtime::result::{ExecutionRecord, InstructionCounts, StorageSnapshot};

/// Executes Soroban contracts in a test environment.
pub const DEFAULT_EXECUTION_TIMEOUT_SECS: u64 = 30;

pub struct ContractExecutor {
    env: Env,
    contract_address: Address,
    last_execution: Option<ExecutionRecord>,
    last_memory_summary: Option<MemorySummary>,
    mock_registry: Arc<Mutex<MockRegistry>>,
    wasm_bytes: Vec<u8>,
    timeout_secs: u64,
    error_db: crate::debugger::error_db::ErrorDatabase,
    debug_env: DebugEnv,
    /// Accumulated CPU instruction deltas keyed by function name.
    per_function_cpu: HashMap<String, u64>,
}

impl ContractExecutor {
    /// Create a new contract executor by loading and registering `wasm`.
    #[tracing::instrument(skip_all)]
    pub fn new(wasm: Vec<u8>) -> Result<Self> {
        let loaded = crate::runtime::loader::load_contract(&wasm)?;
        Ok(Self {
            env: loaded.env,
            contract_address: loaded.contract_address,
            last_execution: None,
            last_memory_summary: None,
            mock_registry: Arc::new(Mutex::new(MockRegistry::default())),
            wasm_bytes: wasm,
            timeout_secs: DEFAULT_EXECUTION_TIMEOUT_SECS,
            error_db: loaded.error_db,
            debug_env: DebugEnv::new(),
            per_function_cpu: HashMap::new(),
        })
    }

    pub fn env(&self) -> &Env {
        &self.env
    }

    pub fn contract_address(&self) -> &Address {
        &self.contract_address
    }

    pub fn set_timeout(&mut self, secs: u64) {
        self.timeout_secs = secs;
    }

    pub fn timeout_secs(&self) -> u64 {
        self.timeout_secs
    }

    /// Enable auth mocking for interactive/test-like execution flows (e.g. REPL).
    pub fn enable_mock_all_auths(&self) {
        self.env.mock_all_auths();
    }

    /// Generate a test account address (StrKey) for REPL shorthand aliases.
    pub fn generate_repl_account_strkey(&self) -> Result<String> {
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
        // 1. Validate function exists in the WASM export section.
        let exported = crate::utils::wasm::parse_functions(&self.wasm_bytes)?;
        if !exported.contains(&function.to_string()) {
            return Err(DebuggerError::InvalidFunction(function.to_string()).into());
        }

        // 2. Parse arguments.
        let parsed_args = match args {
            Some(json) => {
                crate::runtime::parser::parse_args(&self.env, &self.wasm_bytes, function, json)?
            }
            None => vec![],
        };

        // Track function call entry
        let contract_addr_str = format!("{:?}", self.contract_address);
        let arg_strings: Vec<String> = parsed_args.iter().map(|val| format!("{:?}", val)).collect();
        self.debug_env.enter_function(&contract_addr_str, function);

        // 3. Invoke and capture the result.
        let storage_fn = || self.get_storage_snapshot();
        let storage_before = storage_fn()?;

        let timeout_guard = ExecutionTimeoutWatchdog::start(self.timeout_secs);
        let (display, record) = crate::runtime::invoker::invoke_function(
            &self.env,
            &self.contract_address,
            &self.error_db,
            function,
            parsed_args,
            self.timeout_secs,
            storage_fn,
        )?;
        drop(timeout_guard);

        // Track storage changes as accesses
        let storage_after = &record.storage_after;
        self.track_storage_changes(&storage_before, storage_after);

        // Record completed function call
        let result_str = display.clone();
        self.debug_env.record_function_call(
            &contract_addr_str,
            function,
            arg_strings,
            Some(result_str),
            None::<&str>,
        );

        *self
            .per_function_cpu
            .entry(function.to_string())
            .or_insert(0) += record.budget.cpu_instructions;
        self.last_execution = Some(record);
        Ok(display)
    }

    /// Track storage changes by comparing before and after snapshots
    fn track_storage_changes(
        &mut self,
        storage_before: &HashMap<String, String>,
        storage_after: &HashMap<String, String>,
    ) {
        // Track writes (new or modified entries)
        for (key, value) in storage_after {
            if !storage_before.contains_key(key) {
                // New write
                self.debug_env.track_storage_write(key, value);
            } else if storage_before.get(key) != Some(value) {
                // Modified write
                self.debug_env.track_storage_write(key, value);
            }
        }

        // Track reads by checking which keys existed before
        for key in storage_before.keys() {
            if storage_after.contains_key(key) {
                // Key still exists, assume it was read (at minimum)
                self.debug_env.track_storage_read(key);
            }
        }
    }

    // â”€â”€ accessors â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    pub fn last_execution(&self) -> Option<&ExecutionRecord> {
        self.last_execution.as_ref()
    }

    pub fn last_memory_summary(&self) -> Option<&MemorySummary> {
        self.last_memory_summary.as_ref()
    }

    pub fn debug_env(&self) -> &DebugEnv {
        &self.debug_env
    }

    pub fn debug_env_mut(&mut self) -> &mut DebugEnv {
        &mut self.debug_env
    }

    pub fn set_initial_storage(&mut self, storage_json: String) -> Result<()> {
        #[derive(Debug, Clone, Copy)]
        enum Durability {
            Instance,
            Persistent,
            Temporary,
        }

        fn is_typed_annotation(value: &serde_json::Value) -> bool {
            matches!(
                value,
                serde_json::Value::Object(obj) if obj.get("type").is_some() && obj.get("value").is_some()
            )
        }

        fn normalize_numbers(value: &serde_json::Value) -> Result<serde_json::Value> {
            use serde_json::Value;

            if is_typed_annotation(value) {
                return Ok(value.clone());
            }

            match value {
                Value::Null | Value::Bool(_) | Value::String(_) => Ok(value.clone()),
                Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        Ok(serde_json::json!({ "type": "i64", "value": i }))
                    } else if let Some(u) = n.as_u64() {
                        if u <= i64::MAX as u64 {
                            Ok(serde_json::json!({ "type": "i64", "value": u as i64 }))
                        } else {
                            Ok(serde_json::json!({ "type": "u64", "value": u }))
                        }
                    } else {
                        Err(DebuggerError::StorageError(
                            "Floating-point numbers are not supported in --storage".to_string(),
                        )
                        .into())
                    }
                }
                Value::Array(arr) => {
                    let mut out = Vec::with_capacity(arr.len());
                    for item in arr {
                        out.push(normalize_numbers(item)?);
                    }
                    Ok(Value::Array(out))
                }
                Value::Object(map) => {
                    let mut out = serde_json::Map::new();
                    for (k, v) in map {
                        out.insert(k.clone(), normalize_numbers(v)?);
                    }
                    Ok(Value::Object(out))
                }
            }
        }

        fn parse_one_val(env: &Env, value: &serde_json::Value) -> Result<soroban_sdk::Val> {
            let parser = ArgumentParser::new(env.clone());
            let json = serde_json::to_string(value).map_err(|e| {
                DebuggerError::StorageError(format!("Failed to serialize storage JSON value: {e}"))
            })?;
            let mut vals = parser.parse_args_string(&json).map_err(|e| {
                DebuggerError::StorageError(format!("Failed to parse storage value: {e}"))
            })?;
            if vals.len() != 1 {
                return Err(DebuggerError::StorageError(format!(
                    "Storage entry must resolve to exactly 1 value, got {}",
                    vals.len()
                ))
                .into());
            }
            Ok(vals.remove(0))
        }

        fn parse_durability(raw: Option<&serde_json::Value>) -> Result<Durability> {
            let Some(v) = raw else {
                return Ok(Durability::Instance);
            };
            let Some(s) = v.as_str() else {
                return Err(DebuggerError::StorageError(
                    "durability must be a string: instance|persistent|temporary".to_string(),
                )
                .into());
            };
            match s {
                "instance" => Ok(Durability::Instance),
                "persistent" => Ok(Durability::Persistent),
                "temporary" => Ok(Durability::Temporary),
                other => Err(DebuggerError::StorageError(format!(
                    "Unsupported durability '{other}'. Use instance|persistent|temporary."
                ))
                .into()),
            }
        }

        info!("Setting initial storage");
        let root: serde_json::Value = serde_json::from_str(&storage_json).map_err(|e| {
            DebuggerError::StorageError(format!("Failed to parse initial storage JSON: {e}"))
        })?;

        let mut entries: Vec<(Durability, soroban_sdk::Val, soroban_sdk::Val)> = Vec::new();

        match root {
            serde_json::Value::Object(map) => {
                if let Some(entries_field) = map.get("entries") {
                    if entries_field.is_object() {
                        return Err(DebuggerError::StorageError(
                            "Unsupported --storage format: looks like an exported snapshot. Use a plain object mapping keys to values, e.g. {\"c\": 41}, or use the list form [{\"key\":...,\"value\":...}].".to_string(),
                        )
                        .into());
                    }
                }

                for (k, v) in map {
                    let key_json = serde_json::json!({ "type": "symbol", "value": k });
                    let key_val = parse_one_val(&self.env, &key_json)?;
                    let value_json = normalize_numbers(&v)?;
                    let value_val = parse_one_val(&self.env, &value_json)?;
                    entries.push((Durability::Instance, key_val, value_val));
                }
            }
            serde_json::Value::Array(arr) => {
                for item in arr {
                    let serde_json::Value::Object(obj) = item else {
                        return Err(DebuggerError::StorageError(
                            "Storage list entries must be objects with {key,value[,durability]}"
                                .to_string(),
                        )
                        .into());
                    };
                    let durability = parse_durability(obj.get("durability"))?;
                    let Some(key) = obj.get("key") else {
                        return Err(DebuggerError::StorageError(
                            "Storage entry is missing required field 'key'".to_string(),
                        )
                        .into());
                    };
                    let Some(value) = obj.get("value") else {
                        return Err(DebuggerError::StorageError(
                            "Storage entry is missing required field 'value'".to_string(),
                        )
                        .into());
                    };

                    let key_val = parse_one_val(&self.env, key)?;
                    let value_json = normalize_numbers(value)?;
                    let value_val = parse_one_val(&self.env, &value_json)?;
                    entries.push((durability, key_val, value_val));
                }
            }
            other => {
                return Err(DebuggerError::StorageError(format!(
                    "Unsupported --storage JSON: expected object or array, got {other}"
                ))
                .into())
            }
        }

        let contract_address = self.contract_address.clone();
        self.env.as_contract(&contract_address, || {
            for (durability, key_val, value_val) in entries {
                match durability {
                    Durability::Instance => {
                        self.env.storage().instance().set(&key_val, &value_val);
                    }
                    Durability::Persistent => {
                        self.env.storage().persistent().set(&key_val, &value_val);
                    }
                    Durability::Temporary => {
                        self.env.storage().temporary().set(&key_val, &value_val);
                    }
                }
            }
        });

        Ok(())
    }
    /// Apply ledger metadata (sequence, timestamp, network ID) from a network snapshot.
    pub fn apply_snapshot_ledger(
        &mut self,
        snapshot: &crate::simulator::LoadedSnapshot,
    ) -> Result<()> {
        use sha2::{Digest, Sha256};

        let seq = snapshot.ledger_sequence();
        let ts = snapshot.snapshot().ledger.timestamp;
        let passphrase = snapshot.network_passphrase();

        let mut hasher = Sha256::new();
        hasher.update(passphrase.as_bytes());
        let network_id: [u8; 32] = hasher.finalize().into();

        self.env.ledger().with_mut(|l| {
            l.sequence_number = seq;
            l.timestamp = ts;
            l.network_id = network_id;
        });

        info!(
            "Applied snapshot ledger state: sequence={}, timestamp={}",
            seq, ts
        );

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
        self.mock_registry
            .lock()
            .map(|r| r.calls().to_vec())
            .unwrap_or_default()
    }
    pub fn get_instruction_counts(&self) -> Result<InstructionCounts> {
        let mut function_counts: Vec<(String, u64)> = self
            .per_function_cpu
            .iter()
            .map(|(name, &cpu)| (name.clone(), cpu))
            .collect();
        function_counts.sort_by(|a, b| b.1.cmp(&a.1));
        let total = function_counts.iter().map(|(_, c)| c).sum();
        Ok(InstructionCounts {
            function_counts,
            total,
        })
    }
    pub fn host(&self) -> &Host {
        self.env.host()
    }
    pub fn get_auth_tree(&self) -> Result<Vec<crate::inspector::auth::AuthNode>> {
        crate::inspector::auth::AuthInspector::get_auth_tree(&self.env)
    }
    pub fn get_events(&self) -> Result<Vec<crate::inspector::events::ContractEvent>> {
        crate::inspector::events::EventInspector::get_events(self.env.host())
    }
    pub fn get_storage_snapshot(&self) -> Result<HashMap<String, String>> {
        Ok(crate::inspector::storage::StorageInspector::capture_snapshot(self.env.host()))
    }
    pub fn get_ledger_snapshot(&self) -> Result<soroban_ledger_snapshot::LedgerSnapshot> {
        Ok(self.env.to_ledger_snapshot())
    }
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
            DebuggerError::ExecutionError(format!(
                "Failed to finalize host execution tracking: {:?}",
                e
            ))
        })?;
        Ok((storage.footprint.clone(), storage))
    }
    pub fn snapshot_storage(&self) -> Result<StorageSnapshot> {
        let storage = self
            .env
            .host()
            .with_mut_storage(|s| Ok(s.clone()))
            .map_err(|e| {
                DebuggerError::ExecutionError(format!("Failed to snapshot storage: {:?}", e))
            })?;
        Ok(StorageSnapshot { storage })
    }
    pub fn restore_storage(&mut self, snapshot: &StorageSnapshot) -> Result<()> {
        self.env
            .host()
            .with_mut_storage(|s| {
                *s = snapshot.storage.clone();
                Ok(())
            })
            .map_err(|e| {
                DebuggerError::ExecutionError(format!("Failed to restore storage: {:?}", e))
            })?;
        info!("Storage state restored (dry-run rollback)");
        Ok(())
    }
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

    /// Build a structured dynamic trace for security analysis.
    pub fn get_dynamic_trace(&self) -> Result<Vec<DynamicTraceEvent>> {
        let mut out = self.debug_env.dynamic_events().to_vec();

        // Add additional diagnostic events from the host that weren't captured by hooks
        let mut next_sequence = out.iter().map(|e| e.sequence).max().map_or(0, |n| n + 1);
        for event in self.get_diagnostic_events().unwrap_or_default() {
            let message = format!("{:?}", event);
            // Skip events already captured (this is a bit heuristic)
            if out.iter().any(|e| e.message.contains(&message) || message.contains(&e.message)) {
                continue;
            }

            out.push(DynamicTraceEvent {
                sequence: next_sequence,
                kind: classify_diagnostic_event_kind(&message),
                message,
                ..Default::default()
            });
            next_sequence += 1;
        }

        out.sort_by_key(|e| e.sequence);
        Ok(out)
    }

    // â”€â”€ private helpers â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn install_mock_dispatchers(&self) -> Result<()> {
        let ids = self
            .mock_registry
            .lock()
            .map(|r| r.mocked_contract_ids())
            .map_err(|_| DebuggerError::ExecutionError("Mock registry lock poisoned".into()))?;

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
        catch_unwind(AssertUnwindSafe(|| {
            Address::from_str(&self.env, contract_id)
        }))
        .map_err(|_| {
            DebuggerError::InvalidArguments(format!("Invalid contract id in --mock: {contract_id}"))
                .into()
        })
    }
}

/// Token for cooperative execution cancellation (issue #504).
#[derive(Debug, Clone)]
pub struct CancellationToken {
    inner: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn cancel(&self) {
        self.inner.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.load(Ordering::SeqCst)
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

struct ExecutionTimeoutWatchdog {
    done_tx: Option<std::sync::mpsc::Sender<()>>,
    cancellation_token: CancellationToken,
}

impl ExecutionTimeoutWatchdog {
    fn start(timeout_secs: u64) -> Self {
        if timeout_secs == 0 {
            return Self {
                done_tx: None,
                cancellation_token: CancellationToken::new(),
            };
        }

        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let cancel_token = CancellationToken::new();
        let token_clone = cancel_token.clone();

        std::thread::spawn(move || {
            match rx.recv_timeout(std::time::Duration::from_secs(timeout_secs)) {
                Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    tracing::warn!(
                        "Execution timeout after {} seconds. Initiating cooperative cancellation.",
                        timeout_secs
                    );
                    token_clone.cancel();
                }
            }
        });

        Self {
            done_tx: Some(tx),
            cancellation_token: cancel_token,
        }
    }

    #[allow(dead_code)]
    pub fn token(&self) -> CancellationToken {
        self.cancellation_token.clone()
    }
}

impl Drop for ExecutionTimeoutWatchdog {
    fn drop(&mut self) {
        if let Some(tx) = self.done_tx.take() {
            let _ = tx.send(());
        }
    }
}

fn classify_diagnostic_event_kind(message: &str) -> DynamicTraceEventKind {
    let lower = message.to_ascii_lowercase();

    if lower.contains("require_auth") || lower.contains("authorized") {
        DynamicTraceEventKind::Authorization
    } else if lower.contains("invoke_contract")
        || lower.contains("call_contract")
        || lower.contains("contractcall")
    {
        DynamicTraceEventKind::CrossContractCall
    } else if lower.contains("contract_storage_get")
        || lower.contains("contract_storage_has")
        || lower.contains("storage_get")
        || lower.contains("storage_has")
    {
        DynamicTraceEventKind::StorageRead
    } else if lower.contains("contract_storage_put")
        || lower.contains("contract_storage_update")
        || lower.contains("storage_put")
        || lower.contains("storage_update")
    {
        DynamicTraceEventKind::StorageWrite
    } else {
        DynamicTraceEventKind::Diagnostic
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debug_env_storage_tracking() {
        let mut debug_env = DebugEnv::new();

        debug_env.track_storage_read("balance:alice");
        debug_env.track_storage_write("balance:alice", "1000");
        debug_env.track_storage_read("balance:alice");

        assert_eq!(debug_env.storage_access_count(), 3);
        assert_eq!(debug_env.get_key_reads("balance:alice").len(), 2);
        assert_eq!(debug_env.get_key_writes("balance:alice").len(), 1);
    }

    #[test]
    fn test_debug_env_function_call_tracking() {
        let mut debug_env = DebugEnv::new();

        debug_env.enter_function("contract", "transfer");
        debug_env.record_function_call(
            "contract",
            "transfer",
            vec!["alice".to_string(), "bob".to_string(), "100".to_string()],
            Some("success"),
            None::<&str>,
        );

        assert_eq!(debug_env.function_call_count(), 1);
        let calls = debug_env.get_function_calls_for("transfer");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].arguments.len(), 3);
        assert_eq!(calls[0].result, Some("success".to_string()));
    }

    #[test]
    fn test_debug_env_nested_calls() {
        let mut debug_env = DebugEnv::new();

        // Simulate nested call: transfer -> mint
        debug_env.enter_function("contract", "transfer");
        debug_env.enter_function("transfer", "mint");
        debug_env.record_function_call(
            "transfer",
            "mint",
            vec!["100".to_string()],
            Some("ok"),
            None::<&str>,
        );

        assert_eq!(debug_env.current_call_depth(), 1);
        debug_env.record_function_call(
            "contract",
            "transfer",
            vec![],
            Some("complete"),
            None::<&str>,
        );
        assert_eq!(debug_env.current_call_depth(), 0);
    }

    #[test]
    fn test_track_storage_changes_direct() {
        let mut debug_env = DebugEnv::new();

        // Simulate calling track_storage_changes logic directly
        let mut storage_before = HashMap::new();
        storage_before.insert("key1".to_string(), "old_value".to_string());

        let mut storage_after = HashMap::new();
        storage_after.insert("key1".to_string(), "new_value".to_string());
        storage_after.insert("key2".to_string(), "added".to_string());

        // Simulate write operations
        debug_env.track_storage_write("key1", "new_value");
        debug_env.track_storage_write("key2", "added");

        // Simulate read operations
        debug_env.track_storage_read("key1");
        debug_env.track_storage_read("key2");

        assert!(debug_env.storage_access_count() > 0);
        assert_eq!(debug_env.get_key_writes("key1").len(), 1);
        assert_eq!(debug_env.get_key_writes("key2").len(), 1);
    }
}
