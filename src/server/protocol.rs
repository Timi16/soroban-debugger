use serde::{Deserialize, Serialize};

pub const DEBUG_PROTOCOL_VERSION: &str = "1";

/// Structured event category used by dynamic security analysis.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DynamicTraceEventKind {
    Diagnostic,
    FunctionCall,
    StorageRead,
    StorageWrite,
    Authorization,
    CrossContractCall,
}

/// Rich dynamic trace entry produced by the runtime and consumed by analyzers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicTraceEvent {
    pub sequence: usize,
    pub kind: DynamicTraceEventKind,
    pub message: String,
    pub function: Option<String>,
    pub storage_key: Option<String>,
    pub storage_value: Option<String>,
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

/// Wire protocol messages for remote debugging
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DebugRequest {
    /// Authenticate with the server
    Authenticate { token: String },

    /// Load a contract
    LoadContract { contract_path: String },

    /// Execute a function
    Execute {
        function: String,
        args: Option<String>,
    },

    /// Step into next inline/instruction
    StepIn,

    /// Step over current function
    Next,

    /// Step out of current function
    StepOut,

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
    SetBreakpoint { function: String },

    /// Clear a breakpoint
    ClearBreakpoint { function: String },

    /// List all breakpoints
    ListBreakpoints,

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
}

/// Response messages from the server
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DebugResponse {
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
    },

    /// Step result
    StepResult {
        paused: bool,
        current_function: Option<String>,
        step_count: u64,
        source_location: Option<SourceLocation>,
    },

    /// Continue result
    ContinueResult {
        completed: bool,
        output: Option<String>,
        error: Option<String>,
        paused: bool,
        source_location: Option<SourceLocation>,
    },

    /// Inspection result
    InspectionResult {
        function: Option<String>,
        args: Option<String>,
        step_count: u64,
        paused: bool,
        call_stack: Vec<String>,
        source_location: Option<SourceLocation>,
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
    BreakpointSet { function: String },

    /// Breakpoint cleared
    BreakpointCleared { function: String },

    /// List of breakpoints
    BreakpointsList { breakpoints: Vec<String> },

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

    /// Pong response with optional version metadata.
    ///
    /// Fields are optional for backward compatibility with older clients/servers.
    Pong {
        backend_version: Option<String>,
        protocol_version: Option<String>,
    },

    /// Disconnected
    Disconnected,
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
}
