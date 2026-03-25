use crate::server::protocol::{DebugMessage, DebugRequest, DebugResponse};
use crate::{DebuggerError, Result};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use tracing::info;

/// Remote client for connecting to a debug server
pub struct RemoteClient {
    stream: BufReader<TcpStream>,
    message_id: u64,
    authenticated: bool,
}

impl RemoteClient {
    /// Connect to a remote debug server
    pub fn connect(addr: &str, token: Option<String>) -> Result<Self> {
        info!("Connecting to debug server at {}", addr);
        let stream = TcpStream::connect(addr).map_err(|e| {
            DebuggerError::NetworkError(format!("Failed to connect to {}: {}", addr, e))
        })?;

        let mut client = Self {
            stream: BufReader::new(stream),
            message_id: 0,
            authenticated: token.is_none(),
        };

        // Authenticate if token is provided
        if let Some(token) = token {
            client.authenticate(&token)?;
        }

        Ok(client)
    }

    /// Authenticate with the server
    pub fn authenticate(&mut self, token: &str) -> Result<()> {
        let response = self.send_request(DebugRequest::Authenticate {
            token: token.to_string(),
        })?;

        match response {
            DebugResponse::Authenticated { success, message } => {
                if success {
                    self.authenticated = true;
                    info!("Authentication successful");
                    Ok(())
                } else {
                    Err(DebuggerError::ExecutionError(format!(
                        "Authentication failed: {}",
                        message
                    ))
                    .into())
                }
            }
            _ => Err(DebuggerError::ExecutionError(
                "Unexpected response to authentication".to_string(),
            )
            .into()),
        }
    }

    /// Load a contract on the server
    pub fn load_contract(&mut self, contract_path: &str) -> Result<usize> {
        let response = self.send_request(DebugRequest::LoadContract {
            contract_path: contract_path.to_string(),
        })?;

        match response {
            DebugResponse::ContractLoaded { size } => {
                info!("Contract loaded: {} bytes", size);
                Ok(size)
            }
            DebugResponse::Error { message } => Err(DebuggerError::ExecutionError(message).into()),
            _ => Err(DebuggerError::ExecutionError(
                "Unexpected response to LoadContract".to_string(),
            )
            .into()),
        }
    }

    /// Execute a function on the remote server
    pub fn execute(&mut self, function: &str, args: Option<&str>) -> Result<String> {
        let response = self.send_request(DebugRequest::Execute {
            function: function.to_string(),
            args: args.map(|s| s.to_string()),
        })?;

        match response {
            DebugResponse::ExecutionResult {
                success,
                output,
                error,
                ..
            } => {
                if success {
                    Ok(output)
                } else {
                    Err(DebuggerError::ExecutionError(
                        error.unwrap_or_else(|| "Unknown error".to_string()),
                    )
                    .into())
                }
            }
            DebugResponse::Error { message } => Err(DebuggerError::ExecutionError(message).into()),
            _ => Err(
                DebuggerError::ExecutionError("Unexpected response to Execute".to_string()).into(),
            ),
        }
    }

    /// Step into next inline/instruction
    pub fn step_in(&mut self) -> Result<(bool, Option<String>, u64)> {
        let response = self.send_request(DebugRequest::StepIn)?;

        match response {
            DebugResponse::StepResult {
                paused,
                current_function,
                step_count,
                ..
            } => Ok((paused, current_function, step_count)),
            DebugResponse::Error { message } => Err(DebuggerError::ExecutionError(message).into()),
            _ => Err(
                DebuggerError::ExecutionError("Unexpected response to StepIn".to_string()).into(),
            ),
        }
    }

    /// Step over current function
    pub fn step_over(&mut self) -> Result<(bool, Option<String>, u64)> {
        let response = self.send_request(DebugRequest::Next)?;

        match response {
            DebugResponse::StepResult {
                paused,
                current_function,
                step_count,
                ..
            } => Ok((paused, current_function, step_count)),
            DebugResponse::Error { message } => Err(DebuggerError::ExecutionError(message).into()),
            _ => {
                Err(DebuggerError::ExecutionError("Unexpected response to Next".to_string()).into())
            }
        }
    }

    /// Step out of current function
    pub fn step_out(&mut self) -> Result<(bool, Option<String>, u64)> {
        let response = self.send_request(DebugRequest::StepOut)?;

        match response {
            DebugResponse::StepResult {
                paused,
                current_function,
                step_count,
                ..
            } => Ok((paused, current_function, step_count)),
            DebugResponse::Error { message } => Err(DebuggerError::ExecutionError(message).into()),
            _ => Err(
                DebuggerError::ExecutionError("Unexpected response to StepOut".to_string()).into(),
            ),
        }
    }

