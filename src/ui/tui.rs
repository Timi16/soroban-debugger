use crate::debugger::engine::DebuggerEngine;
use crate::inspector::BudgetInspector;
use crate::inspector::{storage::StorageQuery, StorageInspector};
use crate::Result;
use std::io::{self, Write};

#[derive(Debug, Clone)]
struct PendingExecution {
    function: String,
    args: Option<String>,
}

#[derive(Debug, Clone)]
struct StorageDisplayOptions {
    filter: Option<String>,
    jump_to: Option<String>,
    page: usize,
    page_size: usize,
}

impl Default for StorageDisplayOptions {
    fn default() -> Self {
        Self {
            filter: None,
            jump_to: None,
            page: 1,
            page_size: 25,
        }
    }
}

/// Terminal user interface for interactive debugging.
pub struct DebuggerUI {
    engine: DebuggerEngine,
    config: crate::config::Config,
    pending_execution: Option<PendingExecution>,
    last_output: Option<String>,
    last_error: Option<String>,
}

impl DebuggerUI {
    pub fn new(engine: DebuggerEngine) -> crate::Result<Self> {
        Ok(Self {
            engine,
            config: crate::config::Config::load_or_default(),
            pending_execution: None,
            last_output: None,
            last_error: None,
        })
    }

    /// Stage an execution so the session starts "paused" before running.
    ///
    /// Use `continue` to execute the staged call.
    pub fn queue_execution(&mut self, function: String, args: Option<String>) {
        self.engine.stage_execution(&function, args.as_deref());
        self.pending_execution = Some(PendingExecution { function, args });
        self.last_output = None;
        self.last_error = None;
    }

