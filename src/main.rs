#![recursion_limit = "256"]
use clap::{CommandFactory, Parser};
use clap_complete::generate;
use soroban_debugger::cli::{Cli, Commands, Verbosity};
use soroban_debugger::ui::formatter::Formatter;
use std::io;

fn verbosity_to_level(v: Verbosity) -> u8 {
    match v {
        Verbosity::Quiet => 0,
        Verbosity::Normal => 1,
        Verbosity::Verbose => 2,
    }
}

fn initialize_tracing(verbosity: Verbosity) {
    let log_level = verbosity.to_log_level();
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| format!("soroban_debugger={}", log_level).into());

    let use_json = std::env::var("SOROBAN_DEBUG_JSON").is_ok();

    let subscriber = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_target(true)
        .with_level(true)
        .with_env_filter(env_filter);

    if use_json {
        subscriber.json().init();
    } else {
        subscriber.init();
    }
}

fn check_deprecated_flags() {
    let warnings = soroban_debugger::cli::args::warn_deprecated_flags(std::env::args());
    for warning in warnings {
        eprintln!("{}", Formatter::warning(warning));
    }
}

fn banner_text() -> String {
    format!(
        "  ____                  _\n / ___|  ___  _ __ ___ | |__   __ _ _ __\n \\___ \\ / _ \\| '__/ _ \\| '_ \\ / _` | '_ \\\n  ___) | (_) | | | (_) | |_) | (_| | | | |\n |____/ \\___/|_|  \\___/|_.__/ \\__,_|_| |_|  soroban-debugger v{}",
        env!("CARGO_PKG_VERSION")
    )
}

fn print_banner() {
    println!("{}", banner_text());
}

fn env_var_disables_banner(value: Option<&str>) -> bool {
    value.is_some_and(|v| {
        let trimmed = v.trim();
        trimmed == "1" || trimmed.eq_ignore_ascii_case("true")
    })
}

fn should_show_banner_with(args: &Cli, is_interactive: bool, no_banner_env: Option<&str>) -> bool {
    is_interactive && !args.no_banner && !env_var_disables_banner(no_banner_env)
}

fn should_show_banner(args: &Cli) -> bool {
    let no_banner_env = std::env::var("NO_BANNER").ok();
    should_show_banner_with(
        args,
        atty::is(atty::Stream::Stdout),
        no_banner_env.as_deref(),
    )
}

