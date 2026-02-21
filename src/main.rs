use anyhow::Result;
use clap::Parser;
use soroban_debugger::cli::{Cli, Commands, Verbosity};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

fn initialize_tracing(verbosity: Verbosity) {
    let log_level = verbosity.to_log_level();
    let env_filter =
        std::env::var("RUST_LOG").unwrap_or_else(|_| format!("soroban_debugger={}", log_level));

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
                    .unwrap_or_else(|_| env_filter.into()),
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
                    .unwrap_or_else(|_| env_filter.into()),
            )
            .with(fmt_layer)
            .init();
    }
}

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

    // Accessibility: no-unicode flag and NO_COLOR env (screen reader compatible output)
    soroban_debugger::output::OutputConfig::configure(cli.no_unicode);
    soroban_debugger::ui::formatter::Formatter::configure_colors(
        soroban_debugger::output::OutputConfig::colors_enabled(),
    );

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
                Commands::UpgradeCheck(args) => {
                    soroban_debugger::cli::commands::upgrade_check(args)?
                }
                Commands::Compare(args) => soroban_debugger::cli::commands::compare(args)?,
                _ => unreachable!(),
            }
        }
    }

    Ok(())
}
