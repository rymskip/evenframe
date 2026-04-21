//! Generate command - runs the full pipeline (typesync + schemasync).

use crate::cli::{Cli, GenerateArgs, TypesyncArgs};
use crate::config_builders;
use evenframe_core::{config::EvenframeConfig, error::Result, schemasync::Schemasync};
use tracing::{debug, error, info};

/// Runs the full generation pipeline with default settings.
pub async fn run_default(cli: &Cli) -> Result<()> {
    let args = GenerateArgs {
        skip_typesync: false,
        skip_schemasync: false,
        no_mocks: false,
        watch: false,
    };
    run(cli, args).await
}

/// Runs the full generation pipeline.
pub async fn run(_cli: &Cli, args: GenerateArgs) -> Result<()> {
    info!("Starting Evenframe code generation");

    // Load configuration
    let config = match EvenframeConfig::new() {
        Ok(cfg) => {
            info!("Configuration loaded successfully");
            cfg
        }
        Err(e) => {
            error!("Failed to load configuration: {}", e);
            return Err(e);
        }
    };

    // Build all configs
    info!("Building all configs...");
    let build_config = config_builders::BuildConfig::from_toml()?;
    let (enums, tables, objects) = config_builders::build_all_configs(&build_config)?;
    info!(
        "Config building complete. Found {} enums, {} tables, {} objects",
        enums.len(),
        tables.len(),
        objects.len()
    );

    // TypeSync phase — delegates to the typesync command with default args
    if !args.skip_typesync {
        let typesync_args = TypesyncArgs {
            command: None,
            all: false,
            formats: None,
            skip: None,
            per_file: false,
        };
        super::typesync::run(_cli, typesync_args).await?;
    } else {
        debug!("Skipping typesync phase");
    }

    // SchemaSync phase
    if !args.skip_schemasync {
        let (ss_enums, ss_tables, ss_objects) =
            config_builders::filter_for_schemasync(enums, tables, objects);
        run_schemasync(&config, &ss_enums, &ss_tables, &ss_objects, args.no_mocks).await?;
    } else {
        debug!("Skipping schemasync phase");
    }

    info!("Evenframe code generation completed successfully");
    Ok(())
}

async fn run_schemasync(
    _config: &EvenframeConfig,
    enums: &std::collections::BTreeMap<String, evenframe_core::types::TaggedUnion>,
    tables: &std::collections::BTreeMap<String, evenframe_core::schemasync::table::TableConfig>,
    objects: &std::collections::BTreeMap<String, evenframe_core::types::StructConfig>,
    _no_mocks: bool,
) -> Result<()> {
    info!("Starting Schemasync");

    let schemasync = Schemasync::new()
        .with_tables(tables)
        .with_objects(objects)
        .with_enums(enums);

    debug!(
        "Initialized Schemasync with {} tables, {} objects, {} enums",
        tables.len(),
        objects.len(),
        enums.len()
    );

    info!("Running Schemasync...");
    match schemasync.run().await {
        Ok(_) => info!("Schemasync completed successfully"),
        Err(e) => {
            error!("Schemasync failed: {}", e);
            return Err(e);
        }
    }

    Ok(())
}
