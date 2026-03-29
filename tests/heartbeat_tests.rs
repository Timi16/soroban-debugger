use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::Duration;

// Note: Requires the soroban-debug binary to be built (cargo build --bins)

fn get_free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn spawn_server(port: u16, token: &str) -> std::process::Child {
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_soroban-debug"))
        .args(["server", "--port", &port.to_string(), "--token", token])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to spawn soroban-debug server");
    
    // Brief wait to see if it crashes immediately
    std::thread::sleep(Duration::from_millis(200));
    if let Ok(Some(status)) = child.try_wait() {
        let mut stderr = String::new();
        if let Some(mut err_pipe) = child.stderr.take() {
            let _ = std::io::Read::read_to_string(&mut err_pipe, &mut stderr);
        }
        panic!("Server exited immediately with status {:?}. Stderr: {}", status, stderr);
    }
    child
}

fn connect_with_retry(port: u16) -> Result<TcpStream, std::io::Error> {
    let addr = format!("127.0.0.1:{}", port);
    let mut last_err = None;
    for _ in 0..10 {
        match TcpStream::connect(&addr) {
            Ok(stream) => {
                stream.set_read_timeout(Some(Duration::from_secs(2)))?;
                return Ok(stream);
            }
            Err(e) => {
                last_err = Some(e);
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
    Err(last_err.unwrap_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "Failed to connect")))
}

#[test]
fn test_heartbeat_negotiation() {
    let port = get_free_port();
    let token = "test-token";
    let mut server = spawn_server(port, token);

    let result: std::result::Result<(), Box<dyn std::error::Error>> = (|| {
        let mut stream = connect_with_retry(port)?;

        // 1. Handshake with heartbeat/timeout request
        let handshake = format!(
            "{{\"id\":1,\"request\":{{\"type\":\"Handshake\",\"client_name\":\"test\",\"client_version\":\"1.0\",\"protocol_min\":1,\"protocol_max\":1,\"heartbeat_interval_ms\":100,\"idle_timeout_ms\":500}}}}\n"
        );
        stream.write_all(handshake.as_bytes())?;

        let mut reader = BufReader::new(stream);
        let mut response = String::new();
        reader.read_line(&mut response)?;

        assert!(response.contains("HandshakeAck"), "Expected HandshakeAck, got: {}", response);
        assert!(response.contains("\"heartbeat_interval_ms\":100"));
        assert!(response.contains("\"idle_timeout_ms\":500"));

        Ok(())
    })();

    let _ = server.kill();
    let _ = server.wait();
    result.expect("Test failed");
}

#[test]
fn test_server_sends_heartbeats() {
    let port = get_free_port();
    let token = "test-token";
    let mut server = spawn_server(port, token);

    let result: std::result::Result<(), Box<dyn std::error::Error>> = (|| {
        let mut stream = connect_with_retry(port)?;

        // 1. Handshake with short heartbeat interval
        let handshake = format!(
            "{{\"id\":1,\"request\":{{\"type\":\"Handshake\",\"client_name\":\"test\",\"client_version\":\"1.0\",\"protocol_min\":1,\"protocol_max\":1,\"heartbeat_interval_ms\":200}}}}\n"
        );
        stream.write_all(handshake.as_bytes())?;

        let mut reader = BufReader::new(stream);
        let mut response = String::new();
        reader.read_line(&mut response)?; // consume HandshakeAck

        // 2. Wait for heartbeat from server
        response.clear();
        reader.read_line(&mut response)?;
        assert!(response.contains("\"type\":\"Ping\""), "Expected Ping (heartbeat) from server, got: {}", response);

        // 3. Respond with Pong
        let pong = "{\"id\":0,\"response\":{\"type\":\"Pong\"}}\n";
        reader.get_mut().write_all(pong.as_bytes())?;

        // 4. Wait for another heartbeat
        response.clear();
        reader.read_line(&mut response)?;
        assert!(response.contains("\"type\":\"Ping\""), "Expected second Ping from server, got: {}", response);

        Ok(())
    })();

    let _ = server.kill();
    let _ = server.wait();
    result.expect("Test failed");
}

#[test]
fn test_idle_timeout_disconnects_client() {
    let port = get_free_port();
    let token = "test-token";
    let mut server = spawn_server(port, token);

    let result: std::result::Result<(), Box<dyn std::error::Error>> = (|| {
        let mut stream = connect_with_retry(port)?;

        // 1. Handshake with short idle timeout
        let handshake = format!(
            "{{\"id\":1,\"request\":{{\"type\":\"Handshake\",\"client_name\":\"test\",\"client_version\":\"1.0\",\"protocol_min\":1,\"protocol_max\":1,\"idle_timeout_ms\":300}}}}\n"
        );
        stream.write_all(handshake.as_bytes())?;

        let mut reader = BufReader::new(stream);
        let mut response = String::new();
        reader.read_line(&mut response)?; // consume HandshakeAck

        // 2. Wait for timeout and Disconnected message
        response.clear();
        reader.read_line(&mut response)?;
        assert!(response.contains("Disconnected"), "Expected Disconnected message due to idle timeout, got: {}", response);

        // 3. Verify connection is closed
        response.clear();
        match reader.read_line(&mut response) {
            Ok(0) => {}, // Graceful EOF
            Err(_) => {}, // Connection reset or other error after DISCONNECT
            Ok(n) => panic!("Expected connection closure, but got {} bytes: {}", n, response),
        }

        Ok(())
    })();

    let _ = server.kill();
    let _ = server.wait();
    result.expect("Test failed");
}
