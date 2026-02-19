use crate::debugger::engine::DebuggerEngine;
use crate::inspector::{BudgetInspector, CallStackInspector, StorageInspector};
use crate::Result;
use std::io::{self, Write};

/// Terminal user interface for interactive debugging
pub struct DebuggerUI {
    engine: DebuggerEngine,
    storage_inspector: StorageInspector,
    stack_inspector: CallStackInspector,
}

impl DebuggerUI {
    pub fn new(engine: DebuggerEngine) -> Result<Self> {
        Ok(Self {
            engine,
            storage_inspector: StorageInspector::new(),
            stack_inspector: CallStackInspector::new(),
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
                    eprintln!("Error: {}", e);
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
                println!("Stepped");
            }
            "c" | "continue" => {
                self.engine.continue_execution()?;
                println!("Continuing...");
            }
            "i" | "inspect" => {
                self.inspect();
            }
            "storage" => {
                self.storage_inspector.display();
            }
            "stack" => {
                self.stack_inspector.display();
            }
            "budget" => {
                BudgetInspector::display(self.engine.executor().host());
            }
            "break" => {
                if parts.len() < 2 {
                    println!("Usage: break <function_name>");
                } else {
                    self.engine.breakpoints_mut().add(parts[1]);
                    println!("Breakpoint set at: {}", parts[1]);
                }
            }
            "list-breaks" => {
                let breakpoints = self.engine.breakpoints_mut().list();
                if breakpoints.is_empty() {
                    println!("No breakpoints set");
                } else {
                    println!("Breakpoints:");
                    for bp in breakpoints {
                        println!("  - {}", bp);
                    }
                }
            }
            "clear" => {
                if parts.len() < 2 {
                    println!("Usage: clear <function_name>");
                } else if self.engine.breakpoints_mut().remove(parts[1]) {
                    println!("Breakpoint removed: {}", parts[1]);
                } else {
                    println!("No breakpoint at: {}", parts[1]);
                }
            }
            "help" => {
                self.print_help();
            }
            "q" | "quit" | "exit" => {
                println!("Exiting debugger");
                return Ok(true);
            }
            _ => {
                println!(
                    "Unknown command: {}. Type 'help' for available commands.",
                    parts[0]
                );
            }
        }

        Ok(false)
    }

    /// Display current state
    fn inspect(&self) {
        println!("\n=== Current State ===");
        if let Some(func) = self.engine.state().current_function() {
            println!("Function: {}", func);
        } else {
            println!("Function: (none)");
        }
        println!("Steps: {}", self.engine.state().step_count());
        println!("Paused: {}", self.engine.is_paused());
    }

    /// Print help message
    fn print_help(&self) {
        println!("\nAvailable commands:");
        println!("  s, step              Execute next instruction");
        println!("  c, continue          Run until breakpoint or completion");
        println!("  i, inspect           Show current execution state");
        println!("  storage              Display contract storage");
        println!("  stack                Show call stack");
        println!("  budget               Show resource usage (CPU/memory)");
        println!("  break <function>     Set breakpoint at function");
        println!("  list-breaks          List all breakpoints");
        println!("  clear <function>     Remove breakpoint");
        println!("  help                 Show this help message");
        println!("  q, quit              Exit debugger");
    }
}
