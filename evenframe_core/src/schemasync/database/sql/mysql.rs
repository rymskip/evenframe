//! MySQL Database Provider Implementation
//!
//! Stub implementation - to be fully implemented

use async_trait::async_trait;
use sqlx::{MySqlPool, mysql::MySqlPoolOptions, Row};
use std::collections::HashMap;
use tracing::info;

use crate::error::{EvenframeError, Result};
use crate::schemasync::{EdgeConfig, TableConfig};
use crate::types::{FieldType, StructConfig, StructField, TaggedUnion};

use super::{
    MysqlTypeMapper, MysqlSchemaInspector, SchemaInspector,
    JoinTableConfig, generate_join_table_sql,
};
use crate::schemasync::database::{
    DatabaseConfig, DatabaseProvider, ProviderType, Relationship, RelationshipDirection,
    SchemaExport, TableInfo, Transaction,
};
use crate::schemasync::database::type_mapper::TypeMapper;

/// MySQL database provider implementation
pub struct MysqlProvider {
    pool: Option<MySqlPool>,
    config: Option<DatabaseConfig>,
    type_mapper: MysqlTypeMapper,
    database: String,
}

impl MysqlProvider {
    pub fn new() -> Self {
        Self {
            pool: None,
            config: None,
            type_mapper: MysqlTypeMapper,
            database: String::new(),
        }
    }
}

impl Default for MysqlProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DatabaseProvider for MysqlProvider {
    fn name(&self) -> &'static str { "mysql" }
    fn supports_graph_queries(&self) -> bool { false }
    fn supports_embedded_mode(&self) -> bool { false }

    async fn connect(&mut self, config: &DatabaseConfig) -> Result<()> {
        if config.provider != ProviderType::MySql {
            return Err(EvenframeError::config("Wrong provider type"));
        }

        info!("Connecting to MySQL at {}", config.url);

        let pool = MySqlPoolOptions::new()
            .max_connections(config.max_connections.unwrap_or(10))
            .connect(&config.url)
            .await
            .map_err(|e| EvenframeError::database(format!("MySQL connection failed: {e}")))?;

        self.pool = Some(pool);
        self.config = Some(config.clone());
        if let Some(db) = &config.database {
            self.database = db.clone();
        }

        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        if let Some(pool) = self.pool.take() {
            pool.close().await;
        }
        Ok(())
    }

    fn is_connected(&self) -> bool { self.pool.is_some() }

    async fn export_schema(&self) -> Result<SchemaExport> {
        Ok(SchemaExport::default())
    }

    async fn apply_schema(&self, statements: &[String]) -> Result<()> {
        let pool = self.pool.as_ref()
            .ok_or_else(|| EvenframeError::database("Not connected"))?;

        for stmt in statements {
            sqlx::query(stmt).execute(pool).await
                .map_err(|e| EvenframeError::database(format!("Execute failed: {e}")))?;
        }
        Ok(())
    }

    async fn get_table_info(&self, _table_name: &str) -> Result<Option<TableInfo>> {
        Ok(None)
    }

    async fn list_tables(&self) -> Result<Vec<String>> {
        let pool = self.pool.as_ref()
            .ok_or_else(|| EvenframeError::database("Not connected"))?;

        let inspector = MysqlSchemaInspector::new(&self.database);
        let rows = sqlx::query(&inspector.list_tables_query())
            .fetch_all(pool)
            .await
            .map_err(|e| EvenframeError::database(format!("List tables failed: {e}")))?;

        Ok(rows.iter().filter_map(|r| r.try_get::<String, _>("table_name").ok()).collect())
    }

    async fn execute(&self, query: &str) -> Result<Vec<serde_json::Value>> {
        let pool = self.pool.as_ref()
            .ok_or_else(|| EvenframeError::database("Not connected"))?;

        sqlx::query(query).fetch_all(pool).await
            .map_err(|e| EvenframeError::database(format!("Execute failed: {e}")))?;

        Ok(vec![])
    }

    async fn execute_batch(&self, queries: &[String]) -> Result<Vec<Vec<serde_json::Value>>> {
        let mut results = Vec::new();
        for q in queries {
            results.push(self.execute(q).await?);
        }
        Ok(results)
    }

