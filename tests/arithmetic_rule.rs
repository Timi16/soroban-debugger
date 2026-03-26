use soroban_debugger::analyzer::security::{AnalyzerFilter, SecurityAnalyzer};
use soroban_debugger::utils::wasm::{parse_instructions, WasmInstruction};

fn encode_u32(mut value: u32) -> Vec<u8> {
    let mut encoded = Vec::new();
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        encoded.push(byte);
        if value == 0 {
            break;
        }
    }
    encoded
}

fn push_section(module: &mut Vec<u8>, id: u8, payload: Vec<u8>) {
    module.push(id);
    module.extend(encode_u32(payload.len() as u32));
    module.extend(payload);
}

fn wasm_with_single_i32_function(param_count: u8, locals: &[(u32, u8)], ops: &[u8]) -> Vec<u8> {
    let mut module = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];

    let mut type_section = vec![0x01, 0x60, param_count];
    type_section.extend(std::iter::repeat_n(0x7f, param_count as usize));
    type_section.push(0x00);
    push_section(&mut module, 0x01, type_section);

    push_section(&mut module, 0x03, vec![0x01, 0x00]);

    let mut body = encode_u32(locals.len() as u32);
    for (count, val_type) in locals {
        body.extend(encode_u32(*count));
        body.push(*val_type);
    }
    body.extend_from_slice(ops);
    body.push(0x0b);

    let mut code_section = vec![0x01];
    code_section.extend(encode_u32(body.len() as u32));
    code_section.extend(body);
    push_section(&mut module, 0x0a, code_section);

    module
}

fn arithmetic_findings(wasm: &[u8]) -> Vec<soroban_debugger::analyzer::security::SecurityFinding> {
    SecurityAnalyzer::new()
        .analyze(wasm, None, None, &AnalyzerFilter::default())
        .expect("analysis failed")
        .findings
        .into_iter()
        .filter(|finding| finding.rule_id == "arithmetic-overflow")
        .collect()
}

#[test]
fn test_parse_instructions_recognizes_arithmetic() {
    let wasm = vec![0x6A];
    let instructions = parse_instructions(&wasm);
    assert_eq!(instructions.len(), 1);
    assert_eq!(instructions[0], WasmInstruction::I32Add);
}

#[test]
fn test_parse_instructions_recognizes_control_flow() {
    let wasm = vec![0x04, 0x0D];
    let instructions = parse_instructions(&wasm);
    assert_eq!(instructions.len(), 2);
    assert_eq!(instructions[0], WasmInstruction::If);
    assert_eq!(instructions[1], WasmInstruction::BrIf);
}

#[test]
fn test_parse_instructions_handles_unknown() {
    let wasm = vec![0xFF, 0xAB];
    let instructions = parse_instructions(&wasm);
    assert_eq!(instructions.len(), 2);
    assert!(matches!(instructions[0], WasmInstruction::Unknown(0xFF)));
    assert!(matches!(instructions[1], WasmInstruction::Unknown(0xAB)));
}

#[test]
fn test_detects_unchecked_arithmetic_with_high_confidence() {
    let wasm = wasm_with_single_i32_function(2, &[], &[0x20, 0x00, 0x20, 0x01, 0x6a]);

    let findings = arithmetic_findings(&wasm);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].confidence, Some(0.95));
    assert!(findings[0].description.contains("Confidence: high"));
    assert!(findings[0]
        .description
        .contains("No comparison-derived conditional branch"));
}

#[test]
fn test_ignores_semantically_guarded_arithmetic() {
    let wasm = wasm_with_single_i32_function(
        2,
        &[],
        &[
            0x20, 0x00, 0x20, 0x01, 0x6a, 0x20, 0x00, 0x49, 0x04, 0x40, 0x0b,
        ],
    );

    let findings = arithmetic_findings(&wasm);
    assert!(
        findings.is_empty(),
        "compare-derived branch should suppress finding"
    );
}

#[test]
fn test_ignores_non_adjacent_semantic_guard_via_local_flow() {
    let wasm = wasm_with_single_i32_function(
        2,
        &[(1, 0x7f)],
        &[
            0x20, 0x00, 0x20, 0x01, 0x6a, 0x21, 0x02, 0x20, 0x02, 0x41, 0x64, 0x49, 0x04, 0x40,
            0x0b,
        ],
    );

    let findings = arithmetic_findings(&wasm);
    assert!(
        findings.is_empty(),
        "local.set/local.get should preserve the arithmetic-to-compare-to-branch relationship"
    );
}

#[test]
fn test_ignores_call_guarded_arithmetic() {
    // Call is intentionally not treated as an arithmetic guard.
    let wasm = vec![0x10, 0x6A];
    let analyzer = SecurityAnalyzer::new();
    let report = analyzer
        .analyze(&wasm, None, None, &AnalyzerFilter::default())
        .expect("analysis failed");

    let arithmetic_findings: Vec<_> = report
        .findings
        .iter()
        .filter(|f| f.rule_id == "arithmetic-overflow")
        .collect();
    assert!(
        !arithmetic_findings.is_empty(),
        "Call should not suppress arithmetic finding"
    );
}

#[test]
fn test_compared_without_branch_downgrades_confidence() {
    let wasm = wasm_with_single_i32_function(
        2,
        &[],
        &[0x20, 0x00, 0x20, 0x01, 0x6a, 0x20, 0x00, 0x49, 0x1a],
    );

    let findings = arithmetic_findings(&wasm);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].confidence, Some(0.70));
    assert!(findings[0].description.contains("Confidence: medium"));
    assert!(findings[0]
        .description
        .contains("does not drive conditional control flow"));
}

#[test]
fn test_direct_branch_without_compare_is_low_confidence() {
    let wasm =
        wasm_with_single_i32_function(2, &[], &[0x20, 0x00, 0x20, 0x01, 0x6a, 0x04, 0x40, 0x0b]);

    let findings = arithmetic_findings(&wasm);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].confidence, Some(0.40));
    assert!(findings[0].description.contains("Confidence: low"));
    assert!(findings[0]
        .description
        .contains("no recognized compare-and-branch guard"));
}

#[test]
fn test_detects_all_arithmetic_types() {
    let arithmetic_opcodes = vec![0x6a, 0x6b, 0x6c, 0x7c, 0x7d, 0x7e];

    for opcode in arithmetic_opcodes {
        let wasm = wasm_with_single_i32_function(2, &[], &[0x20, 0x00, 0x20, 0x01, opcode]);
        let findings = arithmetic_findings(&wasm);
        assert_eq!(
            findings.len(),
            1,
            "Should detect arithmetic opcode: 0x{:X}",
            opcode
        );
    }
}

#[test]
fn test_multiple_unguarded_arithmetic() {
    let wasm = wasm_with_single_i32_function(
        2,
        &[],
        &[0x20, 0x00, 0x20, 0x01, 0x6a, 0x20, 0x00, 0x20, 0x01, 0x6b],
    );

    let findings = arithmetic_findings(&wasm);
    assert_eq!(findings.len(), 2, "Should detect both arithmetic ops");
}
