/// Wrapper around Soroban Host environment for debugging
/// This will be enhanced in later phases
pub struct DebugEnv {
    // TODO: Add tracking for storage access, function calls, etc.
}

impl DebugEnv {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for DebugEnv {
    fn default() -> Self {
        Self::new()
    }
}
