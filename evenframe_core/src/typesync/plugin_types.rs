//! Serde types for type-transform WASM plugin communication.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The kind of Rust type being processed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TypeKind {
    Struct,
    Enum,
}

/// Full type context sent to a type-transform plugin.
///
/// One call per struct/enum per generator. The plugin inspects the context
/// and returns modifications (type overrides, skipped fields, extra imports, etc.).
#[derive(Debug, Clone, Serialize)]
pub struct TypePluginInput {
    /// The Rust type name (e.g., "User", "OrderStatus").
    pub type_name: String,
    /// Whether this is a struct or enum.
    pub kind: TypeKind,
    /// All Rust derives on the type (e.g., ["Serialize", "Clone", "Debug"]).
    pub rust_derives: Vec<String>,
    /// Custom annotations from `#[annotation("...")]`.
    pub annotations: Vec<String>,
    /// Which pipeline this type participates in ("Both", "Typesync", "Schemasync").
    pub pipeline: String,
    /// The generator being invoked ("macroforge", "arktype", "effect", "surrealdb", etc.).
    pub generator: String,
    /// All fields on the type with their metadata.
    pub fields: Vec<TypePluginFieldInfo>,
}

/// Field metadata included in the plugin input.
#[derive(Debug, Clone, Serialize)]
pub struct TypePluginFieldInfo {
    /// The Rust field name (e.g., "email", "created_at").
    pub field_name: String,
    /// Canonical type name (e.g., "Decimal", "Option<DateTime>", "Vec<String>").
    pub field_type: String,
    /// Field-level annotations.
    pub annotations: Vec<String>,
    /// Field-level validators as strings.
    pub validators: Vec<String>,
}

/// Plugin response — modifications to apply to the generated output.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TypePluginOutput {
    /// Per-field type overrides: field_name -> replacement type string for this generator.
    #[serde(default)]
    pub field_type_overrides: HashMap<String, String>,
    /// Fields to skip entirely in generation.
    #[serde(default)]
    pub skip_fields: Vec<String>,
    /// Extra import lines to add to the generated file.
    #[serde(default)]
    pub extra_imports: Vec<String>,
    /// Extra annotations/decorators per field: field_name -> Vec of annotation strings.
    #[serde(default)]
    pub field_annotations: HashMap<String, Vec<String>>,
    /// If set, override the generated type name.
    #[serde(default)]
    pub type_name_override: Option<String>,
    /// Error message — if set, this plugin's output is skipped with a warning.
    #[serde(default)]
    pub error: Option<String>,
}

/// Accumulated overrides from all type plugins for a single generator.
#[derive(Debug, Clone, Default)]
pub struct TypeOverrides {
    /// (type_name, field_name) -> override type string.
    pub field_types: HashMap<(String, String), String>,
    /// type_name -> fields to skip.
    pub skip_fields: HashMap<String, Vec<String>>,
    /// Extra import lines to add.
    pub extra_imports: Vec<String>,
    /// (type_name, field_name) -> extra annotations.
    pub field_annotations: HashMap<(String, String), Vec<String>>,
    /// type_name -> overridden output name.
    pub type_name_overrides: HashMap<String, String>,
}
