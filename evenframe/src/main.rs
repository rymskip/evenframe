mod cli;
mod commands;
mod config_builders;
mod workspace_scanner;

use clap::Parser;
use cli::{Cli, Commands};
use evenframe_core::{config::EvenframeConfig, error::Result, evenframe_log};
use tracing::{error, info, Level};

#[tokio::main]
async fn main() -> Result<()> {
    // Early env load for logging macros that need env vars before full config init.
    // EvenframeConfig::new() will do the authoritative load from the configured path.
    EvenframeConfig::load_env_early();

    let cli = Cli::parse();

    // Initialize logging based on verbosity
    init_logging(cli.verbose, cli.quiet);

    evenframe_log!("", "tracing.log");
    evenframe_log!("", "errors.log");

    info!("Starting Evenframe");

    // Dispatch to appropriate command handler
    let result = match &cli.command {
        Some(Commands::Typesync(args)) => commands::typesync::run(&cli, args.clone()).await,
        Some(Commands::Schemasync(args)) => commands::schemasync::run(&cli, args.clone()).await,
        Some(Commands::Generate(args)) => commands::generate::run(&cli, args.clone()).await,
        Some(Commands::Init(args)) => commands::init::run(&cli, args.clone()).await,
        Some(Commands::Validate(args)) => commands::validate::run(&cli, args.clone()).await,
        Some(Commands::Info(args)) => commands::info::run(&cli, args.clone()).await,
        None => {
            // Default behavior: run full pipeline (backward compatibility)
            commands::generate::run_default(&cli).await
        }
    };

    match result {
        Ok(_) => {
            info!("Evenframe completed successfully");
            Ok(())
        }
        Err(e) => {
            error!("Evenframe failed: {}", e);
            Err(e)
        }
    }
}

fn init_logging(verbose: u8, quiet: bool) {
    let level = if quiet {
        Level::ERROR
    } else {
        match verbose {
            0 => Level::WARN,
            1 => Level::INFO,
            2 => Level::DEBUG,
            _ => Level::TRACE,
        }
    };

    tracing_subscriber::fmt().with_max_level(level).init();
}
