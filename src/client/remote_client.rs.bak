use crate::protocol::{DebugRequest, DebugResponse};
use crate::Result;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::rustls::{ClientConfig, RootCertStore, ServerName};
use tokio_rustls::TlsConnector;
use tracing::info;

pub struct RemoteClient {
    stream: Box<dyn AsyncReadWrite + Unpin + Send>,
}

trait AsyncReadWrite: tokio::io::AsyncRead + tokio::io::AsyncWrite {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite> AsyncReadWrite for T {}

impl RemoteClient {
    pub async fn connect(addr: &str, token: String, use_tls: bool) -> Result<Self> {
        info!("Connecting to remote debugger at {}", addr);
        let stream = TcpStream::connect(addr).await?;

        let stream: Box<dyn AsyncReadWrite + Unpin + Send> = if use_tls {
            let root_cert_store = RootCertStore::empty();
            // In a real scenario, we might want to load system certs or a specific CA.
            // For now, let's assume the user might want a way to skip verification or provide a cert.
            let config = ClientConfig::builder()
                .with_safe_defaults()
                .with_root_certificates(root_cert_store)
                .with_no_client_auth();
            
            let connector = TlsConnector::from(Arc::new(config));
            let domain = ServerName::try_from("localhost")?; // Should be parsed from addr
            let tls_stream = connector.connect(domain, stream).await?;
            Box::new(tls_stream)
        } else {
            Box::new(stream)
        };

        let mut client = Self { stream };

        // Perform handshake
        let handshake = DebugRequest::Handshake { token };
        let json = serde_json::to_vec(&handshake)?;
        client.stream.write_all(&json).await?;

        let mut buffer = vec![0u8; 8192];
        let n = client.stream.read(&mut buffer).await?;
        let response: DebugResponse = serde_json::from_slice(&buffer[..n])?;

        match response {
            DebugResponse::AuthSuccess => {
                info!("Authentication successful");
                Ok(client)
            }
            DebugResponse::AuthFailed => Err(anyhow::anyhow!("Authentication failed")),
            _ => Err(anyhow::anyhow!("Unexpected response during handshake")),
        }
    }

    pub async fn send_request(&mut self, request: DebugRequest) -> Result<DebugResponse> {
        let json = serde_json::to_vec(&request)?;
        self.stream.write_all(&json).await?;

        let mut buffer = vec![0u8; 8192];
        let n = self.stream.read(&mut buffer).await?;
        let response: DebugResponse = serde_json::from_slice(&buffer[..n])?;
        Ok(response)
use crate::server::protocol::{DebugMessage, DebugRequest, DebugResponse};
use crate::{DebuggerError, Result};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use tracing::info;

/// Remote client for connecting to a debug server
pub struct RemoteClient {
    stream: TcpStream,
    message_id: u64,
    authenticated: bool,
}

impl RemoteClient {
    /// Connect to a remote debug server
    pub fn connect(addr: &str, token: Option<String>) -> Result<Self> {
        info!("Connecting to debug server at {}", addr);
        let stream = TcpStream::connect(addr).map_err(|e| {
            DebuggerError::FileError(format!("Failed to connect to {}: {}", addr, e))
        })?;

        let mut client = Self {
            stream,
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

    /// Step execution
    pub fn step(&mut self) -> Result<(bool, Option<String>, u64)> {
        let response = self.send_request(DebugRequest::Step)?;

        match response {
            DebugResponse::StepResult {
                paused,
                current_function,
                step_count,
            } => Ok((paused, current_function, step_count)),
            DebugResponse::Error { message } => Err(DebuggerError::ExecutionError(message).into()),
            _ => {
                Err(DebuggerError::ExecutionError("Unexpected response to Step".to_string()).into())
            }
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
        let message = DebugMessage::request(self.message_id, request);

        let request_json = serde_json::to_string(&message)
            .map_err(|e| DebuggerError::FileError(format!("Failed to serialize request: {}", e)))?;

        // Send request
        writeln!(self.stream, "{}", request_json)
            .map_err(|e| DebuggerError::FileError(format!("Failed to write to stream: {}", e)))?;
        self.stream
            .flush()
            .map_err(|e| DebuggerError::FileError(format!("Failed to flush stream: {}", e)))?;

        // Read response
        let reader = BufReader::new(&self.stream);
        let mut lines = reader.lines();
        let response_line = lines
            .next()
            .ok_or_else(|| DebuggerError::FileError("No response from server".to_string()))?
            .map_err(|e| DebuggerError::FileError(format!("Failed to read response: {}", e)))?;

        let response_message: DebugMessage = serde_json::from_str(&response_line)
            .map_err(|e| DebuggerError::FileError(format!("Failed to parse response: {}", e)))?;

        response_message.response.ok_or_else(|| {
            DebuggerError::FileError("Response message has no response field".to_string()).into()
        })
    }
}

impl Drop for RemoteClient {
    fn drop(&mut self) {
        let _ = self.disconnect();
    }
}
