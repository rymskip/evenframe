//! SQL-specific schema comparison implementation
//!
//! This module contains the SQL schema comparison logic using information_schema
//! queries to compare database schemas.

use async_trait::async_trait;
use std::collections::{HashMap, HashSet};

use crate::error::Result;
use crate::schemasync::compare::{
    ChangeType, FieldChange, SchemaChanges, TableChanges,
};
use crate::schemasync::database::types::*;
use crate::schemasync::database::DatabaseProvider;
use crate::schemasync::TableConfig;
use crate::types::{StructConfig, TaggedUnion};

use super::SchemaComparator;

/// SQL-based schema comparator that uses information_schema queries
pub struct SqlSchemaComparator<'a> {
    provider: &'a dyn DatabaseProvider,
}

impl<'a> SqlSchemaComparator<'a> {
    pub fn new(provider: &'a dyn DatabaseProvider) -> Self {
        Self { provider }
    }

    /// Generate expected schema from Rust table configs
    fn generate_expected_schema(
        &self,
        tables: &HashMap<String, TableConfig>,
        _objects: &HashMap<String, StructConfig>,
        _enums: &HashMap<String, TaggedUnion>,
    ) -> Vec<TableSchema> {
        let mut result = Vec::new();

        for (table_name, config) in tables {
            let mut columns = Vec::new();

            // Add ID column
            columns.push(ColumnSchema {
                name: "id".to_string(),
                data_type: "UUID".to_string(), // Will be mapped by provider
                database_type: DatabaseType::String { max_length: Some(36) },
                nullable: false,
                default: None,
                constraints: vec![ColumnConstraint::PrimaryKey],
            });

            // Add fields from struct config
            for field in &config.struct_config.fields {
                if field.field_name == "id" {
                    continue;
                }

                let data_type = self.provider.map_field_type(&field.field_type);
                if data_type.is_empty() {
                    continue; // Skip unit types
                }

                let nullable = matches!(field.field_type, crate::types::FieldType::Option(_));

                columns.push(ColumnSchema {
                    name: field.field_name.clone(),
                    data_type,
                    database_type: DatabaseType::Custom(
                        self.provider.map_field_type(&field.field_type),
                    ),
                    nullable,
                    default: None,
                    constraints: if nullable {
                        vec![]
                    } else {
                        vec![ColumnConstraint::NotNull]
                    },
                });
            }

            result.push(TableSchema {
                name: table_name.clone(),
                columns,
                primary_key: vec!["id".to_string()],
                is_relation: config.relation.is_some(),
                unique_constraints: vec![],
                check_constraints: vec![],
            });
        }

        result
    }

    /// Compare two table schemas and return the differences
    fn compare_table_schemas(
        &self,
        table_name: &str,
        current: Option<&TableSchema>,
        expected: &TableSchema,
    ) -> Option<TableChanges> {
        let current = match current {
            Some(c) => c,
            None => {
                // Table doesn't exist - this is a new table, not a modification
                return None;
            }
        };

        let mut changes = TableChanges {
            table_name: table_name.to_string(),
            new_fields: Vec::new(),
            removed_fields: Vec::new(),
            modified_fields: Vec::new(),
            permission_changed: false,
            schema_type_changed: false,
            new_events: Vec::new(),
            removed_events: Vec::new(),
        };

        // Get column names
        let current_columns: HashSet<String> =
            current.columns.iter().map(|c| c.name.clone()).collect();
        let expected_columns: HashSet<String> =
            expected.columns.iter().map(|c| c.name.clone()).collect();

        // Find new columns
        for col_name in expected_columns.difference(&current_columns) {
            changes.new_fields.push(col_name.clone());
        }

        // Find removed columns
        for col_name in current_columns.difference(&expected_columns) {
            changes.removed_fields.push(col_name.clone());
        }

        // Find modified columns
        for col_name in current_columns.intersection(&expected_columns) {
            let current_col = current.columns.iter().find(|c| &c.name == col_name);
            let expected_col = expected.columns.iter().find(|c| &c.name == col_name);

            if let (Some(curr), Some(exp)) = (current_col, expected_col) {
                // Compare data types (normalized for comparison)
                let curr_type = normalize_type(&curr.data_type);
                let exp_type = normalize_type(&exp.data_type);

                if curr_type != exp_type || curr.nullable != exp.nullable {
                    changes.modified_fields.push(FieldChange {
                        field_name: col_name.clone(),
                        old_type: curr.data_type.clone(),
                        new_type: exp.data_type.clone(),
                        change_type: ChangeType::Modified,
                        required_changed: curr.nullable != exp.nullable,
                        default_changed: curr.default != exp.default,
                    });
                }
            }
        }

        // Return None if no changes
        if changes.new_fields.is_empty()
            && changes.removed_fields.is_empty()
            && changes.modified_fields.is_empty()
        {
            None
        } else {
            Some(changes)
        }
    }
}

/// Normalize type names for comparison (handles case differences, aliases, etc.)
fn normalize_type(type_name: &str) -> String {
    type_name
        .to_lowercase()
        .replace("integer", "int")
        .replace("bigint", "int8")
        .replace("smallint", "int2")
        .replace("boolean", "bool")
        .replace("character varying", "varchar")
        .replace("double precision", "float8")
        .replace("timestamp with time zone", "timestamptz")
        .replace("timestamp without time zone", "timestamp")
}

#[async_trait]
impl<'a> SchemaComparator for SqlSchemaComparator<'a> {
    async fn compare_schemas(
        &self,
        tables: &HashMap<String, TableConfig>,
        objects: &HashMap<String, StructConfig>,
        enums: &HashMap<String, TaggedUnion>,
    ) -> Result<SchemaChanges> {
        // Get current schema from database
        let current_schema = self.get_current_schema().await?;

        // Generate expected schema from Rust configs
        let expected_tables = self.generate_expected_schema(tables, objects, enums);

        // Build lookup maps
        let current_table_map: HashMap<String, &TableSchema> = current_schema
            .tables
            .iter()
            .map(|t| (t.name.clone(), t))
            .collect();

        let expected_table_names: HashSet<String> =
            expected_tables.iter().map(|t| t.name.clone()).collect();
        let current_table_names: HashSet<String> = current_table_map.keys().cloned().collect();

        let mut changes = SchemaChanges {
            new_tables: Vec::new(),
            removed_tables: Vec::new(),
            modified_tables: Vec::new(),
            new_accesses: Vec::new(),
            removed_accesses: Vec::new(),
            modified_accesses: Vec::new(),
        };

        // Find new tables
        for table_name in expected_table_names.difference(&current_table_names) {
            changes.new_tables.push(table_name.clone());
        }

        // Find removed tables
        for table_name in current_table_names.difference(&expected_table_names) {
            changes.removed_tables.push(table_name.clone());
        }

        // Find modified tables
        for expected_table in &expected_tables {
            if let Some(current_table) = current_table_map.get(&expected_table.name)
                && let Some(table_changes) = self.compare_table_schemas(
                    &expected_table.name,
                    Some(current_table),
                    expected_table,
                )
            {
                changes.modified_tables.push(table_changes);
            }
        }

        Ok(changes)
    }

    async fn get_current_schema(&self) -> Result<SchemaExport> {
        self.provider.export_schema().await
    }

    fn supports_embedded_comparison(&self) -> bool {
        false
    }
}