    pub fn last_output(&self) -> Option<&str> {
        self.last_output.as_deref()
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Run the interactive UI loop.
    pub fn run(&mut self) -> Result<()> {
        self.print_help();

        loop {
            print!("\n(debug) ");
            io::stdout().flush().map_err(|e| {
                crate::DebuggerError::IoError(format!("Failed to flush stdout: {}", e))
            })?;

            let mut input = String::new();
            let bytes_read = io::stdin().read_line(&mut input).map_err(|e| {
                crate::DebuggerError::IoError(format!("Failed to read line: {}", e))
            })?;
            if bytes_read == 0 {
                tracing::info!("Input stream closed; exiting debugger");
                break;
            }

            let command = input.trim();
            if command.is_empty() {
                continue;
            }

            match self.handle_command(command) {
                Ok(should_exit) => {
                    if should_exit {
                        break;
                    }
                }
                Err(e) => {
                    tracing::error!(error = %e, "Command execution error");
                }
            }
        }

        Ok(())
    }

    pub fn handle_command(&mut self, command: &str) -> Result<bool> {
        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.is_empty() {
            return Ok(false);
        }

        let cmd = parts[0];
        let kb = &self.config.keybindings;

        match cmd {
            c if c == kb.step || c == "step" => {
                self.engine.step()?;
                if let Ok(state) = self.engine.state().lock() {
                    crate::logging::log_step(state.step_count() as u64);
                }
            }
            c if c == kb.continue_exec || c == "continue" => {
                if let Some(pending) = self.pending_execution.take() {
                    match self
                        .engine
                        .execute_without_breakpoints(&pending.function, pending.args.as_deref())
                    {
                        Ok(output) => {
                            self.last_error = None;
                            self.last_output = Some(output.clone());
                            crate::logging::log_display(
                                format!("Result: {}", output),
                                crate::logging::LogLevel::Info,
                            );
                        }
                        Err(e) => {
                            self.last_output = None;
                            self.last_error = Some(e.to_string());
                            crate::logging::log_display(
                                format!("Error: {}", e),
                                crate::logging::LogLevel::Error,
                            );
                        }
                    }
                } else {
                    self.engine.continue_execution()?;
                    tracing::info!("Execution continuing");
                }
            }
            c if c == kb.inspect || c == "inspect" => {
                self.inspect();
            }
            "run" => {
                if parts.len() < 2 {
                    tracing::warn!("run command missing function name");
                } else {
                    let function = parts[1].to_string();
                    let args = if parts.len() > 2 {
                        // Extract raw arguments from the original command string
                        // to preserve internal whitespace and quotes.
                        let mut current_pos = 0;
                        // Skip "run" and function name tokens in the original string.
                        let tokens = [parts[0], parts[1]];
                        for token in tokens {
                            if let Some(pos) = command[current_pos..].find(token) {
                                current_pos += pos + token.len();
                            }
                        }
                        let raw_args = command[current_pos..].trim();
                        if raw_args.is_empty() {
                            None
                        } else {
                            Some(raw_args.to_string())
                        }
                    } else {
                        None
                    };
                    self.queue_execution(function, args);
                }
            }
            "storage" => {
                let options = Self::parse_storage_display_options(&parts[1..])?;
                self.display_storage(&options)?;
            }
            "stack" => {
                if let Ok(state) = self.engine.state().lock() {
                    crate::inspector::CallStackInspector::display_frames(
                        state.call_stack().get_stack(),
                    );
                }
            }
            "budget" => {
                BudgetInspector::display(self.engine.executor().host());
            }
            "diag" | "diagnostics" => {
                self.display_diagnostics();
            }
            "break" => {
                if parts.len() < 2 {
                    tracing::warn!("breakpoint set without function name");
                } else {
                    self.engine.breakpoints_mut().add_simple(parts[1]);
                    crate::logging::log_breakpoint_set(parts[1]);
                }
            }
            "list-breaks" => {
                let breakpoints = self.engine.breakpoints_mut().list_detailed();
                if breakpoints.is_empty() {
                    crate::logging::log_display(
                        "No breakpoints set",
                        crate::logging::LogLevel::Info,
                    );
                } else {
                    for bp in breakpoints {
                        let cond_str = bp
                            .condition
                            .clone()
                            .map(|c| format!(" (if {:?})", c))
                            .unwrap_or_default();
                        crate::logging::log_display(
                            format!("- {}{} hits={}", bp.function, cond_str, bp.hit_count),
                            crate::logging::LogLevel::Info,
                        );
                    }
                }
            }
            "clear" => {
                if parts.len() < 2 {
                    tracing::warn!("clear command missing function name");
                } else if self.engine.breakpoints_mut().remove_function(parts[1]) {
                    crate::logging::log_breakpoint_cleared(parts[1]);
                } else {
                    tracing::debug!(breakpoint = parts[1], "No breakpoint found at function");
                }
            }
            "palette" => {
                self.show_palette()?;
            }
            "help" => self.print_help(),
            c if c == kb.quit || c == "quit" || c == "exit" => {
                tracing::info!("Exiting debugger");
                return Ok(true);
            }
            _ => tracing::warn!(command = cmd, "Unknown command"),
        }

        Ok(false)
    }

    fn inspect(&self) {
        crate::logging::log_display("\n=== Current State ===", crate::logging::LogLevel::Info);
        if let Ok(state) = self.engine.state().lock() {
            let pause_reason = state.pause_reason().map(|reason| reason.as_str());
            if let Some(func) = state.current_function() {
                crate::logging::log_display(
                    format!("Function: {}", func),
                    crate::logging::LogLevel::Info,
                );
            } else {
                crate::logging::log_display("Function: (none)", crate::logging::LogLevel::Info);
            }
            crate::logging::log_display(
                format!("Steps: {}", state.step_count()),
                crate::logging::LogLevel::Info,
            );
            crate::logging::log_display(
                format!("Paused: {}", self.engine.is_paused()),
                crate::logging::LogLevel::Info,
            );
            if let Some(reason) = pause_reason {
                crate::logging::log_display(
                    format!("Pause reason: {}", reason),
                    crate::logging::LogLevel::Info,
                );
            }
            if let Some(output) = &self.last_output {
                crate::logging::log_display(
                    format!("Last result: {}", output),
                    crate::logging::LogLevel::Info,
                );
            } else if let Some(error) = &self.last_error {
                crate::logging::log_display(
                    format!("Last error: {}", error),
                    crate::logging::LogLevel::Info,
                );
            }
            crate::logging::log_display("", crate::logging::LogLevel::Info);
            crate::inspector::CallStackInspector::display_frames(state.call_stack().get_stack());
        } else {
            crate::logging::log_display("State unavailable", crate::logging::LogLevel::Info);
        }
    }

    fn display_storage(&self, options: &StorageDisplayOptions) -> Result<()> {
        let entries = self.engine.executor().get_storage_snapshot()?;

        if entries.is_empty() {
            crate::logging::log_display("Storage is empty", crate::logging::LogLevel::Warn);
            return Ok(());
        }

        let sorted_entries = StorageInspector::sorted_entries_from_map(&entries);
        let query = StorageQuery {
            filter: options.filter.clone(),
            jump_to: options.jump_to.clone(),
            page: options.page.saturating_sub(1),
            page_size: options.page_size,
        };
        let page = StorageInspector::build_page(&sorted_entries, &query);

        crate::logging::log_display("", crate::logging::LogLevel::Info);
        crate::logging::log_display("=== Contract Storage ===", crate::logging::LogLevel::Info);
        crate::logging::log_display(
            format!(
                "Page {}/{}  showing {} of {} filtered entries ({} total)",
                page.page + 1,
                page.total_pages,
                page.entries.len(),
                page.filtered_entries,
                page.total_entries
            ),
            crate::logging::LogLevel::Info,
        );
        if let Some(filter) = query.normalized_filter() {
            crate::logging::log_display(
                format!("Filter: {}", filter),
                crate::logging::LogLevel::Info,
            );
        }
        if let Some(jump) = query.normalized_jump() {
            let jump_status = if let Some(index) = page.jump_match_index {
                format!("Jump target: {} (matched entry #{})", jump, index + 1)
            } else {
                format!("Jump target: {} (no match found)", jump)
            };
            crate::logging::log_display(jump_status, crate::logging::LogLevel::Info);
        }
        crate::logging::log_display("", crate::logging::LogLevel::Info);

        if page.entries.is_empty() {
            crate::logging::log_display(
                "No storage entries matched the current view",
                crate::logging::LogLevel::Info,
            );
        }

        for (offset, (key, value)) in page.entries.iter().enumerate() {
            let absolute_index = page.page_start + offset + 1;
            let prefix = if page.jump_match_index == Some(page.page_start + offset) {
                ">"
            } else {
                " "
            };
            crate::logging::log_display(
                format!("{} {:>4}. {}: {}", prefix, absolute_index, key, value),
                crate::logging::LogLevel::Info,
            );
        }
        crate::logging::log_display("", crate::logging::LogLevel::Info);

        Ok(())
    }

    fn display_diagnostics(&self) {
        let budget = BudgetInspector::get_cpu_usage(self.engine.executor().host());
        let diagnostics = crate::output::collect_runtime_diagnostics(
            self.engine.source_map().is_some(),
            &budget,
            self.last_error(),
        );

        if diagnostics.is_empty() {
            crate::logging::log_display("No active diagnostics", crate::logging::LogLevel::Info);
            return;
        }

        crate::logging::log_display("", crate::logging::LogLevel::Info);
        crate::logging::log_display("=== Diagnostics ===", crate::logging::LogLevel::Info);
        crate::logging::log_display("", crate::logging::LogLevel::Info);

        for diagnostic in diagnostics {
            crate::logging::log_display(
                diagnostic.display_line(),
                match diagnostic.severity {
                    crate::output::DiagnosticSeverity::Notice => crate::logging::LogLevel::Info,
                    crate::output::DiagnosticSeverity::Warning => crate::logging::LogLevel::Warn,
                    crate::output::DiagnosticSeverity::Error => crate::logging::LogLevel::Error,
                },
            );
        }

        crate::logging::log_display("", crate::logging::LogLevel::Info);
    }

    fn parse_storage_display_options(parts: &[&str]) -> Result<StorageDisplayOptions> {
        let mut options = StorageDisplayOptions::default();
        let mut idx = 0;

        while idx < parts.len() {
            match parts[idx] {
                "--page" => {
                    idx += 1;
                    let value = parts
                        .get(idx)
                        .ok_or_else(|| miette::miette!("--page requires a value"))?;
                    options.page = value
                        .parse::<usize>()
                        .map_err(|_| miette::miette!("Invalid value for --page: {}", value))?
                        .max(1);
                }
                "--page-size" => {
                    idx += 1;
                    let value = parts
                        .get(idx)
                        .ok_or_else(|| miette::miette!("--page-size requires a value"))?;
                    options.page_size = value
                        .parse::<usize>()
                        .map_err(|_| miette::miette!("Invalid value for --page-size: {}", value))?
                        .max(1);
                }
                "--jump" => {
                    idx += 1;
                    let value = parts
                        .get(idx)
                        .ok_or_else(|| miette::miette!("--jump requires a value"))?;
                    options.jump_to = Some((*value).to_string());
                }
                value if value.starts_with("--") => {
                    return Err(miette::miette!("Unknown storage option: {}", value));
                }
                value => {
                    let current = options.filter.get_or_insert_with(String::new);
                    if !current.is_empty() {
                        current.push(' ');
                    }
                    current.push_str(value);
                }
            }
            idx += 1;
        }

        Ok(options)
    }

    fn show_palette(&self) -> Result<()> {
        crate::logging::log_display("", crate::logging::LogLevel::Info);
        crate::logging::log_display("=== Command Palette ===", crate::logging::LogLevel::Info);
        crate::logging::log_display("  run <func> [args]", crate::logging::LogLevel::Info);
        crate::logging::log_display("  storage [query]", crate::logging::LogLevel::Info);
        crate::logging::log_display("  diagnostics", crate::logging::LogLevel::Info);
        crate::logging::log_display("  list-breaks", crate::logging::LogLevel::Info);
        crate::logging::log_display("", crate::logging::LogLevel::Info);
        Ok(())
    }

    fn print_help(&self) {
        let kb = &self.config.keybindings;

        crate::logging::log_display(
            "Interactive debugger commands:",
            crate::logging::LogLevel::Info,
        );
        crate::logging::log_display(
            format!("  step | {:<11} Step execution", kb.step),
            crate::logging::LogLevel::Info,
        );
        crate::logging::log_display(
            format!("  continue | {:<7} Continue execution", kb.continue_exec),
            crate::logging::LogLevel::Info,
        );
        crate::logging::log_display(
            format!("  inspect | {:<8} Show current state", kb.inspect),
            crate::logging::LogLevel::Info,
        );
        crate::logging::log_display(
            "  run <func> [args]  Stage a function call",
            crate::logging::LogLevel::Info,
        );
        crate::logging::log_display(
            "  storage [query] [--page N] [--page-size N] [--jump KEY]",
            crate::logging::LogLevel::Info,
        );
        crate::logging::log_display(
            "  stack              Show call stack",
            crate::logging::LogLevel::Info,
        );
        crate::logging::log_display(
            "  budget             Show budget usage",
            crate::logging::LogLevel::Info,
        );
        crate::logging::log_display(
            "  diagnostics | diag Show active diagnostics",
            crate::logging::LogLevel::Info,
        );
        crate::logging::log_display(
            "  break <func> [cond] Set breakpoint with optional condition",
            crate::logging::LogLevel::Info,
        );
        crate::logging::log_display(
            "  list-breaks        List breakpoints with hit counts",
            crate::logging::LogLevel::Info,
        );
        crate::logging::log_display(
            "  clear <func>       Clear breakpoint",
            crate::logging::LogLevel::Info,
        );
        crate::logging::log_display(
            "  palette            Open command palette",
            crate::logging::LogLevel::Info,
        );
        crate::logging::log_display(
            "  help               Show this help",
            crate::logging::LogLevel::Info,
        );
        crate::logging::log_display(
            format!("  quit | {:<11} Exit debugger", kb.quit),
            crate::logging::LogLevel::Info,
        );
    }
}
