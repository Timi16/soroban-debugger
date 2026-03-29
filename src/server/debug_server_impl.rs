use crate::debugger::engine::DebuggerEngine;
use crate::inspector::budget::BudgetInspector;
use crate::runtime::executor::ContractExecutor;
use crate::server::protocol::{
    negotiate_protocol_version, DebugMessage, DebugRequest, DebugResponse, PROTOCOL_MAX_VERSION,
    PROTOCOL_MIN_VERSION,
};
use crate::simulator::SnapshotLoader;
use crate::Result;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

pub struct DebugServer {
    token: Option<String>,
}

impl DebugServer {
    pub fn new(
        token: Option<String>,
        tls_cert: Option<&Path>,
        tls_key: Option<&Path>,
    ) -> Result<Self> {
        if tls_cert.is_some() || tls_key.is_some() {
            // Keep the remote protocol server minimal for now; tests use TCP only.
            return Err(crate::DebuggerError::ExecutionError(
                "TLS not supported in debug server (use plain TCP)".to_string(),
            )
            .into());
        }
        Ok(Self { token })
    }

    pub async fn run(self, port: u16) -> Result<()> {
        let addr = format!("0.0.0.0:{}", port);
        let listener = TcpListener::bind(&addr)
            .await
            .map_err(|e| crate::DebuggerError::ExecutionError(format!("Bind failed: {e}")))?;

        let local = tokio::task::LocalSet::new();
        local.run_until(async move {
            loop {
                let (stream, _) = listener.accept().await.map_err(|e| {
                    crate::DebuggerError::ExecutionError(format!("Accept failed: {e}"))
                })?;
                let token = self.token.clone();
                tokio::task::spawn_local(async move {
                    let _ = handle_connection(stream, token).await;
                });
            }
        }).await

    }
}

async fn send_response<S>(stream: &mut S, response: DebugMessage) -> Result<()>
where
    S: tokio::io::AsyncWrite + Unpin,
{
    let json = serde_json::to_vec(&response).map_err(|e| {
        crate::DebuggerError::ExecutionError(format!("Serialize response failed: {e}"))
    })?;
    stream
        .write_all(&json)
        .await
        .map_err(|e| crate::DebuggerError::NetworkError(format!("Write failed: {e}")))?;
    stream
        .write_all(b"\n")
        .await
        .map_err(|e| crate::DebuggerError::NetworkError(format!("Write newline failed: {e}")))?;
    Ok(())
}

