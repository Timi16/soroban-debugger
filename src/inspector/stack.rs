/// Tracks and displays the call stack
pub struct CallStackInspector {
    stack: Vec<String>,
}

impl CallStackInspector {
    pub fn new() -> Self {
        Self { stack: Vec::new() }
    }

    /// Push a function onto the call stack
    pub fn push(&mut self, function: String) {
        self.stack.push(function);
    }

    /// Pop a function from the call stack
    pub fn pop(&mut self) -> Option<String> {
        self.stack.pop()
    }

    /// Get the current call stack
    pub fn get_stack(&self) -> &[String] {
        &self.stack
    }

    /// Display the call stack
    pub fn display(&self) {
        if self.stack.is_empty() {
            println!("Call Stack: (empty)");
            return;
        }

        println!("Call Stack:");
        for (i, func) in self.stack.iter().enumerate() {
            let indent = "  ".repeat(i);
            if i == self.stack.len() - 1 {
                println!("{}→ {}", indent, func);
            } else {
                println!("{}└─ {}", indent, func);
            }
        }
    }

    /// Clear the call stack
    pub fn clear(&mut self) {
        self.stack.clear();
    }
}

impl Default for CallStackInspector {
    fn default() -> Self {
        Self::new()
    }
}
