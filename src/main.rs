use anyhow::Result;
use clap::{CommandFactory, Parser};
use clap_complete::generate;
use soroban_debugger::cli::{Cli, Commands, Verbosity};
use soroban_debugger::ui::formatter::Formatter;
use std::io;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn initialize_tracing(verbosity: Verbosity) {
    let log_level = verbosity.to_log_level();
    let fallback_filter = format!("soroban_debugger={}", log_level);

    let use_json = std::env::var("SOROBAN_DEBUG_JSON").is_ok();

    if use_json {
        let json_layer = tracing_subscriber::fmt::layer()
            .json()
            .with_writer(std::io::stderr)
            .with_target(true)
            .with_level(true);

        tracing_subscriber::registry()
            .with(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| fallback_filter.clone().into()),
            )
            .with(json_layer)
            .init();
    } else {
        let fmt_layer = tracing_subscriber::fmt::layer()
            .with_writer(std::io::stderr)
            .with_target(true)
            .with_level(true);

        tracing_subscriber::registry()
            .with(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| fallback_filter.into()),
            )
            .with(fmt_layer)
            .init();
    }
}

fn handle_deprecations(cli: &mut Cli) {
    match &mut cli.command {
        Some(Commands::Run(args)) => {
            if let Some(wasm) = args.wasm.take() {
                eprintln!("{}", Formatter::warning("Warning: --wasm and --contract-path are deprecated. Please use --contract instead."));
                args.contract = wasm;
            }
            if let Some(snapshot) = args.snapshot.take() {
                eprintln!(
                    "{}",
                    Formatter::warning(
                        "Warning: --snapshot is deprecated. Please use --network-snapshot instead."
                    )
                );
                args.network_snapshot = Some(snapshot);
            }
        }
        Some(Commands::Interactive(args)) => {
            if let Some(wasm) = args.wasm.take() {
                eprintln!("{}", Formatter::warning("Warning: --wasm and --contract-path are deprecated. Please use --contract instead."));
                args.contract = wasm;
            }
            if let Some(snapshot) = args.snapshot.take() {
                eprintln!(
                    "{}",
                    Formatter::warning(
                        "Warning: --snapshot is deprecated. Please use --network-snapshot instead."
                    )
                );
                args.network_snapshot = Some(snapshot);
            }
        }
        Some(Commands::Inspect(args)) => {
            if let Some(wasm) = args.wasm.take() {
                eprintln!("{}", Formatter::warning("Warning: --wasm and --contract-path are deprecated. Please use --contract instead."));
                args.contract = wasm;
            }
        }
        Some(Commands::Optimize(args)) => {
            if let Some(wasm) = args.wasm.take() {
                eprintln!("{}", Formatter::warning("Warning: --wasm and --contract-path are deprecated. Please use --contract instead."));
                args.contract = wasm;
            }
            if let Some(snapshot) = args.snapshot.take() {
                eprintln!(
                    "{}",
                    Formatter::warning(
                        "Warning: --snapshot is deprecated. Please use --network-snapshot instead."
                    )
                );
                args.network_snapshot = Some(snapshot);
            }
        }
        Some(Commands::Profile(args)) => {
            if let Some(wasm) = args.wasm.take() {
                eprintln!("{}", Formatter::warning("Warning: --wasm and --contract-path are deprecated. Please use --contract instead."));
                args.contract = wasm;
            }
        }
        _ => {}
    }
}

fn main() -> miette::Result<()> {
    Formatter::configure_colors_from_env();
fn main() -> Result<()> {
    Formatter::configure_colors_from_env();

    let mut cli = Cli::parse();
    handle_deprecations(&mut cli);
    let verbosity = cli.verbosity();

    initialize_tracing(verbosity);

    let config = soroban_debugger::config::Config::load_or_default();

    match cli.command {
        Some(Commands::Run(mut args)) => {
    // Execute command
    match cli.command {
        Commands::Run(mut args) => {
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
        Some(Commands::UpgradeCheck(args)) => {
            soroban_debugger::cli::commands::upgrade_check(args, verbosity)
        }
        Some(Commands::Compare(args)) => soroban_debugger::cli::commands::compare(args),
        Some(Commands::Completions(args)) => {
            let mut cmd = Cli::command();
            generate(args.shell, &mut cmd, "soroban-debug", &mut io::stdout());
            Ok(())
        }
        Some(Commands::Profile(args)) => {
            soroban_debugger::cli::commands::profile(args)
        }
        Some(Commands::Symbolic(args)) => {
            soroban_debugger::cli::commands::symbolic(args, verbosity)
        }
        None => {
            if let Some(path) = cli.list_functions {
                return soroban_debugger::cli::commands::inspect(
                    soroban_debugger::cli::args::InspectArgs {
                        contract: path,
                        wasm: None,
                        functions: true,
                        metadata: false,
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
                println!();
                Ok(())
            }
        }
    };

    if let Err(err) = result {
        eprintln!("{}", Formatter::error(format!("Error: {err:#}")));
        return Err(err);
    }
}
