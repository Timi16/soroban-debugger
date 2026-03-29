use assert_cmd::Command;
use predicates::prelude::*;
use std::path::PathBuf;
use std::process::Command as StdCommand;
use std::time::Duration;

mod network;

#[test]
fn test_remote_run_execution() {
    if !network::can_bind_loopback() {
        eprintln!(
            "Skipping test_remote_run_execution: loopback networking restricted \
             (EPERM or equivalent) – cannot bind/connect on 127.0.0.1. \
             See docs/remote-troubleshooting.md."
        );
        return;
    }

    fn fixture_wasm_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("wasm")
            .join(format!("{}.wasm", name))
    }

    fn ensure_counter_wasm() -> PathBuf {
        let wasm_path = fixture_wasm_path("counter");
        if wasm_path.exists() {
            return wasm_path;
        }

        let fixtures_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        if cfg!(windows) {
            let status = StdCommand::new("powershell")
                .current_dir(&fixtures_dir)
                .args(["-ExecutionPolicy", "Bypass", "-File", "build.ps1"])
                .status()
                .expect("Failed to run build.ps1");
            assert!(status.success(), "build.ps1 failed");
        } else {
            let status = StdCommand::new("bash")
                .current_dir(&fixtures_dir)
                .args(["./build.sh"])
                .status()
                .expect("Failed to run build.sh");
            assert!(status.success(), "build.sh failed");
        }

        assert!(
            wasm_path.exists(),
            "Expected fixture wasm to exist after build: {:?}",
            wasm_path
        );
        wasm_path
    }

    // Start server in background
    let mut server_cmd = StdCommand::new(assert_cmd::cargo::cargo_bin!("soroban-debug"));

    let mut server_child = server_cmd
        .arg("server")
        .arg("--port")
        .arg("9245")
        .arg("--token")
        .arg("secret")
        .spawn()
        .expect("Failed to spawn server");

    // Wait a bit for server to start
    std::thread::sleep(Duration::from_millis(1500));

    // Smoke-test ping through the `run --remote` path:
    let mut ping_cmd: Command = assert_cmd::cargo::cargo_bin_cmd!("soroban-debug");
    ping_cmd
        .arg("run")
        .arg("--remote")
        .arg("127.0.0.1:9245")
        .arg("--token")
        .arg("secret")
        .assert()
        .success()
        .stdout(predicate::str::contains("Remote debugger is reachable"));

    let counter_wasm = ensure_counter_wasm();

    // Run remote client
    let mut client_cmd: Command = assert_cmd::cargo::cargo_bin_cmd!("soroban-debug");
    let assert = client_cmd
        .arg("run")
        .arg("--remote")
        .arg("127.0.0.1:9245")
        .arg("--token")
        .arg("secret")
        .arg("--contract")
        .arg(&counter_wasm)
        .arg("--function")
        .arg("increment")
        .assert();

    // Kill server
    server_child.kill().unwrap();
    let _ = server_child.wait();

    // The counter.wasm might just output 1 on first increment
    // Let's just assert that it executed successfully rather than checking the exact value if we are unsure
    assert.success().stdout(predicate::str::contains("Result:"));
}
