//! Validate command - validates configuration and types.

use crate::cli::ValidateArgs;
use crate::target::Target;
use evenframe_core::{
    config::EvenframeConfig,
    error::{EvenframeError, Result},
    schemasync::{check_database_connectivity, config::ConnectionOverrides, load_connected_config},
    typesync::config::OutputMode,
};
use tracing::{info, warn};

/// Runs the validate command.
pub async fn run(args: ValidateArgs) -> Result<()> {
    info!("Validating Evenframe configuration and types");

    let mut has_errors = false;

    let target = match Target::discover() {
        Ok(target) => Some(target),
        Err(e) => {
            // Reported once, under the check the user asked for.
            let check = if args.types_only {
                "Types"
            } else {
                "Configuration"
            };
            println!("{check}: FAILED");
            println!("  {e}");
            if !args.types_only && !args.config_only {
                println!("Types: SKIPPED, the configuration did not load");
            }
            has_errors = true;
            None
        }
    };

    if !args.types_only
        && let Some(target) = &target
    {
        println!("Configuration: OK");
        for output in &typesync_config(target).typesync.outputs {
            let layout = match output.files.mode {
                OutputMode::Single => "",
                OutputMode::PerFile => " (per-file)",
            };
            println!("  Output: {} -> {}{layout}", output.kind, output.dir);
        }
    }

    if !args.config_only
        && let Some(target) = &target
    {
        match validate_types(target) {
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

    if args.check_db
        && let Some(target) = &target
    {
        match check_database(target).await {
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

/// The configuration typesync follows: the project's, or the workspace's.
fn typesync_config(target: &Target) -> &EvenframeConfig {
    match target {
        Target::Project(config) => config,
        Target::Workspace(workspace) => &workspace.config,
    }
}

/// Scans every project and merges their types, which is where two projects
/// disagreeing on a shared type is found.
fn validate_types(target: &Target) -> Result<(usize, usize, usize)> {
    let scanned = target.scan_all()?;
    let (_, configs) = target.typesync_input(&scanned)?;
    Ok((
        configs.enums.len(),
        configs.tables.len(),
        configs.objects.len(),
    ))
}

async fn check_database(target: &Target) -> Result<()> {
    for (_, config) in target.focused() {
        let config = load_connected_config(config, &ConnectionOverrides::default())?;
        check_database_connectivity(&config).await?;
    }
    Ok(())
}
