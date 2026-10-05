use crate::debugger::breakpoint::{BreakpointManager, BreakpointSpec};
use crate::debugger::engine::{DebuggerEngine, StepOverResult};
use crate::history::HistoryManager;
use crate::history::ReconnectionLog;
use crate::inspector::budget::BudgetInspector;
use crate::inspector::events::{ContractEvent, EventInspector};
use crate::server::protocol::{
    negotiate_protocol_version, PROTOCOL_MAX_VERSION, PROTOCOL_MIN_VERSION,
};
use crate::server::protocol::{
    BreakpointCapabilities, BreakpointDescriptor, DebugMessage, DebugRequest, DebugResponse,
    RemoteSessionInfo, ServerCapabilities,
};
use crate::simulator::SnapshotLoader;
use crate::Result;
use chrono::Utc;
use std::fs;
use std::io::BufReader as StdBufReader;
use std::path::Path;
use std::sync::Arc;
use tokio::io::AsyncBufReadExt;
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio_rustls::rustls::{Certificate, PrivateKey, ServerConfig};
use tokio_rustls::TlsAcceptor;
use tracing::{error, info, warn};
use uuid::Uuid;

/// Default grace period (in seconds) that the server will hold a session
/// after the client connection drops before discarding the debugging context.
pub const SESSION_GRACE_PERIOD_SECS: u64 = 300;

pub struct DebugServer {
    host: String,
    engine: Option<DebuggerEngine>,
    token: Option<String>,
    tls_config: Option<ServerConfig>,
    pending_execution: Option<PendingExecution>,
    shutdown: Arc<Notify>,
    contract_wasm: Option<Vec<u8>>,
    repeat_count: Option<u32>,
    storage_filter: Vec<String>,
    show_events: bool,
    event_filter: Vec<String>,
    mock_specs: Vec<String>,
    /// Opaque session identifier issued during the initial handshake.
    /// Clients present this value in a `Reconnect` request to re-attach.
    session_id: String,
    session_created_at: String,
    session_label: Option<String>,
    /// Instant when the last client disconnected (used for grace-period expiry).
    last_disconnect: Option<std::time::Instant>,
    /// Log of successful reconnection events in the current session.
    reconnection_log: ReconnectionLog,
}

struct PendingExecution {
    function: String,
    args: Option<String>,
}

#[derive(Clone)]
struct SessionContext {
    info: RemoteSessionInfo,
}

