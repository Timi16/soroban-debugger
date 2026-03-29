use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents a storage access operation (read or write)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StorageAccessType {
    Read,
    Write,
}

/// Metadata for a single storage operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageAccess {
    pub access_type: StorageAccessType,
    pub key: String,
    pub value: Option<String>,
    pub timestamp: u128,
    pub sequence: usize,
}

/// Metadata for a function call invocation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCallMetadata {
    pub caller: String,
    pub callee: String,
    pub arguments: Vec<String>,
    pub timestamp: u128,
    pub sequence: usize,
    pub depth: usize,
    pub result: Option<String>,
    pub error: Option<String>,
}

/// Wrapper around Soroban Host environment for debugging
/// Tracks all storage reads/writes and function calls for inspection and stepping views
pub struct DebugEnv {
    /// All storage access operations in order
    storage_accesses: Vec<StorageAccess>,
    /// All function calls in order
    function_calls: Vec<FunctionCallMetadata>,
    /// Maps storage keys to their access indices for quick lookup
    key_access_index: HashMap<String, Vec<usize>>,
    /// Global sequence counter for ordering operations
    operation_sequence: usize,
    /// Current call depth for function tracking
    call_depth: usize,
    /// Unified dynamic trace events for analysis
    dynamic_events: Vec<crate::server::protocol::DynamicTraceEvent>,
}

impl DebugEnv {
    pub fn new() -> Self {
        Self {
            storage_accesses: Vec::new(),
            function_calls: Vec::new(),
            key_access_index: HashMap::new(),
            operation_sequence: 0,
            call_depth: 0,
            dynamic_events: Vec::new(),
        }
    }

    /// Record a dynamic trace event
    pub fn record_event(
        &mut self,
        kind: crate::server::protocol::DynamicTraceEventKind,
        message: String,
    ) {
        let event = crate::server::protocol::DynamicTraceEvent {
            sequence: self.operation_sequence,
            kind,
            message,
            call_depth: Some(self.call_depth as u64),
            ..Default::default()
        };
        self.dynamic_events.push(event);
        self.operation_sequence += 1;
    }

    /// Record a storage read operation
    pub fn track_storage_read(&mut self, key: impl Into<String>) {
        let key_str = key.into();
        self.record_event(
            crate::server::protocol::DynamicTraceEventKind::StorageRead,
            format!("Read: {}", key_str),
        );
        let access = StorageAccess {
            access_type: StorageAccessType::Read,
            key: key_str.clone(),
            value: None,
            timestamp: Self::current_timestamp(),
            sequence: self.operation_sequence - 1,
        };
        let index = self.storage_accesses.len();
        self.storage_accesses.push(access);
        self.key_access_index
            .entry(key_str)
            .or_default()
            .push(index);
    }

    /// Record a storage write operation
    pub fn track_storage_write(&mut self, key: impl Into<String>, value: impl Into<String>) {
        let key_str = key.into();
        let value_str = value.into();
        self.record_event(
            crate::server::protocol::DynamicTraceEventKind::StorageWrite,
            format!("Write: {} = {}", key_str, value_str),
        );
        let access = StorageAccess {
            access_type: StorageAccessType::Write,
            key: key_str.clone(),
            value: Some(value_str),
            timestamp: Self::current_timestamp(),
            sequence: self.operation_sequence - 1,
        };
        let index = self.storage_accesses.len();
        self.storage_accesses.push(access);
        self.key_access_index
            .entry(key_str)
            .or_default()
            .push(index);
    }

    /// Record the start of a function call
    pub fn enter_function(&mut self, caller: impl Into<String>, callee: impl Into<String>) {
        let caller_str = caller.into();
        let callee_str = callee.into();
        self.record_event(
            crate::server::protocol::DynamicTraceEventKind::FunctionCall,
            format!("Call: {} -> {}", caller_str, callee_str),
        );
        self.call_depth += 1;
    }

