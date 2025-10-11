use serde::{Deserialize, Serialize};

/// Configuration for Typesync operations (TypeScript/Effect type generation)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TypesyncConfig {
    /// Whether to generate Arktype types
    pub should_generate_arktype_types: bool,
    /// Whether to generate Effect Schema types
    pub should_generate_effect_types: bool,
    /// Whether to generate SurrealDB schema types
    pub should_generate_surrealdb_schemas: bool,
    /// Output path for generated type files
    pub output_path: String,
}
