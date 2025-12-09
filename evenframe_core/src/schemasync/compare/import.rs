//! Schema import types - re-exports from types.rs for backward compatibility
//!
//! This module re-exports schema definition types from the types module.
//! The SurrealDB-specific SchemaImporter has been moved to surql.rs.

// Re-export all types from types.rs for backward compatibility
pub use super::types::{
    AccessDefinition, FieldDefinition, IndexDefinition, ObjectType, PermissionSet,
    SchemaDefinition, SchemaType, TableDefinition,
};

// Re-export SchemaImporter from surql module
pub use super::surql::SchemaImporter;