    /// Record a completed function call with metadata
    pub fn record_function_call(
        &mut self,
        caller: impl Into<String>,
        callee: impl Into<String>,
        arguments: Vec<String>,
        result: Option<impl Into<String>>,
        error: Option<impl Into<String>>,
    ) {
        let res_str = result.map(|r| r.into());
        let err_str = error.map(|e| e.into());

        self.record_event(
            crate::server::protocol::DynamicTraceEventKind::Diagnostic,
            format!(
                "Return: {}",
                res_str.as_deref().or(err_str.as_deref()).unwrap_or("void")
            ),
        );

        let call = FunctionCallMetadata {
            caller: caller.into(),
            callee: callee.into(),
            arguments,
            timestamp: Self::current_timestamp(),
            sequence: self.operation_sequence - 1,
            depth: self.call_depth.saturating_sub(1),
            result: res_str,
            error: err_str,
        };
        self.function_calls.push(call);
        if self.call_depth > 0 {
            self.call_depth -= 1;
        }
    }

    /// Get all dynamic trace events
    pub fn dynamic_events(&self) -> &[crate::server::protocol::DynamicTraceEvent] {
        &self.dynamic_events
    }

    /// Get all storage accesses
    pub fn storage_accesses(&self) -> &[StorageAccess] {
        &self.storage_accesses
    }

    /// Get all function calls
    pub fn function_calls(&self) -> &[FunctionCallMetadata] {
        &self.function_calls
    }

    /// Get all accesses for a specific storage key
    pub fn get_key_accesses(&self, key: &str) -> Option<Vec<&StorageAccess>> {
        self.key_access_index.get(key).map(|indices| {
            indices
                .iter()
                .filter_map(|&idx| self.storage_accesses.get(idx))
                .collect()
        })
    }

    /// Get reads for a specific key
    pub fn get_key_reads(&self, key: &str) -> Vec<&StorageAccess> {
        self.get_key_accesses(key)
            .unwrap_or_default()
            .into_iter()
            .filter(|access| matches!(access.access_type, StorageAccessType::Read))
            .collect()
    }

    /// Get writes for a specific key
    pub fn get_key_writes(&self, key: &str) -> Vec<&StorageAccess> {
        self.get_key_accesses(key)
            .unwrap_or_default()
            .into_iter()
            .filter(|access| matches!(access.access_type, StorageAccessType::Write))
            .collect()
    }

    /// Get function calls for a specific callee
    pub fn get_function_calls_for(&self, callee: &str) -> Vec<&FunctionCallMetadata> {
        self.function_calls
            .iter()
            .filter(|call| call.callee == callee)
            .collect()
    }

    /// Clear all tracked data
    pub fn clear(&mut self) {
        self.storage_accesses.clear();
        self.function_calls.clear();
        self.key_access_index.clear();
        self.operation_sequence = 0;
        self.call_depth = 0;
    }

    /// Get operation sequence count
    pub fn operation_count(&self) -> usize {
        self.operation_sequence
    }

    /// Get storage access count
    pub fn storage_access_count(&self) -> usize {
        self.storage_accesses.len()
    }

    /// Get function call count
    pub fn function_call_count(&self) -> usize {
        self.function_calls.len()
    }

    /// Get the current call depth
    pub fn current_call_depth(&self) -> usize {
        self.call_depth
    }

    fn current_timestamp() -> u128 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    }
}

impl Default for DebugEnv {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_track_storage_read() {
        let mut env = DebugEnv::new();
        env.track_storage_read("balance:alice");

        assert_eq!(env.storage_access_count(), 1);
        let accesses = &env.storage_accesses;
        assert_eq!(accesses[0].key, "balance:alice");
        assert!(matches!(accesses[0].access_type, StorageAccessType::Read));
    }

    #[test]
    fn test_track_storage_write() {
        let mut env = DebugEnv::new();
        env.track_storage_write("balance:alice", "1000");

        assert_eq!(env.storage_access_count(), 1);
        let accesses = &env.storage_accesses;
        assert_eq!(accesses[0].key, "balance:alice");
        assert_eq!(accesses[0].value, Some("1000".to_string()));
        assert!(matches!(accesses[0].access_type, StorageAccessType::Write));
    }

    #[test]
    fn test_storage_access_sequence() {
        let mut env = DebugEnv::new();
        env.track_storage_read("key1");
        env.track_storage_write("key2", "value2");
        env.track_storage_read("key1");

        assert_eq!(env.storage_access_count(), 3);
        assert_eq!(env.storage_accesses[0].sequence, 0);
        assert_eq!(env.storage_accesses[1].sequence, 1);
        assert_eq!(env.storage_accesses[2].sequence, 2);
    }

