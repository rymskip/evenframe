//! PostgreSQL Database Provider Implementation

use async_trait::async_trait;
use sqlx::{PgPool, postgres::PgPoolOptions, Row};
use std::collections::HashMap;
use tracing::{info, trace};

use crate::error::{EvenframeError, Result};
use crate::schemasync::{EdgeConfig, TableConfig};
use crate::types::{FieldType, StructConfig, StructField, TaggedUnion};

use super::{
    PostgresTypeMapper, PostgresSchemaInspector, SchemaInspector,
    JoinTableConfig, generate_join_table_sql,
};
use crate::schemasync::database::{
    DatabaseConfig, DatabaseProvider, ProviderType, Relationship, RelationshipDirection,
    SchemaExport, TableInfo, TableSchema, ColumnSchema, DatabaseType, Transaction,
};
use crate::schemasync::database::type_mapper::TypeMapper;

/// PostgreSQL database provider implementation
pub struct PostgresProvider {
    pool: Option<PgPool>,
    config: Option<DatabaseConfig>,
    type_mapper: PostgresTypeMapper,
    schema: String,
}

impl PostgresProvider {
    /// Create a new PostgreSQL provider
    pub fn new() -> Self {
        Self {
            pool: None,
            config: None,
            type_mapper: PostgresTypeMapper,
            schema: "public".to_string(),
        }
    }

    /// Get a reference to the connection pool
    pub fn pool(&self) -> Option<&PgPool> {
        self.pool.as_ref()
    }

    /// Get the schema inspector
    fn inspector(&self) -> PostgresSchemaInspector {
        PostgresSchemaInspector::new(&self.schema)
    }
}

