//! Mockmake command - inserts mock data from the scan cache.

use crate::cli::MockmakeArgs;
use crate::config_builders::{self, BuildConfig, ScanCache};
use evenframe_core::{
    error::Result,
    schemasync::{Schemasync, config::ConnectionOverrides},
};
use tracing::info;

/// Runs the mockmake command.
pub async fn run(args: MockmakeArgs) -> Result<()> {
    let build_config = BuildConfig::discover()?;
    let cache = ScanCache::load_current(&build_config)?;
    let (enums, tables, objects) = cache.into_configs();
    let (enums, tables, objects) = config_builders::filter_for_schemasync(enums, tables, objects);

    info!(
        "Loaded {} tables, {} objects, {} enums from the scan cache",
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
