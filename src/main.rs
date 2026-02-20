use anyhow::Result;
use clap::Parser;
use soroban_debugger::cli::{Cli, Commands};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "soroban_debugger=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Parse CLI arguments
    let mut cli = Cli::parse();

    // Load configuration
    let config = soroban_debugger::config::Config::load_or_default();

    // Execute command
    match cli.command {
        Commands::Run(mut args) => {
            args.merge_config(&config);
            soroban_debugger::cli::commands::run(args)?;
        }
        Commands::Interactive(mut args) => {
            args.merge_config(&config);
            soroban_debugger::cli::commands::interactive(args)?;
        }
        _ => {
            // Other commands don't have merge_config implemented yet or don't need it
            match cli.command {
                Commands::Inspect(args) => soroban_debugger::cli::commands::inspect(args)?,
                Commands::Optimize(args) => soroban_debugger::cli::commands::optimize(args)?,
                Commands::UpgradeCheck(args) => soroban_debugger::cli::commands::upgrade_check(args)?,
                Commands::Compare(args) => soroban_debugger::cli::commands::compare(args)?,
                _ => unreachable!(),
            }
        }
    }

    Ok(())
}
