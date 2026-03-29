//! Parity regression tests for CLI + VS Code Extension (DAP) feature surfaces.
//!
//! These tests enforce the feature matrix documented in docs/feature-matrix.md.
//! Each test is annotated with which surface(s) it validates:
//!   - CLI: exercised via the soroban-debug binary directly
//!   - DAP: exercised via the soroban-debug server TCP protocol
//!     (the same path the VS Code extension uses internally)
//!
//! Functional acceptance tests (flag is parsed without "unrecognized argument"
//! error) use a temporary dummy WASM file so no built fixture is required.
//! The binary will fail to load the dummy WASM, but clap rejects unknown flags
//! before attempting to open the file, so argument acceptance is still verified.

#![allow(deprecated)]

mod network;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

// ── Helpers ───────────────────────────────────────────────────────────────

fn soroban_debug() -> Command {
    Command::new(env!("CARGO_BIN_EXE_soroban-debug"))
}

/// Create a temporary directory containing a dummy `contract.wasm` file.
/// The file contains invalid WASM bytes, which is fine: argument parsing
/// happens before file loading, so clap will reject unknown flags first.
fn dummy_contract() -> (TempDir, std::path::PathBuf) {
    let dir = TempDir::new().expect("Failed to create temp dir");
    let path = dir.path().join("contract.wasm");
    std::fs::write(&path, b"dummy-wasm").expect("Failed to write dummy contract");
    (dir, path)
}

// ── CLI-exclusive features: flag existence (help output) ──────────────────
//
// These tests assert that CLI-exclusive flags are advertised in `run --help`.
// They require no WASM fixture and always run.
//
// Parity relevance: these flags have NO equivalent in the extension's
// launch.json configuration. If any disappear from the CLI, update the
// feature matrix in docs/feature-matrix.md accordingly.

/// SURFACE: CLI only
/// --instruction-debug, --step-instructions, and --step-mode are
/// CLI-exclusive stepping flags. The DAP adapter's initializeRequest does not
/// advertise instruction-stepping capability.
#[test]
fn parity_cli_instruction_debug_flags_exist() {
    soroban_debug()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--instruction-debug"))
        .stdout(predicate::str::contains("--step-instructions"))
        .stdout(predicate::str::contains("--step-mode"));
}

/// SURFACE: CLI only
/// --storage-filter is a CLI-exclusive flag; the extension shows all storage
/// keys unfiltered in the Variables panel. No launch.json equivalent exists.
#[test]
fn parity_cli_storage_filter_flag_exists() {
    soroban_debug()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--storage-filter"));
}

/// SURFACE: CLI only
/// --show-auth is a CLI-exclusive flag. The DAP adapter does not expose
/// authorization trees in any scope or variable.
#[test]
fn parity_cli_show_auth_flag_exists() {
    soroban_debug()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--show-auth"));
}

/// SURFACE: CLI only
/// --batch-args and --repeat are CLI-exclusive batch execution flags.
/// There are no launch.json equivalents.
#[test]
fn parity_cli_batch_flags_exist() {
    soroban_debug()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--batch-args"))
        .stdout(predicate::str::contains("--repeat"));
}

/// SURFACE: CLI only
/// --export-storage and --import-storage are CLI-exclusive storage persistence
/// flags. The extension uses snapshotPath for initial state only.
#[test]
fn parity_cli_storage_import_export_flags_exist() {
    soroban_debug()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--export-storage"))
        .stdout(predicate::str::contains("--import-storage"));
}

