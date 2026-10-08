//! Schemasync command - synchronizes database schema.

use crate::cli::{DiffFormat, DumpCommands, SchemasyncArgs, SchemasyncCommands};
use crate::target::Target;
use evenframe_core::{
    config::EvenframeConfig,
    error::{EvenframeError, Result},
    schemasync::{
        Schemasync,
        config::{ConnectionOverrides, MockOverrides},
        dump::{DumpScope, dump_surql, write_dump},
    },
    types::SchemasyncTypes,
};
use tracing::{debug, info};

/// Runs the schemasync command for each focused project, against its own
/// database.
pub async fn run(args: SchemasyncArgs) -> Result<()> {
    info!("Starting schema synchronization");
    let target = Target::discover()?;
    let focused = target.scan_focused()?;
    if focused.len() > 1
        && let Some(SchemasyncCommands::Dump(dump_args)) = &args.command
        && (dump_args.file.is_some()
            || matches!(&dump_args.command, Some(DumpCommands::Tables(tables)) if tables.file.is_some()))
    {
        return Err(EvenframeError::config(format!(
            "-o names one file, but this run dumps {} projects; run it from one project's \
             directory, or leave out -o to write each project's own",
            focused.len()
        )));
    }
    for project in focused {
        if !project.name.is_empty() {
            info!("Project {}", project.name);
        }
        run_project(
            project.config,
            project.types.into_schemasync()?,
            args.clone(),
        )
        .await?;
    }
    Ok(())
}

async fn run_project(
    config: &EvenframeConfig,
    types: SchemasyncTypes,
    args: SchemasyncArgs,
) -> Result<()> {
    info!(
        "Found {} enums, {} tables, {} objects",
        types.enums.len(),
        types.tables.len(),
        types.objects.len()
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

                let schemasync = Schemasync::new(config)
                    .with_connection_overrides(overrides.clone())
                    .with_types(&types);

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
                            EvenframeError::config(format!(
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

                    let schemasync = Schemasync::new(config)
                        .with_connection_overrides(overrides.clone())
                        .with_types(&types);

                    let changes = schemasync.diff().await?;
                    println!("{}", changes.summary());
                    return Ok(());
                }

                if !apply_args.yes {
                    use std::io::{self, Write};
                    print!("Apply schema changes to the database? [y/N] ");
                    io::stdout().flush().map_err(|e| {
                        EvenframeError::config(format!("Failed to flush stdout: {e}"))
                    })?;
                    let mut input = String::new();
                    io::stdin().read_line(&mut input).map_err(|e| {
                        EvenframeError::config(format!("Failed to read confirmation input: {e}"))
                    })?;
                    if !input.trim().eq_ignore_ascii_case("y") {
                        println!("Aborted");
                        return Ok(());
                    }
                }

                run_schemasync(config, &types, overrides, mocks).await?;
            }
            SchemasyncCommands::Mock(mock_args) => {
                info!("Generating mock data only...");

                let schemasync = Schemasync::new(config)
                    .with_connection_overrides(overrides.clone())
                    .with_types(&types);

                schemasync
                    .mock_only(mock_args.count, mock_args.tables)
                    .await?;
                info!("Mock data generation completed");
            }
            SchemasyncCommands::Dump(dump_args) => {
                info!("Dumping resolved schema SurrealQL (offline)...");

                let (scope, chosen_file) = match dump_args.command {
                    Some(DumpCommands::Tables(tables_args)) => {
                        (DumpScope::Tables, tables_args.file)
                    }
                    None => (DumpScope::Schema, dump_args.file),
                };
                let ddl = dump_surql(config, &types, scope)?;
                let output_path =
                    chosen_file.unwrap_or_else(|| scope.default_path(config.project_root()));
                let statement_count = ddl
                    .lines()
                    .filter(|line| line.trim_start().starts_with("DEFINE"))
                    .count();
                write_dump(&output_path, &ddl)?;

                println!(
                    "Wrote {statement_count} DEFINE statements across {} tables to {}",
                    types.tables.len(),
                    output_path.display()
                );
            }
        }
        return Ok(());
    }

    // Default: run full schemasync
    run_schemasync(config, &types, overrides, mocks).await
}

/// Runs the full schemasync pipeline over the scanned types.
pub(crate) async fn run_schemasync(
    config: &EvenframeConfig,
    types: &SchemasyncTypes,
    overrides: ConnectionOverrides,
    mocks: MockOverrides,
) -> Result<()> {
    let schemasync = Schemasync::new(config)
        .with_connection_overrides(overrides)
        .with_mock_overrides(mocks)
        .with_types(types);

    debug!(
        "Initialized Schemasync with {} tables, {} objects, {} enums",
        types.tables.len(),
        types.objects.len(),
        types.enums.len()
    );

    info!("Running Schemasync...");
    schemasync.run().await?;
    info!("Schemasync completed successfully");
    Ok(())
}