fn main() -> miette::Result<()> {
    Formatter::configure_colors_from_env();

    let cli = Cli::parse();
    soroban_debugger::output::set_pretty_json(cli.pretty);

    if let Some(ref history_file) = cli.history_file {
        std::env::set_var("SOROBAN_DEBUG_HISTORY_FILE", history_file);
    }
    if should_show_banner(&cli) {
        print_banner();
    }
    check_deprecated_flags();

    let is_json = is_cli_json_mode(&cli);
    let command_name = get_command_name(&cli);
    let verbosity = if is_json {
        Verbosity::Quiet
    } else {
        cli.verbosity()
    };

    Formatter::set_verbosity(verbosity_to_level(verbosity));
    initialize_tracing(verbosity);

    // Load community plugins at startup unless disabled via env var.
    let _ = soroban_debugger::plugin::registry::init_global_plugin_registry();

    let config = soroban_debugger::config::Config::load_or_default();

    let result = match cli.command {
        Some(Commands::Run(mut args)) => {
            args.merge_config(&config);
            soroban_debugger::cli::commands::run(args, verbosity)
        }
        Some(Commands::Interactive(mut args)) => {
            args.merge_config(&config);
            soroban_debugger::cli::commands::interactive(args, verbosity)
        }
        Some(Commands::Tui(args)) => soroban_debugger::cli::commands::tui(args, verbosity),
        Some(Commands::Inspect(args)) => soroban_debugger::cli::commands::inspect(args, verbosity),
        Some(Commands::Optimize(args)) => {
            soroban_debugger::cli::commands::optimize(args, verbosity)
        }
        Some(Commands::UpgradeCheck(args)) => soroban_debugger::cli::commands::upgrade_check(args),
        Some(Commands::Compare(args)) => soroban_debugger::cli::commands::compare(args),
        Some(Commands::Replay(args)) => soroban_debugger::cli::commands::replay(args, verbosity),
        Some(Commands::Completions(args)) => {
            let mut cmd = Cli::command();
            generate(args.shell, &mut cmd, "soroban-debug", &mut io::stdout());
            Ok(())
        }
        Some(Commands::Profile(args)) => soroban_debugger::cli::commands::profile(args),
        Some(Commands::Symbolic(args)) => {
            soroban_debugger::cli::commands::symbolic(args, verbosity)
        }
        Some(Commands::Server(args)) => soroban_debugger::cli::commands::server(args),
        Some(Commands::Remote(args)) => soroban_debugger::cli::commands::remote(args, verbosity),
        Some(Commands::Analyze(args)) => soroban_debugger::cli::commands::analyze(args, verbosity),
        Some(Commands::Scenario(args)) => {
            soroban_debugger::cli::commands::scenario(args, verbosity)
        }
        Some(Commands::Doctor(args)) => soroban_debugger::cli::commands::doctor(args),
        Some(Commands::HistoryPrune(args)) => soroban_debugger::cli::commands::history_prune(args),
        Some(Commands::Repl(mut args)) => {
            args.merge_config(&config);
            tokio::runtime::Runtime::new()
                .map_err(|e: std::io::Error| miette::miette!(e))
                .and_then(|rt| rt.block_on(soroban_debugger::cli::commands::repl(args)))
        }
        Some(Commands::PluginTrustReport(args)) => {
            soroban_debugger::cli::commands::plugin_trust_report(args)
        }
        Some(Commands::PluginInspect(args)) => {
            soroban_debugger::cli::commands::plugin_inspect(args)
        }
        Some(Commands::External(argv)) => {
            if argv.is_empty() {
                return Err(miette::miette!("Missing plugin subcommand"));
            }

            let command = &argv[0];
            let args = argv[1..].to_vec();

            match soroban_debugger::plugin::registry::execute_global_command(command, &args) {
                Ok(Some(output)) => {
                    println!("{}", output);
                    Ok(())
                }
                Ok(None) => {
                    // If no plugin registered a command, try treating this as a formatter invocation.
                    if let Ok(Some(formatted)) =
                        soroban_debugger::plugin::registry::format_global_output(
                            command,
                            &args.join(" "),
                        )
                    {
                        println!("{}", formatted);
                        return Ok(());
                    }

                    let available = soroban_debugger::plugin::registry::global_commands();
                    let formatters = soroban_debugger::plugin::registry::global_formatters();
                    let mut message = format!("Unknown command: '{command}'");
                    if !available.is_empty() {
                        message.push_str("\n\nAvailable plugin commands:\n");
                        for cmd in available {
                            message.push_str(&format!("  - {}: {}\n", cmd.name, cmd.description));
                        }
                    }
                    if !formatters.is_empty() {
                        message.push_str("\nAvailable plugin formatters:\n");
                        for fmt in formatters {
                            message.push_str(&format!("  - {}\n", fmt.name));
                        }
                    }

                    let command_conflicts =
                        soroban_debugger::plugin::registry::global_command_conflicts();
                    if !command_conflicts.is_empty() {
                        let mut conflict_entries: Vec<_> = command_conflicts.iter().collect();
                        conflict_entries.sort_by_key(|(a, _)| *a);
                        message.push_str("\nPlugin command collisions detected:\n");
                        for (cmd, providers) in conflict_entries {
                            if providers.len() > 1 {
                                message.push_str(&format!(
                                    "  - {}: winner {} ignored {}\n",
                                    cmd,
                                    providers[0],
                                    providers[1..].join(", ")
                                ));
                            }
                        }
                    }

                    let formatter_conflicts =
                        soroban_debugger::plugin::registry::global_formatter_conflicts();
                    if !formatter_conflicts.is_empty() {
                        let mut conflict_entries: Vec<_> = formatter_conflicts.iter().collect();
                        conflict_entries.sort_by_key(|(a, _)| *a);
                        message.push_str("\nPlugin formatter collisions detected:\n");
                        for (formatter, providers) in conflict_entries {
                            if providers.len() > 1 {
                                message.push_str(&format!(
                                    "  - {}: winner {} ignored {}\n",
                                    formatter,
                                    providers[0],
                                    providers[1..].join(", ")
                                ));
                            }
                        }
                    }
                    Err(soroban_debugger::DebuggerError::ExecutionError(message).into())
                }
                Err(e) => {
                    Err(soroban_debugger::DebuggerError::ExecutionError(e.to_string()).into())
                }
            }
        }
        None => {
            if let Some(path) = cli.list_functions {
                return soroban_debugger::cli::commands::inspect(
                    soroban_debugger::cli::args::InspectArgs {
                        contract: path,
                        functions: true,
                        metadata: false,
                        format: soroban_debugger::cli::args::OutputFormat::Pretty,
                        source_map_diagnostics: false,
                        source_map_limit: 20,
                        expected_hash: None,
                        dependency_graph: None,
                    },
                    verbosity,
                );
            }
            if cli.budget_trend {
                soroban_debugger::cli::commands::show_budget_trend(
                    cli.trend_contract.as_deref(),
                    cli.trend_function.as_deref(),
                    soroban_debugger::history::RegressionConfig {
                        threshold_pct: cli.trend_regression_threshold_pct,
                        lookback: cli.trend_regression_lookback,
                        smoothing_window: cli.trend_regression_smoothing,
                    },
                )
            } else {
                let mut cmd = Cli::command();
                cmd.print_help().map_err(|e| miette::miette!(e))?;
                tracing::info!("");
                Ok(())
            }
        }
    };

    if let Err(err) = result {
        let err: miette::Report = err;
        if is_json {
            let message = err.to_string();
            let code = err.code().map(|c| c.to_string());
            let suggestion = err.help().map(|h| h.to_string());

            let category = if let Some(ref c) = code {
                match c.as_str() {
                    "debugger::request_timeout" => Some("timeout".to_string()),
                    "debugger::invalid_arguments" | "debugger::invalid_function" => {
                        Some("parser_failure".to_string())
                    }
                    "debugger::auth_failed" => Some("auth_failure".to_string()),
                    _ => Some(soroban_debugger::output::categorize_error(&message).to_string()),
                }
            } else {
                Some(soroban_debugger::output::categorize_error(&message).to_string())
            };

            let output_err = soroban_debugger::output::OutputError {
                message,
                code,
                category,
                suggestion,
            };

            let output = soroban_debugger::output::VersionedOutput::<serde_json::Value> {
                schema_version: soroban_debugger::output::SCHEMA_VERSION,
                command: command_name.clone(),
                status: soroban_debugger::output::OutputStatus::Error,
                result: None,
                error: Some(output_err),
            };

            if let Ok(json) = soroban_debugger::output::to_json_string(&output) {
                println!("{}", json);
            }
        }
        tracing::error!(
            "{}",
            Formatter::error(format!("Error handling deprecations: {err:#}"))
        );
        return Err(err);
    }

    Ok(())
}

