use crate::debugger::state::DebugState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum DebugRequest {
    Handshake {
        token: String,
    },
    Step,
    Continue,
    AddBreakpoint {
        function: String,
    },
    RemoveBreakpoint {
        function: String,
    },
    GetState,
    Execute {
        function: String,
        args: Option<String>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[allow(clippy::large_enum_variant)]
pub enum DebugResponse {
    Ok,
    Error(String),
    State(DebugState),
    ExecutionResult { result: String },
    AuthSuccess,
    AuthFailed,
}
