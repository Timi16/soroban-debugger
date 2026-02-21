use soroban_debugger::utils::ArgumentParser;
use soroban_sdk::{Env, Vec as SorobanVec, Val, TryFromVal};

#[test]
fn test_nested_vec_parsing() {
    let env = Env::default();
    let parser = ArgumentParser::new(env.clone());
    
    // Example from the issue: [[1, 2, 3], ["a", "b"]]
    let json = r#"[ [[1, 2, 3], ["a", "b"]] ]"#;
    let result = parser.parse_args_string(json);
    
    assert!(result.is_ok(), "Failed to parse nested array: {:?}", result.err());
    let args = result.unwrap();
    assert_eq!(args.len(), 1);
    
    // The result should be a Vec containing two Vecs
    let outer_vec = SorobanVec::<Val>::try_from_val(&env, &args[0]).expect("Outer should be a Vec");
    assert_eq!(outer_vec.len(), 2);
    
    let inner_0 = SorobanVec::<Val>::try_from_val(&env, &outer_vec.get(0).unwrap()).expect("Inner 0 should be a Vec");
    assert_eq!(inner_0.len(), 3);
    
    let inner_1 = SorobanVec::<Val>::try_from_val(&env, &outer_vec.get(1).unwrap()).expect("Inner 1 should be a Vec");
    assert_eq!(inner_1.len(), 2);
}

#[test]
fn test_mixed_type_vec_rejection() {
    let env = Env::default();
    let parser = ArgumentParser::new(env.clone());
    
    // Typed vec with mixed content
    let json = r#"[ {"type": "vec", "element_type": "u32", "value": [1, 2, "a"]} ]"#;
    let result = parser.parse_args_string(json);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("element_type 'u32'"));
    
    // Bare vec with mixed content
    let json_bare = r#"[ [1, "a"] ]"#;
    let result_bare = parser.parse_args_string(json_bare);
    assert!(result_bare.is_err());
    assert!(result_bare.unwrap_err().to_string().contains("mixed array"));
}

#[test]
fn test_deeply_nested_vec() {
    let env = Env::default();
    let parser = ArgumentParser::new(env.clone());
    
    let json = r#"[ [[[[1]]]] ]"#;
    let result = parser.parse_args_string(json);
    assert!(result.is_ok());
}
