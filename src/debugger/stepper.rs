/// Handles step-through execution of contracts
/// This will be implemented in Phase 2
pub struct Stepper {
    // TODO: Implement stepping logic
}

impl Stepper {
    pub fn new() -> Self {
        Self {}
    }

    /// Step into next instruction
    pub fn step_into(&mut self) {
        // TODO: Implement
    }

    /// Step over function call
    pub fn step_over(&mut self) {
        // TODO: Implement
    }

    /// Step out of current function
    pub fn step_out(&mut self) {
        // TODO: Implement
    }
}

impl Default for Stepper {
    fn default() -> Self {
        Self::new()
    }
}
