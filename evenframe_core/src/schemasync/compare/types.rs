//! Schema definition types for database-agnostic schema comparison
//!
//! These types represent database schemas in a provider-agnostic way,
//! allowing comparison between code-defined schemas and database schemas.

use crate::schemasync::config::AccessType;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fmt::{self, Display, Formatter},
};

/// Represents a complex object type definition
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ObjectType {
    /// Simple type like string, int, bool, etc.
    Simple(String),
    /// Object with nested fields
    Object(BTreeMap<String, ObjectType>),
    /// Array of a type
    Array(Box<ObjectType>),
    /// Union of multiple types (e.g., string | int)
    Union(Vec<ObjectType>),
    /// An optional value, `option<string>`, which SurrealDB lists as
    /// `none | string`
    Optional(Box<ObjectType>),
}

impl Display for ObjectType {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            ObjectType::Simple(s) => write!(f, "{}", s),
            ObjectType::Object(fields) => {
                let field_strs: Vec<String> = fields
                    .iter()
                    .map(|(name, field_type)| format!("{}: {}", name, field_type))
                    .collect();
                write!(f, "{{ {} }}", field_strs.join(", "))
            }
            ObjectType::Array(inner) => write!(f, "array<{}>", inner),
            ObjectType::Union(types) => {
                let type_strs: Vec<String> = types.iter().map(|t| t.to_string()).collect();
                write!(f, "({})", type_strs.join(" | "))
            }
            ObjectType::Optional(inner) => write!(f, "none | {}", inner),
        }
    }
}

/// Represents a field definition in a schema
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FieldDefinition {
    pub name: String,
    pub field_type: ObjectType,
    pub required: bool,
    pub default_value: Option<String>,
    pub assertions: Vec<String>,
    /// For array wildcard fields (e.g., phones[*]), this stores the parent field name
    pub parent_array_field: Option<String>,
    /// SurrealDB 3.0 COMPUTED expression
    pub computed_expression: Option<String>,
    /// COMMENT string for the field
    pub comment: Option<String>,
}

/// Represents a table definition in a schema
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TableDefinition {
    pub name: String,
    pub schema_type: SchemaType,
    pub fields: BTreeMap<String, FieldDefinition>,
    /// Array wildcard fields (e.g., phones[*]) are stored separately
    /// Key is the parent field name (e.g., "phones"), value is the wildcard field definition
    pub array_wildcard_fields: BTreeMap<String, FieldDefinition>,
    pub permissions: Option<PermissionSet>,
    pub indexes: Vec<IndexDefinition>,
    pub events: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SchemaType {
    Schemafull,
    Schemaless,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PermissionSet {
    pub select: String,
    pub create: String,
    pub update: String,
    pub delete: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IndexDefinition {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
    /// Everything after the column list (kind clause, COMMENT, CONCURRENTLY),
    /// e.g. `FULLTEXT ANALYZER en BM25(1.2,0.75) HIGHLIGHTS`. Compared to
    /// detect same-name indexes whose definition changed.
    #[serde(default)]
    pub definition: String,
}

/// A `DEFINE ANALYZER` statement, normalized (no OVERWRITE / IF NOT EXISTS,
/// no trailing `;`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AnalyzerDefinition {
    pub name: String,
    pub statement: String,
}

/// Represents an access definition in a schema
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccessDefinition {
    pub name: String,
    pub access_type: AccessType,
    pub database_level: bool, // true for DATABASE, false for NAMESPACE
    pub signup_query: Option<String>,
    pub signin_query: Option<String>,
    pub jwt_algorithm: Option<String>,
    pub jwt_key: Option<String>,
    pub jwt_url: Option<String>,
    pub issuer_key: Option<String>,
    pub authenticate: Option<String>,
    pub duration_for_token: Option<String>,
    pub duration_for_session: Option<String>,
    pub bearer_for: Option<String>, // "USER" or "RECORD"
}

/// Complete schema definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaDefinition {
    pub tables: BTreeMap<String, TableDefinition>,
    pub edges: BTreeMap<String, TableDefinition>,
    pub accesses: Vec<AccessDefinition>,
    #[serde(default)]
    pub analyzers: Vec<AnalyzerDefinition>,
}
