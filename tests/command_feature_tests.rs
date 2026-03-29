use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::NamedTempFile;

#[path = "fixtures/mod.rs"]
mod fixtures;

fn fixture_wasm(name: &str) -> std::path::PathBuf {
    fixtures::get_fixture_path(name)
}

fn base_cmd() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_soroban-debug"));
    cmd.env("NO_COLOR", "1");
    cmd.env("NO_BANNER", "1");
    cmd
}

#[test]
fn symbolic_runs_against_counter_fixture() {
    let wasm = fixture_wasm("counter");

    base_cmd()
        .args([
            "symbolic",
            "--contract",
            wasm.to_str().unwrap(),
            "--function",
            "increment",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Function: increment"))
        .stdout(predicate::str::contains("Paths explored:"))
        .stdout(predicate::str::contains("Truncation:"));
}

#[test]
fn symbolic_writes_scenario_toml() {
    let wasm = fixture_wasm("counter");
    let output = NamedTempFile::new().unwrap();

    base_cmd()
        .args([
            "symbolic",
            "--contract",
            wasm.to_str().unwrap(),
            "--function",
            "increment",
            "--output",
            output.path().to_str().unwrap(),
        ])
        .assert()
        .success();

    let written = fs::read_to_string(output.path()).unwrap();
    assert!(written.contains("[metadata]"));
    assert!(written.contains("[[scenario]]"));
    assert!(written.contains("function = \"increment\""));
}

#[test]
fn symbolic_cli_honors_caps_and_reports_truncation() {
    let wasm = fixture_wasm("budget_heavy");

    base_cmd()
        .args([
            "symbolic",
            "--contract",
            wasm.to_str().unwrap(),
            "--function",
            "heavy",
            "--profile",
            "fast",
            "--input-combination-cap",
            "4",
            "--path-cap",
            "2",
            "--max-breadth",
            "10",
            "--timeout",
            "30",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Truncation:"))
        .stdout(predicate::str::contains("input combination cap reached"))
        .stdout(predicate::str::contains("path exploration cap reached"));
}

#[test]
fn symbolic_json_outputs_path_decisions() {
    let wasm = fixture_wasm("counter");

    base_cmd()
        .args([
            "symbolic",
            "--contract",
            wasm.to_str().unwrap(),
            "--function",
            "increment",
            "--format",
            "json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\": \"success\""))
        .stdout(predicate::str::contains("\"kind\": \"StorageWrite\""))
        .stdout(predicate::str::contains("\"kind\": \"StorageRead\""))
        .stdout(predicate::str::contains("\"path_decisions\": ["));
}

#[test]
fn analyze_json_outputs_findings_array() {
    let wasm = fixture_wasm("counter");

    base_cmd()
        .args([
            "analyze",
            "--contract",
            wasm.to_str().unwrap(),
            "--format",
            "json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"findings\""));
}

#[test]
fn analyze_filters_by_severity_and_rule() {
    let wasm = fixture_wasm("counter");

    base_cmd()
        .args([
            "analyze",
            "--contract",
            wasm.to_str().unwrap(),
            "--format",
            "text",
            "--disable-rule",
            "hardcoded-address",
            "--min-severity",
            "high",
        ])
        .assert()
        .success()
        // If there are no high severity findings (or if hardcoded-address is the only one),
        // we should either see specific output or just "No security findings".
        // It's a smoke test to ensure args parse and run without panicking.
        .stdout(
            predicate::str::contains("Findings")
                .or(predicate::str::contains("No security findings")),
        );
}

#[test]
fn analyze_dynamic_execution_reports_function_metadata() {
    let wasm = fixture_wasm("counter");

    base_cmd()
        .args([
            "analyze",
            "--contract",
            wasm.to_str().unwrap(),
            "--function",
            "increment",
            "--args",
            "[]",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Dynamic analysis function: increment",
        ));
}

#[test]
fn scenario_runs_counter_steps() {
    let wasm = fixture_wasm("counter");
    let scenario = NamedTempFile::new().unwrap();
    fs::write(
        scenario.path(),
        r#"
[[steps]]
name = "Increment"
function = "increment"
args = "[]"
expected_return = "I64(1)"

[[steps]]
name = "Read Counter"
function = "get"
expected_return = "I64(1)"
"#,
    )
    .unwrap();

    base_cmd()
        .args([
            "scenario",
            "--scenario",
            scenario.path().to_str().unwrap(),
            "--contract",
            wasm.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "All scenario steps passed successfully!",
        ));
}

#[test]
fn scenario_accepts_timeout_defaults_and_step_overrides() {
    let wasm = fixture_wasm("counter");
    let scenario = NamedTempFile::new().unwrap();
    fs::write(
        scenario.path(),
        r#"
[defaults]
timeout_secs = 15

[[steps]]
name = "Increment"
function = "increment"
args = "[]"
timeout_secs = 0
expected_return = "I64(1)"

[[steps]]
name = "Read Counter"
function = "get"
expected_return = "I64(1)"
"#,
    )
    .unwrap();

    base_cmd()
        .args([
            "scenario",
            "--scenario",
            scenario.path().to_str().unwrap(),
            "--contract",
            wasm.to_str().unwrap(),
            "--timeout",
            "30",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "All scenario steps passed successfully!",
        ));
}

#[test]
fn scenario_passes_when_no_events_are_expected() {
    let wasm = fixture_wasm("counter");
    let scenario = NamedTempFile::new().unwrap();
    fs::write(
        scenario.path(),
        r#"
[[steps]]
name = "Increment"
function = "increment"
args = "[]"
expected_return = "I64(1)"
expected_events = []
"#,
    )
    .unwrap();

    base_cmd()
        .args([
            "scenario",
            "--scenario",
            scenario.path().to_str().unwrap(),
            "--contract",
            wasm.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Event assertion passed"));
}

#[test]
fn scenario_passes_when_budget_limits_are_within_range() {
    let wasm = fixture_wasm("counter");
    let scenario = NamedTempFile::new().unwrap();
    fs::write(
        scenario.path(),
        r#"
[[steps]]
name = "Increment"
function = "increment"
args = "[]"

[steps.budget_limits]
max_cpu_instructions = 10000000
"#,
    )
    .unwrap();

    base_cmd()
        .args([
            "scenario",
            "--scenario",
            scenario.path().to_str().unwrap(),
            "--contract",
            wasm.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("CPU budget assertion passed"));
}

#[test]
fn scenario_fails_when_unexpected_events_are_asserted() {
    let wasm = fixture_wasm("counter");
    let scenario = NamedTempFile::new().unwrap();
    fs::write(
        scenario.path(),
        r#"
[[steps]]
name = "Increment"
function = "increment"
args = "[]"

[[steps.expected_events]]
contract_id = ""
topics = ["topic"]
data = "payload"
"#,
    )
    .unwrap();

    base_cmd()
        .args([
            "scenario",
            "--scenario",
            scenario.path().to_str().unwrap(),
            "--contract",
            wasm.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("Event assertion failed"));
}

#[test]
fn scenario_fails_when_budget_limits_are_exceeded() {
    let wasm = fixture_wasm("counter");
    let scenario = NamedTempFile::new().unwrap();
    fs::write(
        scenario.path(),
        r#"
[[steps]]
name = "Increment"
function = "increment"
args = "[]"

[steps.budget_limits]
max_cpu_instructions = 0
"#,
    )
    .unwrap();

    base_cmd()
        .args([
            "scenario",
            "--scenario",
            scenario.path().to_str().unwrap(),
            "--contract",
            wasm.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("CPU budget assertion failed"));
}

#[test]
fn scenario_passes_when_expected_error_matches() {
    let wasm = fixture_wasm("counter");
    let scenario = NamedTempFile::new().unwrap();
    // Assuming `decrement` isn't a valid function and will fail
    fs::write(
        scenario.path(),
        r#"
[[steps]]
name = "Should fail and match"
function = "decrement"
expected_error = "Invalid function name"
"#,
    )
    .unwrap();

    base_cmd()
        .args([
            "scenario",
            "--scenario",
            scenario.path().to_str().unwrap(),
            "--contract",
            wasm.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Expected error assertion passed"));
}

#[test]
fn scenario_fails_when_expected_error_mismatches() {
    let wasm = fixture_wasm("counter");
    let scenario = NamedTempFile::new().unwrap();
    fs::write(
        scenario.path(),
        r#"
[[steps]]
name = "Should error with wrong message"
function = "decrement"
expected_error = "Totally different error"
"#,
    )
    .unwrap();

    base_cmd()
        .args([
            "scenario",
            "--scenario",
            scenario.path().to_str().unwrap(),
            "--contract",
            wasm.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains(
            "Expected error 'Totally different error', but got",
        ));
}

#[test]
fn scenario_fails_when_expected_to_fail_but_succeeds() {
    let wasm = fixture_wasm("counter");
    let scenario = NamedTempFile::new().unwrap();
    fs::write(
        scenario.path(),
        r#"
[[steps]]
name = "Should fail but succeeds"
function = "increment"
expected_error = "unauthorized"
"#,
    )
    .unwrap();

    base_cmd()
        .args([
            "scenario",
            "--scenario",
            scenario.path().to_str().unwrap(),
            "--contract",
            wasm.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stdout(predicate::str::contains("Step succeeded with"));
}

#[test]
fn symbolic_seed_flag_prints_replay_token() {
    let wasm = fixture_wasm("counter");

    base_cmd()
        .args([
            "symbolic",
            "--contract",
            wasm.to_str().unwrap(),
            "--function",
            "increment",
            "--seed",
            "42",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Replay token: 42"));
}

#[test]
fn symbolic_replay_flag_is_equivalent_to_seed() {
    let wasm = fixture_wasm("counter");

    base_cmd()
        .args([
            "symbolic",
            "--contract",
            wasm.to_str().unwrap(),
            "--function",
            "increment",
            "--replay",
            "42",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Replay token: 42"));
}

#[test]
fn symbolic_seed_and_replay_are_mutually_exclusive() {
    let wasm = fixture_wasm("counter");

    base_cmd()
        .args([
            "symbolic",
            "--contract",
            wasm.to_str().unwrap(),
            "--function",
            "increment",
            "--seed",
            "1",
            "--replay",
            "2",
        ])
        .assert()
        .failure();
}

#[test]
fn symbolic_without_seed_prints_replay_token_none() {
    let wasm = fixture_wasm("counter");

    base_cmd()
        .args([
            "symbolic",
            "--contract",
            wasm.to_str().unwrap(),
            "--function",
            "increment",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Replay token: none"));
}

#[test]
fn scenario_captures_step_output_and_uses_in_expected_return() {
    let wasm = fixture_wasm("counter");
    let scenario = NamedTempFile::new().unwrap();
    fs::write(
        scenario.path(),
        r#"
[[steps]]
name = "Increment"
function = "increment"
args = "[]"
capture = "count"

[[steps]]
name = "Verify Get matches captured value"
function = "get"
expected_return = "{{count}}"
"#,
    )
    .unwrap();

    base_cmd()
        .args([
            "scenario",
            "--scenario",
            scenario.path().to_str().unwrap(),
            "--contract",
            wasm.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Captured return value as 'count'"))
        .stdout(predicate::str::contains(
            "All scenario steps passed successfully!",
        ));
}

#[test]
fn scenario_fails_on_undefined_variable_in_args() {
    let wasm = fixture_wasm("counter");
    let scenario = NamedTempFile::new().unwrap();
    fs::write(
        scenario.path(),
        r#"
[[steps]]
name = "Reference undefined variable"
function = "increment"
args = "[{{undefined_var}}]"
"#,
    )
    .unwrap();

    base_cmd()
        .args([
            "scenario",
            "--scenario",
            scenario.path().to_str().unwrap(),
            "--contract",
            wasm.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("undefined_var"));
}

#[test]
fn repl_accepts_commands_and_exits() {
    let wasm = fixture_wasm("counter");
    let output = Command::new(env!("CARGO_BIN_EXE_soroban-debug"))
        .env("NO_COLOR", "1")
        .args(["repl", "--contract", wasm.to_str().unwrap()])
        .write_stdin("help\ncall increment\nexit\n")
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    println!("COMBINED_OUTPUT: {}", combined);
    assert!(combined.contains("Available Commands:"));
}

#[test]
fn repl_seeds_initial_storage() {
    let wasm = fixture_wasm("counter");
    let output = Command::new(env!("CARGO_BIN_EXE_soroban-debug"))
        .env("NO_COLOR", "1")
        .args([
            "repl",
            "--contract",
            wasm.to_str().unwrap(),
            "--storage",
            r#"{"c": 42}"#,
        ])
        .write_stdin("call get\nexit\n")
        .output()
        .unwrap();

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        combined.contains("Result: I64(42)"),
        "Storage was not seeded correctly in REPL\n{}",
        combined
    );
}

#[test]
fn repl_supports_conditional_breakpoints() {
    let wasm = fixture_wasm("counter");
    let output = Command::new(env!("CARGO_BIN_EXE_soroban-debug"))
        .env("NO_COLOR", "1")
        .env("RUST_LOG", "info")
        .args(["repl", "--contract", wasm.to_str().unwrap()])
        .write_stdin("break increment step_count > 0\ncall increment\ncall increment\nexit\n")
        .output()
        .unwrap();

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(
        combined.contains("Breakpoint set") && combined.contains("increment"),
        "Breakpoint was not set correctly in REPL\n{}",
        combined
    );

    assert!(
        combined.contains("Execution paused") && combined.contains("increment"),
        "Conditional breakpoint was not hit in REPL\n{}",
        combined
    );
}
