use crate::utils::ArgumentParser;
use crate::{DebuggerError, Result};
use soroban_env_host::{ContractFunctionSet, Host, Symbol as HostSymbol, Val as HostVal};
use soroban_sdk::{Env, Val};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use tracing::warn;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct MockKey {
    pub contract_id: String,
    pub function: String,
}

#[derive(Clone, Debug)]
pub struct MockSpec {
    pub key: MockKey,
    pub return_raw: String,
    pub return_val: Val,
}

#[derive(Clone, Debug)]
pub struct MockCallLogEntry {
    pub contract_id: String,
    pub function: String,
    pub args_count: usize,
    pub mocked: bool,
    pub returned: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct MockRegistry {
    entries: HashMap<MockKey, MockSpec>,
    calls: Vec<MockCallLogEntry>,
}

impl MockRegistry {
    pub fn from_cli_specs(env: &Env, specs: &[String]) -> Result<Self> {
        let mut entries = HashMap::with_capacity(specs.len());
        let parser = ArgumentParser::new(env.clone());
        for spec in specs {
            let parsed = Self::parse_spec(&parser, spec)?;
            entries.insert(parsed.key.clone(), parsed);
        }
        Ok(Self {
            entries,
            calls: Vec::new(),
        })
    }

    pub fn mocked_contract_ids(&self) -> HashSet<String> {
        self.entries.keys().map(|k| k.contract_id.clone()).collect()
    }

    pub fn resolve_call(
        &mut self,
        contract_id: &str,
        function: &str,
        args_count: usize,
    ) -> Option<Val> {
        let key = MockKey {
            contract_id: contract_id.to_string(),
            function: function.to_string(),
        };
        if let Some(spec) = self.entries.get(&key) {
            self.calls.push(MockCallLogEntry {
                contract_id: contract_id.to_string(),
                function: function.to_string(),
                args_count,
                mocked: true,
                returned: Some(spec.return_raw.clone()),
            });
            return Some(spec.return_val);
        }
        self.calls.push(MockCallLogEntry {
            contract_id: contract_id.to_string(),
            function: function.to_string(),
            args_count,
            mocked: false,
            returned: None,
        });
        None
    }

    pub fn calls(&self) -> &[MockCallLogEntry] {
        &self.calls
    }

    fn parse_spec(parser: &ArgumentParser, spec: &str) -> Result<MockSpec> {
        let (signature, return_raw) = spec.split_once('=').ok_or_else(|| {
            DebuggerError::InvalidArguments(format!(
                "Invalid mock '{spec}'. Expected CONTRACT_ID.function=return_value"
            ))
        })?;
        let (contract_id, function) = signature.rsplit_once('.').ok_or_else(|| {
            DebuggerError::InvalidArguments(format!(
                "Invalid mock signature '{signature}'. Expected CONTRACT_ID.function"
            ))
        })?;
        let contract_id = contract_id.trim();
        let function = function.trim();
        let return_raw = return_raw.trim();
        if contract_id.is_empty() || function.is_empty() || return_raw.is_empty() {
            return Err(DebuggerError::InvalidArguments(format!(
                "Invalid mock '{spec}'. CONTRACT_ID, function and return_value are required"
            ))
            .into());
        }

        let parsed = parser
            .parse_args_string(return_raw)
            .map_err(|e| DebuggerError::InvalidArguments(e.to_string()))?;
        if parsed.len() != 1 {
            return Err(DebuggerError::InvalidArguments(format!(
                "Mock '{spec}' must parse to exactly one return value"
            ))
            .into());
        }

        Ok(MockSpec {
            key: MockKey {
                contract_id: contract_id.to_string(),
                function: function.to_string(),
            },
            return_raw: return_raw.to_string(),
            return_val: parsed[0],
        })
    }
}

pub struct MockContractDispatcher {
    contract_id: String,
    registry: Arc<Mutex<MockRegistry>>,
}

impl MockContractDispatcher {
    pub fn new(contract_id: String, registry: Arc<Mutex<MockRegistry>>) -> Self {
        Self {
            contract_id,
            registry,
        }
    }

    pub fn boxed(self) -> Rc<dyn ContractFunctionSet> {
        Rc::new(self)
    }
}

impl ContractFunctionSet for MockContractDispatcher {
    fn call(&self, func: &HostSymbol, _host: &Host, args: &[HostVal]) -> Option<HostVal> {
        let debug_str = format!("{:?}", func);
        let function = if let Some(s) = debug_str.strip_prefix("Symbol(") {
            s.trim_end_matches(')').to_string()
        } else if let Some(s) = debug_str.strip_prefix("SymbolSmall(") {
            s.trim_end_matches(')').to_string()
        } else if let Some(s) = debug_str.strip_prefix("SymbolObject(") {
            s.trim_end_matches(')').to_string()
        } else {
            debug_str
        };
        let mut guard = match self.registry.lock() {
            Ok(g) => g,
            Err(_) => return None,
        };
        let resolved = guard.resolve_call(&self.contract_id, &function, args.len());
        if resolved.is_none() {
            warn!(
                contract_id = self.contract_id,
                function, "No mock found for cross-contract call"
            );
        }
        resolved
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::Env;

    #[test]
    fn resolves_mocked_cross_contract_call() {
        let env = Env::default();
        let specs =
            vec!["CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAHK3M.echo=42".to_string()];
        let mut registry = MockRegistry::from_cli_specs(&env, &specs).unwrap();

        let resolved = registry.resolve_call(
            "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAHK3M",
            "echo",
            1,
        );

        assert!(resolved.is_some());
        assert_eq!(registry.calls().len(), 1);
        assert!(registry.calls()[0].mocked);
    }

    #[test]
    fn logs_unmocked_cross_contract_call() {
        let env = Env::default();
        let specs =
            vec!["CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAHK3M.echo=42".to_string()];
        let mut registry = MockRegistry::from_cli_specs(&env, &specs).unwrap();

        let resolved = registry.resolve_call(
            "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAHK3M",
            "transfer",
            2,
        );

        assert!(resolved.is_none());
        assert_eq!(registry.calls().len(), 1);
        assert!(!registry.calls()[0].mocked);
    }
}
