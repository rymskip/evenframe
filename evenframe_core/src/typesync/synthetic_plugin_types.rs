//! Serde types for synthetic-item WASM plugin communication.
//!
//! Unlike [`super::plugin_types`], synthetic plugins don't override existing
//! items — they receive the full set of `StructConfig`, `TaggedUnion`, and
//! `TableConfig` values evenframe has accumulated, and return *new* ones to
//! be merged into the build.
//!
//! # Why full configs, not summaries
//!
//! An earlier iteration passed lightweight summaries (name + a canonical
//! field-type string) to keep the JSON small. That turned out to block
//! legitimate system-wide plugins: a partial-projection plugin, for
//! example, needs to copy field types *verbatim* from an existing struct
//! into a new one, and can't rebuild `FieldType::Option(Box<FieldType::
//! RecordLink<...>>)` from a display string. Full configs solve that — they
//! round-trip the real serde tree, so plugins can splat any field
//! directly into their output.
//!
//! Both input and output use the real [`StructConfig`] / [`TaggedUnion`] /
//! [`TableConfig`] types, which are already `Serialize + Deserialize` in
//! this crate. The plugin crate (`evenframe_plugin`) receives them as
//! `serde_json::Value` maps so plugin authors don't have to pull
//! `evenframe_core` (and all its deps) into their cdylibs.

use crate::schemasync::table::TableConfig;
use crate::types::{StructConfig, TaggedUnion};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Full snapshot of everything the scanner + rule plugins have accumulated,
/// handed to each synthetic plugin for system-wide decisions.
#[derive(Debug, Clone, Serialize)]
pub struct SyntheticPluginInput {
    /// Non-persisted application structs, keyed by struct name.
    pub structs: HashMap<String, StructConfig>,
    /// Tagged unions (Rust enums), keyed by enum name.
    pub enums: HashMap<String, TaggedUnion>,
    /// Persisted structs (tables), keyed by snake_case table name.
    pub tables: HashMap<String, TableConfig>,
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