    /// Continue execution
    pub fn continue_execution(&mut self) -> Result<bool> {
        let response = self.send_request(DebugRequest::Continue)?;

        match response {
            DebugResponse::ContinueResult { completed, .. } => Ok(completed),
            DebugResponse::Error { message } => Err(DebuggerError::ExecutionError(message).into()),
            _ => Err(
                DebuggerError::ExecutionError("Unexpected response to Continue".to_string()).into(),
            ),
        }
    }

    /// Inspect current state
    pub fn inspect(&mut self) -> Result<(Option<String>, u64, bool, Vec<String>)> {
        let response = self.send_request(DebugRequest::Inspect)?;

        match response {
            DebugResponse::InspectionResult {
                function,
                step_count,
                paused,
                call_stack,
                ..
            } => Ok((function, step_count, paused, call_stack)),
            DebugResponse::Error { message } => Err(DebuggerError::ExecutionError(message).into()),
            _ => Err(
                DebuggerError::ExecutionError("Unexpected response to Inspect".to_string()).into(),
            ),
        }
    }

    /// Get storage state
    pub fn get_storage(&mut self) -> Result<String> {
        let response = self.send_request(DebugRequest::GetStorage)?;

        match response {
            DebugResponse::StorageState { storage_json } => Ok(storage_json),
            DebugResponse::Error { message } => Err(DebuggerError::ExecutionError(message).into()),
            _ => Err(DebuggerError::ExecutionError(
                "Unexpected response to GetStorage".to_string(),
            )
            .into()),
        }
    }

    /// Get call stack
    pub fn get_stack(&mut self) -> Result<Vec<String>> {
        let response = self.send_request(DebugRequest::GetStack)?;

        match response {
            DebugResponse::CallStack { stack } => Ok(stack),
            DebugResponse::Error { message } => Err(DebuggerError::ExecutionError(message).into()),
            _ => Err(
                DebuggerError::ExecutionError("Unexpected response to GetStack".to_string()).into(),
            ),
        }
    }

    /// Get budget information
    pub fn get_budget(&mut self) -> Result<(u64, u64)> {
        let response = self.send_request(DebugRequest::GetBudget)?;

        match response {
            DebugResponse::BudgetInfo {
                cpu_instructions,
                memory_bytes,
            } => Ok((cpu_instructions, memory_bytes)),
            DebugResponse::Error { message } => Err(DebuggerError::ExecutionError(message).into()),
            _ => Err(
                DebuggerError::ExecutionError("Unexpected response to GetBudget".to_string())
                    .into(),
            ),
        }
    }

    /// Set a breakpoint
    pub fn set_breakpoint(&mut self, function: &str) -> Result<()> {
        let response = self.send_request(DebugRequest::SetBreakpoint {
            function: function.to_string(),
        })?;

        match response {
            DebugResponse::BreakpointSet { .. } => {
                info!("Breakpoint set at {}", function);
                Ok(())
            }
            DebugResponse::Error { message } => Err(DebuggerError::ExecutionError(message).into()),
            _ => Err(DebuggerError::ExecutionError(
                "Unexpected response to SetBreakpoint".to_string(),
            )
            .into()),
        }
    }

    /// Clear a breakpoint
    pub fn clear_breakpoint(&mut self, function: &str) -> Result<()> {
        let response = self.send_request(DebugRequest::ClearBreakpoint {
            function: function.to_string(),
        })?;

        match response {
            DebugResponse::BreakpointCleared { .. } => {
                info!("Breakpoint cleared at {}", function);
                Ok(())
            }
            DebugResponse::Error { message } => Err(DebuggerError::ExecutionError(message).into()),
            _ => Err(DebuggerError::ExecutionError(
                "Unexpected response to ClearBreakpoint".to_string(),
            )
            .into()),
        }
    }

    /// List all breakpoints
    pub fn list_breakpoints(&mut self) -> Result<Vec<String>> {
        let response = self.send_request(DebugRequest::ListBreakpoints)?;

        match response {
            DebugResponse::BreakpointsList { breakpoints } => Ok(breakpoints),
            DebugResponse::Error { message } => Err(DebuggerError::ExecutionError(message).into()),
            _ => Err(DebuggerError::ExecutionError(
                "Unexpected response to ListBreakpoints".to_string(),
            )
            .into()),
        }
    }

    /// Set initial storage
    pub fn set_storage(&mut self, storage_json: &str) -> Result<()> {
        let response = self.send_request(DebugRequest::SetStorage {
            storage_json: storage_json.to_string(),
        })?;

        match response {
            DebugResponse::StorageState { .. } => {
                info!("Storage set successfully");
                Ok(())
            }
            DebugResponse::Error { message } => Err(DebuggerError::ExecutionError(message).into()),
            _ => Err(DebuggerError::ExecutionError(
                "Unexpected response to SetStorage".to_string(),
            )
            .into()),
        }
    }

