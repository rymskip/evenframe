//! Generate command - runs the full pipeline (typesync + schemasync).

use crate::cli::{Cli, GenerateArgs, TypesyncArgs};
use crate::target::Target;
use evenframe_core::{
    error::Result,
    schemasync::config::{ConnectionOverrides, MockOverrides},
};
use tracing::{debug, info};

/// Runs the full generation pipeline with default settings.
pub async fn run_default(cli: &Cli) -> Result<()> {
    let args = GenerateArgs {
        skip_typesync: false,
        skip_schemasync: false,
        no_mocks: false,
    };
    run(cli, args).await
}

/// Runs the full generation pipeline: one typesync output over every
/// project's scan, then each focused project's schemasync against its own
/// database.
pub async fn run(cli: &Cli, args: GenerateArgs) -> Result<()> {
    info!("Starting Evenframe code generation");
    let target = Target::discover()?;
    let scanned = target.scan_all()?;

    if args.skip_typesync {
        debug!("Skipping typesync phase");
    } else {
        let typesync_args = TypesyncArgs {
            command: None,
            all: false,
            formats: None,
            skip: None,
            per_file: false,
        };
        let (config, configs) = target.typesync_input(&scanned)?;
        super::typesync::generate(cli, typesync_args, &config, &configs)?;
    }

    if args.skip_schemasync {
        debug!("Skipping schemasync phase");
    } else {
        let focused = target.focused();
        for project in scanned {
            if !focused.iter().any(|(name, _)| *name == project.name) {
                continue;
            }
            super::schemasync::run_schemasync(
                project.config,
                &project.types.into_schemasync()?,
                ConnectionOverrides::default(),
                MockOverrides {
                    skip_mocks: args.no_mocks,
                    full_refresh: false,
                },
            )
            .await?;
        }
    }

    info!("Evenframe code generation completed successfully");
    Ok(())
}
