use crate::debugger::breakpoint::{BreakpointManager, BreakpointSpec, ConditionEvaluator};
use crate::debugger::instruction_pointer::StepMode;
use crate::debugger::source_map::{SourceLocation, SourceMap};
use crate::debugger::state::{DebugState, PauseReason};
use crate::debugger::stepper::Stepper;
use crate::output::InvocationReason;
use crate::plugin::{EventContext, ExecutionEvent};
use crate::runtime::executor::ContractExecutor;
use crate::runtime::instruction::Instruction;
use crate::runtime::instrumentation::Instrumenter;
use crate::Result;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::info;

pub struct StepOverResult {
    pub paused: bool,
    pub location: Option<SourceLocation>,
}

/// Core debugging engine that orchestrates execution and debugging.
pub struct DebuggerEngine {
    executor: ContractExecutor,
    breakpoints: BreakpointManager,
    state: Arc<Mutex<DebugState>>,
    stepper: Stepper,
    instrumenter: Instrumenter,
    source_map: Option<SourceMap>,
    paused: bool,
    instruction_debug_enabled: bool,
}

struct EngineConditionEvaluator {
    storage: HashMap<String, String>,
    function: String,
    args: Option<String>,
}

impl EngineConditionEvaluator {
    fn new(storage: HashMap<String, String>, function: &str, args: Option<&str>) -> Self {
        Self {
            storage,
            function: function.to_string(),
            args: args.map(str::to_string),
        }
    }

    fn parse_condition<'a>(
        &self,
        condition: &'a str,
    ) -> crate::Result<(&'a str, &'a str, &'a str)> {
        let condition = condition.trim();
        let (var, op, value) = if let Some(pos) = condition.find(">=") {
            let (var, rest) = condition.split_at(pos);
            (var.trim(), ">=", rest[2..].trim())
        } else if let Some(pos) = condition.find("<=") {
            let (var, rest) = condition.split_at(pos);
            (var.trim(), "<=", rest[2..].trim())
        } else if let Some(pos) = condition.find("==") {
            let (var, rest) = condition.split_at(pos);
            (var.trim(), "==", rest[2..].trim())
        } else if let Some(pos) = condition.find("!=") {
            let (var, rest) = condition.split_at(pos);
            (var.trim(), "!=", rest[2..].trim())
        } else if let Some(pos) = condition.find('>') {
            let (var, rest) = condition.split_at(pos);
            (var.trim(), ">", rest[1..].trim())
        } else if let Some(pos) = condition.find('<') {
            let (var, rest) = condition.split_at(pos);
            (var.trim(), "<", rest[1..].trim())
        } else {
            return Err(crate::DebuggerError::BreakpointError(format!(
                "No operator found in condition: {}",
                condition
            ))
            .into());
        };

        Ok((var, op, value))
    }

    fn normalize_value(value: &str) -> &str {
        value.trim_matches('"').trim_matches('\'')
    }
}

impl ConditionEvaluator for EngineConditionEvaluator {
    fn evaluate(&self, condition: &str) -> crate::Result<bool> {
        let (var, op, value_str) = self.parse_condition(condition)?;
        let actual = self
            .storage
            .get(var)
            .map(String::as_str)
            .unwrap_or_default()
            .trim();
        let actual = Self::normalize_value(actual);
        let expected = Self::normalize_value(value_str);

        if let (Ok(lhs), Ok(rhs)) = (actual.parse::<f64>(), expected.parse::<f64>()) {
            return Ok(match op {
                "==" => lhs == rhs,
                "!=" => lhs != rhs,
                ">" => lhs > rhs,
                "<" => lhs < rhs,
                ">=" => lhs >= rhs,
                "<=" => lhs <= rhs,
                _ => false,
            });
        }

        Ok(match op {
            "==" => actual == expected,
            "!=" => actual != expected,
            ">" => actual > expected,
            "<" => actual < expected,
            ">=" => actual >= expected,
            "<=" => actual <= expected,
            _ => false,
        })
    }

