//! Mockmake command - inserts mock data from the scan registry.

use crate::cli::{Cli, MockmakeArgs};
use crate::config_builders::{self, BuildConfig, ScanRegistry};
use evenframe_core::{
    error::Result,
    schemasync::{Schemasync, config::ConnectionOverrides},
};
use tracing::info;

/// Runs the mockmake command.
pub async fn run(_cli: &Cli, args: MockmakeArgs) -> Result<()> {
    let build_config = BuildConfig::from_toml()?;
    let registry = ScanRegistry::load_current(&build_config)?;
    let (enums, tables, objects) = registry.into_configs();
    let (enums, tables, objects) = config_builders::filter_for_schemasync(enums, tables, objects);

    info!(
        "Loaded {} tables, {} objects, {} enums from the scan registry",
        tables.len(),
        objects.len(),
        enums.len()
    );

    Schemasync::new()
        .with_connection_overrides(ConnectionOverrides {
            url: args.url,
            namespace: args.namespace,
            database: args.database,
        })
        .with_tables(&tables)
        .with_objects(&objects)
        .with_enums(&enums)
        .insert_mock_data(args.count, args.tables)
        .await?;

    println!("Inserted mock data");
    Ok(())
}
