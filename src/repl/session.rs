/// REPL session management with history and state
///
/// Handles user input, command history, and persistent state across
/// multiple function calls within a single REPL session.
use super::commands::ReplCommand;
use super::executor::ReplExecutor;
use super::ReplConfig;
use crate::ui::formatter::Formatter;
use crate::Result;
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::history::FileHistory;
use rustyline::validate::{ValidationContext, ValidationResult, Validator};
use rustyline::{Context, Editor, Helper};
use std::path::PathBuf;

/// REPL session state and editor
pub struct ReplSession {
    editor: Editor<ReplHelper, FileHistory>,
    config: ReplConfig,
    executor: ReplExecutor,
    history_path: PathBuf,
}

#[derive(Clone)]
struct ReplHelper {
    commands: Vec<String>,
    functions: Vec<String>,
}

impl ReplHelper {
    fn new(commands: Vec<String>, functions: Vec<String>) -> Self {
        Self {
            commands,
            functions,
        }
    }

    fn complete_from(candidates: &[String], prefix: &str) -> Vec<Pair> {
        candidates
            .iter()
            .filter(|candidate| candidate.starts_with(prefix))
            .map(|candidate| Pair {
                display: candidate.clone(),
                replacement: candidate.clone(),
            })
            .collect()
    }

    fn complete_for_input(&self, line: &str, pos: usize) -> (usize, Vec<Pair>) {
        let input = &line[..pos];
        let tokens: Vec<&str> = input.split_whitespace().collect();

        // Complete top-level command name.
        if tokens.is_empty() || (tokens.len() == 1 && !input.ends_with(' ')) {
            let (start, prefix) = match tokens.first() {
                Some(prefix) => (pos.saturating_sub(prefix.len()), *prefix),
                None => (pos, ""),
            };
            let matches = Self::complete_from(&self.commands, prefix);
            return (start, matches);
        }

        // Complete function name after `call`.
        if tokens.first() == Some(&"call") {
            if input.ends_with(' ') {
                if tokens.len() == 1 {
                    let start = pos;
                    let matches = Self::complete_from(&self.functions, "");
                    return (start, matches);
                }
                return (pos, Vec::new());
            }

            if tokens.len() == 2 {
                let prefix = tokens[1];
                let start = pos.saturating_sub(prefix.len());
                let matches = Self::complete_from(&self.functions, prefix);
                return (start, matches);
            }
        }

        (pos, Vec::new())
    }
}

impl Helper for ReplHelper {}

impl Hinter for ReplHelper {
    type Hint = String;
}

impl Highlighter for ReplHelper {}

impl Validator for ReplHelper {
    fn validate(&self, _ctx: &mut ValidationContext<'_>) -> rustyline::Result<ValidationResult> {
        Ok(ValidationResult::Valid(None))
    }
}

impl Completer for ReplHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Self::Candidate>)> {
        Ok(self.complete_for_input(line, pos))
    }
}

impl ReplSession {
    /// Create a new REPL session
    pub fn new(config: ReplConfig) -> Result<Self> {
        let history_path = dirs::home_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join(".soroban_repl_history");

        let executor = ReplExecutor::new(&config)?;
        let helper = ReplHelper::new(
            ReplCommand::builtins()
                .iter()
                .map(|cmd| (*cmd).to_string())
                .collect(),
            executor.function_names(),
        );

        let mut editor = Editor::<ReplHelper, FileHistory>::new()
            .map_err(|e| miette::miette!("Failed to initialize REPL editor: {}", e))?;
        editor.set_helper(Some(helper));

        // Load history if it exists
        let _ = editor.load_history(&history_path);

        Ok(ReplSession {
            editor,
            config,
            executor,
            history_path,
        })
    }

    /// Run the REPL event loop
    pub async fn run(&mut self) -> Result<()> {
        self.print_welcome();

        loop {
            let prompt = format!(
                "{}> ",
                Formatter::info(
                    format!(
                        "soroban-debug repl [{}]",
                        self.config.contract_path.display()
                    )
                    .as_str()
                )
            );

            match self.editor.readline(&prompt) {
                Ok(line) => {
                    if line.trim().is_empty() {
                        continue;
                    }

                    // Add to history
                    let _ = self.editor.add_history_entry(line.clone());

                    match self.execute_command(&line).await {
                        Ok(true) => break, // Exit requested
                        Ok(false) => {}    // Continue
                        Err(e) => {
                            tracing::error!(
                                "{}",
                                Formatter::error(format!("Error: {}", e).as_str())
                            );
                        }
                    }
                }
                Err(ReadlineError::Interrupted) => {
                    tracing::info!("\n{}", Formatter::info("Use 'exit' or Ctrl+D to quit"));
                }
                Err(ReadlineError::Eof) => {
                    // Ctrl+D
                    tracing::info!("\n{}", Formatter::success("Goodbye!"));
                    break;
                }
                Err(e) => {
                    tracing::error!("{}", Formatter::error(format!("Error: {}", e).as_str()));
                }
            }
        }

        // Save history
        let _ = self.editor.save_history(&self.history_path);

        Ok(())
    }

