//! Serde types for output rule WASM plugin communication.
//!
//! A plugin receives the full `StructConfig`, `TableConfig`, `TaggedUnion`
//! or `NewtypeConfig` the host holds for the type it processes, so it can read
//! anything about it (events, relations, per-field `define_config`, variant
//! representations) rather than a lossy summary. The plugin crate
//! (`evenframe_plugin`) reads them as `serde_json::Value` maps, so plugins
//! don't pull in `evenframe_core`.

use crate::schemasync::table::TableConfig;
use crate::types::{NewtypeConfig, StructConfig, TaggedUnion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Full context for an output rule plugin call: what kind of type the
/// plugin is looking at (an object struct, a table-backed struct, a tagged
/// union or a newtype) and the complete config the host has for it.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind")]
pub enum OutputRulePluginInput<'a> {
    /// A standalone (non-table) Rust struct.
    Struct {
        /// Which pipeline is consuming the result: "Both", "Typesync",
        /// or "Schemasync".
        pipeline: String,
        /// Which generator is invoking the plugin ("macroforge",
        /// "arktype", etc.), or empty for the schemasync pass.
        generator: String,
        config: &'a StructConfig,
    },
    /// A Rust struct that backs a SurrealDB table.
    Table {
        pipeline: String,
        generator: String,
        struct_config: &'a StructConfig,
        table_config: &'a TableConfig,
    },
    /// A tagged-union Rust enum.
    Enum {
        pipeline: String,
        generator: String,
        config: &'a TaggedUnion,
    },
    /// A struct serde writes as another type: a single-field tuple struct, a
    /// transparent struct, a multi-field tuple struct or a unit struct.
    Newtype {
        pipeline: String,
        generator: String,
        config: &'a NewtypeConfig,
    },
}

/// Type-level override from a rule plugin.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TypeOverride {
    /// Macroforge derives for typesync.
    #[serde(default)]
    pub macroforge_derives: Vec<String>,
    /// Annotations for typesync.
    #[serde(default)]
    pub annotations: Vec<String>,
    /// Table permissions for schemasync.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<PermissionsOverride>,
    /// Event definitions for schemasync.
    #[serde(default)]
    pub events: Vec<EventOverride>,
}

/// Field-level override from a rule plugin.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FieldOverride {
    /// Annotations for typesync.
    #[serde(default)]
    pub annotations: Vec<String>,
}

/// Permissions for schemasync DEFINE TABLE.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PermissionsOverride {
    pub select: String,
    pub create: String,
    pub update: String,
    pub delete: String,
}

/// Event definition for schemasync.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EventOverride {
    pub name: String,
    pub statement: String,
}

/// Output from an output rule plugin.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OutputRulePluginOutput {
    /// Type-level overrides.
    #[serde(default)]
    pub type_override: TypeOverride,
    /// Per-field (or per-variant) overrides, keyed by field/variant name.
    #[serde(default)]
    pub field_overrides: BTreeMap<String, FieldOverride>,
    /// An error the plugin reports, which fails the build for this type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
