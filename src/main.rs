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

fn main() -> Result<()> {
    Formatter::configure_colors_from_env();

    let cli = Cli::parse();
    let verbosity = cli.verbosity();

    initialize_tracing(verbosity);

    let config = soroban_debugger::config::Config::load_or_default();

    let result = match cli.command {
        Some(Commands::Run(mut args)) => {
            args.merge_config(&config);
            soroban_debugger::cli::commands::run(*args, verbosity)
        }
        Some(Commands::Interactive(mut args)) => {
            args.merge_config(&config);
            soroban_debugger::cli::commands::interactive(args, verbosity)
        }
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
            soroban_debugger::cli::commands::profile(args)?;
            Ok(())
        }
        Some(Commands::ListFunctions(args)) => {
            soroban_debugger::cli::commands::list_functions(args, verbosity)
        }
        None => {
            let mut cmd = Cli::command();
            cmd.print_help()?;
            println!();
            Ok(())
        }
    };

    if let Err(err) = result {
        eprintln!("{}", Formatter::error(format!("Error: {err:#}")));
        return Err(err);
    }

    Ok(())
}
