use crate::debugger::instruction_pointer::StepMode;
use crate::runtime::instruction::Instruction;
use crossterm::style::Stylize;
use std::sync::atomic::{AtomicBool, Ordering};

/// Pretty printing utilities for debugger output
pub struct Formatter;

impl Formatter {
    /// Format a value for display.
    pub fn format_value(value: &str) -> String {
        value.to_string()
    }

    /// Format storage key-value pair.
    pub fn format_storage_entry(key: &str, value: &str) -> String {
        format!("{} = {}", key, value)
    }

    /// Format a function call.
    pub fn format_function_call(name: &str, args: Option<&str>) -> String {
        if let Some(args) = args {
            format!("{}({})", name, args)
        } else {
            format!("{}()", name)
        }
    }

    /// Format budget information.
    pub fn format_budget(cpu: u64, cpu_limit: u64, mem: u64, mem_limit: u64) -> String {
        let cpu_pct = if cpu_limit == 0 {
            0.0
        } else {
            (cpu as f64 / cpu_limit as f64) * 100.0
        };
        let mem_pct = if mem_limit == 0 {
            0.0
        } else {
            (mem as f64 / mem_limit as f64) * 100.0
        };

        format!(
            "CPU: {}/{} ({:.1}%) | Memory: {}/{} bytes ({:.1}%)",
            cpu, cpu_limit, cpu_pct, mem, mem_limit, mem_pct
        )
    }

    /// Format a single instruction for display.
    pub fn format_instruction(instruction: &Instruction, is_current: bool) -> String {
        let prefix = if is_current { "►" } else { " " };
        let operands = instruction.operands();

        if operands.is_empty() {
            format!(
                "{} {:08x}: {}",
                prefix,
                instruction.offset,
                instruction.name()
            )
        } else {
            format!(
                "{} {:08x}: {} {}",
                prefix,
                instruction.offset,
                instruction.name(),
                operands
            )
        }
    }

    /// Format instruction context with surrounding instructions.
    pub fn format_instruction_context(
        context: &[(usize, Instruction, bool)],
        _context_size: usize,
    ) -> String {
        if context.is_empty() {
            return "No instructions available".to_string();
        }

        let mut lines = vec!["Instruction Context".to_string()];
        lines.extend(context.iter().map(|(idx, instruction, is_current)| {
            format!(
                "{:>4}: {}",
                idx,
                Self::format_instruction(instruction, *is_current)
            )
        }));
        lines.join("\n")
    }

    /// Format instruction pointer state.
    pub fn format_instruction_pointer_state(
        current_index: usize,
        call_depth: u32,
        step_mode: Option<StepMode>,
        is_stepping: bool,
    ) -> String {
        let mode = match step_mode {
            Some(StepMode::StepInto) => "Step Into",
            Some(StepMode::StepOver) => "Step Over",
            Some(StepMode::StepOut) => "Step Out",
            Some(StepMode::StepBlock) => "Step Block",
            None => "None",
        };

        format!(
            "Instruction Pointer\n  index: {}\n  call_depth: {}\n  step_mode: {}\n  stepping: {}",
            current_index,
            call_depth,
            mode,
            if is_stepping { "Active" } else { "Inactive" }
        )
    }

    /// Format instruction statistics.
    pub fn format_instruction_stats(
        total_instructions: usize,
        current_index: usize,
        instructions_executed: usize,
    ) -> String {
        let progress = if total_instructions == 0 {
            0.0
        } else {
            (current_index as f64 / total_instructions as f64) * 100.0
        };

        format!(
            "Execution Stats\n  total: {}\n  current: {}\n  executed: {}\n  progress: {:.1}%",
            total_instructions, current_index, instructions_executed, progress
        )
    }

    /// Format stepping help.
    pub fn format_stepping_help() -> String {
        [
            "Stepping commands:",
            "  n, next       Step to next instruction",
            "  s, step, into Step into calls",
            "  o, over       Step over calls",
            "  u, out        Step out of function",
            "  b, block      Step to next basic block",
            "  p, prev       Step back",
            "  c, continue   Continue execution",
            "  i, info       Show instruction state",
            "  ctx, context  Show instruction context",
            "  h, help       Show this help",
            "  q, quit       Exit stepping mode",
        ]
        .join("\n")
    }

    /// Format an informational message in blue.
    pub fn info(message: impl AsRef<str>) -> String {
        Self::apply_color(message.as_ref(), ColorKind::Info)
    }

    /// Format a success message in green.
    pub fn success(message: impl AsRef<str>) -> String {
        Self::apply_color(message.as_ref(), ColorKind::Success)
    }

    /// Format a warning message in yellow.
    pub fn warning(message: impl AsRef<str>) -> String {
        Self::apply_color(message.as_ref(), ColorKind::Warning)
    }

    /// Format an error message in red.
    pub fn error(message: impl AsRef<str>) -> String {
        Self::apply_color(message.as_ref(), ColorKind::Error)
    }

    /// Configure whether ANSI colors are enabled.
    pub fn configure_colors(enable: bool) {
        COLOR_ENABLED.store(enable, Ordering::Relaxed);
    }

    /// Auto-configure color output based on environment.
    pub fn configure_colors_from_env() {
        let no_color = std::env::var_os("NO_COLOR").is_some();
        Self::configure_colors(!no_color);
    }

    fn apply_color(message: &str, kind: ColorKind) -> String {
        if !COLOR_ENABLED.load(Ordering::Relaxed) {
            return message.to_string();
        }

        match kind {
            ColorKind::Info => format!("{}", message.blue()),
            ColorKind::Success => format!("{}", message.green()),
            ColorKind::Warning => format!("{}", message.yellow()),
            ColorKind::Error => format!("{}", message.red()),
        }
    }
}

#[derive(Copy, Clone)]
enum ColorKind {
    Info,
    Success,
    Warning,
    Error,
}

static COLOR_ENABLED: AtomicBool = AtomicBool::new(true);
