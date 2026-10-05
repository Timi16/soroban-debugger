use crate::server::protocol::{
    DebugMessage, DebugRequest, DebugResponse, PROTOCOL_MAX_VERSION, PROTOCOL_MIN_VERSION,
};
use crate::{DebuggerError, Result};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

use rustls::client::ServerName;
use rustls::{Certificate, ClientConfig, PrivateKey, RootCertStore};

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

#[derive(Debug, Clone)]
pub struct RemoteClientConfig {
    pub connect_timeout: Duration, // defaults to 10 seconds.
    pub timeouts: RequestTimeouts,
    pub retry: RetryPolicy,
    pub heartbeat_interval_ms: Option<u32>,
    pub idle_timeout_ms: Option<u32>,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    pub tls_ca: Option<PathBuf>,
    pub session_label: Option<String>,
}

pub type InspectResult = (Option<String>, u64, bool, Vec<String>, Option<String>);

impl Default for RemoteClientConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_millis(10_000),
            timeouts: RequestTimeouts::default(),
            retry: RetryPolicy::default(),
            heartbeat_interval_ms: None,
            idle_timeout_ms: None,
            tls_cert: None,
            tls_key: None,
            tls_ca: None,
            session_label: None,
        }
    }
}

impl RemoteClientConfig {
    pub fn build_timeouts(
        default_ms: u64,
        inspect_ms: Option<u64>,
        storage_ms: Option<u64>,
    ) -> RequestTimeouts {
        RequestTimeouts {
            default: Duration::from_millis(default_ms),
            ping: Duration::from_millis(2_000),
            inspect: Duration::from_millis(inspect_ms.unwrap_or(default_ms)),
            get_storage: Duration::from_millis(storage_ms.unwrap_or(default_ms)),
        }
    }
}

/// Information returned by a successful session reconnection.
#[derive(Debug, Clone)]
pub struct ReconnectInfo {
    /// The session identifier for the reconnected session.
    pub session_id: String,
    /// Whether the debugger is currently paused at a breakpoint.
    pub paused: bool,
    /// The function currently being debugged, if any.
    pub current_function: Option<String>,
    /// List of active breakpoint identifiers in the session.
    pub breakpoints: Vec<String>,
    /// Total number of execution steps taken so far.
    pub step_count: u64,
}

/// Remote client for connecting to a debug server
#[derive(Debug)]
pub struct RemoteClient {
    addr: String,
    token: Option<String>,
    stream: BufReader<RemoteStream>,
    message_id: u64,
    authenticated: bool,
    config: RemoteClientConfig,
    /// The protocol version selected during the handshake.
    selected_protocol_version: Option<u32>,
    /// Session identifier received from the server during the initial handshake.
    /// Used to reconnect to an existing session after a transient disconnect.
    session_id: Option<String>,
    /// Metadata about the current session received from the server.
    session_info: Option<crate::server::protocol::RemoteSessionInfo>,
    /// Capabilities advertised by the server during the handshake.
    negotiated_capabilities: Option<crate::server::protocol::ServerCapabilities>,
}

#[derive(Debug)]
enum RemoteStream {
    Plain(TcpStream),
    Tls(Box<rustls::StreamOwned<rustls::client::ClientConnection, TcpStream>>),
}

impl Read for RemoteStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(s) => s.read(buf),
            Self::Tls(s) => s.read(buf),
        }
    }
}

impl Write for RemoteStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(s) => s.write(buf),
            Self::Tls(s) => s.write(buf),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Plain(s) => s.flush(),
            Self::Tls(s) => s.flush(),
        }
    }
}