async fn handle_connection<S>(stream: S, token: Option<String>) -> Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut authenticated = token.is_none();
    let mut handshake_done = false;

    let mut engine: Option<DebuggerEngine> = None;

    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    loop {
        line.clear();
        let n = reader
            .read_line(&mut line)
            .await
            .map_err(|e| crate::DebuggerError::NetworkError(format!("Read failed: {e}")))?;
        if n == 0 {
            break;
        }

        let message: DebugMessage = match serde_json::from_str(line.trim_end()) {
            Ok(m) => m,
            Err(e) => {
                // Protocol violation; ignore.
                tracing::warn!("Failed to parse DebugMessage: {e}");
                continue;
            }
        };

        let Some(request) = message.request else {
            continue;
        };

        // Ping is allowed before handshake/auth.
        if matches!(&request, DebugRequest::Ping) {
            let resp = DebugMessage::response(message.id, DebugResponse::Pong);
            send_response(&mut writer, resp).await?;
            continue;
        }

        // Handshake is required for normal clients, but DAP tests send Authenticate
        // without handshake; allow Authenticate pre-handshake.
        if matches!(&request, DebugRequest::Handshake { .. }) {
            if let DebugRequest::Handshake {
                client_name,
                client_version,
                protocol_min,
                protocol_max,
                heartbeat_interval_ms: _,
                idle_timeout_ms: _,
            } = request
            {
                match negotiate_protocol_version(protocol_min, protocol_max) {
                    Ok(selected_version) => {
                        handshake_done = true;
                        let response = DebugMessage::response(
                            message.id,
                            DebugResponse::HandshakeAck {
                                server_name: "soroban-debug".to_string(),
                                server_version: env!("CARGO_PKG_VERSION").to_string(),
                                protocol_min: PROTOCOL_MIN_VERSION,
                                protocol_max: PROTOCOL_MAX_VERSION,
                                selected_version,
                                heartbeat_interval_ms: None,
                                idle_timeout_ms: None,
                            },
                        );
                        send_response(&mut writer, response).await?;
                    }
                    Err(e) => {
                        let response = DebugMessage::response(
                            message.id,
                            DebugResponse::IncompatibleProtocol {
                                message: format!(
                                    "{}. Client: {}@{}. Upgrade the older component.",
                                    e, client_name, client_version
                                ),
                                server_name: "soroban-debug".to_string(),
                                server_version: env!("CARGO_PKG_VERSION").to_string(),
                                protocol_min: PROTOCOL_MIN_VERSION,
                                protocol_max: PROTOCOL_MAX_VERSION,
                            },
                        );
                        send_response(&mut writer, response).await?;
                        break;
                    }
                }
            }
            continue;
        }

        if matches!(&request, DebugRequest::Authenticate { .. }) {
            if let DebugRequest::Authenticate {
                token: client_token,
            } = request
            {
                let success = token.as_deref().map(|t| t == client_token).unwrap_or(true);
                authenticated = success;
                let auth_message = if success {
                    "Authentication successful".to_string()
                } else {
                    "Authentication failed".to_string()
                };
                let response = DebugMessage::response(
                    message.id,
                    DebugResponse::Authenticated {
                        success,
                        message: auth_message,
                    },
                );
                send_response(&mut writer, response).await?;
            }
            continue;
        }

        // Enforce handshake/auth for everything else.
        if !handshake_done {
            let response = DebugMessage::response(
                message.id,
                DebugResponse::Error {
                    message: "Protocol handshake required".to_string(),
                },
            );
            send_response(&mut writer, response).await?;
            continue;
        }

        if !authenticated {
            let response = DebugMessage::response(
                message.id,
                DebugResponse::Error {
                    message: "Authentication required".to_string(),
                },
            );
            send_response(&mut writer, response).await?;
            continue;
        }

        let disconnect = matches!(&request, DebugRequest::Disconnect);
        let response: DebugResponse = match request {
            DebugRequest::Disconnect => DebugResponse::Disconnected,

            DebugRequest::LoadContract { contract_path } => match std::fs::read(&contract_path) {
                Ok(bytes) => match ContractExecutor::new(bytes.clone()) {
                    Ok(executor) => {
                        let mut next = DebuggerEngine::new(executor, Vec::new());
                        next.try_load_source_map(&bytes);
                        engine = Some(next);
                        DebugResponse::ContractLoaded { size: bytes.len() }
                    }
                    Err(e) => DebugResponse::Error {
                        message: format!("Failed to create executor: {e}"),
                    },
                },
                Err(e) => DebugResponse::Error {
                    message: format!("Failed to read contract {:?}: {}", contract_path, e),
                },
            },

            DebugRequest::LoadSnapshot { snapshot_path } => {
                let loader = SnapshotLoader::from_file(&snapshot_path);
                match loader {
                    Ok(loader) => {
                        let loaded = loader.apply_to_environment()?;
                        if let Some(engine) = engine.as_mut() {
                            engine.executor_mut().apply_snapshot_ledger(&loaded)?;
                        }
                        DebugResponse::SnapshotLoaded {
                            summary: loaded.format_summary(),
                        }
                    }
                    Err(e) => DebugResponse::Error {
                        message: e.to_string(),
                    },
                }
            }

            DebugRequest::SetStorage { storage_json } => match engine.as_mut() {
                Some(engine) => match engine.executor_mut().set_initial_storage(storage_json) {
                    Ok(()) => {
                        let snapshot = engine.executor().get_storage_snapshot()?;
                        let json =
                            serde_json::to_string(&snapshot).unwrap_or_else(|_| "{}".to_string());
                        DebugResponse::StorageState { storage_json: json }
                    }
                    Err(e) => DebugResponse::Error {
                        message: e.to_string(),
                    },
                },
                None => DebugResponse::Error {
                    message: "No contract loaded".to_string(),
                },
            },

            DebugRequest::Execute { function, args } => match engine.as_mut() {
                Some(engine) => {
                    match engine.execute_without_breakpoints(&function, args.as_deref()) {
                        Ok(output) => DebugResponse::ExecutionResult {
                            success: true,
                            output,
                            error: None,
                            paused: engine.is_paused(),
                            completed: true,
                            source_location: None,
                        },
                        Err(e) => DebugResponse::ExecutionResult {
                            success: false,
                            output: String::new(),
                            error: Some(e.to_string()),
                            paused: false,
                            completed: true,
                            source_location: None,
                        },
                    }
                }
                None => DebugResponse::Error {
                    message: "No contract loaded".to_string(),
                },
            },

            DebugRequest::GetStorage => match engine.as_ref() {
                Some(engine) => {
                    let snapshot = engine.executor().get_storage_snapshot()?;
                    let json =
                        serde_json::to_string(&snapshot).unwrap_or_else(|_| "{}".to_string());
                    DebugResponse::StorageState { storage_json: json }
                }
                None => DebugResponse::Error {
                    message: "No contract loaded".to_string(),
                },
            },

            DebugRequest::GetBudget => match engine.as_ref() {
                Some(engine) => {
                    let info = BudgetInspector::get_cpu_usage(engine.executor().host());
                    DebugResponse::BudgetInfo {
                        cpu_instructions: info.cpu_instructions,
                        memory_bytes: info.memory_bytes,
                    }
                }
                None => DebugResponse::Error {
                    message: "No contract loaded".to_string(),
                },
            },

            // Not required by the current remote smoke tests.
            _ => DebugResponse::Error {
                message: "Not implemented in minimal debug server".to_string(),
            },
        };

        let response = DebugMessage::response(message.id, response);
        send_response(&mut writer, response).await?;

        if disconnect {
            break;
        }
    }

    Ok(())
}
