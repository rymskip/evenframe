//! Serde types for output rule WASM plugin communication.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The kind of Rust type being processed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TypeKind {
    Struct,
    Enum,
}

/// Full type context sent to an output rule plugin.
#[derive(Debug, Clone, Serialize)]
pub struct OutputRulePluginInput {
    pub type_name: String,
    pub kind: TypeKind,
    pub rust_derives: Vec<String>,
    pub annotations: Vec<String>,
    pub pipeline: String,
    pub generator: String,
    pub fields: Vec<OutputRulePluginFieldInfo>,
    #[serde(default)]
    pub table_name: String,
    #[serde(default)]
    pub is_relation: bool,
    #[serde(default)]
    pub has_explicit_permissions: bool,
    #[serde(default)]
    pub has_explicit_events: bool,
    #[serde(default)]
    pub has_explicit_mock_data: bool,
    #[serde(default)]
    pub existing_macroforge_derives: Vec<String>,
}

/// Field metadata included in the plugin input.
#[derive(Debug, Clone, Serialize)]
pub struct OutputRulePluginFieldInfo {
    pub field_name: String,
    pub field_type: String,
    pub annotations: Vec<String>,
    pub validators: Vec<String>,
    #[serde(default)]
    pub is_optional: bool,
    #[serde(default)]
    pub record_link_target: Option<String>,
    #[serde(default)]
    pub vec_inner_type: Option<String>,
    #[serde(default)]
    pub has_explicit_format: bool,
    #[serde(default)]
    pub existing_format: Option<String>,
    #[serde(default)]
    pub has_explicit_define: bool,
}

/// Type-level override from a rule plugin.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TypeOverride {
    /// Macroforge derives for typesync (e.g. ["Default", "Serialize", "Deserialize", "Gigaform", "Overview"])
    #[serde(default)]
    pub macroforge_derives: Vec<String>,
    /// Annotations for typesync (e.g. ["@overview({ dataName: \"order\", ... })"])
    #[serde(default)]
    pub annotations: Vec<String>,
    /// Table permissions for schemasync.
    #[serde(default)]
    pub permissions: Option<PermissionsOverride>,
    /// Event definitions for schemasync.
    #[serde(default)]
    pub events: Vec<EventOverride>,
}

/// Field-level override from a rule plugin.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FieldOverride {
    /// Annotations for typesync (e.g. ["@textController({ label: \"Name\" })"])
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

/// Plugin response — type-level and field-level overrides.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OutputRulePluginOutput {
    #[serde(default)]
    pub type_override: TypeOverride,
    #[serde(default)]
    pub field_overrides: HashMap<String, FieldOverride>,
    #[serde(default)]
    pub error: Option<String>,
}
