//! Mockmake command - inserts mock data from the scan cache.

use crate::cli::MockmakeArgs;
use crate::scan_cache::ScanCache;
use crate::target::Target;
use evenframe_core::scan::ScanConfig;
use evenframe_core::{
    error::Result,
    schemasync::{Schemasync, config::ConnectionOverrides},
};
use tracing::info;

/// Runs the mockmake command for each focused project, against its own
/// database.
pub async fn run(args: MockmakeArgs) -> Result<()> {
    let target = Target::discover()?;
    for (name, config) in target.focused() {
        if !name.is_empty() {
            info!("Project {name}");
        }
        let cache = ScanCache::load_current(&ScanConfig::from_config(config))?;
        let types = cache.into_configs().into_schemasync()?;

        info!(
            "Loaded {} tables, {} objects, {} enums from the scan cache",
            types.tables.len(),
            types.objects.len(),
            types.enums.len()
        );

        Schemasync::new(config)
            .with_connection_overrides(ConnectionOverrides {
                url: args.url.clone(),
                namespace: args.namespace.clone(),
                database: args.database.clone(),
            })
            .with_types(&types)
            .insert_mock_data(args.count, args.tables.clone())
            .await?;
    }

    println!("Inserted mock data");
    Ok(())
}
