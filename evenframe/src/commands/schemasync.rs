//! Schemasync command - synchronizes database schema.

use crate::cli::{Cli, SchemasyncArgs, SchemasyncCommands};
use crate::config_builders;
use evenframe_core::{error::Result, schemasync::Schemasync};
use tracing::{debug, error, info};

/// Runs the schemasync command.
pub async fn run(_cli: &Cli, args: SchemasyncArgs) -> Result<()> {
    info!("Starting schema synchronization");

    // Build all configs
    let (enums, tables, objects) = config_builders::build_all_configs();

    info!(
        "Found {} enums, {} tables, {} objects",
        enums.len(),
        tables.len(),
        objects.len()
    );

    // Handle subcommands
    if let Some(cmd) = args.command {
        match cmd {
            SchemasyncCommands::Diff(_diff_args) => {
                info!("Running schema diff (dry-run)...");
                // TODO: Implement diff functionality
                info!("Schema diff not yet implemented");
            }
            SchemasyncCommands::Apply(apply_args) => {
                if apply_args.dry_run {
                    info!("Dry run mode - showing what would be applied...");
                    // TODO: Implement dry run
                    return Ok(());
                }

                if !apply_args.yes {
                    // TODO: Add confirmation prompt
                    info!("Use --yes to skip confirmation");
                }

                run_schemasync(&enums, &tables, &objects).await?;
            }
            SchemasyncCommands::Mock(_mock_args) => {
                info!("Generating mock data only...");
                // TODO: Implement mock-only generation
                info!("Mock-only generation not yet implemented");
            }
        }
        return Ok(());
    }

    // Default: run full schemasync
    run_schemasync(&enums, &tables, &objects).await
}

async fn run_schemasync(
    enums: &std::collections::HashMap<String, evenframe_core::types::TaggedUnion>,
    tables: &std::collections::HashMap<String, evenframe_core::schemasync::table::TableConfig>,
    objects: &std::collections::HashMap<String, evenframe_core::types::StructConfig>,
) -> Result<()> {
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
        Ok(_) => {
            info!("Schemasync completed successfully");
            Ok(())
        }
        Err(e) => {
            error!("Schemasync failed: {}", e);
            Err(e)
        }
    }
}
