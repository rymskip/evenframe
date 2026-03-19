use serde::{Deserialize, Serialize};

/// Whether to emit all types into a single file or split into per-type files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputMode {
    /// All types in one file (default).
    #[default]
    Single,
    /// Each primary type gets its own file; exclusive dependents are co-located.
    PerFile,
}

/// TypeScript array syntax style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArrayStyle {
    /// Shorthand syntax: `Type[]` — default
    #[default]
    Shorthand,
    /// Generic syntax: `Array<Type>`
    Generic,
}

/// Naming convention for generated per-file filenames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileNamingConvention {
    /// PascalCase (e.g. `UserProfile.ts`)
    Pascal,
    /// kebab-case (e.g. `user-profile.ts`) — default
    #[default]
    Kebab,
    /// snake_case (e.g. `user_profile.ts`)
    Snake,
    /// camelCase (e.g. `userProfile.ts`)
    Camel,
}

/// How to handle type name collisions across different source files.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CollisionStrategy {
    /// Stop with a diagnostic error naming both files (default).
    #[default]
    Error,
    /// Automatically prefix the colliding type with its source filename in PascalCase.
    /// e.g. `PaymentMethod` in `invoice.rs` → `InvoicePaymentMethod`.
    AutoRename,
}

/// Per-file output configuration (used under `[typesync.output]`).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct OutputConfig {
    /// Single-file or per-file output mode.
    #[serde(default)]
    pub mode: OutputMode,
    /// Whether to generate a barrel `index.ts` that re-exports everything.
    #[serde(default)]
    pub barrel_file: bool,
    /// Naming convention for generated filenames.
    #[serde(default)]
    pub file_naming: FileNamingConvention,
    /// File extension for generated files (default: `.ts`).
    /// Use `.svelte.ts` for SvelteKit projects, etc.
    #[serde(default = "default_file_extension")]
    pub file_extension: String,
    /// TypeScript array syntax style (default: shorthand `Type[]`).
    /// Set to `generic` for `Array<Type>` syntax.
    #[serde(default)]
    pub array_style: ArrayStyle,
}

fn default_file_extension() -> String {
    ".ts".to_string()
}

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
    /// Per-file output settings.
    #[serde(default)]
    pub output: OutputConfig,
    /// How to handle type name collisions across different source files.
    #[serde(default)]
    pub collision_strategy: CollisionStrategy,
}
