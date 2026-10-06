//! Mockmake command - inserts mock data from the scan cache.

use crate::cli::MockmakeArgs;
use crate::scan_cache::ScanCache;
use evenframe_core::scan::ScanConfig;
use evenframe_core::{
    error::Result,
    schemasync::{Schemasync, config::ConnectionOverrides},
};
use tracing::info;

/// Runs the mockmake command.
pub async fn run(args: MockmakeArgs) -> Result<()> {
    let build_config = ScanConfig::discover()?;
    let cache = ScanCache::load_current(&build_config)?;
    let types = cache.into_configs().into_schemasync()?;

    info!(
        "Loaded {} tables, {} objects, {} enums from the scan cache",
        types.tables.len(),
        types.objects.len(),
        types.enums.len()
    );

    Schemasync::new()
        .with_connection_overrides(ConnectionOverrides {
            url: args.url,
            namespace: args.namespace,
            database: args.database,
        })
        .with_types(&types)
        .insert_mock_data(args.count, args.tables)
        .await?;

    println!("Inserted mock data");
    Ok(())
}
