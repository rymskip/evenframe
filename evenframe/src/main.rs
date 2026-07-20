mod cli;
mod commands;
mod config_builders;
mod workspace_scanner;

use clap::Parser;
use cli::{Cli, Commands};
use evenframe_core::{config::EvenframeConfig, error::Result, evenframe_log};
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<()> {
    // Early env load for logging macros that need env vars before full config init.
    // EvenframeConfig::new() will do the authoritative load from the configured path.
    EvenframeConfig::load_env_early();

    let cli = Cli::parse();

    // Initialize logging based on verbosity
    init_logging(&cli);

    evenframe_log!("", "tracing.log");
    evenframe_log!("", "errors.log");

    info!("Starting Evenframe");

    // Serialize concurrent runs against the same project — schema sync, type
    // generation, and the .evenframe caches all mutate shared state, so a
    // second process waits for the first instead of interleaving with it.
    // Held until process exit; released by the OS even on a crash. No lock
    // when no project exists yet (e.g. `evenframe init`).
    let _lock = EvenframeConfig::find_project_root()
        .map(|root| evenframe_core::lock::ProcessLock::acquire(&root))
        .transpose()?;

    // Dispatch to appropriate command handler
    let result = match &cli.command {
        Some(Commands::Typesync(args)) => commands::typesync::run(&cli, args.clone()).await,
        Some(Commands::Schemasync(args)) => commands::schemasync::run(&cli, args.clone()).await,
        Some(Commands::Generate(args)) => commands::generate::run(&cli, args.clone()).await,
        Some(Commands::Init(args)) => commands::init::run(&cli, args.clone()).await,
        Some(Commands::Validate(args)) => commands::validate::run(&cli, args.clone()).await,
        Some(Commands::Info(args)) => commands::info::run(&cli, args.clone()).await,
        Some(Commands::TestPlugin(args)) => commands::test_plugin::run(&cli, args.clone()).await,
        Some(Commands::Cache(args)) => commands::cache::run(&cli, args.clone()).await,
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

fn init_logging(cli: &Cli) {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| cli.log_filter().into()),
        )
        .init();
}
