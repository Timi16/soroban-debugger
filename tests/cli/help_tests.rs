#![allow(deprecated)]
/// Integration tests for CLI help and version commands
///
/// Tests the help output, version information, and general command structure
/// without requiring WASM files or contract execution.
#[allow(unused_imports)]
use assert_cmd::prelude::*;
use predicates::prelude::*;

#[test]
fn test_help_flag() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("soroban-debug"))
        .stdout(predicate::str::contains("Usage:"));
}

#[test]
fn test_help_subcommand() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.args(["help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("soroban-debug"));
}

#[test]
fn test_help_with_subcommand() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.args(["run", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Run a contract function"));
}

#[test]
fn test_inspect_help() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.args(["inspect", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Inspect contract information"));
}

#[test]
fn test_optimize_help() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.args(["optimize", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Analyze contract"));
}

#[test]
fn test_profile_help() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.args(["profile", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Profile a single function"));
}

#[test]
fn test_compare_help() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.args(["compare", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Compare two execution"));
}

#[test]
fn test_upgrade_check_help() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.args(["upgrade-check", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Check compatibility"));
}

#[test]
fn test_version_flag() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("soroban-debug"))
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn test_unknown_subcommand() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.args(["nonexistent", "--help"])
        .assert()
        .failure()
        .stderr(
            predicate::str::contains("unknown subcommand")
                .or(predicate::str::contains("no such subcommand"))
                .or(predicate::str::contains("unrecognized subcommand")),
        );
}

#[test]
fn test_run_requires_contract() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.args(["run", "--function", "test_func"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn test_run_requires_function() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.arg("run")
        .arg("--contract")
        .arg("dummy.wasm")
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn test_inspect_requires_contract() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.arg("inspect")
        .assert()
        .failure()
        .stderr(predicate::str::contains("required").or(predicate::str::contains("error")));
}

#[test]
fn test_inspect_help_shows_options() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.args(["inspect", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--functions"))
        .stdout(predicate::str::contains("--metadata"));
}

#[test]
fn test_completions_help() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.args(["completions", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("shell"));
}

#[test]
fn test_verbose_flag_accepted() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    // Just test that verbose flag is accepted in help context
    cmd.args(["--verbose", "--help"]).assert().success();
}

#[test]
fn test_quiet_flag_accepted() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    // Just test that quiet flag is accepted in help context
    cmd.args(["--quiet", "--help"]).assert().success();
}

#[test]
fn test_global_flags_before_subcommand() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.args(["--verbose", "run", "--help"]).assert().success();
}

#[test]
fn test_global_flags_after_subcommand() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.args(["run", "--verbose", "--help"]).assert().success();
}

#[test]
fn test_compare_requires_both_files() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.args(["compare"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn test_upgrade_check_requires_files() {
    let mut cmd = assert_cmd::Command::cargo_bin("soroban-debug").expect("Failed to find binary");
    cmd.args(["upgrade-check"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}
