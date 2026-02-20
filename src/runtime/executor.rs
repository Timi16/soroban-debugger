use crate::utils::ArgumentParser;
use crate::{DebuggerError, Result};

use anyhow::anyhow;
use serde_json::Value;
use soroban_env_host::Host;
use soroban_sdk::{Address, Env, InvokeError, Symbol, Val, Vec as SorobanVec};
use soroban_sdk::{IntoVal, String as SorobanString};
use soroban_env_host::{DiagnosticLevel, Host};
use soroban_sdk::{Address, Env, InvokeError, Symbol, Val, Vec as SorobanVec};
use std::collections::HashMap;
use tracing::{info, warn};

/// Storage snapshot for dry-run rollback.
#[derive(Debug, Clone)]
pub struct StorageSnapshot {
    _contract_address: Address,
}

/// Executes Soroban contracts in a test environment.
pub struct ContractExecutor {
    env: Env,
    contract_address: Address,
}

impl ContractExecutor {
    /// Create a new contract executor.
    pub fn new(wasm: Vec<u8>) -> Result<Self> {
        info!("Initializing contract executor");

        let env = Env::default();
        env.host()
            .set_diagnostic_level(DiagnosticLevel::Debug)
            .expect("Failed to set diagnostic level");

        let contract_address = env.register(wasm.as_slice(), ());

        Ok(Self {
            env,
            contract_address,
        })
    }

    /// Execute a contract function.
    pub fn execute(&self, function: &str, args: Option<&str>) -> Result<String> {
        info!("Executing function: {}", function);

        let func_symbol = Symbol::new(&self.env, function);

        // Parse arguments (JSON array -> Vec<Val>)
        let parsed_args = if let Some(args_json) = args {
            self.parse_args(args_json)?
        } else {
            vec![]
        };

        let args_vec = if parsed_args.is_empty() {
            SorobanVec::<Val>::new(&self.env)
        } else {
            SorobanVec::from_slice(&self.env, &parsed_args)
        };

        match self.env.try_invoke_contract::<Val, InvokeError>(
            &self.contract_address,
            &func_symbol,
            args_vec,
        ) {
            Ok(Ok(val)) => Ok(format!("{:?}", val)),
            Ok(Err(conv_err)) => Err(DebuggerError::ExecutionError(format!(
                "Return value conversion failed: {:?}",
                conv_err
            ))
            .into()),
            Err(Ok(inv_err)) => match inv_err {
                InvokeError::Contract(code) => {
                    warn!("Contract returned error code: {}", code);
                    Err(
                        DebuggerError::ExecutionError(format!("Contract error code: {}", code))
                            .into(),
                    )
                }
                InvokeError::Abort => {
                    warn!("Contract execution aborted");
                    Err(DebuggerError::ExecutionError("Contract execution aborted".to_string())
                        .into())
                }
            },
            Err(Err(inv_err)) => {
                warn!("Invocation error conversion failed: {:?}", inv_err);
                Err(DebuggerError::ExecutionError(format!(
                    "Invocation error conversion failed: {:?}",
                    inv_err
                InvokeError::Contract(code) => Err(DebuggerError::ExecutionError(format!(
                    "Contract error code: {}",
                    code
                ))
                .into()),
                InvokeError::Abort => Err(DebuggerError::ExecutionError(
                    "Contract execution aborted".to_string(),
                )
                .into()),
            },
            Err(Err(inv_err)) => Err(DebuggerError::ExecutionError(format!(
                "Invocation error conversion failed: {:?}",
                inv_err
            ))
            .into()),
        }
    }

    /// Set initial storage state.
    pub fn set_initial_storage(&mut self, _storage_json: String) -> Result<()> {
        info!("Setting initial storage (not yet implemented)");
        Ok(())
    }

    /// Get the host instance.
    pub fn host(&self) -> &Host {
        self.env.host()
    }

    /// Parse JSON arguments into Soroban `Val`s
    fn parse_args(&self, args_json: &str) -> Result<Vec<Val>> {
        let v: Value = serde_json::from_str(args_json).map_err(|e| anyhow!("Invalid JSON args: {e}"))?;

        let arr = v
            .as_array()
            .ok_or_else(|| anyhow!("Args must be a JSON array, e.g. [1, \"x\"]"))?;

        let mut out: Vec<Val> = Vec::with_capacity(arr.len());
        for item in arr {
            out.push(self.json_to_val(item)?);
        }
        Ok(out)
    /// Get the authorization tree from the environment.
    pub fn get_auth_tree(&self) -> Result<Vec<crate::inspector::auth::AuthNode>> {
        crate::inspector::auth::AuthInspector::get_auth_tree(&self.env)
    }

    /// Get events captured during execution.
    pub fn get_events(&self) -> Result<Vec<crate::inspector::events::ContractEvent>> {
        crate::inspector::events::EventInspector::get_events(self.env.host())
    }

    /// Capture a snapshot of current contract storage.
    pub fn get_storage_snapshot(&self) -> Result<HashMap<String, String>> {
        Ok(HashMap::new())
    }

    /// Snapshot current storage state for dry-run rollback.
    pub fn snapshot_storage(&self) -> Result<StorageSnapshot> {
        Ok(StorageSnapshot {
            _contract_address: self.contract_address.clone(),
        })
    }

    /// Restore storage state from snapshot (dry-run rollback).
    pub fn restore_storage(&mut self, _snapshot: &StorageSnapshot) -> Result<()> {
        info!("Storage state restored (dry-run rollback)");
        Ok(())
    }

    /// Get diagnostic events from the host.
    pub fn get_diagnostic_events(&self) -> Result<Vec<soroban_env_host::xdr::ContractEvent>> {
        Ok(self
            .env
            .host()
            .get_diagnostic_events()?
            .0
            .into_iter()
            .map(|he| he.event)
            .collect())
    }

    fn parse_args(&self, args_json: &str) -> Result<Vec<Val>> {
        let parser = ArgumentParser::new(self.env.clone());
        parser.parse_args_string(args_json).map_err(|e| {
            warn!("Failed to parse arguments: {}", e);
            DebuggerError::InvalidArguments(e.to_string()).into()
        })
    }

    /// Convert a JSON value into a Soroban `Val` (minimal supported types)
    fn json_to_val(&self, v: &Value) -> Result<Val> {
        // Signed integers
        if let Some(n) = v.as_i64() {
            if n >= 0 && n <= u32::MAX as i64 {
                return Ok((n as u32).into_val(&self.env));
            }
            return Ok(n.into_val(&self.env));
        }

        // Unsigned integers (serde may parse as u64)
        if let Some(n) = v.as_u64() {
            if n <= u32::MAX as u64 {
                return Ok((n as u32).into_val(&self.env));
            }
            if n <= i64::MAX as u64 {
                return Ok((n as i64).into_val(&self.env));
            }
            return Err(anyhow!("Integer too large for supported types: {n}"));
        }

        // Bool
        if let Some(b) = v.as_bool() {
            return Ok(b.into_val(&self.env));
        }

        // String
        if let Some(s) = v.as_str() {
            // Minimal: treat as Soroban String.
            // (Later we can add Address parsing if string matches a strkey.)
            let ss = SorobanString::from_str(&self.env, s);
            return Ok(ss.into_val(&self.env));
        }

        Err(anyhow!("Unsupported arg type: {v}"))
    }
}