fn is_cli_json_mode(cli: &Cli) -> bool {
    match &cli.command {
        Some(Commands::Run(args)) => args.is_json_output(),
        Some(Commands::Inspect(args)) => {
            args.format == soroban_debugger::cli::args::OutputFormat::Json
        }
        Some(Commands::Optimize(args)) => {
            args.format == soroban_debugger::cli::args::OutputFormat::Json
        }
        Some(Commands::UpgradeCheck(args)) => args.output == "json",
        Some(Commands::Analyze(args)) => args.format == "json",
        Some(Commands::Doctor(args)) => {
            args.format == soroban_debugger::cli::args::OutputFormat::Json
        }
        Some(Commands::Replay(args)) => {
            args.format == soroban_debugger::cli::args::OutputFormat::Json
        }
        Some(Commands::Symbolic(args)) => {
            args.format == soroban_debugger::cli::args::OutputFormat::Json
        }
        _ => false,
    }
}

fn get_command_name(cli: &Cli) -> String {
    match &cli.command {
        Some(Commands::Run(_)) => "run",
        Some(Commands::Inspect(_)) => "inspect",
        Some(Commands::Optimize(_)) => "optimize",
        Some(Commands::UpgradeCheck(_)) => "upgrade-check",
        Some(Commands::Analyze(_)) => "analyze",
        Some(Commands::Remote(_)) => "remote",
        Some(Commands::Doctor(_)) => "doctor",
        Some(Commands::Replay(_)) => "replay",
        Some(Commands::Symbolic(_)) => "symbolic",
        _ => "unknown",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_cli(args: &[&str]) -> Cli {
        Cli::try_parse_from(args).expect("failed to parse cli args")
    }

    #[test]
    fn banner_contains_project_name_and_version() {
        let banner = banner_text();
        assert!(banner.contains("soroban-debugger"));
        assert!(banner.contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn banner_is_max_five_lines_tall() {
        let banner = banner_text();
        assert!(banner.lines().count() <= 5);
    }

    #[test]
    fn no_banner_flag_suppresses_output() {
        let args = parse_cli(&["soroban-debug", "--no-banner"]);
        assert!(!should_show_banner_with(&args, true, None));
    }

    #[test]
    fn no_banner_env_var_suppresses_output() {
        let args = parse_cli(&["soroban-debug"]);
        assert!(!should_show_banner_with(&args, true, Some("1")));
        assert!(!should_show_banner_with(&args, true, Some("true")));
        assert!(!should_show_banner_with(&args, true, Some("TRUE")));
    }

    #[test]
    fn non_interactive_output_suppresses_banner() {
        let args = parse_cli(&["soroban-debug"]);
        assert!(!should_show_banner_with(&args, false, None));
    }

    #[test]
    fn interactive_output_shows_banner_when_not_suppressed() {
        let args = parse_cli(&["soroban-debug"]);
        assert!(should_show_banner_with(&args, true, None));
    }
}
