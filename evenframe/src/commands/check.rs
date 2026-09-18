//! Check command - reports whether the scan cache matches the Rust sources.

use crate::cli::CheckArgs;
use evenframe_core::{
    error::{EvenframeError, Result},
    tooling::{BuildConfig, CACHE_REFRESH_COMMAND, CacheStatus, ScanCache},
};
use serde::Serialize;

/// Runs the check command.
pub async fn run(args: CheckArgs) -> Result<()> {
    let config = BuildConfig::discover()?;
    let report = Report::new(
        ScanCache::path(&config.scan_path).display().to_string(),
        ScanCache::status(&config)?,
    );

    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        report.print();
    }

    if !report.in_sync() {
        return Err(EvenframeError::Validation(
            "the scan cache is not current".to_string(),
        ));
    }
    Ok(())
}

/// What the command reports, in both its human and its JSON form.
#[derive(Debug, Serialize)]
struct Report {
    cache_path: String,
    #[serde(flatten)]
    status: ReportStatus,
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum ReportStatus {
    InSync { types: TypeCounts },
    Missing,
    Stale { reason: String },
}

#[derive(Debug, Serialize)]
struct TypeCounts {
    tables: usize,
    objects: usize,
    enums: usize,
}

impl Report {
    fn new(cache_path: String, status: CacheStatus) -> Self {
        let status = match status {
            CacheStatus::Current(cache) => ReportStatus::InSync {
                types: TypeCounts {
                    tables: cache.tables.len(),
                    objects: cache.objects.len(),
                    enums: cache.enums.len(),
                },
            },
            CacheStatus::Absent => ReportStatus::Missing,
            CacheStatus::Stale(reason) => ReportStatus::Stale { reason },
        };
        Self { cache_path, status }
    }

    fn in_sync(&self) -> bool {
        matches!(self.status, ReportStatus::InSync { .. })
    }

    fn print(&self) {
        match &self.status {
            ReportStatus::InSync { types } => {
                println!("Types are in sync with {}", self.cache_path);
                println!(
                    "  tables: {}, objects: {}, enums: {}",
                    types.tables, types.objects, types.enums
                );
            }
            ReportStatus::Missing => {
                println!("No scan cache at {}", self.cache_path);
                println!("  Run `{CACHE_REFRESH_COMMAND}` to write it");
            }
            ReportStatus::Stale { reason } => {
                println!("Types are out of sync: {reason}");
                println!(
                    "  Run `{CACHE_REFRESH_COMMAND}` to refresh {}",
                    self.cache_path
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evenframe_core::tooling::CACHE_FORMAT_VERSION;
    use evenframe_core::types::StructConfig;
    use std::collections::BTreeMap;

    fn cache_with_one_object() -> ScanCache {
        ScanCache {
            format_version: CACHE_FORMAT_VERSION,
            evenframe_version: "0.0.0".to_string(),
            inputs: BTreeMap::new(),
            enums: BTreeMap::new(),
            tables: BTreeMap::new(),
            objects: BTreeMap::from([("Address".to_string(), StructConfig::default())]),
        }
    }

    fn report(status: CacheStatus) -> Report {
        Report::new(".evenframe/cache.json".to_string(), status)
    }

    #[test]
    fn only_a_current_cache_is_in_sync() {
        assert!(report(CacheStatus::Current(cache_with_one_object())).in_sync());
        assert!(!report(CacheStatus::Absent).in_sync());
        assert!(!report(CacheStatus::Stale("src/lib.rs changed".to_string())).in_sync());
    }

    #[test]
    fn json_carries_the_status_counts_and_reason() {
        let json =
            serde_json::to_value(report(CacheStatus::Current(cache_with_one_object()))).unwrap();
        assert_eq!(json["status"], "in_sync");
        assert_eq!(json["cache_path"], ".evenframe/cache.json");
        assert_eq!(json["types"]["objects"], 1);
        assert_eq!(json["types"]["tables"], 0);

        let json = serde_json::to_value(report(CacheStatus::Absent)).unwrap();
        assert_eq!(json["status"], "missing");

        let json =
            serde_json::to_value(report(CacheStatus::Stale("src/lib.rs changed".to_string())))
                .unwrap();
        assert_eq!(json["status"], "stale");
        assert_eq!(json["reason"], "src/lib.rs changed");
    }
}
