mod cli;
mod commands;
mod lock;
mod scan_cache;
mod target;

use clap::Parser;
use cli::{Cli, Commands};
use evenframe_core::{config::EvenframeConfig, error::Result, evenframe_log};
use std::io::IsTerminal;
use std::process::ExitCode;
use tracing::{error, info};

/// Worker stack for the runtime. The in-memory SurrealDB that schema
/// comparison runs needs more than tokio's 2 MiB default on a deep error path;
/// 10 MiB is what SurrealDB documents for an embedded engine.
const WORKER_STACK_BYTES: usize = 10 * 1024 * 1024;

fn main() -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(WORKER_STACK_BYTES)
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("Evenframe failed to start its async runtime: {error}");
            return ExitCode::FAILURE;
        }
    };
    runtime.block_on(start())
}

async fn start() -> ExitCode {
    // A `.env` is read only by the configuration that names it, so it never
    // supplies an argument.
    let cli = Cli::parse();
    if let Some(path) = &cli.config
        && let Err(error) = EvenframeConfig::use_config_file(path)
    {
        eprintln!("Evenframe failed: {error}");
        return ExitCode::FAILURE;
    }

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
        Err(error) => {
            error!("Evenframe failed: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: &Cli) -> Result<()> {
    // Serialize concurrent runs against the same project or workspace: schemasync, typesync
    // and the .evenframe caches all mutate shared state, so a second process
    // waits for the first instead of interleaving with it.
    // Held until the command finishes; released by the OS even on a crash. No lock
    // when no project exists yet (e.g. `evenframe init`).
    let _lock = EvenframeConfig::find_lock_root()
        .map(|root| lock::ProcessLock::acquire(&root))
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
