use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorExplanation {
    pub code: u32,
    pub name: String,
    pub description: String,
    pub common_cause: String,
    pub suggested_fix: String,
}

pub struct ErrorDatabase {
    standard_errors: HashMap<u32, ErrorExplanation>,
    custom_errors: HashMap<u32, ErrorExplanation>,
}

impl ErrorDatabase {
    pub fn new() -> Self {
        let mut db = Self {
            standard_errors: HashMap::new(),
            custom_errors: HashMap::new(),
        };
        db.init_standard_errors();
        db
    }

    fn init_standard_errors(&mut self) {
        let errors = vec![
            ErrorExplanation {
                code: 1,
                name: "UnknownError".to_string(),
                description: "An unknown error occurred during contract execution".to_string(),
                common_cause: "Unhandled exception or unexpected state".to_string(),
                suggested_fix: "Check contract logic for unhandled edge cases or add error handling".to_string(),
            },
            ErrorExplanation {
                code: 2,
                name: "HostError".to_string(),
                description: "Error from the Soroban host environment".to_string(),
                common_cause: "Host operation failed (e.g., storage, budget exceeded)".to_string(),
                suggested_fix: "Check budget limits, storage access, or host resource constraints".to_string(),
            },
            ErrorExplanation {
                code: 3,
                name: "ConversionError".to_string(),
                description: "Failed to convert between value types".to_string(),
                common_cause: "Type mismatch or invalid value format".to_string(),
                suggested_fix: "Verify argument types match function signature and value formats are correct".to_string(),
            },
            ErrorExplanation {
                code: 4,
                name: "StorageError".to_string(),
                description: "Storage operation failed".to_string(),
                common_cause: "Storage key not found, access denied, or storage limit exceeded".to_string(),
                suggested_fix: "Check storage key existence, permissions, and storage budget limits".to_string(),
            },
            ErrorExplanation {
                code: 5,
                name: "BudgetError".to_string(),
                description: "CPU or memory budget exceeded".to_string(),
                common_cause: "Contract execution consumed too many CPU instructions or memory bytes".to_string(),
                suggested_fix: "Optimize contract code, reduce loop iterations, or use more efficient algorithms".to_string(),
            },
            ErrorExplanation {
                code: 6,
                name: "AuthError".to_string(),
                description: "Authorization check failed".to_string(),
                common_cause: "Missing required authorization or insufficient permissions".to_string(),
                suggested_fix: "Ensure proper authorization is provided before calling the function".to_string(),
            },
            ErrorExplanation {
                code: 7,
                name: "MathError".to_string(),
                description: "Mathematical operation failed".to_string(),
                common_cause: "Division by zero, overflow, or invalid mathematical operation".to_string(),
                suggested_fix: "Add bounds checking, validate inputs, and handle edge cases in math operations".to_string(),
            },
            ErrorExplanation {
                code: 8,
                name: "ArrayError".to_string(),
                description: "Array operation failed".to_string(),
                common_cause: "Index out of bounds, invalid array access, or array size exceeded".to_string(),
                suggested_fix: "Validate array indices before access and check array bounds".to_string(),
            },
            ErrorExplanation {
                code: 9,
                name: "StringError".to_string(),
                description: "String operation failed".to_string(),
                common_cause: "Invalid string encoding, length exceeded, or malformed string".to_string(),
                suggested_fix: "Validate string encoding and length before operations".to_string(),
            },
            ErrorExplanation {
                code: 10,
                name: "MapError".to_string(),
                description: "Map operation failed".to_string(),
                common_cause: "Key not found, invalid key type, or map operation failed".to_string(),
                suggested_fix: "Check if key exists before access and validate key types".to_string(),
            },
        ];

        for error in errors {
            self.standard_errors.insert(error.code, error);
        }
    }

    pub fn lookup(&self, code: u32) -> Option<&ErrorExplanation> {
        self.custom_errors
            .get(&code)
            .or_else(|| self.standard_errors.get(&code))
    }

    pub fn add_custom_error(&mut self, error: ErrorExplanation) {
        self.custom_errors.insert(error.code, error);
    }

    pub fn load_custom_errors_from_spec(&mut self, spec_path: &str) -> Result<(), String> {
        use std::fs;
        let content = fs::read_to_string(spec_path)
            .map_err(|e| format!("Failed to read spec file: {}", e))?;

        let parsed: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse spec file: {}", e))?;

        if let Some(errors) = parsed.get("errors").and_then(|e| e.as_array()) {
            for error_obj in errors {
                if let (Some(code), Some(name)) = (
                    error_obj.get("code").and_then(|c| c.as_u64()),
                    error_obj.get("name").and_then(|n| n.as_str()),
                ) {
                    let explanation = ErrorExplanation {
                        code: code as u32,
                        name: name.to_string(),
                        description: error_obj
                            .get("description")
                            .and_then(|d| d.as_str())
                            .unwrap_or("Custom contract error")
                            .to_string(),
                        common_cause: error_obj
                            .get("common_cause")
                            .and_then(|c| c.as_str())
                            .unwrap_or("Contract-specific error condition")
                            .to_string(),
                        suggested_fix: error_obj
                            .get("suggested_fix")
                            .and_then(|f| f.as_str())
                            .unwrap_or("Review contract documentation or source code")
                            .to_string(),
                    };
                    self.add_custom_error(explanation);
                }
            }
        }

        Ok(())
    }

    pub fn display_error(&self, code: u32) {
        if let Some(explanation) = self.lookup(code) {
            println!("\n=== Error Explanation ===");
            println!("Error Code: {}", explanation.code);
            println!("Error Name: {}", explanation.name);
            println!("\nDescription:");
            println!("  {}", explanation.description);
            println!("\nCommon Cause:");
            println!("  {}", explanation.common_cause);
            println!("\nSuggested Fix:");
            println!("  {}", explanation.suggested_fix);
            println!();
        } else {
            println!("\n=== Error Code: {} ===", code);
            println!("No explanation available for this error code.");
            println!("This may be a custom contract error. Check contract documentation.");
            println!();
        }
    }
}

impl Default for ErrorDatabase {
    fn default() -> Self {
        Self::new()
    }
}