    async fn insert(&self, _table: &str, _records: &[serde_json::Value]) -> Result<Vec<String>> {
        Ok(vec![])
    }

    async fn upsert(&self, _table: &str, _records: &[serde_json::Value]) -> Result<Vec<String>> {
        Ok(vec![])
    }

    async fn select(&self, table: &str, filter: Option<&str>) -> Result<Vec<serde_json::Value>> {
        let query = match filter {
            Some(f) => format!("SELECT * FROM `{}` WHERE {}", table, f),
            None => format!("SELECT * FROM `{}`", table),
        };
        self.execute(&query).await
    }

    async fn count(&self, table: &str, filter: Option<&str>) -> Result<u64> {
        let pool = self.pool.as_ref()
            .ok_or_else(|| EvenframeError::database("Not connected"))?;

        let query = match filter {
            Some(f) => format!("SELECT COUNT(*) as count FROM `{}` WHERE {}", table, f),
            None => format!("SELECT COUNT(*) as count FROM `{}`", table),
        };

        let row = sqlx::query(&query).fetch_one(pool).await
            .map_err(|e| EvenframeError::database(format!("Count failed: {e}")))?;

        Ok(row.get::<i64, _>("count") as u64)
    }

    async fn delete(&self, table: &str, ids: &[String]) -> Result<()> {
        let pool = self.pool.as_ref()
            .ok_or_else(|| EvenframeError::database("Not connected"))?;

        for id in ids {
            sqlx::query(&format!("DELETE FROM `{}` WHERE id = '{}'", table, id))
                .execute(pool)
                .await
                .map_err(|e| EvenframeError::database(format!("Delete failed: {e}")))?;
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
        let mut cols = vec!["    `id` INT AUTO_INCREMENT PRIMARY KEY".to_string()];

        for field in &config.struct_config.fields {
            if field.field_name == "id" { continue; }
            let sql_type = self.type_mapper.field_type_to_native(&field.field_type);
            if sql_type.is_empty() { continue; }
            let nullable = matches!(field.field_type, FieldType::Option(_));
            cols.push(format!("    `{}` {}{}", field.field_name, sql_type, if nullable { "" } else { " NOT NULL" }));
        }

        format!("CREATE TABLE IF NOT EXISTS `{}` (\n{}\n);", table_name, cols.join(",\n"))
    }

    fn generate_create_field(
        &self,
        table_name: &str,
        field: &StructField,
        _objects: &HashMap<String, StructConfig>,
        _enums: &HashMap<String, TaggedUnion>,
    ) -> String {
        let sql_type = self.type_mapper.field_type_to_native(&field.field_type);
        format!("ALTER TABLE `{}` ADD COLUMN `{}` {};", table_name, field.field_name, sql_type)
    }

    fn map_field_type(&self, field_type: &FieldType) -> String {
        self.type_mapper.field_type_to_native(field_type)
    }

    fn format_value(&self, field_type: &FieldType, value: &serde_json::Value) -> String {
        self.type_mapper.format_value(field_type, value)
    }

    fn generate_relationship_table(&self, edge: &EdgeConfig) -> Vec<String> {
        generate_join_table_sql(edge, &JoinTableConfig::mysql())
    }

    async fn create_relationship(&self, _edge_table: &str, _from_id: &str, _to_id: &str, _data: Option<&serde_json::Value>) -> Result<String> {
        Ok(String::new())
    }

    async fn delete_relationship(&self, _edge_table: &str, _from_id: &str, _to_id: &str) -> Result<()> {
        Ok(())
    }

    async fn get_relationships(&self, _edge_table: &str, _record_id: &str, _direction: RelationshipDirection) -> Result<Vec<Relationship>> {
        Ok(vec![])
    }

    async fn begin_transaction(&self) -> Result<Box<dyn Transaction>> {
        Err(EvenframeError::database("Transactions not implemented"))
    }

    async fn create_embedded_instance(&self) -> Result<Option<Box<dyn DatabaseProvider>>> {
        Ok(None)
    }
}