    fn interpolate_log(&self, template: &str) -> crate::Result<String> {
        let mut rendered = template.to_string();
        for (key, value) in &self.storage {
            rendered = rendered.replace(&format!("{{{}}}", key), value);
        }
        rendered = rendered.replace("{function}", &self.function);
        if let Some(args) = &self.args {
            rendered = rendered.replace("{args}", args);
            rendered = rendered.replace("{arguments}", args);
        }
        Ok(rendered)
    }
}

impl DebuggerEngine {
    /// Returns the current paused source location (file, line, column) if available.
    pub fn current_source_location(&self) -> Option<crate::debugger::source_map::SourceLocation> {
        let state = self.state.lock().ok()?;
        let inst = state.current_instruction()?;
        self.lookup_source_location(inst.offset)
    }
    /// Create a new debugger engine.
    #[tracing::instrument(skip_all)]
    pub fn new(executor: ContractExecutor, initial_breakpoints: Vec<String>) -> Self {
        Self::new_with_log_points(executor, initial_breakpoints, Vec::new())
    }

    pub fn new_with_log_points(
        executor: ContractExecutor,
        initial_breakpoints: Vec<String>,
        initial_log_points: Vec<BreakpointSpec>,
    ) -> Self {
        let mut breakpoints = BreakpointManager::new();

        for bp in initial_breakpoints {
            breakpoints.add_simple(&bp);
            info!("Breakpoint set at function: {}", bp);
        }

        for lp in initial_log_points {
            breakpoints.add_spec(lp.clone());
            info!(
                "Log point set at function: {} with message: {}",
                lp.function,
                lp.log_message.as_deref().unwrap_or("")
            );
        }

        Self {
            executor,
            breakpoints,
            state: Arc::new(Mutex::new(DebugState::new())),
            stepper: Stepper::new(),
            instrumenter: Instrumenter::new(),
            source_map: None,
            paused: false,
            instruction_debug_enabled: false,
        }
    }

    /// Best-effort DWARF source map loading.
    ///
    /// Missing or malformed debug information does not fail execution; it simply leaves the
    /// engine without source mappings.
    ///
    /// The existing `SourceMap` instance is **reused** across calls so that its
    /// internal parse cache is preserved.  If the WASM bytes have not changed
    /// since the last load, the DWARF sections are not re-parsed.
    pub fn try_load_source_map(&mut self, wasm_bytes: &[u8]) {
        // Reuse the existing instance to keep the hash-based parse cache alive.
        let mut source_map = self.source_map.take().unwrap_or_default();
        match source_map.load(wasm_bytes) {
            Ok(()) => {
                self.source_map = Some(source_map);
            }
            Err(error) => {
                tracing::warn!(error = %error, "Failed to load source map");
                self.source_map = None;
            }
        }
    }

    pub fn source_map(&self) -> Option<&SourceMap> {
        self.source_map.as_ref()
    }

    pub fn source_map_mut(&mut self) -> Option<&mut SourceMap> {
        self.source_map.as_mut()
    }

    pub fn lookup_source_location(&self, wasm_offset: usize) -> Option<SourceLocation> {
        self.source_map.as_ref()?.lookup(wasm_offset)
    }

    fn check_repeated_pause(&mut self) -> Result<()> {
        if self.stepper.pause_repeat_count() >= 5 {
            self.stepper.reset_pause_count();
            return Err(miette::miette!(
                "Repeated identical pause state detected (no progress). If this is intentional, you may issue the continue or step command again."
            ));
        }
        Ok(())
    }

    /// Enable instruction-level debugging.
    pub fn enable_instruction_debug(&mut self, wasm_bytes: &[u8]) -> Result<()> {
        self.try_load_source_map(wasm_bytes);

        let instructions = self
            .instrumenter
            .parse_instructions(wasm_bytes)
            .map_err(|e| miette::miette!("Failed to parse instructions: {}", e))?
            .to_vec();

        if let Ok(mut state) = self.state.lock() {
            state.set_instructions(instructions);
            state.enable_instruction_debug();
        }

        self.instrumenter.enable();
        self.instruction_debug_enabled = true;
        Ok(())
    }

