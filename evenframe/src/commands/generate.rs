//! Generate command - runs the full pipeline (typesync + schemasync).

use crate::cli::{Cli, GenerateArgs, TypesyncArgs};
use crate::config_builders;
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
    let build_config = config_builders::BuildConfig::discover()?;
    let (enums, tables, objects) = config_builders::build_and_record(&build_config)?;

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
        super::typesync::generate(
            cli,
            typesync_args,
            &config,
            enums.clone(),
            tables.clone(),
            objects.clone(),
        )?;
    }

    if args.skip_schemasync {
        debug!("Skipping schemasync phase");
    } else {
        let (enums, tables, objects) =
            config_builders::filter_for_schemasync(enums, tables, objects);
        super::schemasync::run_schemasync(
            &enums,
            &tables,
            &objects,
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