impl Default for PostgresProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DatabaseProvider for PostgresProvider {
    fn name(&self) -> &'static str {
        "postgres"
    }

    fn supports_graph_queries(&self) -> bool {
        false
    }

    fn supports_embedded_mode(&self) -> bool {
        false
    }

    async fn connect(&mut self, config: &DatabaseConfig) -> Result<()> {
        if config.provider != ProviderType::Postgres {
            return Err(EvenframeError::config(format!(
                "PostgreSQL provider cannot connect with provider type: {}",
                config.provider
            )));
        }

        info!("Connecting to PostgreSQL at {}", config.url);

        let pool = PgPoolOptions::new()
            .max_connections(config.max_connections.unwrap_or(10))
            .min_connections(config.min_connections.unwrap_or(1))
            .connect(&config.url)
            .await
            .map_err(|e| EvenframeError::database(format!(
                "Failed to connect to PostgreSQL: {e}"
            )))?;

        if let Some(schema) = &config.schema {
            self.schema = schema.clone();
        }

        self.pool = Some(pool);
        self.config = Some(config.clone());
        info!("Successfully connected to PostgreSQL");

        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        if let Some(pool) = self.pool.take() {
            pool.close().await;
        }
        self.config = None;
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.pool.is_some()
    }

    async fn export_schema(&self) -> Result<SchemaExport> {
        let pool = self.pool.as_ref()
            .ok_or_else(|| EvenframeError::database("Not connected to PostgreSQL"))?;

        let inspector = self.inspector();

        // Get all tables
        let tables_query = inspector.list_tables_query();
        let table_rows: Vec<serde_json::Value> = sqlx::query(&tables_query)
            .fetch_all(pool)
            .await
            .map_err(|e| EvenframeError::database(format!("Failed to list tables: {e}")))?
            .iter()
            .map(|row| {
                let table_name: String = row.get("table_name");
                serde_json::json!({"table_name": table_name})
            })
            .collect();

        let mut tables = Vec::new();
        for row in &table_rows {
            if let Some(table_name) = inspector.parse_table_row(row) {
                // Get columns for this table
                let columns_query = inspector.list_columns_query(&table_name);
                let column_rows: Vec<serde_json::Value> = sqlx::query(&columns_query)
                    .fetch_all(pool)
                    .await
                    .map_err(|e| EvenframeError::database(format!(
                        "Failed to list columns for {}: {e}", table_name
                    )))?
                    .iter()
                    .map(|row| {
                        serde_json::json!({
                            "column_name": row.try_get::<String, _>("column_name").ok(),
                            "data_type": row.try_get::<String, _>("data_type").ok(),
                            "is_nullable": row.try_get::<String, _>("is_nullable").ok(),
                            "column_default": row.try_get::<Option<String>, _>("column_default").ok().flatten(),
                        })
                    })
                    .collect();

                let columns: Vec<ColumnSchema> = column_rows
                    .iter()
                    .filter_map(|r| {
                        let info = inspector.parse_column_row(r)?;
                        Some(ColumnSchema {
                            name: info.name,
                            data_type: info.data_type.clone(),
                            database_type: DatabaseType::Custom(info.data_type),
                            nullable: info.nullable,
                            default: info.default,
                            constraints: vec![],
                        })
                    })
                    .collect();

                tables.push(TableSchema {
                    name: table_name,
                    columns,
                    primary_key: vec![],
                    is_relation: false,
                    unique_constraints: vec![],
                    check_constraints: vec![],
                });
            }
        }

        Ok(SchemaExport {
            tables,
            indexes: vec![],
            relationships: vec![],
            raw_statements: None,
        })
    }

    async fn apply_schema(&self, statements: &[String]) -> Result<()> {
        let pool = self.pool.as_ref()
            .ok_or_else(|| EvenframeError::database("Not connected to PostgreSQL"))?;

        for stmt in statements {
            trace!("Executing: {}", stmt);
            sqlx::query(stmt)
                .execute(pool)
                .await
                .map_err(|e| EvenframeError::database(format!(
                    "Failed to execute statement: {e}\nStatement: {stmt}"
                )))?;
        }

        Ok(())
    }

    async fn get_table_info(&self, table_name: &str) -> Result<Option<TableInfo>> {
        let pool = self.pool.as_ref()
            .ok_or_else(|| EvenframeError::database("Not connected to PostgreSQL"))?;

        let inspector = self.inspector();
        let columns_query = inspector.list_columns_query(table_name);

        let rows = sqlx::query(&columns_query)
            .fetch_all(pool)
            .await
            .map_err(|e| EvenframeError::database(format!(
                "Failed to get table info: {e}"
            )))?;

        if rows.is_empty() {
            return Ok(None);
        }

        let mut columns = HashMap::new();
        for row in rows {
            let name: String = row.get("column_name");
            columns.insert(name.clone(), crate::schemasync::database::types::ColumnInfo {
                name,
                data_type: row.get("data_type"),
                nullable: row.get::<String, _>("is_nullable") == "YES",
                default: row.try_get("column_default").ok(),
                is_primary_key: false,
                max_length: row.try_get::<i32, _>("character_maximum_length").ok().map(|v| v as u32),
                numeric_precision: row.try_get::<i32, _>("numeric_precision").ok().map(|v| v as u8),
                numeric_scale: row.try_get::<i32, _>("numeric_scale").ok().map(|v| v as u8),
            });
        }

        Ok(Some(TableInfo {
            name: table_name.to_string(),
            columns,
            primary_key: vec![],
            foreign_keys: vec![],
            indexes: vec![],
            row_count: None,
        }))
    }

    async fn list_tables(&self) -> Result<Vec<String>> {
        let pool = self.pool.as_ref()
            .ok_or_else(|| EvenframeError::database("Not connected to PostgreSQL"))?;

        let inspector = self.inspector();
        let query = inspector.list_tables_query();

        let rows = sqlx::query(&query)
            .fetch_all(pool)
            .await
            .map_err(|e| EvenframeError::database(format!(
                "Failed to list tables: {e}"
            )))?;

        Ok(rows.iter().map(|row| row.get("table_name")).collect())
    }

    async fn execute(&self, query: &str) -> Result<Vec<serde_json::Value>> {
        let pool = self.pool.as_ref()
            .ok_or_else(|| EvenframeError::database("Not connected to PostgreSQL"))?;

        let rows = sqlx::query(query)
            .fetch_all(pool)
            .await
            .map_err(|e| EvenframeError::database(format!(
                "Failed to execute query: {e}"
            )))?;

        // Convert rows to JSON - this is a simplified implementation
        let results: Vec<serde_json::Value> = rows
            .iter()
            .map(|_row| serde_json::json!({})) // Would need proper column extraction
            .collect();

        Ok(results)
    }

    async fn execute_batch(&self, queries: &[String]) -> Result<Vec<Vec<serde_json::Value>>> {
        let mut results = Vec::with_capacity(queries.len());
        for query in queries {
            let result = self.execute(query).await?;
            results.push(result);
        }
        Ok(results)
    }

    async fn insert(
        &self,
        table: &str,
        records: &[serde_json::Value],
    ) -> Result<Vec<String>> {
        let pool = self.pool.as_ref()
            .ok_or_else(|| EvenframeError::database("Not connected to PostgreSQL"))?;

        let mut ids = Vec::with_capacity(records.len());

        for record in records {
            if let Some(obj) = record.as_object() {
                let columns: Vec<&String> = obj.keys().collect();
                let values: Vec<String> = obj.values()
                    .map(format_pg_value)
                    .collect();

                let query = format!(
                    "INSERT INTO \"{}\" ({}) VALUES ({}) RETURNING id",
                    table,
                    columns.iter().map(|c| format!("\"{}\"", c)).collect::<Vec<_>>().join(", "),
                    values.join(", ")
                );

                let row = sqlx::query(&query)
                    .fetch_one(pool)
                    .await
                    .map_err(|e| EvenframeError::database(format!(
                        "Failed to insert: {e}"
                    )))?;

                // Try to get ID - first try string, then integer
                if let Ok(id) = row.try_get::<String, _>("id") {
                    ids.push(id);
                } else if let Ok(id) = row.try_get::<i64, _>("id") {
                    ids.push(id.to_string());
                }
            }
        }

        Ok(ids)
    }

    async fn upsert(
        &self,
        table: &str,
        records: &[serde_json::Value],
    ) -> Result<Vec<String>> {
        let pool = self.pool.as_ref()
            .ok_or_else(|| EvenframeError::database("Not connected to PostgreSQL"))?;

        let mut ids = Vec::with_capacity(records.len());

        for record in records {
            if let Some(obj) = record.as_object() {
                let columns: Vec<&String> = obj.keys().collect();
                let values: Vec<String> = obj.values()
                    .map(format_pg_value)
                    .collect();

                let update_clause: String = columns
                    .iter()
                    .filter(|c| **c != "id")
                    .map(|c| format!("\"{}\" = EXCLUDED.\"{}\"", c, c))
                    .collect::<Vec<_>>()
                    .join(", ");

                let query = format!(
                    "INSERT INTO \"{}\" ({}) VALUES ({}) ON CONFLICT (id) DO UPDATE SET {} RETURNING id",
                    table,
                    columns.iter().map(|c| format!("\"{}\"", c)).collect::<Vec<_>>().join(", "),
                    values.join(", "),
                    update_clause
                );

                let row = sqlx::query(&query)
                    .fetch_one(pool)
                    .await
                    .map_err(|e| EvenframeError::database(format!(
                        "Failed to upsert: {e}"
                    )))?;

                if let Ok(id) = row.try_get::<String, _>("id") {
                    ids.push(id);
                } else if let Ok(id) = row.try_get::<i64, _>("id") {
                    ids.push(id.to_string());
                }
            }
        }

        Ok(ids)
    }

    async fn select(
        &self,
        table: &str,
        filter: Option<&str>,
    ) -> Result<Vec<serde_json::Value>> {
        let query = if let Some(f) = filter {
            format!("SELECT * FROM \"{}\" WHERE {}", table, f)
        } else {
            format!("SELECT * FROM \"{}\"", table)
        };

        self.execute(&query).await
    }

    async fn count(&self, table: &str, filter: Option<&str>) -> Result<u64> {
        let pool = self.pool.as_ref()
            .ok_or_else(|| EvenframeError::database("Not connected to PostgreSQL"))?;

        let query = if let Some(f) = filter {
            format!("SELECT COUNT(*) as count FROM \"{}\" WHERE {}", table, f)
        } else {
            format!("SELECT COUNT(*) as count FROM \"{}\"", table)
        };

        let row = sqlx::query(&query)
            .fetch_one(pool)
            .await
            .map_err(|e| EvenframeError::database(format!(
                "Failed to count: {e}"
            )))?;

        let count: i64 = row.get("count");
        Ok(count as u64)
    }

    async fn delete(&self, table: &str, ids: &[String]) -> Result<()> {
        let pool = self.pool.as_ref()
            .ok_or_else(|| EvenframeError::database("Not connected to PostgreSQL"))?;

        for id in ids {
            let query = format!("DELETE FROM \"{}\" WHERE id = '{}'", table, id);
            sqlx::query(&query)
                .execute(pool)
                .await
                .map_err(|e| EvenframeError::database(format!(
                    "Failed to delete: {e}"
                )))?;
        }

        Ok(())
    }

    fn generate_create_table(
        &self,
        table_name: &str,
        config: &TableConfig,
        _all_tables: &HashMap<String, TableConfig>,
        _objects: &HashMap<String, StructConfig>,
        _enums: &HashMap<String, TaggedUnion>,
    ) -> String {
        let mut columns = Vec::new();

        // Add ID column
        columns.push("    \"id\" UUID PRIMARY KEY DEFAULT gen_random_uuid()".to_string());

        // Add fields from struct config
        for field in &config.struct_config.fields {
            if field.field_name == "id" {
                continue; // Skip id, already added
            }

            let sql_type = self.type_mapper.field_type_to_native(&field.field_type);
            if sql_type.is_empty() {
                continue; // Skip unit types
            }

            let nullable = matches!(field.field_type, FieldType::Option(_));
            let null_str = if nullable { "" } else { " NOT NULL" };

            columns.push(format!(
                "    \"{}\" {}{}",
                field.field_name,
                sql_type,
                null_str
            ));
        }

        format!(
            "CREATE TABLE IF NOT EXISTS \"{}\" (\n{}\n);",
            table_name,
            columns.join(",\n")
        )
    }

    fn generate_create_field(
        &self,
        table_name: &str,
        field: &StructField,
        _objects: &HashMap<String, StructConfig>,
        _enums: &HashMap<String, TaggedUnion>,
    ) -> String {
        let sql_type = self.type_mapper.field_type_to_native(&field.field_type);
        let nullable = matches!(field.field_type, FieldType::Option(_));
        let null_str = if nullable { "" } else { " NOT NULL" };

        format!(
            "ALTER TABLE \"{}\" ADD COLUMN \"{}\" {}{};",
            table_name,
            field.field_name,
            sql_type,
            null_str
        )
    }

    fn map_field_type(&self, field_type: &FieldType) -> String {
        self.type_mapper.field_type_to_native(field_type)
    }

    fn format_value(&self, field_type: &FieldType, value: &serde_json::Value) -> String {
        self.type_mapper.format_value(field_type, value)
    }

    fn generate_relationship_table(&self, edge: &EdgeConfig) -> Vec<String> {
        let config = JoinTableConfig::postgres();
        generate_join_table_sql(edge, &config)
    }

    async fn create_relationship(
        &self,
        edge_table: &str,
        from_id: &str,
        to_id: &str,
        data: Option<&serde_json::Value>,
    ) -> Result<String> {
        let pool = self.pool.as_ref()
            .ok_or_else(|| EvenframeError::database("Not connected to PostgreSQL"))?;

        let mut columns = vec!["from_id", "to_id"];
        let mut values = vec![
            format!("'{}'", from_id),
            format!("'{}'", to_id),
        ];

        if let Some(data) = data && let Some(obj) = data.as_object() {
            for (k, v) in obj {
                if k != "id" && k != "from_id" && k != "to_id" {
                    columns.push(k);
                    values.push(format_pg_value(v));
                }
            }
        }

        let query = format!(
            "INSERT INTO \"{}\" ({}) VALUES ({}) RETURNING id",
            edge_table,
            columns.iter().map(|c| format!("\"{}\"", c)).collect::<Vec<_>>().join(", "),
            values.join(", ")
        );

        let row = sqlx::query(&query)
            .fetch_one(pool)
            .await
            .map_err(|e| EvenframeError::database(format!(
                "Failed to create relationship: {e}"
            )))?;

        if let Ok(id) = row.try_get::<String, _>("id") {
            Ok(id)
        } else {
            Ok(String::new())
        }
    }

    async fn delete_relationship(
        &self,
        edge_table: &str,
        from_id: &str,
        to_id: &str,
    ) -> Result<()> {
        let pool = self.pool.as_ref()
            .ok_or_else(|| EvenframeError::database("Not connected to PostgreSQL"))?;

        let query = format!(
            "DELETE FROM \"{}\" WHERE from_id = '{}' AND to_id = '{}'",
            edge_table, from_id, to_id
        );

        sqlx::query(&query)
            .execute(pool)
            .await
            .map_err(|e| EvenframeError::database(format!(
                "Failed to delete relationship: {e}"
            )))?;

        Ok(())
    }

    async fn get_relationships(
        &self,
        edge_table: &str,
        record_id: &str,
        direction: RelationshipDirection,
    ) -> Result<Vec<Relationship>> {
        let query = match direction {
            RelationshipDirection::Outgoing => {
                format!("SELECT * FROM \"{}\" WHERE from_id = '{}'", edge_table, record_id)
            }
            RelationshipDirection::Incoming => {
                format!("SELECT * FROM \"{}\" WHERE to_id = '{}'", edge_table, record_id)
            }
            RelationshipDirection::Both => {
                format!(
                    "SELECT * FROM \"{}\" WHERE from_id = '{}' OR to_id = '{}'",
                    edge_table, record_id, record_id
                )
            }
        };

        let pool = self.pool.as_ref()
            .ok_or_else(|| EvenframeError::database("Not connected to PostgreSQL"))?;

        let rows = sqlx::query(&query)
            .fetch_all(pool)
            .await
            .map_err(|e| EvenframeError::database(format!(
                "Failed to get relationships: {e}"
            )))?;

        let relationships = rows
            .iter()
            .filter_map(|row| {
                let id: String = row.try_get::<String, _>("id").ok()?;
                let from_id: String = row.try_get::<String, _>("from_id").ok()?;
                let to_id: String = row.try_get::<String, _>("to_id").ok()?;

                Some(Relationship {
                    id,
                    from_id,
                    to_id,
                    data: None,
                })
            })
            .collect();

        Ok(relationships)
    }

    async fn begin_transaction(&self) -> Result<Box<dyn Transaction>> {
        Err(EvenframeError::database(
            "PostgreSQL transactions not yet implemented in provider abstraction"
        ))
    }

    async fn create_embedded_instance(&self) -> Result<Option<Box<dyn DatabaseProvider>>> {
        // PostgreSQL doesn't support embedded mode
        Ok(None)
    }
}

/// Format a JSON value for PostgreSQL
fn format_pg_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "NULL".to_string(),
        serde_json::Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => format!("'{}'", s.replace('\'', "''")),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            format!("'{}'::JSONB", value.to_string().replace('\'', "''"))
        }
    }
}
