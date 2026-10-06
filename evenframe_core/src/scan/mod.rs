//! Finding a workspace's derived types and turning them into configs: what
//! the build-script API and the CLI both start from.

pub(crate) mod config;
mod configs;
pub mod expansion;
pub mod paths;
mod workspace;

pub use config::{ScanConfig, ScanConfigBuilder};
pub use configs::{ParsedType, TableAttributes, build_all_configs, merge_tables_and_objects};
pub use workspace::{
    EvenframeType, MAX_SCAN_DEPTH, Scan, ScannedItem, TypeKind, WorkspaceScanner,
    canonical_manifests, find_manifests, member_has_own_manifest,
};
