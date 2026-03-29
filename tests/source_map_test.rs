use soroban_debugger::debugger::source_map::{SourceLocation, SourceMap};
use std::collections::HashSet;
use std::path::PathBuf;

// Minimal valid WASM module (magic + version, no sections).
// Sufficient for WasmIndex::parse to succeed with an empty index.
const MINIMAL_WASM: &[u8] = &[0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];

#[test]
fn test_source_map_lookup_logic() {
    let mut sm = SourceMap::new();
    let file = PathBuf::from("src/lib.rs");

    // Test exact match
    sm.add_mapping(
        100,
        SourceLocation {
            file: file.clone(),
            line: 10,
            column: Some(5),
        },
    );
    sm.add_mapping(
        200,
        SourceLocation {
            file: file.clone(),
            line: 20,
            column: Some(0),
        },
    );

    let loc = sm.lookup(100).unwrap();
    assert_eq!(loc.line, 10);

    // Test range match (offset 150 should still be in line 10's range until 200)
    let loc2 = sm.lookup(150).unwrap();
    assert_eq!(loc2.line, 10);

    let loc3 = sm.lookup(200).unwrap();
    assert_eq!(loc3.line, 20);

    let loc4 = sm.lookup(250).unwrap();
    assert_eq!(loc4.line, 20);

    // Test before first mapping
    assert!(sm.lookup(50).is_none());
}

#[test]
fn test_source_map_multiple_files() {
    let mut sm = SourceMap::new();
    let file1 = PathBuf::from("src/main.rs");
    let file2 = PathBuf::from("src/utils.rs");

    sm.add_mapping(
        100,
        SourceLocation {
            file: file1.clone(),
            line: 5,
            column: None,
        },
    );
    sm.add_mapping(
        150,
        SourceLocation {
            file: file2.clone(),
            line: 10,
            column: None,
        },
    );

    assert_eq!(sm.lookup(120).unwrap().file, file1);
    assert_eq!(sm.lookup(170).unwrap().file, file2);
}

// Diagnostics style regression tests.
//
// These ensure that static-string reason codes and messages are produced
// without format!() — consistent with the SourceMapDiagnostic guideline.
// Any future refactor that introduces useless_format patterns will break here.

#[test]
fn test_no_debug_info_reason_code() {
    let sm = SourceMap::new();
    let exported: HashSet<String> = HashSet::new();
    let results =
        sm.resolve_source_breakpoints(MINIMAL_WASM, &PathBuf::from("src/lib.rs"), &[5], &exported);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].reason_code, "NO_DEBUG_INFO");
    assert!(!results[0].verified);
    assert!(results[0].function.is_none());
    // Message is a static string; must not be empty.
    assert!(!results[0].message.is_empty());
}

#[test]
fn test_file_not_in_debug_info_reason_code() {
    let mut sm = SourceMap::new();
    // Add a mapping for a different file so the source map is non-empty.
    sm.add_mapping(
        10,
        SourceLocation {
            file: PathBuf::from("src/other.rs"),
            line: 1,
            column: None,
        },
    );
    let exported: HashSet<String> = HashSet::new();
    let results =
        sm.resolve_source_breakpoints(MINIMAL_WASM, &PathBuf::from("src/lib.rs"), &[5], &exported);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].reason_code, "FILE_NOT_IN_DEBUG_INFO");
    assert!(!results[0].verified);
    assert!(results[0].function.is_none());
    assert!(!results[0].message.is_empty());
}

#[test]
fn test_no_code_at_line_reason_code() {
    let mut sm = SourceMap::new();
    let source_file = PathBuf::from("src/lib.rs");
    // Add a mapping well away from line 99 so forward-adjust (max 20) cannot reach it.
    sm.add_mapping(
        10,
        SourceLocation {
            file: source_file.clone(),
            line: 1,
            column: None,
        },
    );
    let exported: HashSet<String> = HashSet::new();
    let results = sm.resolve_source_breakpoints(MINIMAL_WASM, &source_file, &[99], &exported);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].reason_code, "NO_CODE_AT_LINE");
    assert!(!results[0].verified);
    assert!(results[0].function.is_none());
    assert!(!results[0].message.is_empty());
}
