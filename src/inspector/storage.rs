use std::collections::HashMap;

/// Inspects and displays contract storage
pub struct StorageInspector {
    // Storage will be tracked here
    storage: HashMap<String, String>,
}

impl StorageInspector {
    pub fn new() -> Self {
        Self {
            storage: HashMap::new(),
        }
    }

    /// Get all storage entries
    pub fn get_all(&self) -> &HashMap<String, String> {
        &self.storage
    }

    /// Get a specific storage value
    pub fn get(&self, key: &str) -> Option<&String> {
        self.storage.get(key)
    }

    /// Display storage in a readable format
    pub fn display(&self) {
        if self.storage.is_empty() {
            println!("Storage: (empty)");
            return;
        }

        println!("Storage:");
        for (key, value) in &self.storage {
            println!("  {} = {}", key, value);
        }
    }
}

impl Default for StorageInspector {
    fn default() -> Self {
        Self::new()
    }
}