    #[test]
    fn test_key_access_index() {
        let mut env = DebugEnv::new();
        env.track_storage_read("balance:alice");
        env.track_storage_write("balance:alice", "1000");
        env.track_storage_read("balance:alice");

        let accesses = env.get_key_accesses("balance:alice").unwrap();
        assert_eq!(accesses.len(), 3);
        assert_eq!(accesses[0].sequence, 0);
        assert_eq!(accesses[1].sequence, 1);
        assert_eq!(accesses[2].sequence, 2);
    }

    #[test]
    fn test_get_key_reads() {
        let mut env = DebugEnv::new();
        env.track_storage_read("key1");
        env.track_storage_write("key1", "value");
        env.track_storage_read("key1");

        let reads = env.get_key_reads("key1");
        assert_eq!(reads.len(), 2);
    }

    #[test]
    fn test_get_key_writes() {
        let mut env = DebugEnv::new();
        env.track_storage_read("key1");
        env.track_storage_write("key1", "value1");
        env.track_storage_write("key1", "value2");

        let writes = env.get_key_writes("key1");
        assert_eq!(writes.len(), 2);
    }

    #[test]
    fn test_record_function_call() {
        let mut env = DebugEnv::new();
        env.enter_function("main", "transfer");
        env.record_function_call(
            "main",
            "transfer",
            vec!["alice".to_string(), "bob".to_string(), "100".to_string()],
            Some("success"),
            None::<&str>,
        );

        assert_eq!(env.function_call_count(), 1);
        let calls = &env.function_calls;
        assert_eq!(calls[0].callee, "transfer");
        assert_eq!(calls[0].arguments.len(), 3);
        assert_eq!(calls[0].result, Some("success".to_string()));
        assert_eq!(calls[0].error, None);
    }

    #[test]
    fn test_function_call_with_error() {
        let mut env = DebugEnv::new();
        env.enter_function("main", "transfer");
        env.record_function_call(
            "main",
            "transfer",
            vec!["alice".to_string(), "bob".to_string()],
            None::<&str>,
            Some("insufficient balance"),
        );

        let calls = &env.function_calls;
        assert_eq!(calls[0].error, Some("insufficient balance".to_string()));
        assert_eq!(calls[0].result, None);
    }

    #[test]
    fn test_get_function_calls_for() {
        let mut env = DebugEnv::new();

        env.enter_function("main", "transfer");
        env.record_function_call("main", "transfer", vec![], None::<&str>, None::<&str>);

        env.enter_function("main", "mint");
        env.record_function_call(
            "main",
            "mint",
            vec!["100".to_string()],
            None::<&str>,
            None::<&str>,
        );

        env.enter_function("main", "transfer");
        env.record_function_call("main", "transfer", vec![], None::<&str>, None::<&str>);

        let transfers = env.get_function_calls_for("transfer");
        assert_eq!(transfers.len(), 2);
        assert!(transfers.iter().all(|c| c.callee == "transfer"));
    }

    #[test]
    fn test_call_depth_tracking() {
        let mut env = DebugEnv::new();

        assert_eq!(env.current_call_depth(), 0);

        env.enter_function("main", "level1");
        assert_eq!(env.current_call_depth(), 1);

        env.enter_function("level1", "level2");
        assert_eq!(env.current_call_depth(), 2);

        env.record_function_call("level1", "level2", vec![], None::<&str>, None::<&str>);
        assert_eq!(env.current_call_depth(), 1);
    }

    #[test]
    fn test_clear() {
        let mut env = DebugEnv::new();
        env.track_storage_read("key1");
        env.record_function_call("main", "test", vec![], None::<&str>, None::<&str>);

        assert!(env.storage_access_count() > 0);
        assert!(env.function_call_count() > 0);

        env.clear();

        assert_eq!(env.storage_access_count(), 0);
        assert_eq!(env.function_call_count(), 0);
    }

    #[test]
    fn test_operation_sequence() {
        let mut env = DebugEnv::new();
        env.track_storage_read("key1");
        env.track_storage_write("key2", "value");
        env.enter_function("main", "test");
        env.record_function_call("main", "test", vec![], None::<&str>, None::<&str>);

        // enter_function increments call depth but not operation sequence
        // only track_storage_* and record_function_call increment operation_sequence
        assert_eq!(env.operation_count(), 3);
    }
}
