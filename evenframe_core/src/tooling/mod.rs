//! Build-time tooling for scanning and processing Evenframe types.
//!
//! This module provides utilities for:
//! - Scanning Rust workspaces for types with Evenframe derives
//! - Building configuration from parsed Rust types
//! - Processing struct and enum definitions
//! - Generating TypeScript types in build.rs files
//!
//! ## Quick Start (in build.rs)
//!
//! ```rust,ignore
//! fn main() {
//!     evenframe_core::tooling::generate().expect("Type generation failed");
//!     println!("cargo:rerun-if-changed=src/");
//!     println!("cargo:rerun-if-changed=evenframe.toml");
//! }
//! ```

mod build_config;
mod cache;
mod config_builders;
pub mod expansion_cache;
mod generator;
mod workspace_scanner;

pub use build_config::{BuildConfig, BuildConfigBuilder};
pub use cache::{
    CACHE_FORMAT_VERSION, CACHE_REFRESH_COMMAND, CACHE_RELATIVE_PATH, CacheStatus, InputStamp,
    ScanCache, build_and_record, scan_inputs,
};
pub use config_builders::{
    AllConfigs, ParsedType, TableAttributes, build_all_configs, filter_for_schemasync,
    filter_for_typesync, merge_tables_and_objects,
};
pub use generator::{GenerationReport, TypeGenerator};
pub use workspace_scanner::{
    EvenframeType, MAX_SCAN_DEPTH, ScannedItem, TypeKind, WorkspaceScanner, canonical_manifests,
    find_manifests, member_has_own_manifest,
};

use crate::error::EvenframeError;

/// Generates all enabled type outputs using configuration from evenframe.toml.
///
/// This is the simplest way to use the tooling module. It reads configuration from
/// `evenframe.toml` (searching from `CARGO_MANIFEST_DIR` upward) and generates
/// all enabled type outputs.
///
/// # Errors
///
/// Returns `EvenframeError` if:
/// - Configuration file cannot be found or parsed
/// - Source files cannot be read
/// - Output files cannot be written
pub fn generate() -> Result<GenerationReport, EvenframeError> {
    let config = BuildConfig::from_toml()?;
    TypeGenerator::new(config).generate_all()
}

/// Generates type outputs with a custom configuration.
///
/// Use this when you need to override settings from evenframe.toml or
/// configure generation programmatically.
pub fn generate_with_config(config: BuildConfig) -> Result<GenerationReport, EvenframeError> {
    TypeGenerator::new(config).generate_all()
}