impl DebugServer {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        host: String,
        token: Option<String>,
        cert_path: Option<&Path>,
        key_path: Option<&Path>,
        repeat_count: Option<u32>,
        storage_filter: Vec<String>,
        show_events: bool,
        event_filter: Vec<String>,
        mock_specs: Vec<String>,
    ) -> Result<Self> {
        let tls_config = match (cert_path, key_path) {
            (Some(cp), Some(kp)) => Some(load_tls_config(cp, kp)?),
            (None, None) => None,
            _ => {
                return Err(miette::miette!(
                    "TLS requires both certificate and key paths (--tls-cert and --tls-key). Provide both flags together, or remove both flags to run without native TLS."
                ));
            }
        };

        Ok(Self {
            host,
            engine: None,
            token,
            tls_config,
            pending_execution: None,
            shutdown: Arc::new(Notify::new()),
            contract_wasm: None,
            repeat_count,
            storage_filter,
            show_events,
            event_filter,
            mock_specs,
            session_id: Uuid::new_v4().to_string(),
            session_created_at: Utc::now().to_rfc3339(),
            session_label: None,
            last_disconnect: None,
            reconnection_log: ReconnectionLog::new(),
        })
    }

    pub async fn run(mut self, port: u16) -> Result<()> {
        let addr = format!("{}:{}", self.host, port);
        let listener = TcpListener::bind(&addr)
            .await
            .map_err(|e| miette::miette!("Failed to bind to {}: {}", addr, e))?;
        info!("Debug server listening on {}", addr);
        if self.token.is_some() && self.tls_config.is_none() {
            warn!(
                "Token authentication is enabled without TLS. Treat this as plaintext transport and \
                 restrict access to trusted network boundaries or add TLS termination."
            );
        }

        let acceptor = self
            .tls_config
            .take()
            .map(|cfg| TlsAcceptor::from(Arc::new(cfg)));

        let shutdown = self.shutdown.clone();
        tokio::spawn(setup_signal_handlers(shutdown));

        loop {
            tokio::select! {
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, addr)) => {
                            info!("New connection from {}", addr);
                            let peer = addr.to_string();
                            if let Some(ref acceptor) = acceptor {
                                match acceptor.accept(stream).await {
                                    Ok(tls_stream) => {
                                        if let Err(e) = self.handle_single_connection(tls_stream, &peer).await {
                                            error!("TLS connection error: {}", e);
                                        }
                                    }
                                    Err(e) => error!("TLS accept error: {}", e),
                                }
                            } else if let Err(e) = self.handle_single_connection(stream, &peer).await {
                                error!("TCP connection error: {}", e);
                            }
                        }
                        Err(e) => error!("Failed to accept connection: {}", e),
                    }
                }
                _ = self.shutdown.notified() => {
                    info!("Shutting down debug server");
                    self.log_shutdown_summary();
                    drop(listener);
                    break;
                }
            }
        }

        Ok(())
    }

    async fn handle_single_connection<S>(&mut self, stream: S, peer_addr: &str) -> Result<()>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        // Check if a previously "parked" session has expired before we do anything else.
        if let Some(instant) = self.last_disconnect {
            if instant.elapsed().as_secs() > SESSION_GRACE_PERIOD_SECS {
                info!(
                    "Previous session {} expired after {} seconds. Resetting session state.",
                    self.session_id, SESSION_GRACE_PERIOD_SECS
                );
                self.engine = None;
                self.pending_execution = None;
                self.contract_wasm = None;
                self.last_disconnect = None;
                // Generate a new session id for this fresh connection
                self.session_id = Uuid::new_v4().to_string();
                self.session_created_at = Utc::now().to_rfc3339();
                self.session_label = None;
            }
        }

        let mut authenticated = self.token.is_none();
        let mut handshake_done = false;
        let mut session_ctx = SessionContext {
            info: RemoteSessionInfo {
                session_id: self.session_id.clone(),
                created_at: self.session_created_at.clone(),
                label: self.session_label.clone(),
            },
        };
        let (reader, writer) = tokio::io::split(stream);
        let mut reader = tokio::io::BufReader::new(reader);

        let (tx_in, mut rx_in) = tokio::sync::mpsc::unbounded_channel::<String>();
        let (tx_out, mut rx_out) = tokio::sync::mpsc::unbounded_channel::<DebugMessage>();

        tokio::spawn(async move {
            let mut writer = writer;
            while let Some(msg) = rx_out.recv().await {
                if crate::server::protocol::send_response::<tokio::io::WriteHalf<S>>(
                    &mut writer,
                    msg,
                )
                .await
                .is_err()
                {
                    break;
                }
            }
        });

        let tx_out_reader = tx_out.clone();
        let is_executing = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let is_executing_reader = Arc::clone(&is_executing);

        let reader_handle = tokio::spawn(async move {
            let mut line = String::new();
            loop {
                line.clear();
                let n = reader.read_line(&mut line).await.unwrap_or(0);
                if n == 0 {
                    break;
                }

                if let Ok(msg) = DebugMessage::parse(line.trim_end()) {
                    if matches!(msg.request, Some(DebugRequest::Cancel)) {
                        let response = DebugMessage::response(msg.id, DebugResponse::CancelAck);
                        let _ = tx_out_reader.send(response);
                        if is_executing_reader.load(std::sync::atomic::Ordering::SeqCst) {
                            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                            eprintln!(
                                "Execution cancelled via request. Aborting with exit code 125."
                            );
                            std::process::exit(125);
                        }
                        continue;
                    }
                }

                if tx_in.send(line.clone()).is_err() {
                    break;
                }
            }
        });

        // Helper closure to abstract away tx_out
        let send_msg = |msg: DebugMessage| -> Result<()> {
            tx_out
                .send(msg)
                .map_err(|_| miette::miette!("Connection closed"))
        };

        let mut idle_timeout = None;
        let mut _heartbeat_timer = None;

        loop {
            let next_message = tokio::select! {
                msg = async {
                    if let Some(timeout) = idle_timeout {
                        match tokio::time::timeout(
                            std::time::Duration::from_millis(timeout as u64),
                            rx_in.recv(),
                        )
                        .await
                        {
                            Ok(res) => res,
                            Err(_) => {
                                warn!("Idle timeout reached for connection");
                                let _ = tx_out.send(DebugMessage::response(0, DebugResponse::Disconnected));
                                None
                            }
                        }
                    } else {
                        rx_in.recv().await
                    }
                } => msg,
                _ = self.shutdown.notified() => {
                    info!("Shutdown signal received during active connection");
                    self.shutdown.notify_one();
                    break;
                }
            };

            let line = match next_message {
                Some(l) => l,
                None => break,
            };
            is_executing.store(false, std::sync::atomic::Ordering::SeqCst);

            let message = match DebugMessage::parse(line.trim_end()) {
                Ok(msg) => msg,
                Err(e) => {
                    warn!("Failed to parse request: {}", e);
                    let response = DebugMessage::response(
                        0,
                        DebugResponse::Error {
                            message: format!("Malformed request: {}", e),
                        },
                    );
                    let _ = send_msg(response);
                    continue;
                }
            };
            let Some(request) = message.request else {
                warn!("Received message without request");
                continue;
            };

            if matches!(request, DebugRequest::Unknown) {
                let response = DebugMessage::response(
                    message.id,
                    DebugResponse::Error {
                        message: "Unknown request type. Try upgrading the server.".to_string(),
                    },
                );
                send_msg(response)?;
                continue;
            }

            info!(
                session_id = %session_ctx.info.session_id,
                session_label = ?session_ctx.info.label,
                "Received request: {}",
                summarize_request(&request)
            );

            if matches!(request, DebugRequest::Ping) {
                let response = DebugMessage::response(message.id, DebugResponse::Pong);
                send_msg(response)?;
                continue;
            }

            if let DebugRequest::Handshake {
                client_name,
                client_version,
                protocol_min,
                protocol_max,
                heartbeat_interval_ms,
                idle_timeout_ms,
                session_label,
                reconnect_session_id: _,
                required_capabilities,
            } = &request
            {
                if let Some(label) = session_label
                    .as_ref()
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                {
                    session_ctx.info.label = Some(label.to_string());
                    self.session_label = session_ctx.info.label.clone();
                }
                let server_name = "soroban-debug".to_string();
                let server_version = env!("CARGO_PKG_VERSION").to_string();

                match negotiate_protocol_version(*protocol_min, *protocol_max) {
                    Ok(selected_version) => {
                        handshake_done = true;
                        // Support heartbeat/timeout negotiation
                        idle_timeout = *idle_timeout_ms;

                        // --- Capability negotiation (new block) ---
                        let our_caps = ServerCapabilities::current();
                        if let Some(required) = required_capabilities {
                            let missing = required.unsupported_by(&our_caps);
                            if !missing.is_empty() {
                                let response = DebugMessage::response(
                                    message.id,
                                    DebugResponse::IncompatibleCapabilities {
                                        message: format!(
                                            "Server does not support required capabilities: {}. \
                                             Upgrade the server or disable these features on the client.",
                                            missing.join(", ")
                                        ),
                                        missing_capabilities: missing.iter().map(|s| s.to_string()).collect(),
                                        server_capabilities: our_caps,
                                    },
                                );
                                send_msg(response)?;
                                return Ok(());
                            }
                        }
                        // --- end capability negotiation ---

                        if let Some(interval) = *heartbeat_interval_ms {
                            info!("Negotiated heartbeat interval: {}ms", interval);
                            let tx_heartbeat = tx_out.clone();
                            let interval_ms = interval as u64;
                            _heartbeat_timer = Some(tokio::spawn(async move {
                                let mut interval_timer = tokio::time::interval(
                                    std::time::Duration::from_millis(interval_ms),
                                );
                                // Avoid immediate tick if possible, though tokio interval ticks first.
                                interval_timer.tick().await;

                                loop {
                                    interval_timer.tick().await;
                                    let ping = DebugMessage::request(0, DebugRequest::Ping);
                                    if tx_heartbeat.send(ping).is_err() {
                                        break;
                                    }
                                }
                            }));
                        }
                        if let Some(timeout) = idle_timeout {
                            info!("Negotiated idle timeout: {}ms", timeout);
                        }

                        let response = DebugMessage::response(
                            message.id,
                            DebugResponse::HandshakeAck {
                                server_name,
                                server_version,
                                protocol_min: PROTOCOL_MIN_VERSION,
                                protocol_max: PROTOCOL_MAX_VERSION,
                                selected_version,
                                session_id: Some(session_ctx.info.session_id.clone()),
                                session_created_at: Some(session_ctx.info.created_at.clone()),
                                session_label: session_ctx.info.label.clone(),
                                heartbeat_interval_ms: *heartbeat_interval_ms,
                                idle_timeout_ms: idle_timeout,
                                reconnect_id: Some(self.session_id.clone()),
                                server_capabilities: ServerCapabilities::current(),
                            },
                        );
                        send_msg(response)?;
                        if let Ok(history) = HistoryManager::new() {
                            let _ = history.append_remote_session(
                                crate::history::RemoteSessionRecord {
                                    session_id: session_ctx.info.session_id.clone(),
                                    created_at: session_ctx.info.created_at.clone(),
                                    label: session_ctx.info.label.clone(),
                                    remote_addr: peer_addr.to_string(),
                                    client_name: client_name.clone(),
                                    client_version: client_version.clone(),
                                },
                            );
                        }
                        continue;
                    }
                    Err(e) => {
                        let response = DebugMessage::response(
                            message.id,
                            DebugResponse::IncompatibleProtocol {
                                message: format!(
                                    "{}. Client: {}@{}. Upgrade the older component.",
                                    e, client_name, client_version
                                ),
                                server_name,
                                server_version,
                                protocol_min: PROTOCOL_MIN_VERSION,
                                protocol_max: PROTOCOL_MAX_VERSION,
                            },
                        );
                        send_msg(response)?;
                        return Ok(());
                    }
                }
            }

            // BACKWARD COMPATIBILITY (intentional): Allow `Authenticate` to succeed before
            // the protocol `Handshake` is completed. Older clients (pre-handshake protocol)
            // send `Authenticate` as their first message. Removing or reordering this block
            // would break those clients silently. Any change here MUST be accompanied by an
            // update to the parity test `parity_dap_auth_before_handshake_is_accepted` in
            // tests/parity_tests.rs and a version bump in src/server/protocol.rs.
            if let DebugRequest::Authenticate { token } = &request {
                let success = self
                    .token
                    .as_deref()
                    .map(|server_token| server_token == token)
                    .unwrap_or(true);
                authenticated = success;
                let response = DebugResponse::Authenticated {
                    success,
                    message: if success {
                        "Authentication successful".to_string()
                    } else {
                        "Authentication failed".to_string()
                    },
                };
                let response = DebugMessage::response(message.id, response);
                send_msg(response)?;
                if !success {
                    return Ok(());
                }
                continue;
            }

            if !handshake_done {
                let response = DebugMessage::response(
                    message.id,
                    DebugResponse::Error {
                        message: "Protocol handshake required: send a Handshake request before other debug requests.".to_string(),
                    },
                );
                send_msg(response)?;
                continue;
            }

            if !authenticated {
                if let DebugRequest::Authenticate { token } = request {
                    let success = self.token.as_deref().map(|t| t == token).unwrap_or(true);
                    authenticated = success;
                    let response = DebugResponse::Authenticated {
                        success,
                        message: if success {
                            "Authentication successful".to_string()
                        } else {
                            "Authentication failed".to_string()
                        },
                    };
                    let response = DebugMessage::response(message.id, response);
                    send_msg(response)?;
                    if !success {
                        return Ok(());
                    }
                    continue;
                }

                let response = DebugMessage::response(
                    message.id,
                    DebugResponse::Error {
                        message: "Authentication required".to_string(),
                    },
                );
                send_msg(response)?;
                continue;
            }

            // ── Handle Reconnect before normal request dispatch ──────────
            if let DebugRequest::Reconnect {
                session_id: ref client_session_id,
            } = request
            {
                if *client_session_id != self.session_id {
                    let response = DebugMessage::response(
                        message.id,
                        DebugResponse::SessionExpired {
                            message: "Session ID does not match. The session may have been \
                                      replaced by a newer connection or the server was restarted."
                                .to_string(),
                        },
                    );
                    send_msg(response)?;
                    continue;
                }

                if self.engine.is_none() {
                    let response = DebugMessage::response(
                        message.id,
                        DebugResponse::SessionExpired {
                            message: "No active session found. The session may have been \
                                      cleared due to a manual disconnect or server restart."
                                .to_string(),
                        },
                    );
                    send_msg(response)?;
                    continue;
                }

                if let Some(instant) = self.last_disconnect.take() {
                    self.reconnection_log.record(
                        &self.session_id,
                        instant.elapsed(),
                        self.engine.as_ref().is_some_and(|e| e.is_paused()),
                    );
                }
                info!("Client reconnected to session {}", self.session_id);

                let (paused, current_function, step_count) = self
                    .engine
                    .as_ref()
                    .and_then(|engine| {
                        engine.state().lock().ok().map(|state| {
                            (
                                engine.is_paused(),
                                state.current_function().map(|s| s.to_string()),
                                state.step_count() as u64,
                            )
                        })
                    })
                    .unwrap_or((false, None, 0));

                let breakpoints = self
                    .engine
                    .as_ref()
                    .map(|e| e.breakpoints().list())
                    .unwrap_or_default();

                let response = DebugMessage::response(
                    message.id,
                    DebugResponse::ReconnectAck {
                        session_id: self.session_id.clone(),
                        paused,
                        current_function,
                        breakpoints,
                        step_count,
                    },
                );
                send_msg(response)?;
                continue;
            }

            let is_disconnect = matches!(&request, DebugRequest::Disconnect);
            let response = match request {
                DebugRequest::Authenticate { .. } => DebugResponse::Authenticated {
                    success: true,
                    message: "Already authenticated".to_string(),
                },
                DebugRequest::Handshake { .. } => DebugResponse::Error {
                    message: "Protocol handshake already completed".to_string(),
                },
                DebugRequest::LoadContract { contract_path } => match fs::read(&contract_path) {
                    Ok(bytes) => {
                        match crate::runtime::executor::ContractExecutor::new(bytes.clone()) {
                            Ok(executor) => {
                                let mut engine = DebuggerEngine::new(executor, vec![]);
                                if !self.mock_specs.is_empty() {
                                    if let Err(e) =
                                        engine.executor_mut().set_mock_specs(&self.mock_specs)
                                    {
                                        let msg = format!(
                                            "Invalid mock spec in server configuration: {}",
                                            e
                                        );
                                        DebugResponse::Error { message: msg }
                                    } else {
                                        let _ = engine.enable_instruction_debug(&bytes);
                                        self.engine = Some(engine);
                                        self.pending_execution = None;
                                        self.contract_wasm = Some(bytes);
                                        DebugResponse::ContractLoaded {
                                            size: fs::metadata(&contract_path)
                                                .map(|m| m.len() as usize)
                                                .unwrap_or(0),
                                        }
                                    }
                                } else {
                                    let _ = engine.enable_instruction_debug(&bytes);
                                    self.engine = Some(engine);
                                    self.pending_execution = None;
                                    self.contract_wasm = Some(bytes);
                                    DebugResponse::ContractLoaded {
                                        size: fs::metadata(&contract_path)
                                            .map(|m| m.len() as usize)
                                            .unwrap_or(0),
                                    }
                                }
                            }
                            Err(e) => DebugResponse::Error {
                                message: e.to_string(),
                            },
                        }
                    }
                    Err(e) => DebugResponse::Error {
                        message: format!("Failed to read contract {:?}: {}", contract_path, e),
                    },
                },
                DebugRequest::ResolveSourceBreakpoints {
                    source_path,
                    lines,
                    max_forward_line_adjust,
                } => match (self.engine.as_ref(), self.contract_wasm.as_deref()) {
                    (Some(engine), Some(wasm_bytes)) => {
                        if let Some(source_map) = engine.source_map() {
                            let breakpoints = source_map.resolve_source_breakpoints(
                                wasm_bytes,
                                Path::new(&source_path),
                                &lines,
                                max_forward_line_adjust,
                            );
                            DebugResponse::SourceBreakpointsResolved { breakpoints }
                        } else {
                            let breakpoints = lines
                                .into_iter()
                                .map(|line| crate::debugger::SourceBreakpointResolution {
                                    requested_line: line,
                                    line,
                                    verified: false,
                                    function: None,
                                    reason_code: "NO_DEBUG_INFO".to_string(),
                                    message:
                                        "[NO_DEBUG_INFO] Contract is missing DWARF source mappings; rebuild with debug info to bind source breakpoints accurately.".to_string(),
                                })
                                .collect();
                            DebugResponse::SourceBreakpointsResolved { breakpoints }
                        }
                    }
                    _ => DebugResponse::Error {
                        message: "No contract loaded".to_string(),
                    },
                },
                DebugRequest::Execute { function, args } => {
                    if let Some(count) = self.repeat_count {
                        if count > 1 {
                            if let Some(wasm) = &self.contract_wasm {
                                let breakpoints = self
                                    .engine
                                    .as_ref()
                                    .map(|e| e.breakpoints().list())
                                    .unwrap_or_default();
                                let initial_storage = self
                                    .engine
                                    .as_ref()
                                    .and_then(|e| e.executor().get_storage_snapshot().ok())
                                    .and_then(|s| serde_json::to_string(&s).ok());
                                let runner = crate::repeat::RepeatRunner::new(
                                    wasm.clone(),
                                    breakpoints,
                                    initial_storage,
                                );
                                match runner.run(&function, args.as_deref(), count) {
                                    Ok(stats) => {
                                        let output = format!(
                                            "--- Repeat Execution ({} runs) ---\n\nDuration:\n  Min: {:.2}ms, Max: {:.2}ms, Avg: {:.2}ms\n\nCPU Instructions:\n  Min: {}, Max: {}, Avg: {}\n\nMemory (bytes):\n  Min: {}, Max: {}, Avg: {}\n\nResults: {}",
                                            count,
                                            stats.min_duration.as_secs_f64() * 1000.0,
                                            stats.max_duration.as_secs_f64() * 1000.0,
                                            stats.avg_duration.as_secs_f64() * 1000.0,
                                            stats.min_cpu,
                                            stats.max_cpu,
                                            stats.avg_cpu,
                                            stats.min_memory,
                                            stats.max_memory,
                                            stats.avg_memory,
                                            if stats.inconsistent_results {
                                                "INCONSISTENT"
                                            } else {
                                                "CONSISTENT"
                                            }
                                        );
                                        let resp = DebugResponse::ExecutionResult {
                                            success: true,
                                            output,
                                            error: None,
                                            paused: false,
                                            completed: true,
                                            source_location: None,
                                            pause_reason: None,
                                        };
                                        send_msg(DebugMessage::response(message.id, resp))?;
                                        continue;
                                    }
                                    Err(e) => {
                                        let resp = DebugResponse::Error {
                                            message: e.to_string(),
                                        };
                                        send_msg(DebugMessage::response(message.id, resp))?;
                                        continue;
                                    }
                                }
                            } else {
                                DebugResponse::Error {
                                    message: "No contract loaded for repeat execution".to_string(),
                                }
                            }
                        } else {
                            match self.engine.as_mut() {
                                Some(engine) => {
                                    if engine.breakpoints().should_break(&function) {
                                        match current_storage(engine) {
                                            Ok(storage) => match engine.breakpoints_mut().on_hit(
                                                &function,
                                                &storage,
                                                args.as_deref(),
                                            ) {
                                                Ok(Some(hit)) => {
                                                    for message in hit.log_messages {
                                                        println!("{message}");
                                                    }
                                                    if hit.should_pause {
                                                        engine.prepare_breakpoint_stop(
                                                            &function,
                                                            args.as_deref(),
                                                        );
                                                        self.pending_execution =
                                                            Some(PendingExecution {
                                                                function: function.clone(),
                                                                args: args.clone(),
                                                            });
                                                        DebugResponse::ExecutionResult {
                                                            success: true,
                                                            output: "Paused at function breakpoint"
                                                                .to_string(),
                                                            error: None,
                                                            paused: true,
                                                            completed: false,
                                                            source_location: engine
                                                                .current_source_location()
                                                                .map(Into::into),
                                                            pause_reason: engine
                                                                .pause_reason_label()
                                                                .map(|s| s.to_string()),
                                                        }
                                                    } else {
                                                        is_executing.store(
                                                            true,
                                                            std::sync::atomic::Ordering::SeqCst,
                                                        );
                                                        let resp = execute_without_breakpoints(
                                                            engine,
                                                            &function,
                                                            args,
                                                            self.show_events,
                                                            &self.event_filter,
                                                        );
                                                        is_executing.store(
                                                            false,
                                                            std::sync::atomic::Ordering::SeqCst,
                                                        );
                                                        resp
                                                    }
                                                }
                                                Ok(None) => {
                                                    is_executing.store(
                                                        true,
                                                        std::sync::atomic::Ordering::SeqCst,
                                                    );
                                                    let resp = execute_without_breakpoints(
                                                        engine,
                                                        &function,
                                                        args,
                                                        self.show_events,
                                                        &self.event_filter,
                                                    );
                                                    is_executing.store(
                                                        false,
                                                        std::sync::atomic::Ordering::SeqCst,
                                                    );
                                                    resp
                                                }
                                                Err(e) => DebugResponse::Error {
                                                    message: e.to_string(),
                                                },
                                            },
                                            Err(e) => DebugResponse::Error {
                                                message: e.to_string(),
                                            },
                                        }
                                    } else {
                                        is_executing
                                            .store(true, std::sync::atomic::Ordering::SeqCst);
                                        let resp = execute_without_breakpoints(
                                            engine,
                                            &function,
                                            args,
                                            self.show_events,
                                            &self.event_filter,
                                        );
                                        is_executing
                                            .store(false, std::sync::atomic::Ordering::SeqCst);
                                        resp
                                    }
                                }
                                None => DebugResponse::Error {
                                    message: "No contract engine initialized".to_string(),
                                },
                            }
                        }
                    } else {
                        match self.engine.as_mut() {
                            Some(engine) => {
                                if engine.breakpoints().should_break(&function) {
                                    match current_storage(engine) {
                                        Ok(storage) => match engine.breakpoints_mut().on_hit(
                                            &function,
                                            &storage,
                                            args.as_deref(),
                                        ) {
                                            Ok(Some(hit)) => {
                                                for message in hit.log_messages {
                                                    println!("{message}");
                                                }
                                                if hit.should_pause {
                                                    engine.prepare_breakpoint_stop(
                                                        &function,
                                                        args.as_deref(),
                                                    );
                                                    self.pending_execution =
                                                        Some(PendingExecution {
                                                            function: function.clone(),
                                                            args: args.clone(),
                                                        });
                                                    DebugResponse::ExecutionResult {
                                                        success: true,
                                                        output: "Paused at function breakpoint"
                                                            .to_string(),
                                                        error: None,
                                                        paused: true,
                                                        completed: false,
                                                        source_location: engine
                                                            .current_source_location()
                                                            .map(Into::into),
                                                        pause_reason: engine
                                                            .pause_reason_label()
                                                            .map(|s| s.to_string()),
                                                    }
                                                } else {
                                                    is_executing.store(
                                                        true,
                                                        std::sync::atomic::Ordering::SeqCst,
                                                    );
                                                    let resp = execute_without_breakpoints(
                                                        engine,
                                                        &function,
                                                        args,
                                                        self.show_events,
                                                        &self.event_filter,
                                                    );
                                                    is_executing.store(
                                                        false,
                                                        std::sync::atomic::Ordering::SeqCst,
                                                    );
                                                    resp
                                                }
                                            }
                                            Ok(None) => {
                                                is_executing.store(
                                                    true,
                                                    std::sync::atomic::Ordering::SeqCst,
                                                );
                                                let resp = execute_without_breakpoints(
                                                    engine,
                                                    &function,
                                                    args,
                                                    self.show_events,
                                                    &self.event_filter,
                                                );
                                                is_executing.store(
                                                    false,
                                                    std::sync::atomic::Ordering::SeqCst,
                                                );
                                                resp
                                            }
                                            Err(e) => DebugResponse::Error {
                                                message: e.to_string(),
                                            },
                                        },
                                        Err(e) => DebugResponse::Error {
                                            message: e.to_string(),
                                        },
                                    }
                                } else {
                                    is_executing.store(true, std::sync::atomic::Ordering::SeqCst);
                                    let resp = execute_without_breakpoints(
                                        engine,
                                        &function,
                                        args,
                                        self.show_events,
                                        &self.event_filter,
                                    );
                                    is_executing.store(false, std::sync::atomic::Ordering::SeqCst);
                                    resp
                                }
                            }
                            None => DebugResponse::Error {
                                message: "No contract engine initialized".to_string(),
                            },
                        }
                    }
                }
                DebugRequest::Step | DebugRequest::StepIn => match self.engine.as_mut() {
                    Some(engine) => match engine.step_into() {
                        Ok(_) => {
                            let (current_function, step_count) = engine
                                .state()
                                .lock()
                                .map(|state| {
                                    (
                                        state.current_function().map(|s| s.to_string()),
                                        state.step_count() as u64,
                                    )
                                })
                                .unwrap_or((None, 0));
                            DebugResponse::StepResult {
                                paused: engine.is_paused(),
                                current_function,
                                step_count,
                                source_location: engine.current_source_location().map(Into::into),
                                pause_reason: engine.pause_reason_label().map(|s| s.to_string()),
                            }
                        }
                        Err(e) => DebugResponse::Error {
                            message: e.to_string(),
                        },
                    },
                    None => DebugResponse::Error {
                        message: "No contract loaded".to_string(),
                    },
                },
                DebugRequest::Next => match self.engine.as_mut() {
                    Some(engine) => match engine.step_over() {
                        Ok(_) => {
                            let (current_function, step_count) = engine
                                .state()
                                .lock()
                                .map(|state| {
                                    (
                                        state.current_function().map(|s| s.to_string()),
                                        state.step_count() as u64,
                                    )
                                })
                                .unwrap_or((None, 0));
                            DebugResponse::StepResult {
                                paused: engine.is_paused(),
                                current_function,
                                step_count,
                                source_location: engine.current_source_location().map(Into::into),
                                pause_reason: engine.pause_reason_label().map(|s| s.to_string()),
                            }
                        }
                        Err(e) => DebugResponse::Error {
                            message: e.to_string(),
                        },
                    },
                    None => DebugResponse::Error {
                        message: "No contract loaded".to_string(),
                    },
                },
                DebugRequest::StepOut => match self.engine.as_mut() {
                    Some(engine) => {
                        // When paused at a function-level breakpoint (pending execution),
                        // step-out means executing the function to completion.
                        if let Some(pending) = self.pending_execution.take() {
                            let (current_function, step_count) = engine
                                .state()
                                .lock()
                                .map(|state| {
                                    (
                                        state.current_function().map(|s| s.to_string()),
                                        state.step_count() as u64,
                                    )
                                })
                                .unwrap_or((None, 0));
                            let exec_result = {
                                is_executing.store(true, std::sync::atomic::Ordering::SeqCst);
                                let r = engine.execute_without_breakpoints(
                                    &pending.function,
                                    pending.args.as_deref(),
                                );
                                is_executing.store(false, std::sync::atomic::Ordering::SeqCst);
                                r
                            };
                            match exec_result {
                                Ok(_) => DebugResponse::StepResult {
                                    paused: false,
                                    current_function,
                                    step_count,
                                    source_location: engine
                                        .current_source_location()
                                        .map(Into::into),
                                    pause_reason: engine
                                        .pause_reason_label()
                                        .map(|s| s.to_string()),
                                },
                                Err(e) => DebugResponse::Error {
                                    message: e.to_string(),
                                },
                            }
                        } else {
                            match engine.step_out() {
                                Ok(_) => {
                                    let (current_function, step_count) = engine
                                        .state()
                                        .lock()
                                        .map(|state| {
                                            (
                                                state.current_function().map(|s| s.to_string()),
                                                state.step_count() as u64,
                                            )
                                        })
                                        .unwrap_or((None, 0));
                                    DebugResponse::StepResult {
                                        paused: engine.is_paused(),
                                        current_function,
                                        step_count,
                                        source_location: engine
                                            .current_source_location()
                                            .map(Into::into),
                                        pause_reason: engine
                                            .pause_reason_label()
                                            .map(|s| s.to_string()),
                                    }
                                }
                                Err(e) => DebugResponse::Error {
                                    message: e.to_string(),
                                },
                            }
                        }
                    }
                    None => DebugResponse::Error {
                        message: "No contract loaded".to_string(),
                    },
                },
                DebugRequest::StepOverLine => match self.engine.as_mut() {
                    Some(engine) => match engine.step_over_source_line() {
                        Ok(StepOverResult { paused, location }) => {
                            DebugResponse::StepOverLineResult {
                                paused,
                                file: location
                                    .as_ref()
                                    .map(|l| l.file.to_string_lossy().into_owned()),
                                line: location.as_ref().map(|l| l.line),
                                column: location.and_then(|l| l.column),
                            }
                        }
                        Err(e) => DebugResponse::Error {
                            message: format!("StepOverLine failed: {}", e),
                        },
                    },
                    None => DebugResponse::Error {
                        message: "No contract loaded".to_string(),
                    },
                },
                DebugRequest::Continue => match self.engine.as_mut() {
                    Some(engine) => {
                        if let Some(pending) = self.pending_execution.take() {
                            let exec_result = {
                                is_executing.store(true, std::sync::atomic::Ordering::SeqCst);
                                let r = engine.execute_without_breakpoints(
                                    &pending.function,
                                    pending.args.as_deref(),
                                );
                                is_executing.store(false, std::sync::atomic::Ordering::SeqCst);
                                r
                            };
                            match exec_result {
                                Ok(output) => DebugResponse::ContinueResult {
                                    completed: true,
                                    output: Some(output),
                                    error: None,
                                    paused: false,
                                    source_location: engine
                                        .current_source_location()
                                        .map(Into::into),
                                    pause_reason: engine
                                        .pause_reason_label()
                                        .map(|s| s.to_string()),
                                },
                                Err(e) => DebugResponse::ContinueResult {
                                    completed: false,
                                    output: None,
                                    error: Some(e.to_string()),
                                    paused: false,
                                    source_location: engine
                                        .current_source_location()
                                        .map(Into::into),
                                    pause_reason: engine
                                        .pause_reason_label()
                                        .map(|s| s.to_string()),
                                },
                            }
                        } else {
                            match engine.continue_execution() {
                                Ok(_) => DebugResponse::ContinueResult {
                                    completed: true,
                                    output: None,
                                    error: None,
                                    paused: engine.is_paused(),
                                    source_location: engine
                                        .current_source_location()
                                        .map(Into::into),
                                    pause_reason: engine
                                        .pause_reason_label()
                                        .map(|s| s.to_string()),
                                },
                                Err(e) => DebugResponse::ContinueResult {
                                    completed: false,
                                    output: None,
                                    error: Some(e.to_string()),
                                    paused: engine.is_paused(),
                                    source_location: engine
                                        .current_source_location()
                                        .map(Into::into),
                                    pause_reason: engine
                                        .pause_reason_label()
                                        .map(|s| s.to_string()),
                                },
                            }
                        }
                    }
                    None => DebugResponse::Error {
                        message: "No contract loaded".to_string(),
                    },
                },
                DebugRequest::Inspect => match self.engine.as_ref() {
                    Some(engine) => match engine.state().lock() {
                        Ok(state) => {
                            let call_stack = state
                                .call_stack()
                                .get_stack()
                                .iter()
                                .map(|frame| {
                                    let suffix = frame
                                        .contract_id
                                        .as_ref()
                                        .map(|id| format!(" [{}]", id))
                                        .unwrap_or_default();
                                    format!("{}{}", frame.function, suffix)
                                })
                                .collect();
                            let function = state.current_function().map(|s| s.to_string());
                            let args = state.current_args().map(|s| s.to_string());
                            let step_count = state.step_count() as u64;
                            drop(state);

                            DebugResponse::InspectionResult {
                                function,
                                args,
                                step_count,
                                paused: engine.is_paused(),
                                call_stack,
                                source_location: engine.current_source_location().map(Into::into),
                                pause_reason: engine.pause_reason_label().map(|s| s.to_string()),
                            }
                        }
                        Err(e) => DebugResponse::Error {
                            message: format!("Failed to acquire state lock: {}", e),
                        },
                    },
                    None => DebugResponse::Error {
                        message: "No contract loaded".to_string(),
                    },
                },
                DebugRequest::GetStorage => match self.engine.as_ref() {
                    Some(engine) => match engine.executor().get_storage_snapshot() {
                        Ok(mut snapshot) => {
                            if !self.storage_filter.is_empty() {
                                if let Ok(filter) = crate::inspector::storage::StorageFilter::new(
                                    &self.storage_filter,
                                ) {
                                    snapshot.retain(|k, _| filter.matches(k));
                                }
                            }
                            match serde_json::to_string(&snapshot) {
                                Ok(json) => DebugResponse::StorageState { storage_json: json },
                                Err(e) => DebugResponse::Error {
                                    message: format!("Failed to serialize storage snapshot: {}", e),
                                },
                            }
                        }
                        Err(e) => DebugResponse::Error {
                            message: e.to_string(),
                        },
                    },
                    None => DebugResponse::Error {
                        message: "No contract loaded".to_string(),
                    },
                },
                DebugRequest::GetStack => match self.engine.as_ref() {
                    Some(engine) => match engine.state().lock() {
                        Ok(state) => {
                            let stack = state
                                .call_stack()
                                .get_stack()
                                .iter()
                                .map(|frame| {
                                    let suffix = frame
                                        .contract_id
                                        .as_ref()
                                        .map(|id| format!(" [{}]", id))
                                        .unwrap_or_default();
                                    format!("{}{}", frame.function, suffix)
                                })
                                .collect();
                            DebugResponse::CallStack { stack }
                        }
                        Err(e) => DebugResponse::Error {
                            message: format!("Failed to acquire state lock: {}", e),
                        },
                    },
                    None => DebugResponse::Error {
                        message: "No contract loaded".to_string(),
                    },
                },
                DebugRequest::GetBudget => match self.engine.as_ref() {
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
                DebugRequest::SetBreakpoint {
                    id,
                    function,
                    condition,
                    hit_condition,
                    log_message,
                } => match self.engine.as_mut() {
                    Some(engine) => {
                        let condition = match condition {
                            Some(condition) => match BreakpointManager::parse_condition(&condition)
                            {
                                Ok(condition) => Some(condition),
                                Err(e) => {
                                    let response = DebugMessage::response(
                                        message.id,
                                        DebugResponse::Error {
                                            message: e.to_string(),
                                        },
                                    );
                                    send_msg(response)?;
                                    continue;
                                }
                            },
                            None => None,
                        };
                        let hit_condition = match hit_condition {
                            Some(hit_condition) => {
                                match BreakpointManager::parse_hit_condition(&hit_condition) {
                                    Ok(hit_condition) => Some(hit_condition),
                                    Err(e) => {
                                        let response = DebugMessage::response(
                                            message.id,
                                            DebugResponse::Error {
                                                message: e.to_string(),
                                            },
                                        );
                                        send_msg(response)?;
                                        continue;
                                    }
                                }
                            }
                            None => None,
                        };

                        engine.breakpoints_mut().add_spec(BreakpointSpec {
                            id: id.clone(),
                            function: function.clone(),
                            condition,
                            hit_condition,
                            log_message,
                        });
                        DebugResponse::BreakpointSet { id, function }
                    }
                    None => DebugResponse::Error {
                        message: "No contract loaded".to_string(),
                    },
                },
                DebugRequest::ClearBreakpoint { id } => match self.engine.as_mut() {
                    Some(engine) => {
                        engine.breakpoints_mut().remove_by_id(&id);
                        DebugResponse::BreakpointCleared { id }
                    }
                    None => DebugResponse::Error {
                        message: "No contract loaded".to_string(),
                    },
                },
                DebugRequest::ListBreakpoints => match self.engine.as_mut() {
                    Some(engine) => DebugResponse::BreakpointsList {
                        breakpoints: engine
                            .breakpoints_mut()
                            .list_detailed()
                            .into_iter()
                            .map(|breakpoint| BreakpointDescriptor {
                                id: breakpoint.id.clone(),
                                function: breakpoint.function.clone(),
                                condition: breakpoint.condition.clone(),
                                hit_condition: breakpoint.hit_condition.clone(),
                                log_message: breakpoint.log_message.clone(),
                                hit_count: breakpoint.hit_count,
                            })
                            .collect(),
                    },
                    None => DebugResponse::Error {
                        message: "No contract loaded".to_string(),
                    },
                },
                DebugRequest::GetCapabilities => DebugResponse::Capabilities {
                    breakpoints: BreakpointCapabilities {
                        conditional_breakpoints: true,
                        hit_conditional_breakpoints: true,
                        log_points: true,
                    },
                },
                DebugRequest::GetEvents => match self.engine.as_ref() {
                    Some(engine) => match engine.executor().get_dynamic_trace() {
                        Ok(events) => DebugResponse::EventsList { events },
                        Err(e) => DebugResponse::Error {
                            message: e.to_string(),
                        },
                    },
                    None => DebugResponse::Error {
                        message: "No contract loaded".to_string(),
                    },
                },
                DebugRequest::SetStorage { storage_json } => match self.engine.as_mut() {
                    Some(engine) => match engine.executor_mut().set_initial_storage(storage_json) {
                        Ok(_) => match engine.executor().get_storage_snapshot() {
                            Ok(snapshot) => match serde_json::to_string(&snapshot) {
                                Ok(json) => DebugResponse::StorageState { storage_json: json },
                                Err(e) => DebugResponse::Error {
                                    message: format!("Failed to serialize storage snapshot: {}", e),
                                },
                            },
                            Err(e) => DebugResponse::Error {
                                message: e.to_string(),
                            },
                        },
                        Err(e) => DebugResponse::Error {
                            message: e.to_string(),
                        },
                    },
                    None => DebugResponse::Error {
                        message: "No contract loaded".to_string(),
                    },
                },
                DebugRequest::LoadSnapshot { snapshot_path } => {
                    match SnapshotLoader::from_file(snapshot_path) {
                        Ok(loader) => match loader.apply_to_environment() {
                            Ok(loaded) => DebugResponse::SnapshotLoaded {
                                summary: loaded.format_summary(),
                            },
                            Err(e) => DebugResponse::Error {
                                message: e.to_string(),
                            },
                        },
                        Err(e) => DebugResponse::Error {
                            message: e.to_string(),
                        },
                    }
                }
                DebugRequest::Evaluate { expression, .. } => match self.engine.as_ref() {
                    Some(engine) => {
                        // First try to look up the expression as a storage key
                        match engine.executor().get_storage_snapshot() {
                            Ok(snapshot) => {
                                if let Some(value) = snapshot.get(&expression) {
                                    let result = serde_json::to_string(value)
                                        .unwrap_or_else(|_| format!("{:?}", value));
                                    DebugResponse::EvaluateResult {
                                        result,
                                        result_type: Some("storage".to_string()),
                                        variables_reference: 0,
                                    }
                                } else {
                                    // Try matching built-in state fields
                                    let state_result = engine.state().lock().ok().and_then(
                                        |state| match expression.as_str() {
                                            "function" | "current_function" => state
                                                .current_function()
                                                .map(|f| (f.to_string(), "string".to_string())),
                                            "args" | "arguments" => state
                                                .current_args()
                                                .map(|a| (a.to_string(), "string".to_string())),
                                            "step_count" | "steps" => Some((
                                                state.step_count().to_string(),
                                                "number".to_string(),
                                            )),
                                            _ => None,
                                        },
                                    );

                                    match state_result {
                                        Some((result, result_type)) => {
                                            DebugResponse::EvaluateResult {
                                                result,
                                                result_type: Some(result_type),
                                                variables_reference: 0,
                                            }
                                        }
                                        None => DebugResponse::Error {
                                            message: format!(
                                                "Cannot evaluate '{}': only storage key lookup \
                                                 and built-in fields (function, args, \
                                                 step_count) are supported",
                                                expression
                                            ),
                                        },
                                    }
                                }
                            }
                            Err(e) => DebugResponse::Error {
                                message: format!("Failed to access storage for evaluation: {}", e),
                            },
                        }
                    }
                    None => DebugResponse::Error {
                        message: "No contract loaded. Evaluation requires an active debug session."
                            .to_string(),
                    },
                },
                DebugRequest::Ping => DebugResponse::Pong,
                DebugRequest::Disconnect => DebugResponse::Disconnected,
                DebugRequest::Cancel => DebugResponse::CancelAck,
                DebugRequest::Reconnect { .. } => {
                    // Already handled above; this branch is unreachable
                    DebugResponse::Error {
                        message: "Reconnect handled out of band".to_string(),
                    }
                }
                DebugRequest::Unknown => DebugResponse::Error {
                    message: "Unknown request type. Try upgrading the server.".to_string(),
                },
            };

            let response = DebugMessage::response(message.id, response);
            send_msg(response)?;

            if is_disconnect {
                // Explicit disconnect: client intentionally ended the session.
                // Clear the engine so the session cannot be reconnected.
                info!("Client explicitly disconnected, clearing session state");
                self.engine = None;
                self.pending_execution = None;
                self.contract_wasm = None;
                self.last_disconnect = None;
                // Generate a new session id for the next session
                self.session_id = Uuid::new_v4().to_string();
                break;
            }
        }

        // If we reach here via a broken connection (not an explicit Disconnect),
        // preserve the engine for reconnection and record the disconnect time.
        if self.engine.is_some() {
            info!(
                "Client connection lost; preserving session {} for up to {} seconds",
                self.session_id, SESSION_GRACE_PERIOD_SECS
            );
            self.last_disconnect = Some(std::time::Instant::now());
        }

        reader_handle.abort();

        Ok(())
    }
}