    /// Load network snapshot
    pub fn load_snapshot(&mut self, snapshot_path: &str) -> Result<String> {
        let response = self.send_request(DebugRequest::LoadSnapshot {
            snapshot_path: snapshot_path.to_string(),
        })?;

        match response {
            DebugResponse::SnapshotLoaded { summary } => {
                info!("Snapshot loaded: {}", summary);
                Ok(summary)
            }
            DebugResponse::Error { message } => Err(DebuggerError::ExecutionError(message).into()),
            _ => Err(DebuggerError::ExecutionError(
                "Unexpected response to LoadSnapshot".to_string(),
            )
            .into()),
        }
    }

    /// Ping the server
    pub fn ping(&mut self) -> Result<()> {
        let response = self.send_request(DebugRequest::Ping)?;

        match response {
            DebugResponse::Pong { .. } => {
                info!("Server responded to ping");
                Ok(())
            }
            _ => {
                Err(DebuggerError::ExecutionError("Unexpected response to Ping".to_string()).into())
            }
        }
    }

    /// Disconnect from the server
    pub fn disconnect(&mut self) -> Result<()> {
        let _ = self.send_request(DebugRequest::Disconnect);
        info!("Disconnected from server");
        Ok(())
    }

    /// Send a request and wait for response
    fn send_request(&mut self, request: DebugRequest) -> Result<DebugResponse> {
        if !self.authenticated
            && !matches!(
                request,
                DebugRequest::Authenticate { .. } | DebugRequest::Ping
            )
        {
            return Err(DebuggerError::ExecutionError(
                "Not authenticated. Call authenticate() first.".to_string(),
            )
            .into());
        }

        self.message_id += 1;
        let expected_id = self.message_id;
        let message = DebugMessage::request(expected_id, request);

        let request_json = serde_json::to_string(&message)
            .map_err(|e| DebuggerError::FileError(format!("Failed to serialize request: {}", e)))?;

        // Send request
        writeln!(self.stream.get_mut(), "{}", request_json).map_err(|e| {
            DebuggerError::NetworkError(format!("Failed to write to stream: {}", e))
        })?;
        self.stream
            .get_mut()
            .flush()
            .map_err(|e| DebuggerError::NetworkError(format!("Failed to flush stream: {}", e)))?;

        // Read response
        let mut response_line = String::new();
        let n = self
            .stream
            .read_line(&mut response_line)
            .map_err(|e| DebuggerError::NetworkError(format!("Failed to read response: {}", e)))?;
        if n == 0 {
            return Err(DebuggerError::NetworkError("No response from server".to_string()).into());
        }

        parse_response_line(expected_id, response_line.trim_end())
    }
}

fn parse_response_line(expected_id: u64, response_line: &str) -> Result<DebugResponse> {
    let response_message: DebugMessage = serde_json::from_str(response_line)
        .map_err(|e| DebuggerError::FileError(format!("Failed to parse response: {}", e)))?;

    if response_message.id != expected_id {
        return Err(DebuggerError::ExecutionError(format!(
            "Mismatched response id: expected {} got {}",
            expected_id, response_message.id
        ))
        .into());
    }

    response_message.response.ok_or_else(|| {
        DebuggerError::FileError("Response message has no response field".to_string()).into()
    })
}

impl Drop for RemoteClient {
    fn drop(&mut self) {
        let _ = self.disconnect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::protocol::DebugResponse;

    #[test]
    fn parse_response_line_rejects_mismatched_ids() {
        let msg = DebugMessage::response(
            42,
            DebugResponse::Pong {
                backend_version: Some("0.1.0".to_string()),
                protocol_version: Some("1".to_string()),
            },
        );
        let line = serde_json::to_string(&msg).unwrap();
        let err = parse_response_line(7, &line).unwrap_err();
        assert!(err.to_string().contains("Mismatched response id"));
    }

    #[test]
    fn parse_response_line_accepts_matching_ids() {
        let msg = DebugMessage::response(
            7,
            DebugResponse::Pong {
                backend_version: Some("0.1.0".to_string()),
                protocol_version: Some("1".to_string()),
            },
        );
        let line = serde_json::to_string(&msg).unwrap();
        let resp = parse_response_line(7, &line).unwrap();
        assert!(matches!(resp, DebugResponse::Pong { .. }));
    }

    #[test]
    fn connect_failure_is_network_error_category() {
        let err = RemoteClient::connect("127.0.0.1:1", None).unwrap_err();
        assert!(err.to_string().contains("Network/transport error"));
    }
}
