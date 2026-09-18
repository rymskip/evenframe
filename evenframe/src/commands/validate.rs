//! Validate command - validates configuration and types.

use crate::cli::ValidateArgs;
use crate::config_builders;
use evenframe_core::{
    config::EvenframeConfig,
    error::{EvenframeError, Result},
    typesync::config::OutputMode,
};
use tracing::{info, warn};

/// Runs the validate command.
pub async fn run(args: ValidateArgs) -> Result<()> {
    info!("Validating Evenframe configuration and types");

    let mut has_errors = false;

    if !args.types_only {
        match EvenframeConfig::new() {
            Ok(config) => {
                println!("Configuration: OK");
                for output in &config.typesync.outputs {
                    let layout = match output.files.mode {
                        OutputMode::Single => "",
                        OutputMode::PerFile => " (per-file)",
                    };
                    println!("  Output: {} -> {}{layout}", output.kind, output.dir);
                }
            }
            Err(e) => {
                println!("Configuration: FAILED");
                println!("  {e}");
                has_errors = true;
            }
        }
    }

    if !args.config_only {
        match validate_types() {
            Ok((enums, tables, objects)) => {
                println!("Types: OK");
                println!("  tables: {tables}, objects: {objects}, enums: {enums}");
                if enums + tables + objects == 0 {
                    warn!("No Evenframe types found in workspace");
                }
            }
            Err(e) => {
                println!("Types: FAILED");
                println!("  {e}");
                has_errors = true;
            }
        }
    }

    if args.check_db {
        match check_database().await {
            Ok(()) => println!("Database: OK"),
            // Connectivity depends on the environment, not the project, so it
            // is reported without failing validation.
            Err(e) => {
                println!("Database: UNREACHABLE");
                println!("  {e}");
            }
        }
    }

    if has_errors {
        return Err(EvenframeError::Validation(
            "validation failed with errors".to_string(),
        ));
    }
    println!("Validation passed");
    Ok(())
}

fn validate_types() -> Result<(usize, usize, usize)> {
    let build_config = config_builders::BuildConfig::discover()?;
    let (enums, tables, objects) = config_builders::build_and_record(&build_config)?;
    Ok((enums.len(), tables.len(), objects.len()))
}

async fn check_database() -> Result<()> {
    evenframe_core::schemasync::check_database_connectivity().await
}
