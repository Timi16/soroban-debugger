use soroban_debugger::debugger::source_map::{SourceMap, SourceLocation};
use std::path::PathBuf;

#[test]
fn test_source_map_lookup_logic() {
    let mut sm = SourceMap::new();
    let file = PathBuf::from("src/lib.rs");
    
    // Test exact match
    sm.add_mapping(100, SourceLocation { file: file.clone(), line: 10, column: Some(5) });
    sm.add_mapping(200, SourceLocation { file: file.clone(), line: 20, column: Some(0) });
    
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
    
    sm.add_mapping(100, SourceLocation { file: file1.clone(), line: 5, column: None });
    sm.add_mapping(150, SourceLocation { file: file2.clone(), line: 10, column: None });
    
    assert_eq!(sm.lookup(120).unwrap().file, file1);
    assert_eq!(sm.lookup(170).unwrap().file, file2);
}
