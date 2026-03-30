#![allow(dead_code)]

use soroban_debugger::server::DebugServer;
use std::path::Path;

#[test]
fn test_server_creation_without_token() {
    let server = DebugServer::new(None, None, None, None, vec![]);
    assert!(server.is_ok(), "Server should be creatable without token");
}

#[test]
fn test_server_creation_with_token() {
    let token = "valid-test-token-1234567890".to_string();
    let server = DebugServer::new(Some(token.clone()), None, None, None, vec![])
        .expect("Failed to create server with token");

    let _ = server;
}

#[test]
fn test_server_rejects_tls_configuration() {
    let fake_cert = Path::new("tests/fixtures/cert.pem");
    match DebugServer::new(None, Some(fake_cert), None, None, vec![]) {
        Ok(_) => panic!("expected TLS unsupported error"),
        Err(err) => {
            assert!(
                err.to_string().contains("TLS not supported"),
                "expected TLS unsupported error"
            );
        }
    }
}