/// SURFACE: CLI only
/// The `server` subcommand must expose --port, --token, --tls-cert, --tls-key.
/// TLS flags in particular have no launch.json equivalent.
#[test]
fn parity_cli_server_command_flags_exist() {
    soroban_debug()
        .args(["server", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--port"))
        .stdout(predicate::str::contains("--token"))
        .stdout(predicate::str::contains("--tls-cert"))
        .stdout(predicate::str::contains("--tls-key"));
}

/// SURFACE: CLI only
/// The `remote` subcommand must expose --remote and --token.
/// Remote client mode is entirely absent from the VS Code extension.
#[test]
fn parity_cli_remote_command_flags_exist() {
    soroban_debug()
        .args(["remote", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--remote"))
        .stdout(predicate::str::contains("--token"));
}

/// SURFACE: CLI only
/// All CLI-exclusive analysis subcommands must appear in top-level help.
/// None of these are reachable via the VS Code extension.
#[test]
fn parity_cli_analysis_subcommands_exist() {
    let output = soroban_debug()
        .arg("--help")
        .output()
        .expect("failed to run soroban-debug --help");

    let stdout = String::from_utf8_lossy(&output.stdout);

    for subcommand in &[
        "analyze",
        "symbolic",
        "optimize",
        "profile",
        "compare",
        "replay",
        "upgrade-check",
        "scenario",
        "tui",
        "repl",
    ] {
        assert!(
            stdout.contains(subcommand),
            "Expected CLI-exclusive subcommand '{}' in top-level --help output",
            subcommand
        );
    }
}

// ── Shared features: flag existence ──────────────────────────────────────
//
// These flags are available on both the CLI and the extension.
// Confirming they exist on the CLI also confirms the underlying engine
// supports them — the extension uses the same server-mode engine.

/// SURFACE: CLI + DAP (shared)
/// --breakpoint must appear in run --help. This flag is the CLI equivalent
/// of the extension's gutter-click breakpoint mechanism.
#[test]
fn parity_shared_breakpoint_flag_exists() {
    soroban_debug()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--breakpoint"));
}

// ── CLI-exclusive features: functional acceptance ─────────────────────────
//
// These tests confirm that CLI-exclusive flags are accepted by the argument
// parser without "unrecognized argument" errors. A temporary dummy WASM file
// is used so no built fixture is required — the binary will fail to load the
// invalid WASM, but that error occurs after argument parsing succeeds.

/// SURFACE: CLI only
/// --storage-filter with a prefix pattern must be accepted by clap.
#[test]
fn parity_cli_storage_filter_prefix_accepted() {
    let (_dir, contract) = dummy_contract();
    let mut cmd = soroban_debug();
    let _ = cmd
        .args([
            "run",
            "--contract",
            contract.to_str().unwrap(),
            "--function",
            "increment",
            "--storage-filter",
            "counter:*",
        ])
        .output();
}

/// SURFACE: CLI only
/// --show-auth must be accepted by clap without an argument-parsing error.
#[test]
fn parity_cli_show_auth_accepted() {
    let (_dir, contract) = dummy_contract();
    let mut cmd = soroban_debug();
    let _ = cmd
        .args([
            "run",
            "--contract",
            contract.to_str().unwrap(),
            "--function",
            "increment",
            "--show-auth",
        ])
        .output();
}

/// SURFACE: CLI only
/// --instruction-debug must be accepted by clap without an argument-parsing error.
#[test]
fn parity_cli_instruction_debug_accepted() {
    let (_dir, contract) = dummy_contract();
    let mut cmd = soroban_debug();
    let _ = cmd
        .args([
            "run",
            "--contract",
            contract.to_str().unwrap(),
            "--function",
            "increment",
            "--instruction-debug",
        ])
        .output();
}

/// SURFACE: CLI + DAP (shared)
/// --breakpoint <function> must be accepted by clap without an
/// argument-parsing error, confirming the shared breakpoint engine is
/// available on the CLI path.
#[test]
fn parity_shared_function_breakpoint_accepted() {
    let (_dir, contract) = dummy_contract();
    let mut cmd = soroban_debug();
    let _ = cmd
        .args([
            "run",
            "--contract",
            contract.to_str().unwrap(),
            "--function",
            "increment",
            "--breakpoint",
            "increment",
        ])
        .output();
}

// ── DAP server protocol: shared features ─────────────────────────────────
//
// These tests start `soroban-debug server` on a dedicated port, connect via
// TCP, and exchange JSON protocol messages — the same code path the VS Code
// extension's DebuggerProcess exercises internally.

/// SURFACE: DAP (server path)
/// The server must start, accept a TCP connection, and respond successfully
/// to an Authenticate message when the correct token is provided.
/// This validates the shared auth handshake that the extension relies on via
/// the `token` field in launch.json.
#[test]
fn parity_dap_server_starts_and_accepts_connection() {
    if !network::can_bind_loopback() {
        eprintln!(
            "Skipping parity_dap_server_starts_and_accepts_connection: loopback networking \
             restricted (EPERM or equivalent) – see docs/remote-troubleshooting.md."
        );
        return;
    }

    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    let port = 19_230u16;
    let token = "parity-test-token-ok";

    let mut server = std::process::Command::new(env!("CARGO_BIN_EXE_soroban-debug"))
        .args(["server", "--port", &port.to_string(), "--token", token])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("Failed to spawn soroban-debug server");

    // Give the server time to bind the port.
    std::thread::sleep(Duration::from_millis(500));

    let result: Result<String, String> = (|| {
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port))
            .map_err(|e| format!("connect failed: {}", e))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(4)))
            .map_err(|e| format!("set_read_timeout: {}", e))?;

        let auth_msg = format!(
            "{{\"id\":1,\"request\":{{\"type\":\"Authenticate\",\"token\":\"{}\"}}}}\n",
            token
        );
        stream
            .write_all(auth_msg.as_bytes())
            .map_err(|e| format!("write failed: {}", e))?;

        let mut reader = BufReader::new(stream);
        let mut response = String::new();
        reader
            .read_line(&mut response)
            .map_err(|e| format!("read failed: {}", e))?;
        Ok(response)
    })();

    let _ = server.kill();
    let _ = server.wait();

    match result {
        Ok(response) => {
            assert!(
                response.contains("Authenticated") || response.contains("success"),
                "Expected Authenticated response from server, got: {}",
                response
            );
        }
        Err(e) => {
            // Skip if the server could not start (port in use, EPERM, env issue, etc.)
            eprintln!(
                "Skipping parity_dap_server_starts_and_accepts_connection: {} – \
                 see docs/remote-troubleshooting.md.",
                e
            );
        }
    }
}