    pub fn load_source_map(&mut self, wasm_bytes: &[u8]) -> Result<()> {
        // Reuse the existing instance to keep the hash-based parse cache alive.
        let mut source_map = self.source_map.take().unwrap_or_default();
        source_map.load(wasm_bytes)?;
        self.source_map = Some(source_map);
        Ok(())
    }

    /// Disable instruction-level debugging.
    pub fn disable_instruction_debug(&mut self) {
        self.instrumenter.disable();
        self.instrumenter.remove_hook();
        if let Ok(mut state) = self.state.lock() {
            state.disable_instruction_debug();
        }
        self.instruction_debug_enabled = false;
    }

    /// Check if instruction-level debugging is enabled.
    pub fn is_instruction_debug_enabled(&self) -> bool {
        self.instruction_debug_enabled
    }

    /// Execute a contract function with debugging.
    #[tracing::instrument(skip(self), fields(function = function))]
    pub fn execute(&mut self, function: &str, args: Option<&str>) -> Result<String> {
        self.execute_internal(function, args, true)
    }

    pub fn execute_without_breakpoints(
        &mut self,
        function: &str,
        args: Option<&str>,
    ) -> Result<String> {
        self.execute_internal(function, args, false)
    }

    fn execute_internal(
        &mut self,
        function: &str,
        args: Option<&str>,
        check_breakpoints: bool,
    ) -> Result<String> {
        info!("Executing function: {}", function);
        self.paused = false;

        if let Ok(mut state) = self.state.lock() {
            state.clear_pause_reason();
            state.set_current_function(
                function.to_string(),
                args.map(str::to_string),
                Some(InvocationReason::Entrypoint),
            );
            state.call_stack_mut().clear();
            state.call_stack_mut().push(function.to_string(), None);
        }

        let mut plugin_ctx = EventContext::new();
        plugin_ctx.stack_depth = self
            .state
            .lock()
            .map(|s| s.call_stack().get_stack().len())
            .unwrap_or(0);
        plugin_ctx.is_paused = self.paused;
        crate::plugin::registry::dispatch_global_event(
            &ExecutionEvent::BeforeFunctionCall {
                function: function.to_string(),
                args: args.map(str::to_string),
            },
            &mut plugin_ctx,
        );

        if check_breakpoints {
            let storage = self.executor.get_storage_snapshot().unwrap_or_default();
            let evaluator = EngineConditionEvaluator::new(storage, function, args);
            match self
                .breakpoints
                .should_break_with_context(function, &evaluator)
            {
                Ok((should_pause, log_message)) => {
                    if let Some(msg) = log_message {
                        // Log point hit - output message but don't pause
                        crate::logging::log_breakpoint_log(function, &msg);
                        println!("[LOG @{}] {}", function, msg);
                    }
                    if should_pause {
                        let condition = self
                            .breakpoints
                            .get_breakpoint(function)
                            .and_then(|bp| bp.condition.clone());
                        self.pause_at_function(function, condition);
                    }
                }
                Err(e) => {
                    tracing::warn!("Breakpoint evaluation failed: {}", e);
                }
            }
        }

        let start_time = std::time::Instant::now();
        let result = self.executor.execute(function, args);
        let duration = start_time.elapsed();

        self.update_call_stack(duration)?;

        let event_result = match &result {
            Ok(output) => Ok(output.clone()),
            Err(e) => Err(e.to_string()),
        };
        crate::plugin::registry::dispatch_global_event(
            &ExecutionEvent::AfterFunctionCall {
                function: function.to_string(),
                result: event_result,
                duration,
            },
            &mut plugin_ctx,
        );

        if let Err(ref e) = result {
            tracing::error!("Execution failed: {}", e);
            self.paused = true;
            if let Ok(mut state) = self.state.lock() {
                state.set_pause_reason(PauseReason::Panic);
            }
            let mut plugin_ctx = EventContext::new();
            plugin_ctx.is_paused = true;
            crate::plugin::registry::dispatch_global_event(
                &ExecutionEvent::ExecutionPaused {
                    reason: PauseReason::Panic.as_str().to_string(),
                },
                &mut plugin_ctx,
            );
            if let Ok(state) = self.state.lock() {
                state.call_stack().display();
            }
        } else if self.is_paused() {
            if let Ok(state) = self.state.lock() {
                state.call_stack().display();
            }
            self.check_repeated_pause()?;
        }

        result
    }

