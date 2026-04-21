//! Schemasync command - synchronizes database schema.

use crate::cli::{Cli, DiffFormat, SchemasyncArgs, SchemasyncCommands};
use crate::config_builders;
use evenframe_core::{error::Result, schemasync::Schemasync};
use tracing::{debug, error, info};

/// Runs the schemasync command.
pub async fn run(_cli: &Cli, args: SchemasyncArgs) -> Result<()> {
    info!("Starting schema synchronization");

    // Build all configs and filter to schemasync-eligible types
    let build_config = config_builders::BuildConfig::from_toml()?;
    let (enums, tables, objects) = config_builders::build_all_configs(&build_config)?;
    let (enums, tables, objects) = config_builders::filter_for_schemasync(enums, tables, objects);

    info!(
        "Found {} enums, {} tables, {} objects",
        enums.len(),
        tables.len(),
        objects.len()
    );

    // Handle subcommands
    if let Some(cmd) = args.command {
        match cmd {
            SchemasyncCommands::Diff(diff_args) => {
                info!("Running schema diff...");

                let schemasync = Schemasync::new()
                    .with_tables(&tables)
                    .with_objects(&objects)
                    .with_enums(&enums);

                let changes = schemasync.diff().await?;

                match diff_args.format {
                    DiffFormat::Pretty => {
                        println!("{}", changes.summary());
                        for tc in &changes.modified_tables {
                            for field in &tc.new_fields {
                                println!("  + {}.{}", tc.table_name, field);
                            }
                            for field in &tc.removed_fields {
                                println!("  - {}.{}", tc.table_name, field);
                            }
                            for fc in &tc.modified_fields {
                                println!(
                                    "  ~ {}.{}: {} -> {}",
                                    tc.table_name, fc.field_name, fc.old_type, fc.new_type
                                );
                            }
                        }
                    }
                    DiffFormat::Json => {
                        let json = serde_json::to_string_pretty(&changes).map_err(|e| {
                            evenframe_core::error::EvenframeError::config(format!(
                                "Failed to serialize changes to JSON: {e}"
                            ))
                        })?;
                        println!("{json}");
                    }
                    DiffFormat::Plain => {
                        println!("{}", changes.summary());
                    }
                }
            }
            SchemasyncCommands::Apply(apply_args) => {
                if apply_args.dry_run {
                    info!("Dry run mode - showing what would be applied...");

                    let schemasync = Schemasync::new()
                        .with_tables(&tables)
                        .with_objects(&objects)
                        .with_enums(&enums);

                    let changes = schemasync.diff().await?;
                    println!("{}", changes.summary());
                    return Ok(());
                }

                if !apply_args.yes {
                    use std::io::{self, Write};
                    print!("Apply schema changes to the database? [y/N] ");
                    io::stdout().flush().map_err(|e| {
                        evenframe_core::error::EvenframeError::config(format!(
                            "Failed to flush stdout: {e}"
                        ))
                    })?;
                    let mut input = String::new();
                    io::stdin().read_line(&mut input).map_err(|e| {
                        evenframe_core::error::EvenframeError::config(format!(
                            "Failed to read confirmation input: {e}"
                        ))
                    })?;
                    if !input.trim().eq_ignore_ascii_case("y") {
                        info!("Aborted by user");
                        return Ok(());
                    }
                }

                run_schemasync(&enums, &tables, &objects).await?;
            }
            SchemasyncCommands::Mock(mock_args) => {
                info!("Generating mock data only...");

                let schemasync = Schemasync::new()
                    .with_tables(&tables)
                    .with_objects(&objects)
                    .with_enums(&enums);

                schemasync
                    .mock_only(mock_args.count, mock_args.tables)
                    .await?;
                info!("Mock data generation completed");
            }
        }
        return Ok(());
    }

    // Default: run full schemasync
    run_schemasync(&enums, &tables, &objects).await
}

async fn run_schemasync(
    enums: &std::collections::BTreeMap<String, evenframe_core::types::TaggedUnion>,
    tables: &std::collections::BTreeMap<String, evenframe_core::schemasync::table::TableConfig>,
    objects: &std::collections::BTreeMap<String, evenframe_core::types::StructConfig>,
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
