//! Serde types for synthetic-item WASM plugin communication.
//!
//! Unlike [`super::plugin_types`], synthetic plugins don't override existing
//! items: they receive every `StructConfig`, `TaggedUnion` and `TableConfig`
//! evenframe has accumulated, and return new ones to merge into the build.
//! Full configs let a plugin copy field types verbatim (a partial
//! projection, say) instead of rebuilding them from a display string. The
//! plugin crate (`evenframe_plugin`) reads them as `serde_json::Value` maps,
//! so plugins don't pull in `evenframe_core`.

use crate::schemasync::table::TableConfig;
use crate::types::{StructConfig, TaggedUnion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Full snapshot of everything the scanner + rule plugins have accumulated,
/// handed to each synthetic plugin for system-wide decisions.
#[derive(Debug, Clone, Serialize)]
pub struct SyntheticPluginInput<'a> {
    /// Non-persisted application structs, keyed by struct name.
    pub structs: &'a BTreeMap<String, StructConfig>,
    /// Tagged unions (Rust enums), keyed by enum name.
    pub enums: &'a BTreeMap<String, TaggedUnion>,
    /// Persisted structs (tables), keyed by snake_case table name.
    pub tables: &'a BTreeMap<String, TableConfig>,
}

/// Plugin response: a set of brand-new items to merge into the build.
///
/// All three lists are independent and optional; a plugin that only
/// generates structs can leave `new_enums` and `new_tables` empty.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct SyntheticPluginOutput {
    #[serde(default)]
    pub new_structs: Vec<StructConfig>,
    #[serde(default)]
    pub new_enums: Vec<TaggedUnion>,
    #[serde(default)]
    pub new_tables: Vec<TableConfig>,
    #[serde(default)]
    pub error: Option<String>,
}
