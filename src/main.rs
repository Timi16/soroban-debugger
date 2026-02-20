use anyhow::Result;
use clap::Parser;
use soroban_debugger::cli::{Cli, Commands};
use soroban_debugger::ui::formatter::Formatter;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn main() -> Result<()> {
    Formatter::configure_colors_from_env();

    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "soroban_debugger=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

fn initialize_tracing() {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "soroban_debugger=info".into());

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(true)
        .with_level(true);

    // Check if JSON output is requested via SOROBAN_DEBUG_JSON env var
    let use_json = std::env::var("SOROBAN_DEBUG_JSON").is_ok();

    if use_json {
        let json_layer = tracing_subscriber::fmt::layer()
            .json()
            .with_writer(std::io::stderr)
            .with_target(true)
            .with_level(true);

        tracing_subscriber::registry()
            .with(env_filter)
            .with(json_layer)
            .init();
    } else {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .init();
    }
}

fn main() -> Result<()> {
    // Initialize structured logging with tracing
    initialize_tracing();

    // Parse CLI arguments
    let cli = Cli::parse();

    // Execute command
    let result = match cli.command {
        Commands::Run(args) => soroban_debugger::cli::commands::run(args),
        Commands::Interactive(args) => soroban_debugger::cli::commands::interactive(args),
        Commands::Inspect(args) => soroban_debugger::cli::commands::inspect(args),
        Commands::Optimize(args) => soroban_debugger::cli::commands::optimize(args),
        Commands::UpgradeCheck(args) => soroban_debugger::cli::commands::upgrade_check(args),
    };

    if let Err(err) = result {
        eprintln!("{}", Formatter::error(format!("Error: {err:#}")));
        return Err(err);
    }

    Ok(())
}