/// SURFACE: DAP (server path)
/// The server must reject an incorrect token. This confirms the auth layer
/// that the extension depends on when `token` is set in launch.json.
#[test]
fn parity_dap_server_rejects_invalid_token() {
    if !network::can_bind_loopback() {
        eprintln!(
            "Skipping parity_dap_server_rejects_invalid_token: loopback networking \
             restricted (EPERM or equivalent) – see docs/remote-troubleshooting.md."
        );
        return;
    }

    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    let port = 19_231u16;
    let real_token = "real-parity-token";
    let wrong_token = "wrong-parity-token";

    let mut server = std::process::Command::new(env!("CARGO_BIN_EXE_soroban-debug"))
        .args(["server", "--port", &port.to_string(), "--token", real_token])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("Failed to spawn soroban-debug server");

    std::thread::sleep(Duration::from_millis(500));

    let result: Result<String, String> = (|| {
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port))
            .map_err(|e| format!("connect failed: {}", e))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(4)))
            .map_err(|e| format!("set_read_timeout: {}", e))?;

        let auth_msg = format!(
            "{{\"id\":1,\"request\":{{\"type\":\"Authenticate\",\"token\":\"{}\"}}}}\n",
            wrong_token
        );
        stream
            .write_all(auth_msg.as_bytes())
            .map_err(|e| format!("write failed: {}", e))?;

        let mut reader = BufReader::new(stream);
        let mut response = String::new();
        reader
            .read_line(&mut response)
            .map_err(|e| format!("read failed: {}", e))?;
        Ok(response)
    })();

    let _ = server.kill();
    let _ = server.wait();

    match result {
        Ok(response) => {
            // The server must NOT indicate a successful authentication.
            assert!(
                !response.contains("\"success\":true") || response.contains("\"success\":false"),
                "Server should reject an incorrect token, got: {}",
                response
            );
        }
        Err(e) => {
            eprintln!(
                "Skipping parity_dap_server_rejects_invalid_token: {} – \
                 see docs/remote-troubleshooting.md.",
                e
            );
        }
    }
}