    pub fn prepare_breakpoint_stop(&mut self, function: &str, args: Option<&str>) {
        if let Ok(mut state) = self.state.lock() {
            state.set_current_function(
                function.to_string(),
                args.map(str::to_string),
                Some(InvocationReason::Entrypoint),
            );
            state.call_stack_mut().clear();
            state.call_stack_mut().push(function.to_string(), None);
            state.set_pause_reason(PauseReason::Breakpoint);
        }

        crate::logging::log_breakpoint(function);
        self.paused = true;

        let mut plugin_ctx = EventContext::new();
        plugin_ctx.stack_depth = 1;
        plugin_ctx.is_paused = true;
        let condition = self
            .breakpoints
            .get_breakpoint(function)
            .and_then(|bp| bp.condition.as_ref().map(|c| format!("{:?}", c)));
        let hit_count = self
            .breakpoints
            .get_breakpoint(function)
            .map(|bp| bp.hit_count)
            .unwrap_or(0);

        crate::plugin::registry::dispatch_global_event(
            &ExecutionEvent::BreakpointHit {
                function: function.to_string(),
                condition,
                hit_count,
            },
            &mut plugin_ctx,
        );
        crate::plugin::registry::dispatch_global_event(
            &ExecutionEvent::ExecutionPaused {
                reason: PauseReason::Breakpoint.as_str().to_string(),
            },
            &mut plugin_ctx,
        );
    }

    /// Stage an execution so the debugger starts in a paused state without
    /// emitting a breakpoint log event.
    pub fn stage_execution(&mut self, function: &str, args: Option<&str>) {
        if let Ok(mut state) = self.state.lock() {
            state.set_current_function(
                function.to_string(),
                args.map(str::to_string),
                Some(InvocationReason::Entrypoint),
            );
            state.call_stack_mut().clear();
            state.call_stack_mut().push(function.to_string(), None);
            state.set_pause_reason(PauseReason::UserInterrupt);
        }

        self.paused = true;
    }

    fn update_call_stack(&mut self, total_duration: std::time::Duration) -> Result<()> {
        let events = self.executor.get_diagnostic_events()?;

        let current_func = if let Ok(state) = self.state.lock() {
            state.current_function().unwrap_or("entry").to_string()
        } else {
            "entry".to_string()
        };

        if let Ok(mut state) = self.state.lock() {
            let stack = state.call_stack_mut();
            stack.clear();
            stack.push(current_func, None);

            for event in events {
                // Check if this is a diagnostic event by examining the event topics
                if let Some(first_topic) = self.get_first_event_topic(&event) {
                    if first_topic == "fn_call" {
                        // This is a cross-contract call
                        let contract_id =
                            event.contract_id.as_ref().map(|cid| format!("{:?}", cid));
                        stack.push("nested_call".to_string(), contract_id);
                    } else if first_topic == "fn_return" && stack.get_stack().len() > 1 {
                        // This is a return from a cross-contract call
                        stack.pop();
                    }
                }
            }

            if let Some(mut frame) = stack.pop() {
                frame.duration = Some(total_duration);
                stack.push_frame(frame);
            }
        }

        Ok(())
    }

    /// Extract the first topic from a ContractEvent as a string, if available
    fn get_first_event_topic(
        &self,
        event: &soroban_env_host::xdr::ContractEvent,
    ) -> Option<String> {
        match &event.body {
            soroban_env_host::xdr::ContractEventBody::V0(v0) => {
                if let Some(first_topic) = v0.topics.first() {
                    // Check if the topic is a Symbol and extract its value
                    match first_topic {
                        soroban_env_host::xdr::ScVal::Symbol(sym) => {
                            // Convert the symbol bytes to a string
                            String::from_utf8(sym.0.to_vec()).ok()
                        }
                        _ => {
                            // For non-symbol topics, fall back to debug format
                            Some(format!("{:?}", first_topic))
                        }
                    }
                } else {
                    None
                }
            }
        }
    }

