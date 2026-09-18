//! Schemasync command - synchronizes database schema.

use crate::cli::{DiffFormat, DumpCommands, SchemasyncArgs, SchemasyncCommands};
use crate::config_builders;
use evenframe_core::{
    error::Result,
    schemasync::table::TableConfig,
    schemasync::{
        Schemasync,
        config::{ConnectionOverrides, MockOverrides},
    },
    types::{StructConfig, TaggedUnion},
};
use std::collections::BTreeMap;
use tracing::{debug, info};

/// Runs the schemasync command.
pub async fn run(args: SchemasyncArgs) -> Result<()> {
    info!("Starting schema synchronization");

    // Build all configs and filter to schemasync-eligible types
    let build_config = config_builders::BuildConfig::discover()?;
    let (enums, tables, objects) = config_builders::build_and_record(&build_config)?;
    let (enums, tables, objects) = config_builders::filter_for_schemasync(enums, tables, objects);

    info!(
        "Found {} enums, {} tables, {} objects",
        enums.len(),
        tables.len(),
        objects.len()
    );

    let overrides = ConnectionOverrides {
        url: args.url.clone(),
        namespace: args.namespace.clone(),
        database: args.database.clone(),
    };
    let mocks = MockOverrides {
        skip_mocks: args.no_mocks,
        full_refresh: args.full_refresh,
    };

    // Handle subcommands
    if let Some(cmd) = args.command {
        match cmd {
            SchemasyncCommands::Diff(diff_args) => {
                info!("Running schema diff...");

                let schemasync = Schemasync::new()
                    .with_connection_overrides(overrides.clone())
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
                        .with_connection_overrides(overrides.clone())
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
                        println!("Aborted");
                        return Ok(());
                    }
                }

                run_schemasync(&enums, &tables, &objects, overrides, mocks).await?;
            }
            SchemasyncCommands::Mock(mock_args) => {
                info!("Generating mock data only...");

                let schemasync = Schemasync::new()
                    .with_connection_overrides(overrides.clone())
                    .with_tables(&tables)
                    .with_objects(&objects)
                    .with_enums(&enums);

                schemasync
                    .mock_only(mock_args.count, mock_args.tables)
                    .await?;
                info!("Mock data generation completed");
            }
            SchemasyncCommands::Dump(dump_args) => {
                info!("Dumping resolved schema SurrealQL (offline)...");

                // This path opens no database connection, so the connection
                // settings' env vars aren't required.
                let config = evenframe_core::config::EvenframeConfig::new_offline()?;
                let registry = evenframe_core::types::ForeignTypeRegistry::from_config(
                    &config.general.foreign_types,
                );
                let allow_scripting = config.schemasync.mock_gen_config.scripting_asserts;

                let tables_surql = evenframe_core::schemasync::dump::tables_surql(
                    &tables,
                    &objects,
                    &enums,
                    &registry,
                    allow_scripting,
                );
                let (ddl, output_path) = match dump_args.command {
                    Some(DumpCommands::Tables(tables_args)) => (
                        tables_surql,
                        tables_args.file.unwrap_or_else(|| {
                            config.project_root().join(".evenframe/surql/tables.surql")
                        }),
                    ),
                    None => (
                        evenframe_core::schemasync::dump::schema_surql(
                            &config.schemasync.database,
                            &tables_surql,
                        ),
                        dump_args.file.unwrap_or_else(|| {
                            config.project_root().join(".evenframe/surql/schema.surql")
                        }),
                    ),
                };
                let statement_count = ddl
                    .lines()
                    .filter(|line| line.trim_start().starts_with("DEFINE"))
                    .count();

                if let Some(parent) = output_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        evenframe_core::error::EvenframeError::config(format!(
                            "Failed to create output directory {}: {e}",
                            parent.display()
                        ))
                    })?;
                }

                std::fs::write(&output_path, &ddl).map_err(|e| {
                    evenframe_core::error::EvenframeError::config(format!(
                        "Failed to write schema dump to {}: {e}",
                        output_path.display()
                    ))
                })?;

                println!(
                    "Wrote {statement_count} DEFINE statements across {} tables to {}",
                    tables.len(),
                    output_path.display()
                );
            }
        }
        return Ok(());
    }

    // Default: run full schemasync
    run_schemasync(&enums, &tables, &objects, overrides, mocks).await
}

/// Runs the full schemasync pipeline over the scanned types.
pub(crate) async fn run_schemasync(
    enums: &BTreeMap<String, TaggedUnion>,
    tables: &BTreeMap<String, TableConfig>,
    objects: &BTreeMap<String, StructConfig>,
    overrides: ConnectionOverrides,
    mocks: MockOverrides,
) -> Result<()> {
    let schemasync = Schemasync::new()
        .with_connection_overrides(overrides)
        .with_mock_overrides(mocks)
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
    schemasync.run().await?;
    info!("Schemasync completed successfully");
    Ok(())
}
