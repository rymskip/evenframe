//! Serde types for WASM plugin communication.

use serde::{Deserialize, Serialize};

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

/// Output from a field-level plugin.
#[derive(Debug, Deserialize)]
pub struct PluginFieldOutput {
    /// The generated SurrealQL-compatible value.
    pub value: Option<String>,
    /// Error message if generation failed.
    pub error: Option<String>,
}