impl DebugServer {
    /// Log a human-readable summary of the session when the server shuts down.
    fn log_shutdown_summary(&self) {
        info!("Debug server shutting down (session {})", self.session_id);
        if let Some(wasm) = &self.contract_wasm {
            info!("Loaded contract size: {} bytes", wasm.len());
        }
        let reconnects = self.reconnection_log.count();
        if reconnects > 0 {
            info!("Reconnections during this session: {}", reconnects);
        }
    }
}

fn execute_without_breakpoints(
    engine: &mut DebuggerEngine,
    function: &str,
    args: Option<String>,
    show_events: bool,
    event_filters: &[String],
) -> DebugResponse {
    match engine.execute_without_breakpoints(function, args.as_deref()) {
        Ok(res) => {
            maybe_print_events(engine, show_events, event_filters);
            DebugResponse::ExecutionResult {
                success: true,
                output: res,
                error: None,
                paused: engine.is_paused(),
                completed: true,
                source_location: engine.current_source_location().map(Into::into),
                pause_reason: engine.pause_reason_label().map(|s| s.to_string()),
            }
        }
        Err(e) => DebugResponse::ExecutionResult {
            success: false,
            output: String::new(),
            error: Some(e.to_string()),
            paused: false,
            completed: true,
            source_location: engine.current_source_location().map(Into::into),
            pause_reason: engine.pause_reason_label().map(|s| s.to_string()),
        },
    }
}