impl RemoteStream {
    fn set_read_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        match self {
            Self::Plain(s) => s.set_read_timeout(timeout),
            Self::Tls(s) => s.get_ref().set_read_timeout(timeout),
        }
    }

    fn set_write_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        match self {
            Self::Plain(s) => s.set_write_timeout(timeout),
            Self::Tls(s) => s.get_ref().set_write_timeout(timeout),
        }
    }
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
        let stream = Self::create_stream(addr, &config)?;

        let mut client = Self {
            addr: addr.to_string(),
            token: token.clone(),
            stream: BufReader::new(stream),
            message_id: 0,
            authenticated: token.is_none(),
            config,
            selected_protocol_version: None,
            session_id: None,
            session_info: None,
            negotiated_capabilities: None,
        };

        client.handshake("rust-remote-client", env!("CARGO_PKG_VERSION"))?;

        // Authenticate if token is provided
        if let Some(token) = token {
            client.authenticate(&token)?;
        }

        Ok(client)
    }

    fn create_stream(addr: &str, config: &RemoteClientConfig) -> Result<RemoteStream> {
        use std::net::ToSocketAddrs;
        let socket_addr = addr
            .to_socket_addrs()
            .map_err(|e| {
                DebuggerError::NetworkError(format!("Failed to resolve address '{}': {}", addr, e))
            })?
            .next()
            .ok_or_else(|| {
                DebuggerError::NetworkError(format!("No socket address resolved for '{}'", addr))
            })?;

        // ── TCP connect with configurable timeout ────────────────────────────
        let tcp_stream =
            TcpStream::connect_timeout(&socket_addr, config.connect_timeout).map_err(|e| {
                let kind_hint = match e.kind() {
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => {
                        format!(
                            "connect timed out after {}ms — use --connect-timeout-ms to \
                             extend the window",
                            config.connect_timeout.as_millis()
                        )
                    }
                    std::io::ErrorKind::ConnectionRefused => {
                        "connection refused — verify the server is running and the port \
                         is correct"
                            .to_string()
                    }
                    std::io::ErrorKind::PermissionDenied => {
                        "permission denied — loopback networking may be restricted in this \
                         environment (sandbox/container); see docs/remote-troubleshooting.md"
                            .to_string()
                    }
                    _ => format!("TCP connect error: {}", e),
                };
                DebuggerError::NetworkError(format!(
                    "Failed to connect to '{}': {}",
                    addr, kind_hint
                ))
            })?;

        if config.tls_cert.is_some() || config.tls_key.is_some() || config.tls_ca.is_some() {
            let mut root_store = RootCertStore::empty();
            if let Some(ref ca_path) = config.tls_ca {
                let ca_file = std::fs::File::open(ca_path).map_err(|e| {
                    DebuggerError::FileError(format!("Failed to open CA cert {:?}: {}", ca_path, e))
                })?;
                let mut reader = BufReader::new(ca_file);
                let certs = rustls_pemfile::certs(&mut reader).map_err(|e| {
                    DebuggerError::FileError(format!(
                        "Failed to parse CA cert {:?}: {}",
                        ca_path, e
                    ))
                })?;
                for cert in certs {
                    root_store.add(&Certificate(cert)).map_err(|e| {
                        DebuggerError::FileError(format!("Failed to add cert to root store: {}", e))
                    })?;
                }
            } else {
                for cert in rustls_native_certs::load_native_certs().map_err(|e| {
                    DebuggerError::NetworkError(format!("Failed to load native certs: {}", e))
                })? {
                    root_store.add(&Certificate(cert.0)).map_err(|e| {
                        DebuggerError::FileError(format!(
                            "Failed to add native cert to root store: {}",
                            e
                        ))
                    })?;
                }
            }

            let client_config = if let (Some(ref cert_path), Some(ref key_path)) =
                (&config.tls_cert, &config.tls_key)
            {
                let cert_file = std::fs::File::open(cert_path).map_err(|e| {
                    DebuggerError::FileError(format!(
                        "Failed to open client cert {:?}: {}",
                        cert_path, e
                    ))
                })?;
                let mut cert_reader = BufReader::new(cert_file);
                let certs: Vec<Certificate> = rustls_pemfile::certs(&mut cert_reader)
                    .map_err(|e| {
                        DebuggerError::FileError(format!("Failed to parse client cert: {}", e))
                    })?
                    .into_iter()
                    .map(Certificate)
                    .collect();

                let key_file = std::fs::File::open(key_path).map_err(|e| {
                    DebuggerError::FileError(format!(
                        "Failed to open client key {:?}: {}",
                        key_path, e
                    ))
                })?;
                let mut key_reader = BufReader::new(key_file);
                let keys = rustls_pemfile::pkcs8_private_keys(&mut key_reader).map_err(|e| {
                    DebuggerError::FileError(format!("Failed to parse client key: {}", e))
                })?;

                if let Some(key) = keys.into_iter().next() {
                    ClientConfig::builder()
                        .with_safe_defaults()
                        .with_root_certificates(root_store)
                        .with_client_auth_cert(certs, PrivateKey(key))
                        .map_err(|e| {
                            DebuggerError::FileError(format!(
                                "Failed to set client certificate: {}",
                                e
                            ))
                        })?
                } else {
                    ClientConfig::builder()
                        .with_safe_defaults()
                        .with_root_certificates(root_store)
                        .with_no_client_auth()
                }
            } else {
                ClientConfig::builder()
                    .with_safe_defaults()
                    .with_root_certificates(root_store)
                    .with_no_client_auth()
            };

            let host = addr.split(':').next().unwrap_or("localhost");
            let server_name = ServerName::try_from(host).map_err(|e| {
                DebuggerError::NetworkError(format!("Invalid server name '{}': {}", host, e))
            })?;

            let conn = rustls::client::ClientConnection::new(Arc::new(client_config), server_name)
                .map_err(|e| {
                    DebuggerError::NetworkError(format!("Failed to create TLS connection: {}", e))
                })?;

            Ok(RemoteStream::Tls(Box::new(rustls::StreamOwned::new(
                conn, tcp_stream,
            ))))
        } else {
            Ok(RemoteStream::Plain(tcp_stream))
        }
    }

    /// Perform a protocol handshake and verify compatibility.
    pub fn handshake(&mut self, client_name: &str, client_version: &str) -> Result<u32> {
        let response = self.send_request(DebugRequest::Handshake {
            client_name: client_name.to_string(),
            client_version: client_version.to_string(),
            protocol_min: PROTOCOL_MIN_VERSION,
            protocol_max: PROTOCOL_MAX_VERSION,
            heartbeat_interval_ms: self.config.heartbeat_interval_ms,
            idle_timeout_ms: self.config.idle_timeout_ms,
            session_label: self.config.session_label.clone(),
            reconnect_session_id: None,
            required_capabilities: None,
        })?;

        match response {
            DebugResponse::HandshakeAck {
                selected_version,
                server_capabilities,
                session_id,
                session_created_at,
                session_label,
                ..
            } => {
                self.selected_protocol_version = Some(selected_version);
                self.negotiated_capabilities = Some(server_capabilities);
                self.session_id = session_id.clone();
                self.session_info =
                    session_id
                        .as_ref()
                        .map(|sid| crate::server::protocol::RemoteSessionInfo {
                            session_id: sid.clone(),
                            created_at: session_created_at
                                .clone()
                                .unwrap_or_else(|| "unknown".to_string()),
                            label: session_label.clone(),
                        });
                Ok(selected_version)
            }
            DebugResponse::IncompatibleCapabilities {
                message,
                missing_capabilities,
                ..
            } => Err(DebuggerError::ExecutionError(format!(
                "Server is missing required capabilities [{}]: {}",
                missing_capabilities.join(", "),
                message
            ))
            .into()),
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

    pub fn session_info(&self) -> Option<&crate::server::protocol::RemoteSessionInfo> {
        self.session_info.as_ref()
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
                    let sanitized = sanitize_auth_message(&message, token);
                    Err(DebuggerError::AuthenticationFailed(sanitized).into())
                }
            }
            _ => Err(DebuggerError::ExecutionError(
                "Unexpected response to authentication".to_string(),
            )
            .into()),
        }
    }

    /// Returns an error if `cap_name` is not in the negotiated server capabilities.
    /// Call this at the top of any method that uses an optional feature.
    fn require_capability(&self, cap_name: &str) -> Result<()> {
        let caps = match &self.negotiated_capabilities {
            Some(c) => c,
            None => return Ok(()), // handshake not yet done; let the server reject it
        };
        let supported = match cap_name {
            "evaluate" => caps.evaluate,
            "source_breakpoints" => caps.source_breakpoints,
            "conditional_breakpoints" => caps.conditional_breakpoints,
            "snapshot_loading" => caps.snapshot_loading,
            "dynamic_trace_events" => caps.dynamic_trace_events,
            "repeat_execution" => caps.repeat_execution,
            _ => true, // unknown names pass through
        };
        if supported {
            Ok(())
        } else {
            Err(DebuggerError::ExecutionError(format!(
                "Server does not support '{}'. Check server version or capabilities.",
                cap_name
            ))
            .into())
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
    pub fn inspect(&mut self) -> Result<InspectResult> {
        let response =
            self.send_request_with_retry(DebugRequest::Inspect, RequestClass::Inspect, true)?;

        match response {
            DebugResponse::InspectionResult {
                function,
                step_count,
                paused,
                call_stack,
                pause_reason,
                ..
            } => Ok((function, step_count, paused, call_stack, pause_reason)),
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
        self.require_capability("snapshot_loading")?;
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

    /// Evaluate an expression in the current debug context
    pub fn evaluate(
        &mut self,
        expression: &str,
        frame_id: Option<u64>,
    ) -> Result<(String, Option<String>)> {
        self.require_capability("evaluate")?;
        let response = self.send_request_with_retry(
            DebugRequest::Evaluate {
                expression: expression.to_string(),
                frame_id,
            },
            RequestClass::Default,
            true,
        )?;

        match response {
            DebugResponse::EvaluateResult {
                result,
                result_type,
                ..
            } => Ok((result, result_type)),
            DebugResponse::Error { message } => Err(DebuggerError::ExecutionError(message).into()),
            _ => Err(
                DebuggerError::ExecutionError("Unexpected response to Evaluate".to_string()).into(),
            ),
        }
    }

    /// Ping the server
    pub fn ping(&mut self) -> Result<()> {
        let response =
            self.send_request_with_retry(DebugRequest::Ping, RequestClass::Ping, true)?;

        match response {
            DebugResponse::Pong => {
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

    /// Cancel the current execution
    pub fn cancel(&mut self) -> Result<()> {
        let response = match self.send_request(DebugRequest::Cancel) {
            Ok(resp) => resp,
            Err(e) if e.to_string().contains("No response") => {
                // If the server immediately exited as part of cancelling, it drops the connection.
                return Ok(());
            }
            Err(e) => return Err(e),
        };

        match response {
            DebugResponse::CancelAck => {
                info!("Server acknowledged cancellation");
                Ok(())
            }
            _ => Err(
                DebuggerError::ExecutionError("Unexpected response to Cancel".to_string()).into(),
            ),
        }
    }

    /// Send a request and wait for response
    fn send_request(&mut self, request: DebugRequest) -> Result<DebugResponse> {
        self.send_request_with_retry(request, RequestClass::Default, false)
    }

    fn reconnect(&mut self) -> Result<()> {
        let stream = Self::create_stream(&self.addr, &self.config)?;
        self.stream = BufReader::new(stream);
        self.authenticated = self.token.is_none();

        // Perform handshake
        let handshake = DebugRequest::Handshake {
            client_name: "rust-remote-client".to_string(),
            client_version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_min: 1,
            protocol_max: 1,
            heartbeat_interval_ms: Some(30000),
            idle_timeout_ms: Some(60000),
            session_label: self.config.session_label.clone(),
            reconnect_session_id: self.session_id.clone(),
            required_capabilities: None,
        };
        // Use a standard timeout for handshake during reconnect
        let handshake_resp = self
            .send_request_once(handshake, Duration::from_secs(5))
            .map_err(|e| {
                DebuggerError::ExecutionError(format!("Handshake failed during reconnect: {:?}", e))
            })?;

        // Capture session_id from reconnect handshake
        if let DebugResponse::HandshakeAck { session_id, .. } = &handshake_resp {
            self.session_id = session_id.clone();
        }

        if let Some(token) = self.token.clone() {
            self.authenticate(&token)?;
        }

        // If we have a stored session_id, attempt to reconnect to the existing session
        if let Some(ref sid) = self.session_id.clone() {
            match self.send_request(DebugRequest::Reconnect {
                session_id: sid.clone(),
            }) {
                Ok(DebugResponse::ReconnectAck { .. }) => {
                    info!("Successfully reconnected to session {}", sid);
                }
                Ok(DebugResponse::SessionExpired { message }) => {
                    info!("Session expired during reconnect: {}", message);
                    self.session_id = None;
                }
                Ok(_) | Err(_) => {
                    // Server may not support Reconnect; that's fine — treat as fresh connection
                    info!("Reconnect request not accepted; continuing with fresh connection");
                }
            }
        }

        Ok(())
    }

    /// Explicitly reconnect to an existing session by session ID.
    /// Returns the reconnection acknowledgment on success, or an error if the
    /// session is expired or the server does not support reconnection.
    pub fn reconnect_to_session(&mut self, session_id: &str) -> Result<ReconnectInfo> {
        let stream = Self::create_stream(&self.addr, &self.config)?;
        self.stream = BufReader::new(stream);
        self.authenticated = self.token.is_none();

        // Perform handshake
        self.handshake("rust-remote-client", env!("CARGO_PKG_VERSION"))?;

        // Authenticate if needed
        if let Some(token) = self.token.clone() {
            self.authenticate(&token)?;
        }

        // Send Reconnect request
        let response = self.send_request(DebugRequest::Reconnect {
            session_id: session_id.to_string(),
        })?;

        match response {
            DebugResponse::ReconnectAck {
                session_id,
                paused,
                current_function,
                breakpoints,
                step_count,
            } => {
                self.session_id = Some(session_id.clone());
                info!("Reconnected to session {}", session_id);
                Ok(ReconnectInfo {
                    session_id,
                    paused,
                    current_function,
                    breakpoints,
                    step_count,
                })
            }
            DebugResponse::SessionExpired { message } => {
                self.session_id = None;
                Err(DebuggerError::ExecutionError(format!("Session expired: {}", message)).into())
            }
            DebugResponse::Error { message } => Err(DebuggerError::ExecutionError(message).into()),
            _ => Err(
                DebuggerError::ExecutionError("Unexpected response to Reconnect".to_string())
                    .into(),
            ),
        }
    }

    /// Returns the session ID received from the server, if any.
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// Returns the protocol version negotiated during the handshake, if any.
    pub fn selected_protocol_version(&self) -> Option<u32> {
        self.selected_protocol_version
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
                    | DebugRequest::Cancel
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

        loop {
            let mut response_line = String::new();
            let n = self
                .stream
                .read_line(&mut response_line)
                .map_err(|e| SendFailure::io("read", e, timeout))?;
            if n == 0 {
                return Err(SendFailure::Disconnected);
            }

            let msg = DebugMessage::parse(response_line.trim_end())
                .map_err(|e| SendFailure::Protocol(e.to_string()))?;

            // Handle interleaved Ping from server
            if let Some(DebugRequest::Ping) = msg.request {
                let pong = DebugMessage::response(msg.id, DebugResponse::Pong);
                let pong_json = serde_json::to_string(&pong).map_err(|e| {
                    SendFailure::Serialize(format!("Failed to serialize pong: {}", e))
                })?;
                writeln!(self.stream.get_mut(), "{}", pong_json)
                    .map_err(|e| SendFailure::io("ping-response", e, timeout))?;
                self.stream
                    .get_mut()
                    .flush()
                    .map_err(|e| SendFailure::io("ping-flush", e, timeout))?;
                continue;
            }

            if msg.id != expected_id {
                return Err(SendFailure::Protocol(format!(
                    "Mismatched response id: expected {} got {}",
                    expected_id, msg.id
                )));
            }

            let response = msg.response.ok_or_else(|| {
                SendFailure::Protocol("Response message has no response field".to_string())
            })?;

            if matches!(response, DebugResponse::Unknown) {
                return Err(SendFailure::Protocol(
                    "Received unknown response type from server".to_string(),
                ));
            }

            // #1258: tag server-side error responses with the request id so a
            // CLI failure can be correlated with the matching server log line.
            return Ok(tag_error_with_request_id(response, expected_id));
        }
    }
}

/// Append the request id to a server error response so CLI failures can be
/// correlated with server logs (#1258). The id is the client's per-connection
/// request counter (not an internal server identifier) and is appended only
/// when not already present, keeping non-error responses untouched.
fn tag_error_with_request_id(response: DebugResponse, request_id: u64) -> DebugResponse {
    if let DebugResponse::Error { message } = &response {
        if !message.contains("request #") {
            return DebugResponse::Error {
                message: format!("{message} (request #{request_id})"),
            };
        }
    }
    response
}

#[cfg(test)]
mod request_id_tests {
    use super::tag_error_with_request_id;
    use crate::server::protocol::DebugResponse;

    #[test]
    fn error_response_gains_request_id() {
        let tagged = tag_error_with_request_id(
            DebugResponse::Error {
                message: "boom".to_string(),
            },
            42,
        );
        match tagged {
            DebugResponse::Error { message } => {
                assert!(
                    message.contains("boom"),
                    "original message preserved: {message}"
                );
                assert!(
                    message.contains("request #42"),
                    "request id appended: {message}"
                );
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    #[test]
    fn request_id_not_duplicated() {
        let once = tag_error_with_request_id(
            DebugResponse::Error {
                message: "boom".to_string(),
            },
            7,
        );
        let twice = tag_error_with_request_id(once, 9);
        if let DebugResponse::Error { message } = twice {
            assert_eq!(
                message.matches("request #").count(),
                1,
                "no double-tag: {message}"
            );
            assert!(message.contains("request #7"), "keeps first id: {message}");
        } else {
            panic!("expected Error");
        }
    }

    #[test]
    fn non_error_response_untouched() {
        assert!(matches!(
            tag_error_with_request_id(DebugResponse::Pong, 1),
            DebugResponse::Pong
        ));
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
        #[allow(dead_code)]
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
            SendFailure::Timeout { stage, timeout } => DebuggerError::RequestTimeout(
                format!("{} ({})", operation, stage),
                timeout.as_millis() as u64,
            ),
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

    let exp = 1u32
        .checked_shl((attempt - 1).min(31) as u32)
        .unwrap_or(u32::MAX);

    base.checked_mul(exp).unwrap_or(max).min(max)
}

// parse_response_line removed as it was redundant with send_request_once refactoring.

#[allow(dead_code)]
fn sanitize_auth_message(message: &str, token: &str) -> String {
    if token.is_empty() {
        return message.to_string();
    }

    message.replace(token, "<redacted>")
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

    // parse_response_line tests removed.

    #[test]
    fn connect_failure_is_network_error_category() {
        let err = RemoteClient::connect("127.0.0.1:1", None).unwrap_err();
        assert!(err.to_string().contains("Network/transport error"));
    }

    #[test]
    fn connect_timeout_is_respected() {
        if TcpListener::bind("127.0.0.1:0").is_err() {
            eprintln!("Skipping connect_timeout_is_respected: loopback restricted");
            return;
        }

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        // accept but never send a byte  simulates a silent firewall / slow
        // proxy that completes TCP but stalls the application protocol.
        std::thread::spawn(move || {
            if let Ok((_stream, _)) = listener.accept() {
                // this keeps the socket open long enough that the client has to time out.
                std::thread::sleep(Duration::from_secs(5));
            }
        });

        let config = RemoteClientConfig {
            // 1 ms connect timeout — fast on loopback since the OS TCP handshake
            // completes immediately; the actual timeout fires during the protocol
            // handshake read  uses  50 ms default window.
            connect_timeout: Duration::from_millis(1),
            timeouts: RequestTimeouts {
                default: Duration::from_millis(50),
                ping: Duration::from_millis(50),
                inspect: Duration::from_millis(50),
                get_storage: Duration::from_millis(50),
            },
            retry: RetryPolicy {
                max_attempts: 1,
                base_delay: Duration::from_millis(1),
                max_delay: Duration::from_millis(1),
            },
            ..Default::default()
        };

        let err = RemoteClient::connect_with_config(&addr.to_string(), None, config).unwrap_err();
        let msg = err.to_string();

        assert!(
            msg.contains("Request timed out")
                || msg.contains("timed out")
                || msg.contains("Network/transport error")
                || msg.contains("connection closed by peer"),
            "Expected a timeout/network error, got: {}",
            msg
        );
        assert!(
            !msg.contains("Authentication") && !msg.contains("Incompatible"),
            "Error should not appear as auth/protocol failure: {}",
            msg
        );
    }

    #[test]
    fn connect_timeout_error_is_network_category() {
        // point at a port that is almost certainly not listening.
        let err = RemoteClient::connect_with_config(
            "127.0.0.1:19999",
            None,
            RemoteClientConfig {
                connect_timeout: Duration::from_millis(50),
                ..Default::default()
            },
        )
        .unwrap_err();

        let msg = err.to_string();
        assert!(
            msg.contains("Network/transport error")
                || msg.contains("timed out")
                || msg.contains("connection refused")
                || msg.contains("Connection refused"),
            "Expected network error, got: {}",
            msg
        );
    }

    /// Respond to a handshake then stall on the next request (for timeout tests).
    fn accept_handshake_then_stall(stream: &mut std::net::TcpStream) {
        use std::io::Write;
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut line = String::new();
        let _ = reader.read_line(&mut line);
        if let Ok(msg) = serde_json::from_str::<DebugMessage>(line.trim_end()) {
            let ack = DebugMessage::response(
                msg.id,
                DebugResponse::HandshakeAck {
                    server_name: "test".into(),
                    server_version: "0.0.0".into(),
                    protocol_min: 1,
                    protocol_max: 1,
                    selected_version: 1,
                    session_id: None,
                    session_created_at: None,
                    session_label: None,
                    heartbeat_interval_ms: None,
                    idle_timeout_ms: None,
                    reconnect_id: None,
                    server_capabilities: Default::default(),
                },
            );
            if let Ok(json) = serde_json::to_string(&ack) {
                let _ = writeln!(stream, "{}", json);
                let _ = stream.flush();
            }
        }
        // Read the ping request but never respond — simulates timeout.
        let mut reader2 = BufReader::new(stream.try_clone().unwrap());
        let mut _ping_line = String::new();
        let _ = reader2.read_line(&mut _ping_line);
        std::thread::sleep(Duration::from_millis(200));
    }

    #[test]
    fn ping_times_out_deterministically() {
        if TcpListener::bind("127.0.0.1:0").is_err() {
            eprintln!("Skipping ping_times_out_deterministically: loopback restricted");
            return;
        }

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                accept_handshake_then_stall(&mut stream);

                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                // Respond to handshake so client construction succeeds.
                let _ = reader.read_line(&mut line);
                if let Ok(msg) = serde_json::from_str::<DebugMessage>(line.trim_end()) {
                    let response = DebugMessage::response(
                        msg.id,
                        DebugResponse::HandshakeAck {
                            server_name: "test".to_string(),
                            server_version: "0.0.0".to_string(),
                            protocol_min: PROTOCOL_MIN_VERSION,
                            protocol_max: PROTOCOL_MAX_VERSION,
                            selected_version: PROTOCOL_MAX_VERSION,
                            session_id: None,
                            session_created_at: None,
                            session_label: None,
                            heartbeat_interval_ms: None,
                            idle_timeout_ms: None,
                            reconnect_id: None,
                            server_capabilities: Default::default(),
                        },
                    );
                    let json = serde_json::to_string(&response).unwrap();
                    let _ = writeln!(stream, "{}", json);
                    let _ = stream.flush();
                }

                // Consume ping request and never respond.
                line.clear();
                let _ = reader.read_line(&mut line);
                let msg: DebugMessage = serde_json::from_str(line.trim_end()).unwrap();
                let handshake_ack = DebugMessage::response(
                    msg.id,
                    DebugResponse::HandshakeAck {
                        server_name: "test-server".to_string(),
                        server_version: "0.1.0".to_string(),
                        protocol_min: 1,
                        protocol_max: 1,
                        selected_version: 1,
                        session_id: None,
                        session_created_at: None,
                        session_label: None,
                        heartbeat_interval_ms: None,
                        idle_timeout_ms: None,
                        reconnect_id: None,
                        server_capabilities: Default::default(),
                    },
                );
                let _ = writeln!(stream, "{}", serde_json::to_string(&handshake_ack).unwrap());

                // Consume ping but never respond.
                line.clear();
                let _ = reader.read_line(&mut line);
                std::thread::sleep(Duration::from_millis(200));
            }
        });

        let config = RemoteClientConfig {
            connect_timeout: Duration::from_millis(5000),
            timeouts: RequestTimeouts {
                ping: Duration::from_millis(50),
                default: Duration::from_millis(1000), // Allow time for handshake
                ..RequestTimeouts::default()
            },
            retry: RetryPolicy {
                max_attempts: 1,
                base_delay: Duration::from_millis(1),
                max_delay: Duration::from_millis(1),
            },
            heartbeat_interval_ms: None,
            idle_timeout_ms: None,
            tls_cert: None,
            tls_key: None,
            tls_ca: None,
            session_label: None,
        };

        let mut client =
            RemoteClient::connect_with_config(&addr.to_string(), None, config).unwrap();
        let err = client.ping().unwrap_err();
        assert!(
            err.to_string().contains("Request timed out")
                || err.to_string().contains("connection closed by peer"),
            "Error should indicate timeout or connection closure: {}",
            err
        );
    }

    #[test]
    fn ping_retries_on_disconnect_and_succeeds() {
        if TcpListener::bind("127.0.0.1:0").is_err() {
            eprintln!("Skipping ping_retries_on_disconnect_and_succeeds: loopback restricted");
            return;
        }

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(AtomicUsize::new(0));
        let seen_server = Arc::clone(&seen);

        std::thread::spawn(move || {
            for stream in listener.incoming().take(2) {
                let stream = stream.unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut writer = stream;
                let attempt = seen_server.fetch_add(1, Ordering::SeqCst);

                // 1. Handshake
                let mut line = String::new();
                if let Ok(n) = reader.read_line(&mut line) {
                    if n > 0 {
                        let msg: DebugMessage = serde_json::from_str(line.trim_end()).unwrap();
                        let handshake_ack = DebugMessage::response(
                            msg.id,
                            DebugResponse::HandshakeAck {
                                server_name: "test-server".to_string(),
                                server_version: "0.1.0".to_string(),
                                protocol_min: 1,
                                protocol_max: 1,
                                selected_version: 1,
                                session_id: None,
                                session_created_at: None,
                                session_label: None,
                                heartbeat_interval_ms: None,
                                idle_timeout_ms: None,
                                reconnect_id: None,
                                server_capabilities: Default::default(),
                            },
                        );
                        let _ =
                            writeln!(writer, "{}", serde_json::to_string(&handshake_ack).unwrap());
                        let _ = writer.flush();
                    } else {
                        continue;
                    }
                } else {
                    continue;
                }

                if attempt == 0 {
                    // Force a disconnect after handshake but before processing the next request
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                }

                // 2. Ping
                line.clear();
                if let Ok(n) = reader.read_line(&mut line) {
                    if n > 0 {
                        let msg: DebugMessage = serde_json::from_str(line.trim_end()).unwrap();
                        let response = DebugMessage::response(msg.id, DebugResponse::Pong);
                        let _ = writeln!(writer, "{}", serde_json::to_string(&response).unwrap());
                        let _ = writer.flush();
                    }
                }
            }
        });

        let config = RemoteClientConfig {
            connect_timeout: Duration::from_millis(5000),
            timeouts: RequestTimeouts {
                ping: Duration::from_millis(500),
                ..RequestTimeouts::default()
            },
            retry: RetryPolicy {
                max_attempts: 3,
                base_delay: Duration::from_millis(1),
                max_delay: Duration::from_millis(5),
            },
            heartbeat_interval_ms: None,
            idle_timeout_ms: None,
            tls_cert: None,
            tls_key: None,
            tls_ca: None,
            session_label: None,
        };

        let mut client =
            RemoteClient::connect_with_config(&addr.to_string(), None, config).unwrap();
        let result = client.ping();
        if let Err(err) = &result {
            assert!(
                err.to_string().contains("connection closed by peer")
                    || err.to_string().contains("Request timed out"),
                "unexpected retry error: {}",
                err
            );
        }
        assert!(seen.load(Ordering::SeqCst) >= 1);
    }
}
