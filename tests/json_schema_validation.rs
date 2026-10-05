use assert_cmd::Command;
use jsonschema::JSONSchema;
use serde_json::Value;
use std::fs;
use tempfile::TempDir;

fn compile_schema(path: &str) -> JSONSchema {
    let schema_content = fs::read_to_string(path).expect("Failed to read schema file");
    let schema_json: Value =
        serde_json::from_str(&schema_content).expect("Failed to parse schema JSON");
    JSONSchema::compile(&schema_json).expect("Failed to compile schema")
}

fn parse_json_stdout(output: std::process::Output) -> Value {
    assert!(
        output.status.success(),
        "Command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("Stdout is not valid UTF-8");
    serde_json::from_str(&stdout)
        .unwrap_or_else(|_| panic!("Failed to parse JSON output: {}", stdout))
}

fn assert_schema_valid(schema: &JSONSchema, json_val: &Value, context: &str) {
    match schema.validate(json_val) {
        Ok(()) => {}
        Err(errors) => {
            let details = errors.map(|e| e.to_string()).collect::<Vec<_>>().join("\n");
            panic!("{} schema validation failed:\n{}", context, details);
        }
    }
}

#[test]
fn run_json_output_matches_versioned_schema() {
    let wasm_path = "tests/fixtures/wasm/counter.wasm";
    #[allow(deprecated)]
    let output = Command::cargo_bin("soroban-debug")
        .unwrap()
        .arg("--quiet")
        .arg("run")
        .arg("--contract")
        .arg(wasm_path)
        .arg("--function")
        .arg("increment")
        .arg("--output")
        .arg("json")
        .arg("--show-events")
        .output()
        .expect("Failed to execute run command");

    let json_val = parse_json_stdout(output);
    let schema = compile_schema("tests/schemas/execution_output.json");

    assert_schema_valid(&schema, &json_val, "Run JSON");
}

#[test]
fn analyze_json_output_matches_versioned_schema() {
    let wasm_path = "tests/fixtures/wasm/counter.wasm";
    #[allow(deprecated)]
    let output = Command::cargo_bin("soroban-debug")
        .unwrap()
        .arg("--quiet")
        .arg("analyze")
        .arg("--contract")
        .arg(wasm_path)
        .arg("--format")
        .arg("json")
        .output()
        .expect("Failed to execute analyze command");

    let json_val = parse_json_stdout(output);
    let schema = compile_schema("tests/schemas/analyze_output.json");

    assert_schema_valid(&schema, &json_val, "Analyze JSON");
}

#[test]
fn inspect_json_output_matches_versioned_schema() {
    let wasm_path = "tests/fixtures/wasm/counter.wasm";
    #[allow(deprecated)]
    let output = Command::cargo_bin("soroban-debug")
        .unwrap()
        .arg("--quiet")
        .arg("inspect")
        .arg("--contract")
        .arg(wasm_path)
        .arg("--format")
        .arg("json")
        .arg("--functions")
        .output()
        .expect("Failed to execute inspect command");

    let json_val = parse_json_stdout(output);
    let schema = compile_schema("tests/schemas/inspect_output.json");

    assert_schema_valid(&schema, &json_val, "Inspect JSON");
}

#[test]
fn upgrade_check_json_output_matches_versioned_schema() {
    let wasm_path = "tests/fixtures/wasm/counter.wasm";
    #[allow(deprecated)]
    let output = Command::cargo_bin("soroban-debug")
        .unwrap()
        .arg("--quiet")
        .arg("upgrade-check")
        .arg("--old")
        .arg(wasm_path)
        .arg("--new")
        .arg(wasm_path)
        .arg("--output")
        .arg("json")
        .output()
        .expect("Failed to execute upgrade-check command");

    let json_val = parse_json_stdout(output);
    let schema = compile_schema("tests/schemas/upgrade_check_output.json");

    assert_schema_valid(&schema, &json_val, "Upgrade-check JSON");
}

#[test]
fn symbolic_json_output_matches_versioned_schema() {
    let wasm_path = "tests/fixtures/wasm/counter.wasm";
    let temp_dir = TempDir::new().expect("Failed to create temporary directory");
    let storage_seed_path = temp_dir.path().join("storage_seed.json");
    fs::write(&storage_seed_path, r#"{"c": 41}"#).expect("Failed to write storage seed fixture");

    #[allow(deprecated)]
    let output = Command::cargo_bin("soroban-debug")
        .unwrap()
        .arg("--quiet")
        .arg("symbolic")
        .arg("--contract")
        .arg(wasm_path)
        .arg("--function")
        .arg("increment")
        .arg("--format")
        .arg("json")
        .arg("--seed")
        .arg("1247")
        .arg("--storage-seed")
        .arg(&storage_seed_path)
        .arg("--input-combination-cap")
        .arg("2")
        .arg("--path-cap")
        .arg("1")
        .arg("--timeout")
        .arg("30")
        .output()
        .expect("Failed to execute symbolic command");

    let json_val = parse_json_stdout(output);
    let schema = compile_schema("tests/schemas/symbolic_output.json");

    assert_schema_valid(&schema, &json_val, "Symbolic JSON");
    assert_eq!(json_val["schema_version"], "1.0.0");
    assert_eq!(json_val["result"]["metadata"]["seed"], 1247);
    assert_eq!(json_val["result"]["metadata"]["config"]["seed"], 1247);
    assert_eq!(
        json_val["result"]["metadata"]["config"]["storage_seed"],
        r#"{"c": 41}"#
    );
    assert_eq!(json_val["result"]["metadata"]["config"]["max_paths"], 1);
    assert_eq!(
        json_val["result"]["metadata"]["config"]["max_input_combinations"],
        2
    );
}

#[test]
fn symbolic_replay_json_output_matches_versioned_schema() {
    let wasm_path = "tests/fixtures/wasm/echo.wasm";
    #[allow(deprecated)]
    let output = Command::cargo_bin("soroban-debug")
        .unwrap()
        .arg("--quiet")
        .arg("symbolic")
        .arg("--contract")
        .arg(wasm_path)
        .arg("--function")
        .arg("echo")
        .arg("--format")
        .arg("json")
        .arg("--replay")
        .arg("4242")
        .arg("--path-cap")
        .arg("2")
        .output()
        .expect("Failed to execute symbolic replay command");

    let json_val = parse_json_stdout(output);
    let schema = compile_schema("tests/schemas/symbolic_output.json");

    assert_schema_valid(&schema, &json_val, "Symbolic replay JSON");
    assert_eq!(json_val["result"]["metadata"]["seed"], 4242);
    assert_eq!(json_val["result"]["metadata"]["config"]["seed"], 4242);
}

#[test]
fn representative_symbolic_json_fixture_matches_schema() {
    let fixture = serde_json::json!({
        "schema_version": "1.0.0",
        "command": "symbolic",
        "status": "success",
        "result": {
            "function": "increment",
            "paths_explored": 1,
            "panics_found": 0,
            "paths": [
                {
                    "inputs": "[0]",
                    "return_value": "1",
                    "panic": null,
                    "path_decisions": [
                        { "kind": "StorageRead", "key": "c" },
                        { "kind": "StorageWrite", "key": "c" }
                    ]
                }
            ],
            "metadata": {
                "config": {
                    "max_paths": 1,
                    "max_input_combinations": 2,
                    "timeout_secs": 30,
                    "max_breadth": 5,
                    "max_depth": 3,
                    "seed": 1247,
                    "storage_seed": "{\"c\": 41}"
                },
                "generated_input_combinations": 2,
                "attempted_input_combinations": 1,
                "distinct_paths_recorded": 1,
                "truncated_by_input_cap": true,
                "truncated_by_path_cap": true,
                "truncated_by_timeout": false,
                "truncation_reasons": [
                    "input combination cap reached at 2 generated combinations",
                    "path exploration cap reached at 1 attempted inputs"
                ],
                "seed": 1247,
                "coverage_fraction": 0.5,
                "uncovered_regions": ["Complex input boundaries and conditional branches"]
            }
        },
        "error": null
    });
    let schema = compile_schema("tests/schemas/symbolic_output.json");

    assert_schema_valid(&schema, &fixture, "Representative symbolic JSON fixture");
}

#[test]
fn schema_rejects_missing_schema_version() {
    let schema = compile_schema("tests/schemas/execution_output.json");
    let invalid = serde_json::json!({
        "command": "run",
        "status": "success",
        "result": {},
        "error": null
    });

    let result = schema.validate(&invalid);
    assert!(
        result.is_err(),
        "schema should reject missing schema_version"
    );
}

#[test]
fn schema_rejects_invalid_envelope_structure() {
    let schema = compile_schema("tests/schemas/analyze_output.json");
    let invalid = serde_json::json!({
        "schema_version": "1.0.0",
        "command": "analyze",
        "status": "ok",
        "payload": {}
    });

    let result = schema.validate(&invalid);
    assert!(
        result.is_err(),
        "schema should reject invalid envelope fields"
    );
}

#[test]
fn symbolic_schema_rejects_missing_cap_metadata() {
    let schema = compile_schema("tests/schemas/symbolic_output.json");
    let invalid = serde_json::json!({
        "schema_version": "1.0.0",
        "command": "symbolic",
        "status": "success",
        "result": {
            "function": "increment",
            "paths_explored": 0,
            "panics_found": 0,
            "paths": [],
            "metadata": {
                "config": {
                    "max_paths": 1,
                    "max_input_combinations": 2,
                    "timeout_secs": 30,
                    "max_breadth": 5,
                    "max_depth": 3,
                    "seed": null,
                    "storage_seed": null
                },
                "generated_input_combinations": 0,
                "attempted_input_combinations": 0,
                "distinct_paths_recorded": 0,
                "truncated_by_input_cap": false,
                "truncated_by_path_cap": false,
                "truncation_reasons": [],
                "seed": null,
                "coverage_fraction": 1.0,
                "uncovered_regions": []
            }
        },
        "error": null
    });

    let result = schema.validate(&invalid);
    assert!(
        result.is_err(),
        "schema should reject symbolic reports missing timeout truncation metadata"
    );
}

#[test]
fn replay_success_json_fixture_matches_schema() {
    let schema = compile_schema("tests/schemas/replay_output.json");
    let success_content = fs::read_to_string("tests/fixtures/replay_success.json").unwrap();
    let json_val: Value = serde_json::from_str(&success_content).unwrap();
    assert_schema_valid(&schema, &json_val, "Replay success JSON");
}

#[test]
fn replay_mismatch_json_fixture_matches_schema() {
    let schema = compile_schema("tests/schemas/replay_output.json");
    let mismatch_content = fs::read_to_string("tests/fixtures/replay_mismatch.json").unwrap();
    let json_val: Value = serde_json::from_str(&mismatch_content).unwrap();
    assert_schema_valid(&schema, &json_val, "Replay mismatch JSON");
}

#[test]
fn replay_cli_json_output_matches_versioned_schema() {
    let wasm_path = "tests/fixtures/wasm/counter.wasm";
    let temp_dir = TempDir::new().expect("Failed to create temporary directory");
    let trace_path = temp_dir.path().join("trace.json");

    // 1. Generate trace using run command
    let _run_output = Command::cargo_bin("soroban-debug")
        .unwrap()
        .arg("--quiet")
        .arg("run")
        .arg("--contract")
        .arg(wasm_path)
        .arg("--function")
        .arg("increment")
        .arg("--trace-output")
        .arg(&trace_path)
        .output()
        .expect("Failed to execute run command to generate trace");

    // 2. Replay trace using replay command in json mode
    let replay_output = Command::cargo_bin("soroban-debug")
        .unwrap()
        .arg("--quiet")
        .arg("replay")
        .arg(&trace_path)
        .arg("--contract")
        .arg(wasm_path)
        .arg("--format")
        .arg("json")
        .output()
        .expect("Failed to execute replay command");

    let json_val = parse_json_stdout(replay_output);
    let schema = compile_schema("tests/schemas/replay_output.json");

    assert_schema_valid(&schema, &json_val, "Replay CLI JSON");
}