    /// Step into next instruction.
    pub fn step_into(&mut self) -> Result<bool> {
        if !self.instruction_debug_enabled {
            return Err(miette::miette!("Instruction debugging not enabled"));
        }

        let stepped = if let Ok(mut state) = self.state.lock() {
            self.stepper.step_into(&mut state)
        } else {
            false
        };
        self.paused = stepped;
        if let Ok(mut state) = self.state.lock() {
            if stepped {
                state.set_pause_reason(PauseReason::StepBoundary);
            } else {
                state.set_pause_reason(PauseReason::EndOfExecution);
            }
        }
        if stepped {
            self.check_repeated_pause()?;
        }
        Ok(stepped)
    }

    /// Step over function calls.
    pub fn step_over(&mut self) -> Result<bool> {
        if !self.instruction_debug_enabled {
            return Err(miette::miette!("Instruction debugging not enabled"));
        }

        let stepped = if let Ok(mut state) = self.state.lock() {
            self.stepper.step_over(&mut state)
        } else {
            false
        };
        self.paused = stepped;
        if let Ok(mut state) = self.state.lock() {
            if stepped {
                state.set_pause_reason(PauseReason::StepBoundary);
            } else {
                state.set_pause_reason(PauseReason::EndOfExecution);
            }
        }
        if stepped {
            self.check_repeated_pause()?;
        }
        Ok(stepped)
    }

    /// Step to the next basic block boundary.
    pub fn step_block(&mut self) -> Result<bool> {
        if !self.instruction_debug_enabled {
            return Err(miette::miette!("Instruction debugging not enabled"));
        }

        let stepped = if let Ok(mut state) = self.state.lock() {
            self.stepper.step_block(&mut state)
        } else {
            false
        };
        self.paused = stepped;
        if let Ok(mut state) = self.state.lock() {
            if stepped {
                state.set_pause_reason(PauseReason::StepBoundary);
            } else {
                state.set_pause_reason(PauseReason::EndOfExecution);
            }
        }
        if stepped {
            self.check_repeated_pause()?;
        }
        Ok(stepped)
    }

    pub fn step_over_source_line(&mut self) -> Result<StepOverResult> {
        if !self.instruction_debug_enabled {
            return Err(miette::miette!("Instruction debugging not enabled"));
        }

        let (paused, location) = if let (Ok(mut state), Some(source_map)) =
            (self.state.lock(), self.source_map.as_ref())
        {
            let advanced = self.stepper.step_over_source_line(&mut state, source_map);
            let loc = state
                .current_instruction()
                .and_then(|i| source_map.lookup(i.offset));
            (advanced, loc)
        } else {
            (false, None)
        };

        self.paused = paused;
        if let Ok(mut state) = self.state.lock() {
            if paused {
                state.set_pause_reason(PauseReason::StepBoundary);
            } else {
                state.set_pause_reason(PauseReason::EndOfExecution);
            }
        }
        if paused {
            self.check_repeated_pause()?;
        }
        Ok(StepOverResult { paused, location })
    }

    /// Step out of current function.
    pub fn step_out(&mut self) -> Result<bool> {
        if !self.instruction_debug_enabled {
            return Err(miette::miette!("Instruction debugging not enabled"));
        }

        let stepped = if let Ok(mut state) = self.state.lock() {
            self.stepper.step_out(&mut state)
        } else {
            false
        };
        self.paused = stepped;
        if let Ok(mut state) = self.state.lock() {
            if stepped {
                state.set_pause_reason(PauseReason::StepBoundary);
            } else {
                state.set_pause_reason(PauseReason::EndOfExecution);
            }
        }
        if stepped {
            self.check_repeated_pause()?;
        }
        Ok(stepped)
    }

