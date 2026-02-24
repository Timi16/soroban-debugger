use crate::runtime::instruction::{Instruction, InstructionParser};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use walrus::{FunctionId, Module, ModuleConfig};

/// Callback function type for instruction hooks
pub type InstructionHook = Arc<dyn Fn(usize, &Instruction) -> bool + Send + Sync>;

/// Instruction counter for tracking per-function execution
#[derive(Debug, Clone)]
pub struct InstructionCounter {
    /// Map of function name to instruction count
    pub counts: Arc<Mutex<HashMap<String, u64>>>,
    /// Total instructions executed
    pub total: Arc<Mutex<u64>>,
}

impl InstructionCounter {
    /// Create a new instruction counter
    pub fn new() -> Self {
        Self {
            counts: Arc::new(Mutex::new(HashMap::new())),
            total: Arc::new(Mutex::new(0)),
        }
    }

    /// Increment count for a function
    pub fn increment(&self, function_name: &str, count: u64) {
        if let Ok(mut counts) = self.counts.lock() {
            *counts.entry(function_name.to_string()).or_insert(0) += count;
        }
        if let Ok(mut total) = self.total.lock() {
            *total += count;
        }
    }

    /// Get count for a specific function
    pub fn get(&self, function_name: &str) -> u64 {
        self.counts
            .lock()
            .ok()
            .and_then(|c| c.get(function_name).copied())
            .unwrap_or(0)
    }

    /// Get total instruction count
    pub fn get_total(&self) -> u64 {
        self.total.lock().ok().map(|t| *t).unwrap_or(0)
    }

    /// Get all counts as a sorted vector (highest first)
    pub fn get_sorted(&self) -> Vec<(String, u64)> {
        let mut counts = self
            .counts
            .lock()
            .ok()
            .map(|c| c.iter().map(|(k, v)| (k.clone(), *v)).collect::<Vec<_>>())
            .unwrap_or_default();
        counts.sort_by(|a, b| b.1.cmp(&a.1));
        counts
    }

    /// Clear all counts
    pub fn reset(&self) {
        if let Ok(mut counts) = self.counts.lock() {
            counts.clear();
        }
        if let Ok(mut total) = self.total.lock() {
            *total = 0;
        }
    }
}

impl Default for InstructionCounter {
    fn default() -> Self {
        Self::new()
    }
}

/// WASM instrumentation for adding debug hooks
pub struct Instrumenter {
    /// Whether instrumentation is enabled
    enabled: bool,
    /// Instruction hook callback
    hook: Option<InstructionHook>,
    /// Parsed instructions for reference
    instructions: Vec<Instruction>,
    /// Instruction counter
    pub counter: InstructionCounter,
}

impl Instrumenter {
    /// Create a new instrumenter
    pub fn new() -> Self {
        Self {
            enabled: false,
            hook: None,
            instructions: Vec::new(),
            counter: InstructionCounter::new(),
        }
    }

    /// Enable instrumentation
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Disable instrumentation
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// Check if instrumentation is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Set instruction hook callback
    pub fn set_hook<F>(&mut self, hook: F)
    where
        F: Fn(usize, &Instruction) -> bool + Send + Sync + 'static,
    {
        self.hook = Some(Arc::new(hook));
    }

    /// Remove instruction hook
    pub fn remove_hook(&mut self) {
        self.hook = None;
    }

    /// Parse instructions from WASM bytecode
    pub fn parse_instructions(&mut self, wasm_bytes: &[u8]) -> Result<&[Instruction], String> {
        let mut parser = InstructionParser::new();
        let instructions = parser.parse(wasm_bytes)?;
        self.instructions = instructions.to_vec();
        Ok(&self.instructions)
    }

    /// Get parsed instructions
    pub fn instructions(&self) -> &[Instruction] {
        &self.instructions
    }

    /// Instrument WASM bytecode with debugging hooks
    ///
    /// This adds calls to a debug callback function before each instruction
    /// when debug mode is enabled.
    pub fn instrument(&self, wasm_bytes: &[u8]) -> Result<Vec<u8>, String> {
        if !self.enabled || self.hook.is_none() {
            // If not enabled or no hook, return original WASM
            return Ok(wasm_bytes.to_vec());
        }

        // Parse the WASM module
        let _config = ModuleConfig::new();
        let mut module = Module::from_buffer(wasm_bytes)
            .map_err(|e| format!("Failed to parse WASM module: {}", e))?;

        // Add a debug callback import
        let debug_callback_type = module.types.add(&[], &[]);
        let (debug_callback, _) = module.add_import_func("debug", "callback", debug_callback_type);

        // Instrument each function (simplified for now)
        let func_ids: Vec<FunctionId> = module.funcs.iter_local().map(|(id, _)| id).collect();
        for func_id in func_ids {
            self.instrument_function(&mut module, func_id, debug_callback)?;
        }

        // Emit the instrumented WASM
        Ok(module.emit_wasm())
    }

    /// Instrument a single function with debug hooks
    fn instrument_function(
        &self,
        _module: &mut Module,
        _func_id: FunctionId,
        _debug_callback: FunctionId,
    ) -> Result<(), String> {
        // Simplified implementation for now
        // Full implementation would require deep integration with walrus IR
        Ok(())
    }

    /// Instrument a basic block with debug hooks
    #[allow(dead_code)]
    fn instrument_block(
        &self,
        _module: &mut Module,
        _debug_callback: FunctionId,
    ) -> Result<(), String> {
        // Simplified implementation for now
        // This is complex and requires deep understanding of the walrus IR
        Ok(())
    }

    /// Call the instruction hook if present
    pub fn call_hook(&self, instruction_index: usize) -> bool {
        if let (Some(hook), Some(instruction)) =
            (&self.hook, self.instructions.get(instruction_index))
        {
            hook(instruction_index, instruction)
        } else {
            false // Continue execution
        }
    }

    /// Create a simple instrumenter that just parses instructions
    pub fn parse_only(wasm_bytes: &[u8]) -> Result<Self, String> {
        let mut instrumenter = Self::new();
        instrumenter.parse_instructions(wasm_bytes)?;
        Ok(instrumenter)
    }
}

impl Default for Instrumenter {
    fn default() -> Self {
        Self::new()
    }
}