fn maybe_print_events(engine: &DebuggerEngine, show_events: bool, event_filters: &[String]) {
    if !show_events && event_filters.is_empty() {
        return;
    }

    let events = match engine.executor().get_events() {
        Ok(events) => events,
        Err(_) => return,
    };

    let filtered = filter_events_for_output(&events, event_filters);
    for line in EventInspector::format_events(&filtered) {
        println!("{}", line);
    }
}

fn filter_events_for_output(events: &[ContractEvent], filters: &[String]) -> Vec<ContractEvent> {
    if filters.is_empty() {
        return events.to_vec();
    }

    events
        .iter()
        .filter(|event| {
            let haystack = format!(
                "{} {} {}",
                event.contract_id.as_deref().unwrap_or_default(),
                event.topics.join(" "),
                event.data
            )
            .to_lowercase();

            filters.iter().any(|filter| {
                let pattern = filter.trim();
                if pattern.is_empty() {
                    return false;
                }

                if let Some(regex_text) = pattern.strip_prefix("re:") {
                    if let Ok(regex) = regex::Regex::new(regex_text) {
                        return regex.is_match(&haystack);
                    }
                    return false;
                }

                haystack.contains(&pattern.to_lowercase())
            })
        })
        .cloned()
        .collect()
}

fn current_storage(engine: &DebuggerEngine) -> Result<std::collections::HashMap<String, String>> {
    engine.executor().get_storage_snapshot()
}

