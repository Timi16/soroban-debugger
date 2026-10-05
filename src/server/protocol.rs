use serde::{Deserialize, Serialize};
use std::fmt;

/// Current protocol version implemented by this backend.
pub const PROTOCOL_VERSION: u32 = 1;
/// Minimum protocol version this backend can communicate with.
pub const PROTOCOL_MIN_VERSION: u32 = 1;
/// Maximum protocol version this backend can communicate with.
pub const PROTOCOL_MAX_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolNegotiationError {
    InvalidClientRange {
        min: u32,
        max: u32,
    },
    NoOverlap {
        client_min: u32,
        client_max: u32,
        server_min: u32,
        server_max: u32,
    },
}

impl fmt::Display for ProtocolNegotiationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidClientRange { min, max } => {
                write!(
                    f,
                    "Invalid client protocol range (min={} > max={})",
                    min, max
                )
            }
            Self::NoOverlap {
                client_min,
                client_max,
                server_min,
                server_max,
            } => write!(
                f,
                "Protocol mismatch: client supports [{}..={}], server supports [{}..={}]",
                client_min, client_max, server_min, server_max
            ),
        }
    }
}

impl std::error::Error for ProtocolNegotiationError {}

pub fn negotiate_protocol_version(
    client_min: u32,
    client_max: u32,
) -> Result<u32, ProtocolNegotiationError> {
    if client_min > client_max {
        return Err(ProtocolNegotiationError::InvalidClientRange {
            min: client_min,
            max: client_max,
        });
    }

    let negotiated_min = client_min.max(PROTOCOL_MIN_VERSION);
    let negotiated_max = client_max.min(PROTOCOL_MAX_VERSION);
    if negotiated_min > negotiated_max {
        return Err(ProtocolNegotiationError::NoOverlap {
            client_min,
            client_max,
            server_min: PROTOCOL_MIN_VERSION,
            server_max: PROTOCOL_MAX_VERSION,
        });
    }

    Ok(negotiated_max)
}

use crate::debugger::SourceBreakpointResolution;

/// Structured event category used by dynamic security analysis.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum DynamicTraceEventKind {
    #[default]
    Diagnostic,
    FunctionCall,
    /// Read-side storage pressure feeds unbounded-iteration analysis.
    StorageRead,
    /// Write-side storage pressure feeds storage-write-pressure analysis.
    StorageWrite,
    Authorization,
    CrossContractCall,
    CrossContractReturn,
    Branch,
}

/// Rich dynamic trace entry produced by the runtime and consumed by analyzers.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DynamicTraceEvent {
    pub sequence: usize,
    pub kind: DynamicTraceEventKind,
    pub message: String,
    pub caller: Option<String>,
    pub function: Option<String>,
    pub call_depth: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_value: Option<String>,
    /// Actor address associated with this event (e.g., the address being authorized).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation_reason: Option<crate::output::InvocationReason>,
}

/// Source location information (file, line, column)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceLocation {
    /// Source file path (relative or absolute)
    pub file: String,
    /// 1-based line number
    pub line: u32,
    /// 0-based column (optional)
    pub column: Option<u32>,
}

