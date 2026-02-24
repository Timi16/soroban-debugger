use soroban_debugger::cli::output::CommandOutput;

#[test]
fn test_json_output_valid() {
    let output = CommandOutput {
        status: "success".to_string(),
        result: Some("ok"),
        budget: None,
        errors: None,
    };

    let json = serde_json::to_string(&output).unwrap();
    assert!(serde_json::from_str::<serde_json::Value>(&json).is_ok());
}

#[test]
fn test_json_contains_required_fields() {
    let output = CommandOutput::<()> {
        status: "error".to_string(),
        result: None,
        budget: None,
        errors: Some(vec!["failure".to_string()]),
    };

    let value = serde_json::to_value(output).unwrap();

    assert!(value.get("status").is_some());
    assert!(value.get("result").is_some());
    assert!(value.get("budget").is_some());
    assert!(value.get("errors").is_some());
}
