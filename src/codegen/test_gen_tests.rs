#[cfg(test)]
mod tests {
    use crate::codegen::TestGenerator;
    use std::collections::HashMap;
    use tempfile::tempdir;

    #[test]
    fn test_generate_test_file() {
        let dir = tempdir().unwrap();
        let generator = TestGenerator::new(dir.path().to_path_buf());

        let contract_path = std::path::PathBuf::from("test.wasm");
        let function = "hello";
        let args = vec!["\"world\"".to_string()];
        let output = "Success";
        let storage_before = HashMap::new();
        let storage_after = HashMap::new();

        let result = generator.generate_test(
            &contract_path,
            function,
            args,
            output,
            &storage_before,
            &storage_after,
        );

        assert!(result.is_ok());
        let file_path = result.unwrap();
        assert!(file_path.exists());

        let content = std::fs::read_to_string(file_path).unwrap();
        assert!(content.contains("fn test_hello_"));
        assert!(content.contains("const WASM: &[u8] = include_bytes!(\"test.wasm\");"));
        assert!(content.contains("let args = (\"world\");"));
    }
}