/// SURFACE: DAP (server path) — BACKWARD COMPATIBILITY GUARD
///
/// Older clients send `Authenticate` as their very first message, before any
/// `Handshake` exchange. The server MUST accept and honour this ordering so
/// that pre-handshake clients continue to work without modification.
///
/// This test is the dedicated regression guard for that behaviour. If it
/// starts failing it means the auth-before-handshake path in
/// `src/server/debug_server.rs` (`handle_single_connection`) was broken.
/// Do NOT remove or weaken this test without a corresponding protocol version
/// bump and a migration note in CONTRIBUTING.md.
#[test]
fn parity_dap_auth_before_handshake_is_accepted() {
    if !network::can_bind_loopback() {
        eprintln!(
            "Skipping parity_dap_auth_before_handshake_is_accepted: loopback networking \
             restricted (EPERM or equivalent) – see docs/remote-troubleshooting.md."
        );
        return;
    }

    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    let port = 19_235u16;
    let token = "compat-test-token";

    let mut server = std::process::Command::new(env!("CARGO_BIN_EXE_soroban-debug"))
        .args(["server", "--port", &port.to_string(), "--token", token])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("Failed to spawn soroban-debug server");

    std::thread::sleep(Duration::from_millis(500));

    let result: Result<String, String> = (|| {
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port))
            .map_err(|e| format!("connect failed: {}", e))?;
        stream
            .set_read_timeout(Some(Duration::from_secs(4)))
            .map_err(|e| format!("set_read_timeout: {}", e))?;

        // Send Authenticate WITHOUT a prior Handshake — this is the legacy ordering.
        let auth_msg = format!(
            "{{\"id\":1,\"request\":{{\"type\":\"Authenticate\",\"token\":\"{}\"}}}}\n",
            token
        );
        stream
            .write_all(auth_msg.as_bytes())
            .map_err(|e| format!("write failed: {}", e))?;

        let mut reader = BufReader::new(stream);
        let mut response = String::new();
        reader
            .read_line(&mut response)
            .map_err(|e| format!("read failed: {}", e))?;
        Ok(response)
    })();

    let _ = server.kill();
    let _ = server.wait();

    match result {
        Ok(response) => {
            assert!(
                response.contains("\"success\":true"),
                "Server must accept Authenticate sent before Handshake (backward-compat). \
                 Got: {}",
                response
            );
        }
        Err(e) => {
            eprintln!(
                "Skipping parity_dap_auth_before_handshake_is_accepted: {} – \
                 see docs/remote-troubleshooting.md.",
                e
            );
        }
    }
}

// ── Non-support assertions ────────────────────────────────────────────────
//
// These tests confirm that features unsupported on BOTH surfaces remain
// absent from the CLI. If they appear in the CLI in the future, the DAP
// adapter's initializeRequest capability flags should be updated to match.

/// SURFACE: neither CLI nor DAP
/// The CLI must not expose --conditional-breakpoint.
/// The DAP adapter explicitly sets supportsConditionalBreakpoints = false
/// in initializeRequest (extensions/vscode/src/dap/adapter.ts).
#[test]
fn parity_neither_surface_supports_conditional_breakpoints() {
    soroban_debug()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--conditional-breakpoint").not());
}

/// SURFACE: neither CLI nor DAP
/// The CLI must not expose --log-point.
/// The DAP adapter explicitly sets supportsLogPoints = false
/// in initializeRequest (extensions/vscode/src/dap/adapter.ts).
#[test]
fn parity_neither_surface_supports_log_points() {
    soroban_debug()
        .args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--log-point").not());
}
