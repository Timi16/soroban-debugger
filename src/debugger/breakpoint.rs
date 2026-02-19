use std::collections::HashSet;

/// Manages breakpoints during debugging
pub struct BreakpointManager {
    breakpoints: HashSet<String>,
}

impl BreakpointManager {
    /// Create a new breakpoint manager
    pub fn new() -> Self {
        Self {
            breakpoints: HashSet::new(),
        }
    }

    /// Add a breakpoint at a function name
    pub fn add(&mut self, function: &str) {
        self.breakpoints.insert(function.to_string());
    }

    /// Remove a breakpoint
    pub fn remove(&mut self, function: &str) -> bool {
        self.breakpoints.remove(function)
    }

    /// Check if execution should break at this function
    pub fn should_break(&self, function: &str) -> bool {
        self.breakpoints.contains(function)
    }

    /// List all breakpoints
    pub fn list(&self) -> Vec<String> {
        self.breakpoints.iter().cloned().collect()
    }

    /// Clear all breakpoints
    pub fn clear(&mut self) {
        self.breakpoints.clear();
    }

    /// Check if there are any breakpoints set
    pub fn is_empty(&self) -> bool {
        self.breakpoints.is_empty()
    }

    /// Get count of breakpoints
    pub fn count(&self) -> usize {
        self.breakpoints.len()
    }
}

impl Default for BreakpointManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_breakpoint() {
        let mut manager = BreakpointManager::new();
        manager.add("transfer");
        assert!(manager.should_break("transfer"));
        assert!(!manager.should_break("mint"));
    }

    #[test]
    fn test_remove_breakpoint() {
        let mut manager = BreakpointManager::new();
        manager.add("transfer");
        assert!(manager.remove("transfer"));
        assert!(!manager.should_break("transfer"));
    }

    #[test]
    fn test_list_breakpoints() {
        let mut manager = BreakpointManager::new();
        manager.add("transfer");
        manager.add("mint");
        let list = manager.list();
        assert_eq!(list.len(), 2);
        assert!(list.contains(&"transfer".to_string()));
        assert!(list.contains(&"mint".to_string()));
    }
}
