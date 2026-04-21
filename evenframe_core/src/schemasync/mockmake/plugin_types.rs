//! Serde types for WASM plugin communication.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Input context sent to a field-level plugin.
#[derive(Debug, Serialize)]
pub struct PluginFieldInput {
    pub table_name: String,
    pub field_name: String,
    pub field_type: String,
    pub record_index: usize,
    pub total_records: usize,
    pub record_id: String,
}

/// Input context sent to a table-level plugin.
#[derive(Debug, Serialize)]
pub struct PluginTableInput {
    pub table_name: String,
    pub record_index: usize,
    pub total_records: usize,
    pub record_id: String,
    pub fields: Vec<PluginFieldInfo>,
}

/// Field metadata included in table-level plugin input.
#[derive(Debug, Serialize)]
pub struct PluginFieldInfo {
    pub field_name: String,
    pub field_type: String,
}

/// Output from a field-level plugin.
#[derive(Debug, Deserialize)]
pub struct PluginFieldOutput {
    /// The generated SurrealQL-compatible value.
    pub value: Option<String>,
    /// Error message if generation failed.
    pub error: Option<String>,
}

/// Output from a table-level plugin.
#[derive(Debug, Deserialize)]
pub struct PluginTableOutput {
    /// Map of field_name → SurrealQL-compatible value.
    pub fields: Option<BTreeMap<String, String>>,
    /// Error message if generation failed.
    pub error: Option<String>,
}
