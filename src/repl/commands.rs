/// REPL command parsing and representation
///
/// Parses user input into structured REPL commands.
use crate::Result;

/// Represents a REPL command
#[derive(Debug, Clone)]
pub enum ReplCommand {
    /// Call a contract function: call <function> [args...]
    Call {
        function: String,
        args: Vec<String>,
    },
    /// Inspect storage: storage
    Storage,
    /// Show command history: history
    History,
    /// Clear screen: clear
    Clear,
    /// Show help: help
    Help,
    /// Exit REPL: exit
    Exit,
    /// Set a breakpoint: break <function> [condition]
    Break {
        function: String,
        condition: Option<String>,
    },
    /// List breakpoints: list-breaks
    ListBreaks,
    /// Clear a breakpoint: clear-break <function>
    ClearBreak {
        function: String,
    },
    Functions,
}

impl ReplCommand {
    /// Built-in REPL commands for completion
    pub fn builtins() -> &'static [&'static str] {
        &[
            "call",
            "storage",
            "history",
            "clear",
            "help",
            "exit",
            "quit",
            "break",
            "list-breaks",
            "clear-break",
            "functions",
        ]
    }

    /// Parse a command string into a ReplCommand
    pub fn parse(input: &str) -> Result<Self> {
        let trimmed = input.trim();
        let parts: Vec<&str> = trimmed.split_whitespace().collect();

        if parts.is_empty() {
            return Err(miette::miette!("Empty command"));
        }

        match parts[0] {
            "call" => {
                if parts.len() < 2 {
                    return Err(miette::miette!("call requires a function name"));
                }
                let function = parts[1].to_string();
                let args = parts[2..].iter().map(|s| s.to_string()).collect();
                Ok(ReplCommand::Call { function, args })
            }
            "break" => {
                if parts.len() < 2 {
                    return Err(miette::miette!("break requires a function name"));
                }
                let function = parts[1].to_string();
                let condition = if parts.len() > 2 {
                    Some(parts[2..].join(" "))
                } else {
                    None
                };
                Ok(ReplCommand::Break {
                    function,
                    condition,
                })
            }
            "list-breaks" => Ok(ReplCommand::ListBreaks),
            "clear-break" => {
                if parts.len() < 2 {
                    return Err(miette::miette!("clear-break requires a function name"));
                }
                let function = parts[1].to_string();
                Ok(ReplCommand::ClearBreak { function })
            }
            "storage" => Ok(ReplCommand::Storage),
            "history" => Ok(ReplCommand::History),
            "functions" => Ok(ReplCommand::Functions),
            "clear" => Ok(ReplCommand::Clear),
            "help" => Ok(ReplCommand::Help),
            "exit" | "quit" => Ok(ReplCommand::Exit),
            _ => Err(miette::miette!(
                "Unknown command: '{}'. Type 'help' for available commands.",
                parts[0]
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_call_command() {
        let cmd = ReplCommand::parse("call transfer Alice Bob 100").unwrap();
        match cmd {
            ReplCommand::Call { function, args } => {
                assert_eq!(function, "transfer");
                assert_eq!(args, vec!["Alice", "Bob", "100"]);
            }
            _ => panic!("Expected Call command"),
        }
    }

    #[test]
    fn test_parse_storage_command() {
        let cmd = ReplCommand::parse("storage").unwrap();
        assert!(matches!(cmd, ReplCommand::Storage));
    }

    #[test]
    fn test_parse_exit_command() {
        let cmd = ReplCommand::parse("exit").unwrap();
        assert!(matches!(cmd, ReplCommand::Exit));

        let cmd = ReplCommand::parse("quit").unwrap();
        assert!(matches!(cmd, ReplCommand::Exit));
    }

    #[test]
    fn test_parse_help_command() {
        let cmd = ReplCommand::parse("help").unwrap();
        assert!(matches!(cmd, ReplCommand::Help));
    }

    #[test]
    fn test_empty_call_fails() {
        let result = ReplCommand::parse("call");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_functions_command() {
        let cmd = ReplCommand::parse("functions").unwrap();
        assert!(matches!(cmd, ReplCommand::Functions));
    }

    #[test]
    fn test_unknown_command_fails() {
        let result = ReplCommand::parse("unknown");
        assert!(result.is_err());
    }
}
