use soroban_debugger::analyzer::security::{AnalyzerFilter, SecurityAnalyzer};
use soroban_debugger::server::protocol::{DynamicTraceEvent, DynamicTraceEventKind};

fn uleb128(mut value: usize) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
    out
}

fn encode_string(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend_from_slice(&uleb128(value.len()));
    bytes.extend_from_slice(value.as_bytes());
}

fn append_section(module: &mut Vec<u8>, id: u8, section: &[u8]) {
    module.push(id);
    module.extend_from_slice(&uleb128(section.len()));
    module.extend_from_slice(section);
}

fn make_wasm_with_import(module_name: &str, import_name: &str) -> Vec<u8> {
    let mut module = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];

    // Type section: one () -> () function type.
    let mut ty = Vec::new();
    ty.extend_from_slice(&uleb128(1));
    ty.push(0x60);
    ty.push(0x00);
    ty.push(0x00);
    append_section(&mut module, 1, &ty);

    // Import section: import a single function.
    let mut import = Vec::new();
    import.extend_from_slice(&uleb128(1));
    encode_string(&mut import, module_name);
    encode_string(&mut import, import_name);
    import.push(0x00); // import kind: function
    import.extend_from_slice(&uleb128(0)); // type index 0
    append_section(&mut module, 2, &import);

    // Function section: one local function using type index 0.
    let mut functions = Vec::new();
    functions.extend_from_slice(&uleb128(1));
    functions.extend_from_slice(&uleb128(0));
    append_section(&mut module, 3, &functions);

    // Export section: export local function at index 1 (import is index 0).
    let mut exports = Vec::new();
    exports.extend_from_slice(&uleb128(1));
    encode_string(&mut exports, "entrypoint");
    exports.push(0x00); // export kind: function
    exports.extend_from_slice(&uleb128(1));
    append_section(&mut module, 7, &exports);

    // Code section: body = end (no call needed for import detection).
    let mut code = Vec::new();
    code.extend_from_slice(&uleb128(1)); // one body
    let body = vec![0x00, 0x0B]; // no locals, end
    code.extend_from_slice(&uleb128(body.len()));
    code.extend_from_slice(&body);
    append_section(&mut module, 10, &code);

    module
}

fn has_cross_contract_import_finding(wasm: &[u8]) -> bool {
    let analyzer = SecurityAnalyzer::new();
    let filter = AnalyzerFilter::default();
    let report = analyzer
        .analyze(wasm, None, None, &filter)
        .expect("analysis failed");
    report
        .findings
        .iter()
        .any(|f| f.rule_id == "cross-contract-import")
}

#[test]
fn detects_cross_contract_import_variants() {
    let cases = [
        ("env", "invoke_contract"),
        ("env", "invoke_contract_v2"),
        ("env", "try_invoke_contract"),
        ("soroban_env_host", "invoke_contract"),
        ("soroban-env-host", "try_invoke_contract_v3"),
        ("soroban_env", "call_contract"),
        ("env", "try_call"),
    ];

    for (module_name, import_name) in cases {
        let wasm = make_wasm_with_import(module_name, import_name);
        assert!(
            has_cross_contract_import_finding(&wasm),
            "expected finding for {module_name}::{import_name}"
        );
    }
}

#[test]
fn does_not_match_unrelated_import_names() {
    let wasm = make_wasm_with_import("env", "reinvoke_contract");
    assert!(!has_cross_contract_import_finding(&wasm));
}

#[test]
fn does_not_match_unrelated_modules() {
    let wasm = make_wasm_with_import("not_env", "invoke_contract");
    assert!(!has_cross_contract_import_finding(&wasm));
}
#[test]
fn reentrancy_detection_handles_optional_function_metadata_with_depth() {
    let wasm = vec![0x00, 0x61, 0x73, 0x6D, 0x01, 0x00, 0x00, 0x00];
    let analyzer = SecurityAnalyzer::new();
    let trace = vec![
        DynamicTraceEvent {
            sequence: 1,
            kind: DynamicTraceEventKind::CrossContractCall,
            message: "external call".to_string(),
            caller: None,
            function: None,
            call_depth: Some(0),
            storage_key: None,
            storage_value: None,
            address: None,
        },
        DynamicTraceEvent {
            sequence: 2,
            kind: DynamicTraceEventKind::StorageWrite,
            message: "update state".to_string(),
            caller: None,
            function: Some("withdraw".to_string()),
            call_depth: Some(0),
            storage_key: Some("balance:alice".to_string()),
            storage_value: Some("0".to_string()),
            address: None,
        },
    ];

    let report = analyzer
        .analyze(&wasm, None, Some(&trace), &AnalyzerFilter::default())
        .expect("analysis failed");

    assert!(report
        .findings
        .iter()
        .any(|f| f.rule_id == "reentrancy-pattern"));
}