    /// Execute a single command
    async fn execute_command(&mut self, line: &str) -> Result<bool> {
        let cmd = ReplCommand::parse(line)?;

        match cmd {
            ReplCommand::Exit => Ok(true),
            ReplCommand::Help => {
                self.print_help();
                Ok(false)
            }
            ReplCommand::History => {
                self.print_history();
                Ok(false)
            }
            ReplCommand::Storage => {
                self.executor.inspect_storage()?;
                Ok(false)
            }
            ReplCommand::Call { function, args } => {
                self.executor.call_function(&function, args).await?;
                Ok(false)
            }
            ReplCommand::Clear => {
                // Print ANSI escape code to clear screen
                print!("\x1B[2J\x1B[1;1H");
                Ok(false)
            }
            ReplCommand::Break {
                function,
                condition,
            } => {
                self.executor
                    .add_breakpoint(&function, condition.as_deref())?;
                tracing::info!(
                    "{}",
                    Formatter::success(format!("Breakpoint set: {}", function).as_str())
                );
                Ok(false)
            }
            ReplCommand::ListBreaks => {
                let breaks = self.executor.list_breakpoints();
                if breaks.is_empty() {
                    tracing::info!("{}", Formatter::info("No breakpoints set"));
                } else {
                    tracing::info!("{}", Formatter::success("Breakpoints:"));
                    for bp in breaks {
                        let cond = bp
                            .condition
                            .map(|c| format!(" (if {:?})", c))
                            .unwrap_or_default();
                        tracing::info!("  - {}{}", bp.function, cond);
                    }
                }
                Ok(false)
            }
            ReplCommand::ClearBreak { function } => {
                if self.executor.remove_breakpoint(&function) {
                    tracing::info!(
                        "{}",
                        Formatter::success(format!("Breakpoint cleared: {}", function).as_str())
                    );
                } else {
                    tracing::info!(
                        "{}",
                        Formatter::info(format!("No breakpoint found: {}", function).as_str())
                    );
                }
                Ok(false)
            }
            ReplCommand::Functions => {
                self.executor.display_functions()?;
                Ok(false)
            }
        }
    }

    fn print_welcome(&self) {
        tracing::info!("{}", Formatter::success("=== Soroban Debug REPL ==="));
        tracing::info!(
            "{}",
            Formatter::info(format!("Contract: {}", self.config.contract_path.display()).as_str())
        );
        tracing::info!("{}", Formatter::info("Type 'help' for available commands"));
        tracing::info!("");
    }

    fn print_help(&self) {
        tracing::info!("");
        tracing::info!("{}", Formatter::success("Available Commands:"));
        tracing::info!(
            "  {} <func> [args...]  Call a contract function",
            Formatter::info("call")
        );
        tracing::info!(
            "  {}                 Show contract storage state",
            Formatter::info("storage")
        );
        tracing::info!(
            "  {}                 Show command history",
            Formatter::info("history")
        );
        tracing::info!(
            "  {}                    Clear the screen",
            Formatter::info("clear")
        );
        tracing::info!(
            "  {}                     Show this help message",
            Formatter::info("help")
        );
        tracing::info!(
            "  {} <func> [cond] Set a breakpoint with optional condition",
            Formatter::info("break")
        );
        tracing::info!(
            "  {}                 List all active breakpoints",
            Formatter::info("list-breaks")
        );
        tracing::info!(
            "  {} <func>         Clear a specific breakpoint",
            Formatter::info("clear-break")
        );
        tracing::info!(
            "  {}                 Show available contract functions",
            Formatter::info("functions")
        );
        tracing::info!(
            "  {}                     Exit the REPL",
            Formatter::info("exit")
        );
        tracing::info!("");
    }

    fn print_history(&self) {
        tracing::info!("");
        tracing::info!("{}", Formatter::success("Command History:"));
        for (idx, item) in self.editor.history().iter().enumerate() {
            tracing::info!("  {}: {}", idx, item);
        }
        tracing::info!("");
    }
}
