use anyhow::Result;
use chrono::Local;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Generates Rust unit tests from execution traces.
pub struct TestGenerator {
    output_dir: PathBuf,
}

impl TestGenerator {
    pub fn new(output_dir: PathBuf) -> Self {
        Self { output_dir }
    }

    /// Generate a test file from execution data.
    pub fn generate_test(
        &self,
        contract_path: &Path,
        function: &str,
        args: Vec<String>,
        output: &str,
        storage_before: &HashMap<String, String>,
        storage_after: &HashMap<String, String>,
    ) -> Result<PathBuf> {
        if !self.output_dir.exists() {
            fs::create_dir_all(&self.output_dir)?;
        }

        let timestamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
        let file_name = format!("test_{}_{}.rs", function, timestamp);
        let file_path = self.output_dir.join(file_name);

        let contract_wasm_path = contract_path.to_str().unwrap_or("contract.wasm");

        let mut test_code = String::new();
        test_code.push_str("use soroban_sdk::{Env, BytesN, testutils::Address as _};\n");
        test_code.push_str("use soroban_sdk::testutils::storage::Storage as _;\n\n");

        test_code.push_str(&format!(
            "const WASM: &[u8] = include_bytes!(\"{}\");\n\n",
            contract_wasm_path
        ));

        test_code.push_str("#[test]\n");
        test_code.push_str(&format!("fn test_{}_{}() {{\n", function, timestamp));
        test_code.push_str("    let env = Env::default();\n");
        test_code.push_str("    let contract_id = env.register_contract_wasm(None, WASM);\n");
        // For simplicity in this first version, we'll use a generic client if possible,
        // but Soroban clients are usually generated.
        // We might need to use env.invoke_contract directly if we don't have the client.

        test_code.push_str("    \n    // Setup storage state\n");
        for (key, value) in storage_before {
            test_code.push_str(&format!("    // Storage key: {}, value: {}\n", key, value));
            // Actual storage injection would require parsing XDR and using env.host().with_ledger_info(...)
            // or specialized test utils. For now, we'll add placeholders.
        }

        test_code.push_str("\n    // Arguments\n");
        test_code.push_str("    let args = (");
        for (i, arg) in args.iter().enumerate() {
            test_code.push_str(arg);
            if i < args.len() - 1 {
                test_code.push_str(", ");
            }
        }
        test_code.push_str(");\n\n");

        test_code.push_str("    // Execute\n");
        test_code.push_str(&format!("    let result: soroban_sdk::Val = env.invoke_contract(&contract_id, &soroban_sdk::Symbol::new(&env, \"{}\"), args.into_val(&env));\n", function));

        test_code.push_str("\n    // Verify output\n");
        test_code.push_str("    println!(\"Result: {:?}\", result);\n");
        test_code.push_str(&format!("    // Expected output: {}\n", output));

        test_code.push_str("\n    // Verify storage changes\n");
        for (key, value) in storage_after {
            test_code.push_str(&format!(
                "    // Post-storage key: {}, value: {}\n",
                key, value
            ));
        }

        test_code.push_str("}\n");

        fs::write(&file_path, test_code)?;
        Ok(file_path)
    }
}