fn load_tls_config(cert_path: &Path, key_path: &Path) -> Result<ServerConfig> {
    let cert_file = fs::File::open(cert_path)
        .map_err(|e| miette::miette!("Failed to open cert file {:?}: {}", cert_path, e))?;
    let mut cert_reader = StdBufReader::new(cert_file);
    let certs = rustls_pemfile::certs(&mut cert_reader)
        .map_err(|e| miette::miette!("Failed to read certs: {}", e))?
        .into_iter()
        .map(Certificate)
        .collect();

    let key_file = fs::File::open(key_path)
        .map_err(|e| miette::miette!("Failed to open key file {:?}: {}", key_path, e))?;
    let mut key_reader = StdBufReader::new(key_file);
    let keys = rustls_pemfile::pkcs8_private_keys(&mut key_reader)
        .map_err(|e| miette::miette!("Failed to read private keys: {}", e))?;
    if keys.is_empty() {
        return Err(miette::miette!("No private key found"));
    }
    let key = PrivateKey(keys[0].clone());

    let config = ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| miette::miette!("Failed to setup TLS config: {}", e))?;

    Ok(config)
}

fn summarize_request(request: &DebugRequest) -> String {
    match request {
        DebugRequest::Authenticate { token } => format!(
            "Authenticate {{ token: <redacted:{} chars> }}",
            token.chars().count()
        ),
        DebugRequest::SetStorage { .. } => "SetStorage { storage_json: <redacted> }".to_string(),
        _ => format!("{request:?}"),
    }
}

