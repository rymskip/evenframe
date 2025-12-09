use serde::{Deserialize, Serialize};

/// Configuration for Typesync operations (TypeScript/Effect type generation)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TypesyncConfig {
    /// Whether to generate Arktype types
    pub should_generate_arktype_types: bool,
    /// Whether to generate Effect Schema types
    pub should_generate_effect_types: bool,
    /// Whether to generate Macroforge TypeScript interfaces with JSDoc annotations
    #[serde(default)]
    pub should_generate_macroforge_types: bool,
    /// Whether to generate FlatBuffers schema files (.fbs)
    #[serde(default)]
    pub should_generate_flatbuffers_types: bool,
    /// Optional namespace for FlatBuffers schema (e.g., "com.example.app")
    #[serde(default)]
    pub flatbuffers_namespace: Option<String>,
    /// Whether to generate Protocol Buffers schema files (.proto)
    #[serde(default)]
    pub should_generate_protobuf_types: bool,
    /// Optional package name for Protocol Buffers schema (e.g., "com.example.app")
    #[serde(default)]
    pub protobuf_package: Option<String>,
    /// Whether to import validate.proto for validation rules in Protocol Buffers
    #[serde(default)]
    pub protobuf_import_validate: bool,
    /// Whether to generate SurrealDB schema types
    pub should_generate_surrealdb_schemas: bool,
    /// Output path for generated type files
    pub output_path: String,
}
