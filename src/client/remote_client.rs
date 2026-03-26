use crate::server::protocol::{
    DebugMessage, DebugRequest, DebugResponse, PROTOCOL_MAX_VERSION, PROTOCOL_MIN_VERSION,
};
use crate::{DebuggerError, Result};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;
use tracing::info;

#[derive(Debug, Clone)]
pub struct RequestTimeouts {
    pub default: Duration,
    pub ping: Duration,
    pub inspect: Duration,
    pub get_storage: Duration,
}

impl Default for RequestTimeouts {
    fn default() -> Self {
        Self {
            default: Duration::from_millis(30_000),
            ping: Duration::from_millis(2_000),
            inspect: Duration::from_millis(5_000),
            get_storage: Duration::from_millis(10_000),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub max_attempts: usize,
    pub base_delay: Duration,
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(200),
            max_delay: Duration::from_millis(2_000),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct RemoteClientConfig {
    pub timeouts: RequestTimeouts,
    pub retry: RetryPolicy,
}

/// Remote client for connecting to a debug server
#[derive(Debug)]
pub struct RemoteClient {
    addr: String,
    token: Option<String>,
    stream: BufReader<TcpStream>,
    message_id: u64,
    authenticated: bool,
    config: RemoteClientConfig,
}

impl RemoteClient {
    /// Connect to a remote debug server
    pub fn connect(addr: &str, token: Option<String>) -> Result<Self> {
        Self::connect_with_config(addr, token, RemoteClientConfig::default())
    }

    pub fn connect_with_config(
        addr: &str,
        token: Option<String>,
        config: RemoteClientConfig,
    ) -> Result<Self> {
        info!("Connecting to debug server at {}", addr);
        let stream = TcpStream::connect(addr).map_err(|e| {
            DebuggerError::NetworkError(format!("Failed to connect to {}: {}", addr, e))
        })?;

        let mut client = Self {
            addr: addr.to_string(),
            token: token.clone(),
            stream: BufReader::new(stream),
            message_id: 0,
            authenticated: token.is_none(),
            config,
        };

        client.handshake("rust-remote-client", env!("CARGO_PKG_VERSION"))?;

        // Authenticate if token is provided
        if let Some(token) = token {
            client.authenticate(&token)?;
        }

        Ok(client)
    }

    /// Perform a protocol handshake and verify compatibility.
    pub fn handshake(&mut self, client_name: &str, client_version: &str) -> Result<u32> {
        let response = self.send_request(DebugRequest::Handshake {
            client_name: client_name.to_string(),
            client_version: client_version.to_string(),
            protocol_min: PROTOCOL_MIN_VERSION,
            protocol_max: PROTOCOL_MAX_VERSION,
        })?;

        match response {
            DebugResponse::HandshakeAck {
                selected_version, ..
            } => Ok(selected_version),
            DebugResponse::IncompatibleProtocol { message, .. } => {
                Err(DebuggerError::ExecutionError(format!(
                    "Incompatible debugger protocol: {}",
                    message
                ))
                .into())
            }
            DebugResponse::Error { message } => Err(DebuggerError::ExecutionError(message).into()),
            _ => Err(
                DebuggerError::ExecutionError("Unexpected response to Handshake".to_string())
                    .into(),
            ),
        }
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
                    Err(DebuggerError::AuthenticationFailed(message).into())
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
        let response =
            self.send_request_with_retry(DebugRequest::Inspect, RequestClass::Inspect, true)?;

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
        let response =
            self.send_request_with_retry(DebugRequest::GetStorage, RequestClass::GetStorage, true)?;

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
        let response =
            self.send_request_with_retry(DebugRequest::GetStack, RequestClass::Default, true)?;

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
        let response =
            self.send_request_with_retry(DebugRequest::GetBudget, RequestClass::Default, true)?;

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
    pub fn set_breakpoint(&mut self, function: &str, _condition: Option<String>) -> Result<()> {
        let response = self.send_request(DebugRequest::SetBreakpoint {
            id: function.to_string(),
            function: function.to_string(),
            condition: None,
            hit_condition: None,
            log_message: None,
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
            id: function.to_string(),
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
            DebugResponse::BreakpointsList { breakpoints } => Ok(breakpoints
                .into_iter()
                .map(|breakpoint| breakpoint.function)
                .collect()),
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
        let response =
            self.send_request_with_retry(DebugRequest::Ping, RequestClass::Ping, true)?;

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
        self.send_request_with_retry(request, RequestClass::Default, false)
    }

    fn reconnect(&mut self) -> Result<()> {
        let stream = TcpStream::connect(&self.addr).map_err(|e| {
            DebuggerError::NetworkError(format!("Failed to reconnect to {}: {}", self.addr, e))
        })?;
        self.stream = BufReader::new(stream);
        self.authenticated = self.token.is_none();
        if let Some(token) = self.token.clone() {
            self.authenticate(&token)?;
        }
        Ok(())
    }

    fn timeout_for_class(&self, class: RequestClass) -> Duration {
        match class {
            RequestClass::Ping => self.config.timeouts.ping,
            RequestClass::Inspect => self.config.timeouts.inspect,
            RequestClass::GetStorage => self.config.timeouts.get_storage,
            RequestClass::Default => self.config.timeouts.default,
        }
    }

    fn send_request_with_retry(
        &mut self,
        request: DebugRequest,
        class: RequestClass,
        idempotent: bool,
    ) -> Result<DebugResponse> {
        let timeout = self.timeout_for_class(class);
        let operation = class.operation_name();

        let max_attempts = if idempotent {
            self.config.retry.max_attempts.max(1)
        } else {
            1
        };

        for attempt in 1..=max_attempts {
            match self.send_request_once(request.clone(), timeout) {
                Ok(resp) => return Ok(resp),
                Err(failure) => {
                    if !idempotent || attempt >= max_attempts || !failure.is_transient() {
                        return Err(failure.into_error(operation).into());
                    }

                    // On transient failures, prefer reconnecting to clear any partial state/buffers.
                    let _ = self.reconnect();
                    std::thread::sleep(backoff_delay(
                        self.config.retry.base_delay,
                        self.config.retry.max_delay,
                        attempt,
                    ));
                }
            }
        }

        Err(DebuggerError::NetworkError(format!(
            "Failed to complete {} after {} attempt(s)",
            operation, max_attempts
        ))
        .into())
    }

    fn send_request_once(
        &mut self,
        request: DebugRequest,
        timeout: Duration,
    ) -> std::result::Result<DebugResponse, SendFailure> {
        if !self.authenticated
            && !matches!(
                request,
                DebugRequest::Handshake { .. }
                    | DebugRequest::Authenticate { .. }
                    | DebugRequest::Ping
            )
        {
            return Err(SendFailure::NotAuthenticated);
        }

        self.message_id += 1;
        let expected_id = self.message_id;
        let message = DebugMessage::request(expected_id, request);

        let request_json = serde_json::to_string(&message)
            .map_err(|e| SendFailure::Serialize(format!("Failed to serialize request: {}", e)))?;

        self.stream
            .get_mut()
            .set_read_timeout(Some(timeout))
            .map_err(|e| SendFailure::Io {
                stage: "set_read_timeout",
                source: e,
            })?;
        self.stream
            .get_mut()
            .set_write_timeout(Some(timeout))
            .map_err(|e| SendFailure::Io {
                stage: "set_write_timeout",
                source: e,
            })?;

        writeln!(self.stream.get_mut(), "{}", request_json)
            .map_err(|e| SendFailure::io("write", e, timeout))?;
        self.stream
            .get_mut()
            .flush()
            .map_err(|e| SendFailure::io("flush", e, timeout))?;

        let mut response_line = String::new();
        let n = self
            .stream
            .read_line(&mut response_line)
            .map_err(|e| SendFailure::io("read", e, timeout))?;
        if n == 0 {
            return Err(SendFailure::Disconnected);
        }

        parse_response_line(expected_id, response_line.trim_end())
            .map_err(|e| SendFailure::Protocol(e.to_string()))
    }
}

#[derive(Debug, Clone, Copy)]
enum RequestClass {
    Ping,
    Inspect,
    GetStorage,
    Default,
}

impl RequestClass {
    fn operation_name(self) -> &'static str {
        match self {
            RequestClass::Ping => "Ping",
            RequestClass::Inspect => "Inspect",
            RequestClass::GetStorage => "GetStorage",
            RequestClass::Default => "Request",
        }
    }
}

#[derive(Debug)]
enum SendFailure {
    NotAuthenticated,
    Disconnected,
    Timeout {
        stage: &'static str,
        timeout: Duration,
    },
    Io {
        stage: &'static str,
        source: std::io::Error,
    },
    Serialize(String),
    Protocol(String),
}

impl SendFailure {
    fn io(stage: &'static str, source: std::io::Error, timeout: Duration) -> Self {
        match source.kind() {
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => {
                SendFailure::Timeout { stage, timeout }
            }
            _ => SendFailure::Io { stage, source },
        }
    }

    fn is_transient(&self) -> bool {
        match self {
            SendFailure::Timeout { .. } | SendFailure::Disconnected => true,
            SendFailure::Io { source, .. } => matches!(
                source.kind(),
                std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::WouldBlock
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::NotConnected
                    | std::io::ErrorKind::UnexpectedEof
            ),
            _ => false,
        }
    }

    fn into_error(self, operation: &str) -> DebuggerError {
        match self {
            SendFailure::NotAuthenticated => DebuggerError::AuthenticationFailed(
                "Not authenticated. Call authenticate() first.".to_string(),
            ),
            SendFailure::Disconnected => DebuggerError::NetworkError(format!(
                "{} failed: connection closed by peer",
                operation
            )),
            SendFailure::Timeout { stage, timeout } => DebuggerError::RequestTimeout {
                operation: format!("{} ({})", operation, stage),
                timeout_ms: timeout.as_millis() as u64,
            },
            SendFailure::Io { stage, source } => DebuggerError::NetworkError(format!(
                "{} failed during {}: {}",
                operation, stage, source
            )),
            SendFailure::Serialize(message) => DebuggerError::FileError(message),
            SendFailure::Protocol(message) => DebuggerError::NetworkError(format!(
                "{} failed: protocol error: {}",
                operation, message
            )),
        }
    }
}

fn backoff_delay(base: Duration, max: Duration, attempt: usize) -> Duration {
    if attempt <= 1 {
        return base.min(max);
    }

    let exp = 1u32.checked_shl((attempt - 1).min(31) as u32).unwrap_or(0);
    base.checked_mul(exp).unwrap_or(max).min(max)
}

fn parse_response_line(expected_id: u64, response_line: &str) -> Result<DebugResponse> {
    let response_message = DebugMessage::parse(response_line)
        .map_err(|e| DebuggerError::FileError(format!("Failed to parse response: {}", e)))?;

    if response_message.id != expected_id {
        return Err(DebuggerError::ExecutionError(format!(
            "Mismatched response id: expected {} got {}",
            expected_id, response_message.id
        ))
        .into());
    }

    let response = response_message.response.ok_or_else(|| {
        DebuggerError::FileError("Response message has no response field".to_string())
    })?;

    if matches!(response, DebugResponse::Unknown) {
        return Err(DebuggerError::ExecutionError(
            "Received unknown response type from server. Try upgrading the client.".to_string(),
        )
        .into());
    }

    Ok(response)
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
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

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

    #[test]
    fn ping_times_out_deterministically() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                // Consume one request line but never respond.
                let mut reader = BufReader::new(&mut stream);
                let mut line = String::new();
                let _ = reader.read_line(&mut line);
                std::thread::sleep(Duration::from_millis(200));
            }
        });

        let config = RemoteClientConfig {
            timeouts: RequestTimeouts {
                ping: Duration::from_millis(50),
                ..RequestTimeouts::default()
            },
            retry: RetryPolicy {
                max_attempts: 1,
                base_delay: Duration::from_millis(1),
                max_delay: Duration::from_millis(1),
            },
        };

        let mut client =
            RemoteClient::connect_with_config(&addr.to_string(), None, config).unwrap();
        let err = client.ping().unwrap_err();
        assert!(err.to_string().contains("Request timed out"));
    }

    #[test]
    fn ping_retries_on_disconnect_and_succeeds() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(AtomicUsize::new(0));
        let seen_server = Arc::clone(&seen);

        std::thread::spawn(move || {
            for stream in listener.incoming().take(2) {
                let mut stream = stream.unwrap();
                let attempt = seen_server.fetch_add(1, Ordering::SeqCst);

                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                let _ = reader.read_line(&mut line);

                if attempt == 0 {
                    // Drop connection without responding.
                    drop(stream);
                    continue;
                }

                let msg: DebugMessage = serde_json::from_str(line.trim_end()).unwrap();
                let id = msg.id;
                let response = DebugMessage::response(
                    id,
                    DebugResponse::Pong {
                        backend_version: Some("0.1.0".to_string()),
                        protocol_version: Some("1".to_string()),
                    },
                );
                let json = serde_json::to_string(&response).unwrap();
                let _ = writeln!(stream, "{}", json);
                let _ = stream.flush();
            }
        });

        let config = RemoteClientConfig {
            timeouts: RequestTimeouts {
                ping: Duration::from_millis(500),
                ..RequestTimeouts::default()
            },
            retry: RetryPolicy {
                max_attempts: 3,
                base_delay: Duration::from_millis(1),
                max_delay: Duration::from_millis(5),
            },
        };

        let mut client =
            RemoteClient::connect_with_config(&addr.to_string(), None, config).unwrap();
        client.ping().unwrap();
        assert!(seen.load(Ordering::SeqCst) >= 2);
    }
}
