//! Validate command - validates configuration and types.

use crate::cli::{Cli, ValidateArgs};
use crate::config_builders;
use crate::workspace_scanner::WorkspaceScanner;
use evenframe_core::{config::EvenframeConfig, error::Result};
use tracing::{error, info, warn};

/// Runs the validate command.
pub async fn run(_cli: &Cli, args: ValidateArgs) -> Result<()> {
    info!("Validating Evenframe configuration and types");

    let mut has_errors = false;

    // Validate configuration
    if !args.types_only {
        info!("Checking configuration...");
        match EvenframeConfig::new() {
            Ok(config) => {
                info!("  Configuration file: OK");
                info!("    Output path: {}", config.typesync.output_path);
                info!(
                    "    Generators: arktype={}, effect={}, macroforge={}, flatbuffers={}, protobuf={}",
                    config.typesync.should_generate_arktype_types,
                    config.typesync.should_generate_effect_types,
                    config.typesync.should_generate_macroforge_types,
                    config.typesync.should_generate_flatbuffers_types,
                    config.typesync.should_generate_protobuf_types
                );
            }
            Err(e) => {
                error!("  Configuration file: FAILED");
                error!("    Error: {}", e);
                has_errors = true;
            }
        }
    }

    // Validate types
    if !args.config_only {
        info!("Checking types...");
        match validate_types() {
            Ok((enums, tables, objects)) => {
                info!("  Types: OK");
                info!("    Enums: {}", enums);
                info!("    Tables: {}", tables);
                info!("    Objects: {}", objects);
            }
            Err(e) => {
                error!("  Types: FAILED");
                error!("    Error: {}", e);
                has_errors = true;
            }
        }
    }

    // Check database connectivity
    if args.check_db {
        info!("Checking database connectivity...");
        match check_database().await {
            Ok(_) => {
                info!("  Database: OK");
            }
            Err(e) => {
                warn!("  Database: FAILED");
                warn!("    Error: {}", e);
                // Don't fail validation for DB connectivity issues
            }
        }
    }

    if has_errors {
        error!("Validation failed with errors");
    } else {
        info!("Validation passed");
    }

    Ok(())
}

fn validate_types() -> Result<(usize, usize, usize)> {
    let config = EvenframeConfig::new()?;

    let scanner = WorkspaceScanner::new(config.general.apply_aliases)?;
    let types = scanner.scan_for_evenframe_types()?;

    if types.is_empty() {
        warn!("No Evenframe types found in workspace");
    }

    let (enums, tables, objects) = config_builders::build_all_configs();
    Ok((enums.len(), tables.len(), objects.len()))
}

async fn check_database() -> Result<()> {
    // TODO: Implement database connectivity check
    // This would connect to SurrealDB and verify the connection works
    info!("    (Database connectivity check not yet implemented)");
    Ok(())
}
