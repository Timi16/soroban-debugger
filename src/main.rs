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

fn print_deprecation_warning(deprecated_flag: &str, new_flag: &str) {
    eprintln!(
        "{}",
        Formatter::warning(format!(
            " Flag '{}' is deprecated. Please use '{}' instead.",
            deprecated_flag, new_flag
        ))
    );
}

fn handle_deprecations(cli: &mut Cli) {
    match &mut cli.command {
        Some(Commands::Run(args)) => {
            if let Some(wasm) = args.wasm.take() {
                print_deprecation_warning("--wasm", "--contract");
                args.contract = wasm;
            }
            if let Some(snapshot) = args.snapshot.take() {
                print_deprecation_warning("--snapshot", "--network-snapshot");
                args.network_snapshot = Some(snapshot);
            }
        }
        Some(Commands::Interactive(args)) => {
            if let Some(wasm) = args.wasm.take() {
                print_deprecation_warning("--wasm", "--contract");
                args.contract = wasm;
            }
            if let Some(snapshot) = args.snapshot.take() {
                print_deprecation_warning("--snapshot", "--network-snapshot");
                args.network_snapshot = Some(snapshot);
            }
        }
        Some(Commands::Inspect(args)) => {
            if let Some(wasm) = args.wasm.take() {
                print_deprecation_warning("--wasm", "--contract");
                args.contract = wasm;
            }
        }
        Some(Commands::Optimize(args)) => {
            if let Some(wasm) = args.wasm.take() {
                print_deprecation_warning("--wasm", "--contract");
                args.contract = wasm;
            }
            if let Some(snapshot) = args.snapshot.take() {
                print_deprecation_warning("--snapshot", "--network-snapshot");
                args.network_snapshot = Some(snapshot);
            }
        }
        Some(Commands::Profile(args)) => {
            if let Some(wasm) = args.wasm.take() {
                print_deprecation_warning("--wasm", "--contract");
                args.contract = wasm;
            }
        }
        Some(Commands::Repl(args)) => {
            if let Some(wasm) = args.wasm.take() {
                print_deprecation_warning("--wasm", "--contract");
                args.contract = wasm;
            }
            if let Some(snapshot) = args.snapshot.take() {
                print_deprecation_warning("--snapshot", "--network-snapshot");
                args.network_snapshot = Some(snapshot);
            }
        }
        _ => {}
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

    let mut cli = Cli::parse();
    if should_show_banner(&cli) {
        print_banner();
    }
    handle_deprecations(&mut cli);

    let run_json_output_requested = matches!(
        cli.command.as_ref(),
        Some(Commands::Run(args))
            if args.output_format == soroban_debugger::cli::args::OutputFormat::Json
                || args.json
                || args
                    .format
                    .as_deref()
                    .is_some_and(|f| f.eq_ignore_ascii_case("json"))
    );
    let verbosity = cli.verbosity();

    Formatter::set_verbosity(verbosity_to_level(verbosity));
    initialize_tracing(verbosity);

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
        Some(Commands::Repl(mut args)) => {
            args.merge_config(&config);
            tokio::runtime::Runtime::new()
                .map_err(|e: std::io::Error| miette::miette!(e))
                .and_then(|rt| rt.block_on(soroban_debugger::cli::commands::repl(args)))
        }
        None => {
            if let Some(path) = cli.list_functions {
                return soroban_debugger::cli::commands::inspect(
                    soroban_debugger::cli::args::InspectArgs {
                        contract: path,
                        wasm: None,
                        functions: true,
                        metadata: false,
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
        if run_json_output_requested {
            let output = soroban_debugger::cli::output::CommandOutput::<()> {
                status: "error".to_string(),
                result: None,
                budget: None,
                errors: Some(vec![err.to_string()]),
            };
            if let Ok(json) = serde_json::to_string_pretty(&output) {
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
