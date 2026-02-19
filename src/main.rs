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
    let cli = Cli::parse();

    // Execute command
    match cli.command {
        Commands::Run(args) => {
            soroban_debugger::cli::commands::run(args)?;
        }
        Commands::Interactive(args) => {
            soroban_debugger::cli::commands::interactive(args)?;
        }
        Commands::Inspect(args) => {
            soroban_debugger::cli::commands::inspect(args)?;
        }
        Commands::Optimize(args) => {
            soroban_debugger::cli::commands::optimize(args)?;
        }
    }

    Ok(())
}
