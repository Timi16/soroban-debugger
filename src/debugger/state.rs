/// Represents the current state of the debugger
#[derive(Debug, Clone)]
pub struct DebugState {
    current_function: Option<String>,
    step_count: usize,
}

impl DebugState {
    /// Create a new debug state
    pub fn new() -> Self {
        Self {
            current_function: None,
            step_count: 0,
        }
    }

    /// Set the current function being executed
    pub fn set_current_function(&mut self, function: String) {
        self.current_function = Some(function);
    }

    /// Get the current function
    pub fn current_function(&self) -> Option<&str> {
        self.current_function.as_deref()
    }

    /// Increment step count
    pub fn increment_step(&mut self) {
        self.step_count += 1;
    }

    /// Get current step count
    pub fn step_count(&self) -> usize {
        self.step_count
    }

    /// Reset the state
    pub fn reset(&mut self) {
        self.current_function = None;
        self.step_count = 0;
    }
}

impl Default for DebugState {
    fn default() -> Self {
        Self::new()
    }
}
