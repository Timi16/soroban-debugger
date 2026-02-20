use std::time::Duration;

/// Represents a single frame in the call stack
#[derive(Debug, Clone)]
pub struct CallFrame {
    pub function: String,
    pub contract_id: Option<String>,
    pub duration: Option<Duration>,
}

/// Tracks and displays the call stack
#[derive(Debug, Clone, Default)]
pub struct CallStackInspector {
    stack: Vec<CallFrame>,
}

impl CallStackInspector {
    pub fn new() -> Self {
        Self { stack: Vec::new() }
    }

    /// Push a function onto the call stack
    pub fn push(&mut self, function: String, contract_id: Option<String>) {
        self.stack.push(CallFrame {
            function,
            contract_id,
            duration: None,
        });
    }

    /// Push a frame with duration
    pub fn push_frame(&mut self, frame: CallFrame) {
        self.stack.push(frame);
    }

    /// Pop a function from the call stack
    pub fn pop(&mut self) -> Option<CallFrame> {
        self.stack.pop()
    }

    /// Get the current call stack
    pub fn get_stack(&self) -> &[CallFrame] {
        &self.stack
    }

    /// Display the call stack
    pub fn display(&self) {
        if self.stack.is_empty() {
            tracing::info!("Call stack is empty");
            return;
        }

        println!("Call Stack:");
        for (i, frame) in self.stack.iter().enumerate() {
            let indent = "  ".repeat(i);
            let contract_ctx = if let Some(ref id) = frame.contract_id {
                format!(" [{}]", id)
            } else {
                "".to_string()
            };

            let duration_ctx = if let Some(duration) = frame.duration {
                format!(" ({:?})", duration)
            } else {
                "".to_string()
            };

            if i == self.stack.len() - 1 {
                println!(
                    "{}→ {}{}{}",
                    indent, frame.function, contract_ctx, duration_ctx
                );
            } else {
                println!(
                    "{}└─ {}{}{}",
                    indent, frame.function, contract_ctx, duration_ctx
                );
            }
        }
    }

    /// Clear the call stack
    pub fn clear(&mut self) {
        self.stack.clear();
    }
}