async fn setup_signal_handlers(shutdown: Arc<Notify>) {
    #[cfg(unix)]
    let mut ctrl_c = Box::pin(tokio::signal::ctrl_c());
    #[cfg(not(unix))]
    let ctrl_c = tokio::signal::ctrl_c();

    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm = signal(SignalKind::terminate()).expect("Failed to setup SIGTERM handler");

        tokio::select! {
            _ = &mut ctrl_c => {
                info!("Received SIGINT, initiating shutdown");
            }
            _ = sigterm.recv() => {
                info!("Received SIGTERM, initiating shutdown");
            }
        }
    }

    #[cfg(not(unix))]
    {
        let _ = ctrl_c.await;
        info!("Received SIGINT, initiating shutdown");
    }

    shutdown.notify_one();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::protocol::DebugRequest;

    #[test]
    fn request_summary_redacts_auth_token() {
        let summary = summarize_request(&DebugRequest::Authenticate {
            token: "super-secret-token".to_string(),
        });
        assert!(summary.contains("<redacted:18 chars>"));
        assert!(!summary.contains("super-secret-token"));
    }

    #[test]
    fn request_summary_redacts_storage_payloads() {
        let summary = summarize_request(&DebugRequest::SetStorage {
            storage_json: "{\"token\":\"secret\"}".to_string(),
        });
        assert!(summary.contains("<redacted>"));
        assert!(!summary.contains("secret"));
    }

    #[tokio::test]
    async fn test_graceful_shutdown_on_signal() {
        let server = DebugServer::new(
            "127.0.0.1".to_string(),
            None,
            None,
            None,
            None,
            Vec::new(),
            false,
            Vec::new(),
            Vec::new(),
        )
        .expect("Failed to create server");
        let shutdown = server.shutdown.clone();

        let local = tokio::task::LocalSet::new();
        local
            .run_until(async move {
                let server_task = tokio::task::spawn_local(async move {
                    let _ = server.run(0).await;
                });

                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                shutdown.notify_one();

                tokio::time::timeout(tokio::time::Duration::from_secs(5), server_task)
                    .await
                    .expect("Server shutdown timed out")
                    .expect("Server task panicked");
            })
            .await;
    }

    #[test]
    fn test_server_initialization() {
        let server = DebugServer::new(
            "127.0.0.1".to_string(),
            None,
            None,
            None,
            None,
            Vec::new(),
            false,
            Vec::new(),
            Vec::new(),
        )
        .expect("Failed to create server");
        assert_eq!(server.host, "127.0.0.1");
        assert!(server.engine.is_none());
        assert!(server.token.is_none());
        assert!(server.tls_config.is_none());
    }

    #[test]
    fn test_server_with_token() {
        let token = "test-token-12345678".to_string();
        let server = DebugServer::new(
            "127.0.0.1".to_string(),
            Some(token.clone()),
            None,
            None,
            None,
            Vec::new(),
            false,
            Vec::new(),
            Vec::new(),
        )
        .expect("Failed to create server");
        assert_eq!(server.token, Some(token));
    }

    #[test]
    fn test_shutdown_summary_output() {
        let mut server = DebugServer::new(
            "127.0.0.1".to_string(),
            None,
            None,
            None,
            None,
            Vec::new(),
            false,
            Vec::new(),
            Vec::new(),
        )
        .unwrap();

        // Inject fake state to verify no panics during formatting
        server.contract_wasm = Some(vec![0x00, 0x61, 0x73, 0x6d]);
        server.log_shutdown_summary();
    }

    #[test]
    fn test_server_rejects_partial_tls_configuration() {
        let result = DebugServer::new(
            "127.0.0.1".to_string(),
            None,
            Some(Path::new("cert.pem")),
            None,
            None,
            Vec::new(),
            false,
            Vec::new(),
            Vec::new(),
        );
        assert!(
            result.is_err(),
            "Expected partial TLS configuration to fail"
        );
        let err = result
            .err()
            .unwrap_or_else(|| miette::miette!("missing error"));

        assert!(
            err.to_string()
                .contains("TLS requires both certificate and key paths"),
            "unexpected error: {err}"
        );
    }
}
