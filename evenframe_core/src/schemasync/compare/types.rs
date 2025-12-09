//! Schema definition types for database-agnostic schema comparison
//!
//! These types represent database schemas in a provider-agnostic way,
//! allowing comparison between code-defined schemas and database schemas.

use crate::{Result, schemasync::TableConfig, schemasync::config::AccessType};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt::{self, Display, Formatter},
};
use tracing;

/// Represents a complex object type definition
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ObjectType {
    /// Simple type like string, int, bool, etc.
    Simple(String),
    /// Object with nested fields
    Object(HashMap<String, ObjectType>),
    /// Array of a type
    Array(Box<ObjectType>),
    /// Union of multiple types (e.g., string | int)
    Union(Vec<ObjectType>),
    /// Nullable type (e.g., null | string)
    Nullable(Box<ObjectType>),
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
            ObjectType::Nullable(inner) => write!(f, "null | {}", inner),
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
}

/// Represents a table definition in a schema
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TableDefinition {
    pub name: String,
    pub schema_type: SchemaType,
    pub fields: HashMap<String, FieldDefinition>,
    /// Array wildcard fields (e.g., phones[*]) are stored separately
    /// Key is the parent field name (e.g., "phones"), value is the wildcard field definition
    pub array_wildcard_fields: HashMap<String, FieldDefinition>,
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
    pub tables: HashMap<String, TableDefinition>,
    pub edges: HashMap<String, TableDefinition>,
    pub accesses: Vec<AccessDefinition>,
}

impl SchemaDefinition {
    /// Create from TableConfig HashMap (for code-based schema generation)
    pub fn from_table_configs(tables: &HashMap<String, TableConfig>) -> Result<Self> {
        tracing::debug!(
            table_count = tables.len(),
            "Creating SchemaDefinition from TableConfigs"
        );
        let mut schema_tables = HashMap::new();
        let mut schema_edges = HashMap::new();

        for (name, config) in tables {
            let table_def = TableDefinition {
                name: name.clone(),
                schema_type: SchemaType::Schemafull,
                fields: Self::extract_fields_from_config(config)?,
                array_wildcard_fields: HashMap::new(),
                permissions: Self::extract_permissions_from_config(config),
                indexes: Vec::new(),
                events: config
                    .events
                    .iter()
                    .map(|event| event.statement.clone())
                    .collect(),
            };

            if config.relation.is_some() {
                schema_edges.insert(name.clone(), table_def);
            } else {
                schema_tables.insert(name.clone(), table_def);
            }
        }

        let definition = Self {
            tables: schema_tables.clone(),
            edges: schema_edges.clone(),
            accesses: Vec::new(),
        };

        tracing::debug!(
            tables = definition.tables.len(),
            edges = definition.edges.len(),
            "SchemaDefinition created from configs"
        );

        Ok(definition)
    }

    fn extract_fields_from_config(
        config: &TableConfig,
    ) -> Result<HashMap<String, FieldDefinition>> {
        let mut fields = HashMap::new();

        for field in &config.struct_config.fields {
            // Check if field has a default value
            let default_value = field
                .define_config
                .as_ref()
                .and_then(|dc| dc.default.clone().or(dc.default_always.clone()));

            // Field is required if it doesn't have a default value and isn't skipped
            let is_required = default_value.is_none()
                && !field
                    .define_config
                    .as_ref()
                    .map(|dc| dc.should_skip)
                    .unwrap_or(false);

            let field_def = FieldDefinition {
                name: field.field_name.clone(),
                field_type: ObjectType::Simple(field.field_type.to_string()),
                required: is_required,
                default_value,
                assertions: field
                    .define_config
                    .as_ref()
                    .and_then(|dc| dc.assert.clone())
                    .map(|a| vec![a])
                    .unwrap_or_default(),
                parent_array_field: None,
            };
            fields.insert(field.field_name.clone(), field_def);
        }

        Ok(fields)
    }

    fn extract_permissions_from_config(config: &TableConfig) -> Option<PermissionSet> {
        tracing::trace!("Extracting permissions from table config");
        config.permissions.as_ref().map(|perms| PermissionSet {
            select: perms
                .all_permissions
                .clone()
                .or(perms.select_permissions.clone())
                .unwrap_or_else(|| "FULL".to_string()),
            create: perms
                .all_permissions
                .clone()
                .or(perms.create_permissions.clone())
                .unwrap_or_else(|| "FULL".to_string()),
            update: perms
                .all_permissions
                .clone()
                .or(perms.update_permissions.clone())
                .unwrap_or_else(|| "FULL".to_string()),
            delete: perms
                .all_permissions
                .clone()
                .or(perms.delete_permissions.clone())
                .unwrap_or_else(|| "FULL".to_string()),
        })
    }
}