    /// Step back to previous instruction.
    pub fn step_back(&mut self) -> Result<bool> {
        if !self.instruction_debug_enabled {
            return Err(miette::miette!("Instruction debugging not enabled"));
        }

        let stepped = if let Ok(mut state) = self.state.lock() {
            self.stepper.step_back(&mut state)
        } else {
            false
        };
        self.paused = stepped;
        if let Ok(mut state) = self.state.lock() {
            if stepped {
                state.set_pause_reason(PauseReason::StepBoundary);
            } else {
                state.set_pause_reason(PauseReason::EndOfExecution);
            }
        }
        if stepped {
            self.check_repeated_pause()?;
        }
        Ok(stepped)
    }

    /// Start instruction stepping with given mode.
    pub fn start_instruction_stepping(&mut self, mode: StepMode) -> Result<()> {
        if !self.instruction_debug_enabled {
            return Err(miette::miette!("Instruction debugging not enabled"));
        }

        if let Ok(mut state) = self.state.lock() {
            self.stepper.start(mode, &mut state);
            self.paused = true;
            state.set_pause_reason(PauseReason::StepBoundary);
        }

        Ok(())
    }

    /// Continue execution until next breakpoint.
    pub fn continue_execution(&mut self) -> Result<()> {
        self.paused = false;
        if let Ok(mut state) = self.state.lock() {
            self.stepper.continue_execution(&mut state);
            state.clear_pause_reason();
        }

        let mut plugin_ctx = EventContext::new();
        plugin_ctx.is_paused = false;
        crate::plugin::registry::dispatch_global_event(
            &ExecutionEvent::ExecutionResumed,
            &mut plugin_ctx,
        );
        Ok(())
    }

    fn pause_at_function(&mut self, function: &str, condition: Option<String>) {
        crate::logging::log_breakpoint(function);
        self.paused = true;

        if let Ok(mut state) = self.state.lock() {
            state.set_current_function(
                function.to_string(),
                None,
                Some(InvocationReason::Entrypoint),
            );
            state.set_pause_reason(PauseReason::Breakpoint);
            state.call_stack().display();
        }

        let mut plugin_ctx = EventContext::new();
        plugin_ctx.is_paused = true;
        crate::plugin::registry::dispatch_global_event(
            &ExecutionEvent::BreakpointHit {
                function: function.to_string(),
                condition,
                hit_count: self
                    .breakpoints
                    .get_breakpoint(function)
                    .map(|bp| bp.hit_count)
                    .unwrap_or(0),
            },
            &mut plugin_ctx,
        );
        crate::plugin::registry::dispatch_global_event(
            &ExecutionEvent::ExecutionPaused {
                reason: PauseReason::Breakpoint.as_str().to_string(),
            },
            &mut plugin_ctx,
        );
    }

    pub fn pause_reason(&self) -> Option<PauseReason> {
        self.state
            .lock()
            .ok()
            .and_then(|state| state.pause_reason())
    }

    pub fn pause_reason_label(&self) -> Option<&'static str> {
        self.pause_reason().map(PauseReason::as_str)
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn state(&self) -> Arc<Mutex<DebugState>> {
        Arc::clone(&self.state)
    }

    pub fn current_instruction(&self) -> Option<Instruction> {
        self.state
            .lock()
            .ok()
            .and_then(|state| state.current_instruction().cloned())
    }

    pub fn get_instruction_context(&self, context_size: usize) -> Vec<(usize, Instruction, bool)> {
        if let Ok(state) = self.state.lock() {
            state
                .get_instruction_context(context_size)
                .into_iter()
                .map(|(idx, inst, current)| (idx, inst.clone(), current))
                .collect()
        } else {
            Vec::new()
        }
    }

    pub fn breakpoints_mut(&mut self) -> &mut BreakpointManager {
        &mut self.breakpoints
    }

    pub fn breakpoints(&self) -> &BreakpointManager {
        &self.breakpoints
    }

    pub fn executor(&self) -> &ContractExecutor {
        &self.executor
    }

    pub fn executor_mut(&mut self) -> &mut ContractExecutor {
        &mut self.executor
    }

    /// Compatibility method for non-instruction stepping.
    pub fn step(&mut self) -> Result<()> {
        if self.instruction_debug_enabled {
            let _ = self.step_into()?;
        }
        if let Ok(mut state) = self.state.lock() {
            state.increment_step();
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "engine_test.rs"]
mod engine_test;
