//! Generate command - runs the full pipeline (typesync + schemasync).

use crate::cli::{Cli, GenerateArgs, TypesyncArgs};
use crate::scan_cache::build_and_record;
use evenframe_core::scan::ScanConfig;
use evenframe_core::{
    config::EvenframeConfig,
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

/// Runs the full generation pipeline over a single workspace scan.
pub async fn run(cli: &Cli, args: GenerateArgs) -> Result<()> {
    info!("Starting Evenframe code generation");
    let config = EvenframeConfig::new()?;
    let build_config = ScanConfig::from_config(&config);
    let configs = build_and_record(&build_config)?;

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
        super::typesync::generate(cli, typesync_args, &config, &configs)?;
    }

    if args.skip_schemasync {
        debug!("Skipping schemasync phase");
    } else {
        super::schemasync::run_schemasync(
            &configs.into_schemasync()?,
            ConnectionOverrides::default(),
            MockOverrides {
                skip_mocks: args.no_mocks,
                full_refresh: false,
            },
        )
        .await?;
    }

    info!("Evenframe code generation completed successfully");
    Ok(())
}
