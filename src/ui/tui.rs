use crate::debugger::engine::DebuggerEngine;
use crate::inspector::{BudgetInspector, StorageInspector};
use crate::Result;
use std::io::{self, Write};

/// Terminal user interface for interactive debugging
pub struct DebuggerUI {
    engine: DebuggerEngine,
    storage_inspector: StorageInspector,
}

impl DebuggerUI {
    pub fn new(engine: DebuggerEngine) -> Result<Self> {
        Ok(Self {
            engine,
            storage_inspector: StorageInspector::new(),
        })
    }

    /// Run the interactive UI loop
    pub fn run(&mut self) -> Result<()> {
        self.print_help();

        loop {
            print!("\n(debug) ");
            io::stdout().flush()?;

            let mut input = String::new();
            io::stdin().read_line(&mut input)?;

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

    /// Handle a single command
    fn handle_command(&mut self, command: &str) -> Result<bool> {
        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.is_empty() {
            return Ok(false);
        }

        match parts[0] {
            "s" | "step" => {
                self.engine.step()?;
                crate::logging::log_step(self.engine.state().step_count() as u64);
            }
            "c" | "continue" => {
                self.engine.continue_execution()?;
                tracing::info!("Execution continuing");
            }
            "i" | "inspect" => {
                self.inspect();
            }
            "storage" => {
                self.storage_inspector.display();
            }
            "stack" => {
                self.engine.state().call_stack().display();
            }
            "budget" => {
                BudgetInspector::display(self.engine.executor().host());
            }
            "break" => {
                if parts.len() < 2 {
                    tracing::warn!("breakpoint set without function name");
                } else {
                    self.engine.breakpoints_mut().add(parts[1]);
                    crate::logging::log_breakpoint_set(parts[1]);
                }
            }
            "list-breaks" => {
                let breakpoints = self.engine.breakpoints_mut().list();
                if !breakpoints.is_empty() {
                    for bp in breakpoints {
                        tracing::debug!(breakpoint = bp, "Active breakpoint");
                    }
                } else {
                    tracing::debug!("No breakpoints currently set");
                }
            }
            "clear" => {
                if parts.len() < 2 {
                    tracing::warn!("clear command missing function name");
                } else if self.engine.breakpoints_mut().remove(parts[1]) {
                    crate::logging::log_breakpoint_cleared(parts[1]);
                } else {
                    tracing::debug!(breakpoint = parts[1], "No breakpoint found at function");
                }
            }
            "help" => {
                self.print_help();
            }
            "q" | "quit" | "exit" => {
                tracing::info!("Exiting debugger");
                return Ok(true);
            }
            _ => {
                tracing::warn!(command = parts[0], "Unknown command");
            }
        }

        Ok(false)
    }

    /// Display current state
    fn inspect(&self) {
        let steps = self.engine.state().step_count();
        let paused = self.engine.is_paused();
        if let Some(func) = self.engine.state().current_function() {
            tracing::info!(function = func, steps = steps, paused = paused, "Current execution state");
        } else {
            tracing::info!(steps = steps, paused = paused, "Current execution state");
        }
        println!("Steps: {}", self.engine.state().step_count());
        println!("Paused: {}", self.engine.is_paused());

        println!();
        self.engine.state().call_stack().display();
    }

    /// Print help message
    fn print_help(&self) {
        tracing::info!("Interactive debugger commands: step, continue, inspect, storage, stack, budget, break, list-breaks, clear, help, quit");
    }
}
