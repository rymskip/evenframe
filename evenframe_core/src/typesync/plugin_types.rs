//! Serde types for output rule WASM plugin communication.
//!
//! # Why full configs, not summaries
//!
//! Output rule plugins receive the full serialized `StructConfig` /
//! `TaggedUnion` / `TableConfig` that describes the type they're
//! processing. An earlier iteration passed a hand-trimmed "input" struct
//! with ~20 flat fields (annotations, validators, a handful of
//! `has_explicit_*` booleans, etc.) but that shape turned out to be
//! actively hostile:
//!
//! - Several of the flat booleans (`has_explicit_permissions`,
//!   `has_explicit_events`, `has_explicit_mock_data`) were hardcoded
//!   `false`, masking the real underlying `Option` / `Vec` state.
//! - Plugins that needed to introspect full-fidelity data (event
//!   statements already on the table, the table's relation `EdgeConfig`,
//!   `define_config` per field, pipeline membership, per-variant
//!   representation details) couldn't reach it.
//! - The field list was flattened into a lossy `OutputRulePluginFieldInfo`
//!   that dropped `define_config`, `edge_config`, `mock_plugin`, and
//!   other per-field metadata.
//!
//! Synthetic plugins (see [`super::synthetic_plugin_types`]) solved the
//! same problem by round-tripping the real serde tree. Output rule
//! plugins now use the same approach: plugins get exactly the
//! `StructConfig` + `TableConfig` (or `TaggedUnion`) the host is holding
//! and can match on a tagged union to dispatch on kind.
//!
//! The plugin crate (`evenframe_plugin`) receives these as
//! `serde_json::Value` maps so plugin authors don't have to pull
//! `evenframe_core` (and all its deps) into their cdylibs.

use crate::schemasync::table::TableConfig;
use crate::types::{StructConfig, TaggedUnion};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Full context for an output rule plugin call. The variant tells the
/// plugin exactly what it's looking at — a free-standing object struct,
/// a table-backed struct, or a tagged-union enum — and carries the
/// complete config(s) the host has for that type.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind")]
pub enum OutputRulePluginInput {
    /// A standalone (non-table) Rust struct.
    Struct {
        /// Which pipeline is consuming the result: "Both", "Typesync",
        /// or "Schemasync".
        pipeline: String,
        /// Which generator is invoking the plugin ("macroforge",
        /// "arktype", etc.), or empty for the schemasync pass.
        generator: String,
        config: StructConfig,
    },
    /// A Rust struct that backs a SurrealDB table.
    Table {
        pipeline: String,
        generator: String,
        struct_config: StructConfig,
        table_config: TableConfig,
    },
    /// A tagged-union Rust enum.
    Enum {
        pipeline: String,
        generator: String,
        config: TaggedUnion,
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
    pub field_overrides: HashMap<String, FieldOverride>,
    /// Error message — if set, this plugin's output is skipped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
