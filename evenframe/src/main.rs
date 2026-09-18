mod cli;
mod commands;
mod config_builders;
mod workspace_scanner;

use clap::Parser;
use cli::{Cli, Commands};
use evenframe_core::{config::EvenframeConfig, error::Result, evenframe_log};
use std::io::IsTerminal;
use std::process::ExitCode;
use tracing::{error, info};

#[tokio::main]
async fn main() -> ExitCode {
    // `--config` picks the project whose `.env` loads early, and that `.env`
    // may set variables that logging and some arguments default from, so the
    // arguments are parsed again once it is loaded. EvenframeConfig::new()
    // does the authoritative load later.
    let cli = Cli::parse();
    if let Some(path) = &cli.config
        && let Err(e) = EvenframeConfig::use_config_file(path)
    {
        eprintln!("Evenframe failed: {e}");
        return ExitCode::FAILURE;
    }
    EvenframeConfig::load_env_early();
    let cli = Cli::parse();

    init_logging(&cli);

    evenframe_log!("", "tracing.log");
    evenframe_log!("", "errors.log");

    info!("Starting Evenframe");

    // Reported here rather than returned: a `Result` from `main` would print
    // the error a second time in its `Debug` form.
    match run(&cli).await {
        Ok(()) => {
            info!("Evenframe completed successfully");
            ExitCode::SUCCESS
        }
        Err(e) => {
            error!("Evenframe failed: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: &Cli) -> Result<()> {
    // Serialize concurrent runs against the same project — schema sync, type
    // generation, and the .evenframe caches all mutate shared state, so a
    // second process waits for the first instead of interleaving with it.
    // Held until the command finishes; released by the OS even on a crash. No lock
    // when no project exists yet (e.g. `evenframe init`).
    let _lock = EvenframeConfig::find_project_root()
        .map(|root| evenframe_core::lock::ProcessLock::acquire(&root))
        .transpose()?;

    match &cli.command {
        Some(Commands::Typesync(args)) => commands::typesync::run(cli, args.clone()).await,
        Some(Commands::Schemasync(args)) => commands::schemasync::run(args.clone()).await,
        Some(Commands::Mockmake(args)) => commands::mockmake::run(args.clone()).await,
        Some(Commands::Generate(args)) => commands::generate::run(cli, args.clone()).await,
        Some(Commands::Init(args)) => commands::init::run(cli, args.clone()).await,
        Some(Commands::Check(args)) => commands::check::run(args.clone()).await,
        Some(Commands::Validate(args)) => commands::validate::run(args.clone()).await,
        Some(Commands::Info(args)) => commands::info::run(args.clone()).await,
        Some(Commands::TestPlugin(args)) => commands::test_plugin::run(args.clone()).await,
        Some(Commands::Expand(args)) => commands::expand::run(args.clone()).await,
        None => {
            // Default behavior: run full pipeline (backward compatibility)
            commands::generate::run_default(cli).await
        }
    }
}

/// Logs go to stderr so stdout carries only command output (such as
/// `check --json`), and are colored only when stderr is a terminal.
fn init_logging(cli: &Cli) {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| cli.log_filter().into()),
        )
        .init();
}
