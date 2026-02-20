use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn test_help_command() {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_soroban-debug"));
    cmd.arg("--help");
    cmd.assert().success();
}

#[test]
fn test_version_command() {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_soroban-debug"));
    cmd.arg("--version");
    cmd.assert().success();
}

#[test]
fn test_upgrade_check_help_command() {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_soroban-debug"));
    cmd.arg("upgrade-check").arg("--help");
    cmd.assert().success();
}

#[test]
fn test_run_command_event_flags() {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_soroban-debug"));
    cmd.arg("run")
        .arg("--contract")
        .arg("test.wasm") // File doesn't need to exist for argument parsing check if we just check help or similar, but here we might fail on file read.
        .arg("--show-events")
        .arg("--filter-topic")
        .arg("my-topic");

    // We expect failure because test.wasm doesn't exist, but valid flags should be parsed.
    // Actually, clap fails if required args are missing.
    // Let's just check help to see if flags are listed.
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_soroban-debug"));
    cmd.arg("run").arg("--help");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("--show-events"))
        .stdout(predicate::str::contains("--filter-topic"));
}

#[test]
fn test_run_command_repeat_flag() {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_soroban-debug"));
    cmd.arg("run").arg("--help");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("--repeat"))
        .stdout(predicate::str::contains("stress testing"));
}

#[test]
fn test_compare_help_command() {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_soroban-debug"));
    cmd.arg("compare").arg("--help");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("TRACE_A"))
        .stdout(predicate::str::contains("TRACE_B"))
        .stdout(predicate::str::contains("--output"));
}