impl From<crate::debugger::source_map::SourceLocation> for SourceLocation {
    fn from(loc: crate::debugger::source_map::SourceLocation) -> Self {
        Self {
            file: loc.file.to_string_lossy().to_string(),
            line: loc.line,
            column: loc.column,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakpointCapabilities {
    pub conditional_breakpoints: bool,
    pub hit_conditional_breakpoints: bool,
    pub log_points: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakpointDescriptor {
    pub id: String,
    pub function: String,
    pub condition: Option<String>,
    pub hit_condition: Option<String>,
    pub log_message: Option<String>,
    #[serde(default)]
    pub hit_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteSessionInfo {
    pub session_id: String,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Feature capabilities a debug server supports. Exchanged during the
/// handshake so clients can gate optional features (Issue #837).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerCapabilities {
    pub conditional_breakpoints: bool,
    pub source_breakpoints: bool,
    pub evaluate: bool,
    pub tls: bool,
    pub token_auth: bool,
    pub session_lifecycle: bool,
    pub repeat_execution: bool,
    pub symbolic_analysis: bool,
    pub snapshot_loading: bool,
    pub dynamic_trace_events: bool,
}

impl ServerCapabilities {
    /// Capabilities supported by this build of the server.
    pub fn current() -> Self {
        Self {
            conditional_breakpoints: true,
            source_breakpoints: true,
            evaluate: true,
            tls: true,
            token_auth: true,
            session_lifecycle: true,
            repeat_execution: true,
            symbolic_analysis: false,
            snapshot_loading: true,
            dynamic_trace_events: true,
        }
    }

    /// Returns the names of capabilities required by `self` that `other`
    /// does not support.
    pub fn unsupported_by(&self, other: &Self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        let pairs = [
            (
                self.conditional_breakpoints,
                other.conditional_breakpoints,
                "conditional_breakpoints",
            ),
            (
                self.source_breakpoints,
                other.source_breakpoints,
                "source_breakpoints",
            ),
            (self.evaluate, other.evaluate, "evaluate"),
            (self.tls, other.tls, "tls"),
            (self.token_auth, other.token_auth, "token_auth"),
            (
                self.session_lifecycle,
                other.session_lifecycle,
                "session_lifecycle",
            ),
            (
                self.repeat_execution,
                other.repeat_execution,
                "repeat_execution",
            ),
            (
                self.symbolic_analysis,
                other.symbolic_analysis,
                "symbolic_analysis",
            ),
            (
                self.snapshot_loading,
                other.snapshot_loading,
                "snapshot_loading",
            ),
            (
                self.dynamic_trace_events,
                other.dynamic_trace_events,
                "dynamic_trace_events",
            ),
        ];
        for (required, supported, name) in pairs {
            if required && !supported {
                missing.push(name);
            }
        }
        missing
    }
}

/// Wire protocol messages for remote debugging
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DebugRequest {
    /// Protocol handshake / version negotiation.
    Handshake {
        client_name: String,
        client_version: String,
        protocol_min: u32,
        protocol_max: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        heartbeat_interval_ms: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        idle_timeout_ms: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_label: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reconnect_session_id: Option<String>,
        /// Capabilities the client requires the server to support.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        required_capabilities: Option<ServerCapabilities>,
    },

    /// Authenticate with the server
    Authenticate { token: String },

    /// Load a contract
    LoadContract { contract_path: String },

    /// Execute a function
    Execute {
        function: String,
        args: Option<String>,
    },

    /// Get server capabilities
    GetCapabilities,

    /// Step execution (instruction-level)
    Step,
    /// Step into next inline/instruction
    StepIn,

    /// Step over current function
    Next,

    /// Step out of current function
    StepOut,

    /// Step over to next source line in the same frame
    StepOverLine,

    /// Continue execution
    Continue,

    /// Inspect current state
    Inspect,

    /// Get storage state
    GetStorage,

    /// Get call stack
    GetStack,

    /// Get budget information
    GetBudget,

    /// Set a breakpoint
    SetBreakpoint {
        id: String,
        function: String,
        condition: Option<String>,
        hit_condition: Option<String>,
        log_message: Option<String>,
    },

    /// Clear a breakpoint
    ClearBreakpoint { id: String },

    /// List all breakpoints
    ListBreakpoints,

    /// Resolve source breakpoints (file + line) into concrete exported function breakpoints.
    ResolveSourceBreakpoints {
        source_path: String,
        lines: Vec<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        max_forward_line_adjust: Option<u32>,
    },

    /// Set initial storage
    SetStorage { storage_json: String },

    /// Load network snapshot
    LoadSnapshot { snapshot_path: String },

    /// Evaluate an expression in the current debug context
    Evaluate {
        expression: String,
        frame_id: Option<u64>,
    },

    /// Ping to check connection
    Ping,

    /// Disconnect
    Disconnect,

    /// Get diagnostic and contract events
    GetEvents,

    /// Cancel a running execution
    Cancel,

    /// Reconnect to an existing session after a transient disconnect.
    /// The client provides the session_id it received from a previous HandshakeAck.
    Reconnect { session_id: String },

    /// Catch-all for forward compatibility
    #[serde(other)]
    Unknown,
}

/// Response messages from the server
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DebugResponse {
    /// Handshake successful. Both sides have at least one compatible protocol version.
    HandshakeAck {
        server_name: String,
        server_version: String,
        protocol_min: u32,
        protocol_max: u32,
        selected_version: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_created_at: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_label: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        heartbeat_interval_ms: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        idle_timeout_ms: Option<u32>,
        /// Opaque session identifier the client can use to reconnect after a
        /// transient disconnect. Absent on servers that do not support reconnection.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reconnect_id: Option<String>,
        /// Capabilities supported by the server, so the client can gate
        /// optional features before issuing unsupported requests.
        #[serde(default)]
        server_capabilities: ServerCapabilities,
    },

    /// Handshake failed due to protocol mismatch.
    IncompatibleProtocol {
        message: String,
        server_name: String,
        server_version: String,
        protocol_min: u32,
        protocol_max: u32,
    },

    /// Handshake rejected because the client requires capabilities the server doesn't support.
    IncompatibleCapabilities {
        message: String,
        /// The capability names the client required but the server lacks.
        missing_capabilities: Vec<String>,
        /// What the server does support, so the client can report it.
        server_capabilities: ServerCapabilities,
    },

    /// Authentication result
    Authenticated { success: bool, message: String },

    /// Contract loaded
    ContractLoaded { size: usize },

    /// Execution result
    ExecutionResult {
        success: bool,
        output: String,
        error: Option<String>,
        paused: bool,
        completed: bool,
        source_location: Option<SourceLocation>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pause_reason: Option<String>,
    },

    /// Step result
    StepResult {
        paused: bool,
        current_function: Option<String>,
        step_count: u64,
        source_location: Option<SourceLocation>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pause_reason: Option<String>,
    },

    /// Source-level step-over result
    StepOverLineResult {
        paused: bool,
        file: Option<String>,
        line: Option<u32>,
        column: Option<u32>,
    },

    /// Continue result
    ContinueResult {
        completed: bool,
        output: Option<String>,
        error: Option<String>,
        paused: bool,
        source_location: Option<SourceLocation>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pause_reason: Option<String>,
    },

    /// Acknowledgment of a successful session reconnection.
    ReconnectAck {
        session_id: String,
        paused: bool,
        current_function: Option<String>,
        breakpoints: Vec<String>,
        step_count: u64,
    },

    /// Reconnection failed because the session has expired or been purged.
    SessionExpired { message: String },

    /// Inspection result
    InspectionResult {
        function: Option<String>,
        args: Option<String>,
        step_count: u64,
        paused: bool,
        call_stack: Vec<String>,
        source_location: Option<SourceLocation>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pause_reason: Option<String>,
    },

    /// Storage state
    StorageState { storage_json: String },

    /// Call stack
    CallStack { stack: Vec<String> },

    /// Budget information
    BudgetInfo {
        cpu_instructions: u64,
        memory_bytes: u64,
    },

    /// Breakpoint set
    BreakpointSet { id: String, function: String },

    /// Breakpoint cleared
    BreakpointCleared { id: String },

    /// List of breakpoints
    BreakpointsList {
        breakpoints: Vec<BreakpointDescriptor>,
    },

    /// Backend capabilities
    Capabilities { breakpoints: BreakpointCapabilities },

    /// Resolved source breakpoints.
    SourceBreakpointsResolved {
        breakpoints: Vec<SourceBreakpointResolution>,
    },

    /// Snapshot loaded
    SnapshotLoaded { summary: String },

    /// Error response
    Error { message: String },

    /// Evaluation result
    EvaluateResult {
        result: String,
        result_type: Option<String>,
        variables_reference: u64,
    },

    /// Pong response
    Pong,

    /// Disconnected
    Disconnected,

    /// Cancel acknowledged
    CancelAck,

    /// List of events
    EventsList {
        events: Vec<crate::server::protocol::DynamicTraceEvent>,
    },

    /// Catch-all for forward compatibility
    #[serde(other)]
    Unknown,
}

/// Message wrapper for the protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugMessage {
    /// Correlation id used to match a response to the originating request.
    pub id: u64,
    pub request: Option<DebugRequest>,
    pub response: Option<DebugResponse>,
}

impl DebugMessage {
    pub fn request(id: u64, request: DebugRequest) -> Self {
        Self {
            id,
            request: Some(request),
            response: None,
        }
    }

    pub fn response(id: u64, response: DebugResponse) -> Self {
        Self {
            id,
            request: None,
            response: Some(response),
        }
    }

    pub fn is_response_for(&self, expected_id: u64) -> bool {
        self.id == expected_id && self.response.is_some()
    }

    /// Parse a JSON string into a DebugMessage with field-aware error reporting.
    pub fn parse(json: &str) -> std::result::Result<Self, String> {
        let deserializer = &mut serde_json::Deserializer::from_str(json);
        serde_path_to_error::deserialize(deserializer)
            .map_err(|e| format!("Protocol error at '{}': {}", e.path(), e.inner()))
    }
}

use tokio::io::AsyncWriteExt;

/// Helper to send a response to a writer
pub async fn send_response<S>(
    writer: &mut S,
    response: DebugMessage,
) -> std::result::Result<(), String>
where
    S: tokio::io::AsyncWrite + Unpin,
{
    let json = serde_json::to_string(&response).map_err(|e| e.to_string())?;
    writer
        .write_all(json.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    writer.write_all(b"\n").await.map_err(|e| e.to_string())?;
    writer.flush().await.map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiate_protocol_version_accepts_exact_match() {
        let v = negotiate_protocol_version(PROTOCOL_MIN_VERSION, PROTOCOL_MAX_VERSION).unwrap();
        assert_eq!(v, PROTOCOL_VERSION);
    }

    #[test]
    fn negotiate_protocol_version_selects_highest_common_version() {
        let v = negotiate_protocol_version(0, 999).unwrap();
        assert_eq!(v, PROTOCOL_MAX_VERSION);
    }

    #[test]
    fn negotiate_protocol_version_rejects_older_client() {
        let err = negotiate_protocol_version(0, PROTOCOL_MIN_VERSION - 1).unwrap_err();
        assert!(matches!(err, ProtocolNegotiationError::NoOverlap { .. }));
        assert!(err.to_string().contains("Protocol mismatch"));
    }

    #[test]
    fn negotiate_protocol_version_rejects_newer_client() {
        let err = negotiate_protocol_version(PROTOCOL_MAX_VERSION + 1, PROTOCOL_MAX_VERSION + 2)
            .unwrap_err();
        assert!(matches!(err, ProtocolNegotiationError::NoOverlap { .. }));
        assert!(err.to_string().contains("Protocol mismatch"));
    }

    #[test]
    fn negotiate_protocol_version_rejects_malformed_range() {
        let err = negotiate_protocol_version(2, 1).unwrap_err();
        assert!(matches!(
            err,
            ProtocolNegotiationError::InvalidClientRange { .. }
        ));
        assert!(err.to_string().contains("Invalid client protocol range"));
    }

    #[test]
    fn test_tolerate_unknown_fields_in_struct() {
        let json = r#"{
            "id": 1,
            "request": {
                "type": "Handshake",
                "client_name": "test",
                "client_version": "1.0",
                "protocol_min": 1,
                "protocol_max": 2,
                "unknown_field": "ignore me"
            }
        }"#;
        let msg = DebugMessage::parse(json).expect("Should tolerate unknown fields");
        if let Some(DebugRequest::Handshake { client_name, .. }) = msg.request {
            assert_eq!(client_name, "test");
        } else {
            panic!("Expected Handshake request");
        }
    }

    #[test]
    fn test_strict_required_fields() {
        let json = r#"{
            "id": 1,
            "request": {
                "type": "Handshake",
                "client_name": "test"
            }
        }"#;
        let err = DebugMessage::parse(json).unwrap_err();
        assert!(
            err.contains("client_version"),
            "Error should mention missing field: {}",
            err
        );
    }

    #[test]
    fn test_forward_compat_unknown_enum_variant() {
        let json = r#"{
            "id": 1,
            "request": {
                "type": "FutureRequestType",
                "some_data": 42
            }
        }"#;
        let msg = DebugMessage::parse(json).expect("Should tolerate unknown request type");
        assert!(matches!(msg.request, Some(DebugRequest::Unknown)));
    }

    #[test]
    fn test_dynamic_trace_event_unified_call_depth() {
        let json = r#"{
            "sequence": 1,
            "kind": "FunctionCall",
            "message": "test",
            "call_depth": 5
        }"#;
        let event: DynamicTraceEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.call_depth, Some(5));
    }

    #[test]
    fn breakpoint_descriptor_serializes_hit_count() {
        let descriptor = BreakpointDescriptor {
            id: "bp-1".to_string(),
            function: "transfer".to_string(),
            condition: None,
            hit_condition: None,
            log_message: None,
            hit_count: 3,
        };

        let value = serde_json::to_value(descriptor).unwrap();

        assert_eq!(value["hit_count"], 3);
    }
}
