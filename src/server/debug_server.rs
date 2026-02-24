use crate::debugger::engine::DebuggerEngine;
use crate::protocol::{DebugRequest, DebugResponse};
use crate::Result;
use std::fs;
use std::io::BufReader;
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::rustls::{Certificate, PrivateKey, ServerConfig};
use tokio_rustls::TlsAcceptor;
use tracing::{error, info, warn};

pub struct DebugServer {
    engine: DebuggerEngine,
    token: String,
    tls_config: Option<ServerConfig>,
}

impl DebugServer {
    pub fn new(
        engine: DebuggerEngine,
        token: String,
        cert_path: Option<&Path>,
        key_path: Option<&Path>,
    ) -> Result<Self> {
        let tls_config = if let (Some(cp), Some(kp)) = (cert_path, key_path) {
            Some(load_tls_config(cp, kp)?)
        } else {
            None
        };

        Ok(Self {
            engine,
            token,
            tls_config,
        })
    }

    pub async fn run(mut self, port: u16) -> Result<()> {
        let addr = format!("0.0.0.0:{}", port);
        let listener = TcpListener::bind(&addr)
            .await
            .map_err(|e| miette::miette!("Failed to bind to {}: {}", addr, e))?;
        info!("Debug server listening on {}", addr);

        let acceptor = self
            .tls_config
            .take()
            .map(|cfg| TlsAcceptor::from(Arc::new(cfg)));

        loop {
            let (stream, addr) = listener
                .accept()
                .await
                .map_err(|e| miette::miette!("Failed to accept connection: {}", e))?;
            info!("New connection from {}", addr);

            if let Some(ref acceptor) = acceptor {
                match acceptor.accept(stream).await {
                    Ok(tls_stream) => {
                        if let Err(e) = self.handle_single_connection(tls_stream).await {
                            error!("TLS connection error: {}", e);
                        }
                    }
                    Err(e) => error!("TLS accept error: {}", e),
                }
            } else if let Err(e) = self.handle_single_connection(stream).await {
                error!("TCP connection error: {}", e);
            }
        }
    }

    async fn handle_single_connection<S>(&mut self, mut stream: S) -> Result<()>
    where
        S: AsyncReadExt + AsyncWriteExt + Unpin,
    {
        let mut authenticated = false;
        let mut buffer = vec![0u8; 8192];

        loop {
            let n = stream
                .read(&mut buffer)
                .await
                .map_err(|e| miette::miette!("Failed to read from stream: {}", e))?;
            if n == 0 {
                break;
            }

            let request: DebugRequest = match serde_json::from_slice(&buffer[..n]) {
                Ok(req) => req,
                Err(e) => {
                    warn!("Failed to parse request: {}", e);
                    continue;
                }
            };
            info!("Received request: {:?}", request);

            if !authenticated {
                if let DebugRequest::Handshake {
                    token: ref req_token,
                } = request
                {
                    if req_token == &self.token {
                        authenticated = true;
                        send_response(&mut stream, DebugResponse::AuthSuccess).await?;
                        continue;
                    } else {
                        send_response(&mut stream, DebugResponse::AuthFailed).await?;
                        return Ok(());
                    }
                } else {
                    send_response(
                        &mut stream,
                        DebugResponse::Error("Authentication required".to_string()),
                    )
                    .await?;
                    return Ok(());
                }
            }

            let response = match request {
                DebugRequest::Handshake { .. } => {
                    DebugResponse::Error("Already authenticated".to_string())
                }
                DebugRequest::Step => match self.engine.step() {
                    Ok(_) => DebugResponse::Ok,
                    Err(e) => DebugResponse::Error(e.to_string()),
                },
                DebugRequest::Continue => match self.engine.continue_execution() {
                    Ok(_) => DebugResponse::Ok,
                    Err(e) => DebugResponse::Error(e.to_string()),
                },
                DebugRequest::AddBreakpoint { function } => {
                    self.engine.breakpoints_mut().add(&function);
                    DebugResponse::Ok
                }
                DebugRequest::RemoveBreakpoint { function } => {
                    self.engine.breakpoints_mut().remove(&function);
                    DebugResponse::Ok
                }
                DebugRequest::GetState => {
                    let state = self.engine.state().lock().unwrap().clone();
                    DebugResponse::State(state)
                }
                DebugRequest::Execute { function, args } => {
                    match self.engine.execute(&function, args.as_deref()) {
                        Ok(res) => DebugResponse::ExecutionResult { result: res },
                        Err(e) => DebugResponse::Error(e.to_string()),
                    }
                }
            };

            send_response(&mut stream, response).await?;
        }

        Ok(())
    }
}

async fn send_response<S>(stream: &mut S, response: DebugResponse) -> Result<()>
where
    S: AsyncWriteExt + Unpin,
{
    let json = serde_json::to_vec(&response)
        .map_err(|e| miette::miette!("Failed to serialize response: {}", e))?;
    stream
        .write_all(&json)
        .await
        .map_err(|e| miette::miette!("Failed to write response: {}", e))?;
    Ok(())
}

fn load_tls_config(cert_path: &Path, key_path: &Path) -> Result<ServerConfig> {
    let cert_file = fs::File::open(cert_path)
        .map_err(|e| miette::miette!("Failed to open cert file {:?}: {}", cert_path, e))?;
    let mut cert_reader = BufReader::new(cert_file);
    let certs = rustls_pemfile::certs(&mut cert_reader)
        .map_err(|e| miette::miette!("Failed to read certs: {}", e))?
        .into_iter()
        .map(Certificate)
        .collect();

    let key_file = fs::File::open(key_path)
        .map_err(|e| miette::miette!("Failed to open key file {:?}: {}", key_path, e))?;
    let mut key_reader = BufReader::new(key_file);
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
