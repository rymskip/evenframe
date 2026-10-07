//! The build-script API: scan the workspace, then write the configured type
//! outputs (`typesync`, with `build-typesync`) or the schema's SurrealQL
//! (`schemadump`, with `build-schemadump`).
//!
//! ```rust,ignore
//! fn main() {
//!     evenframe::build::typesync().expect("type generation failed");
//!     evenframe::build::schemadump().expect("schema dump failed");
//!     println!("cargo:rerun-if-changed=src/");
//!     println!("cargo:rerun-if-changed=evenframe.toml");
//! }
//! ```

#[cfg(feature = "build-typesync")]
mod generator;

#[cfg(feature = "build-typesync")]
pub use generator::{GenerationReport, TypeGenerator};

use crate::error::EvenframeError;
use crate::scan::ScanConfig;

/// Writes every configured type output, using configuration from
/// evenframe.toml (searching from `CARGO_MANIFEST_DIR` upward).
///
/// # Errors
///
/// Returns `EvenframeError` if the configuration cannot be found or parsed,
/// a source file cannot be read, or an output cannot be written.
#[cfg(feature = "build-typesync")]
pub fn typesync() -> Result<GenerationReport, EvenframeError> {
    TypeGenerator::new(ScanConfig::from_toml()?).generate_all()
}

/// Writes every output `config` names, for configuration built in code.
#[cfg(feature = "build-typesync")]
pub fn typesync_with(config: ScanConfig) -> Result<GenerationReport, EvenframeError> {
    TypeGenerator::new(config).generate_all()
}

/// Writes the schema's SurrealQL to `.evenframe/surql/schema.surql`, as
/// `evenframe schemasync dump` does, using configuration from evenframe.toml
/// (searching from `CARGO_MANIFEST_DIR` upward). It connects to no database,
/// and leaves the file untouched when the schema has not changed. Returns
/// the path written.
#[cfg(feature = "build-schemadump")]
pub fn schemadump() -> Result<std::path::PathBuf, EvenframeError> {
    use crate::scan::build_all_configs;
    use crate::schemasync::dump::{DumpScope, dump_surql, write_dump};

    let config = crate::scan::config::build_script_config()?;
    let types = build_all_configs(&ScanConfig::from_config(&config))?.into_schemasync()?;
    let path = DumpScope::Schema.default_path(config.project_root());
    write_dump(&path, &dump_surql(&config, &types, DumpScope::Schema)?)?;
    Ok(path)
}
