//! SurrealDB Database Provider Implementation
//!
//! This module implements the DatabaseProvider trait for SurrealDB,
//! containing all SurrealQL query generation and execution logic.

pub mod access;
pub mod assert;
pub mod define;
pub mod execute;
pub mod insert;
pub mod query;
pub mod remove;
mod type_mapper;
pub mod upsert;
pub mod value;

use async_trait::async_trait;
use std::collections::HashMap;
use surrealdb::{
    Surreal,
    engine::remote::http::{Client, Http},
    opt::auth::Root,
};
use tracing::{debug, info, trace};

use crate::error::{EvenframeError, Result};
use crate::schemasync::{EdgeConfig, TableConfig};
use crate::types::{FieldType, StructConfig, StructField, TaggedUnion};

use self::define::generate_define_statements;
use self::value::to_surreal_string;

use super::{
    DatabaseConfig, DatabaseProvider, ProviderType, Relationship, RelationshipDirection,
    SchemaExport, TableInfo, Transaction,
};

pub use type_mapper::SurrealdbTypeMapper;

/// SurrealDB database provider implementation.
///
/// This provider wraps the existing SurrealDB functionality and implements
/// the DatabaseProvider trait for use with the abstracted SchemaSync system.
pub struct SurrealdbProvider {
    /// The SurrealDB client connection
    client: Option<Surreal<Client>>,
    /// Connection configuration
    config: Option<DatabaseConfig>,
    /// Type mapper for SurrealDB
    type_mapper: SurrealdbTypeMapper,
}

impl SurrealdbProvider {
    /// Create a new SurrealDB provider instance
    pub fn new() -> Self {
        Self {
            client: None,
            config: None,
            type_mapper: SurrealdbTypeMapper,
        }
    }

    /// Get a reference to the underlying SurrealDB client.
    ///
    /// This is useful for backward compatibility with existing code
    /// that needs direct access to the client.
    pub fn client(&self) -> Option<&Surreal<Client>> {
        self.client.as_ref()
    }

    /// Get a mutable reference to the underlying SurrealDB client.
    pub fn client_mut(&mut self) -> Option<&mut Surreal<Client>> {
        self.client.as_mut()
    }

    /// Take ownership of the underlying client.
    ///
    /// After calling this, the provider will be disconnected.
    pub fn take_client(&mut self) -> Option<Surreal<Client>> {
        self.client.take()
    }
}

impl Default for SurrealdbProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl DatabaseProvider for SurrealdbProvider {
    fn name(&self) -> &'static str {
        "surrealdb"
    }

    fn supports_graph_queries(&self) -> bool {
        true
    }

    fn supports_embedded_mode(&self) -> bool {
        true
    }

    async fn connect(&mut self, config: &DatabaseConfig) -> Result<()> {
        if config.provider != ProviderType::SurrealDb {
            return Err(EvenframeError::config(format!(
                "SurrealDB provider cannot connect with provider type: {}",
                config.provider
            )));
        }

        info!("Connecting to SurrealDB at {}", config.url);

        let client = Surreal::new::<Http>(&config.url)
            .await
            .map_err(|e| EvenframeError::database(format!(
                "Failed to create SurrealDB HTTP client: {e}"
            )))?;

        // Sign in if credentials provided
        if let (Some(username), Some(password)) = (&config.username, &config.password) {
            debug!("Signing in to SurrealDB as {}", username);
            client
                .signin(Root {
                    username: username.to_string(),
                    password: password.to_string(),
                })
                .await
                .map_err(|e| EvenframeError::database(format!(
                    "Failed to sign in to SurrealDB: {e}"
                )))?;
        }

        // Select namespace and database
        if let (Some(ns), Some(db)) = (&config.namespace, &config.database) {
            debug!("Using namespace '{}' and database '{}'", ns, db);
            client
                .use_ns(ns)
                .use_db(db)
                .await
                .map_err(|e| EvenframeError::database(format!(
                    "Failed to select namespace/database: {e}"
                )))?;
        }

        self.client = Some(client);
        self.config = Some(config.clone());
        info!("Successfully connected to SurrealDB");

        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        self.client = None;
        self.config = None;
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.client.is_some()
    }

    async fn export_schema(&self) -> Result<SchemaExport> {
        let client = self.client.as_ref()
            .ok_or_else(|| EvenframeError::database("Not connected to SurrealDB"))?;

        // Use SurrealDB's export functionality
        let mut export_stream = client
            .export(())
            .await
            .map_err(|e| EvenframeError::database(format!("Failed to export schema: {e}")))?;

        use futures::StreamExt;
        let mut raw_statements = String::new();
        while let Some(chunk) = export_stream.next().await {
            let chunk = chunk.map_err(|e| EvenframeError::database(format!(
                "Error reading export stream: {e}"
            )))?;
            raw_statements.push_str(&String::from_utf8_lossy(&chunk));
        }

        Ok(SchemaExport {
            tables: vec![],
            indexes: vec![],
            relationships: vec![],
            raw_statements: Some(raw_statements),
        })
    }

    async fn apply_schema(&self, statements: &[String]) -> Result<()> {
        let client = self.client.as_ref()
            .ok_or_else(|| EvenframeError::database("Not connected to SurrealDB"))?;

        for stmt in statements {
            trace!("Executing schema statement: {}", stmt);
            client
                .query(stmt)
                .await
                .map_err(|e| EvenframeError::database(format!(
                    "Failed to execute schema statement: {e}\nStatement: {stmt}"
                )))?;
        }

        Ok(())
    }

    async fn get_table_info(&self, table_name: &str) -> Result<Option<TableInfo>> {
        let client = self.client.as_ref()
            .ok_or_else(|| EvenframeError::database("Not connected to SurrealDB"))?;

        let query = format!("INFO FOR TABLE {}", table_name);
        let response: surrealdb::IndexedResults = client
            .query(&query)
            .await
            .map_err(|e| EvenframeError::database(format!(
                "Failed to get table info: {e}"
            )))?;

        // Parse response - for now return None if table doesn't exist
        // TODO: Parse the INFO response into TableInfo struct
        let _ = response;
        Ok(None)
    }

    async fn list_tables(&self) -> Result<Vec<String>> {
        let client = self.client.as_ref()
            .ok_or_else(|| EvenframeError::database("Not connected to SurrealDB"))?;

        let query = "INFO FOR DB";
        let mut response = client
            .query(query)
            .await
            .map_err(|e| EvenframeError::database(format!(
                "Failed to list tables: {e}"
            )))?;

        // Parse the response to extract table names
        let result: Option<serde_json::Value> = response
            .take(0)
            .map_err(|e| EvenframeError::database(format!(
                "Failed to parse table list: {e}"
            )))?;

        let mut tables = Vec::new();
        if let Some(value) = result
            && let Some(tb) = value.get("tables").and_then(|v| v.as_object())
        {
            tables.extend(tb.keys().cloned());
        }

        Ok(tables)
    }

    async fn execute(&self, query: &str) -> Result<Vec<serde_json::Value>> {
        let client = self.client.as_ref()
            .ok_or_else(|| EvenframeError::database("Not connected to SurrealDB"))?;

        let mut response = client
            .query(query)
            .await
            .map_err(|e| EvenframeError::database(format!(
                "Failed to execute query: {e}"
            )))?;

        let results: Vec<serde_json::Value> = response
            .take(0)
            .map_err(|e| EvenframeError::database(format!(
                "Failed to parse query results: {e}"
            )))?;

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
        let client = self.client.as_ref()
            .ok_or_else(|| EvenframeError::database("Not connected to SurrealDB"))?;

        let mut ids = Vec::with_capacity(records.len());

        for record in records {
            let created: Option<serde_json::Value> = client
                .create(table)
                .content(record.clone())
                .await
                .map_err(|e| EvenframeError::database(format!(
                    "Failed to insert record: {e}"
                )))?;

            if let Some(created) = created
                && let Some(id) = created.get("id").and_then(|v| v.as_str())
            {
                ids.push(id.to_string());
            }
        }

        Ok(ids)
    }

    async fn upsert(
        &self,
        table: &str,
        records: &[serde_json::Value],
    ) -> Result<Vec<String>> {
        let client = self.client.as_ref()
            .ok_or_else(|| EvenframeError::database("Not connected to SurrealDB"))?;

        let mut ids = Vec::with_capacity(records.len());

        for record in records {
            // Extract ID if present
            let id = record.get("id").and_then(|v| v.as_str());

            let upserted: Option<serde_json::Value> = if let Some(id) = id {
                client
                    .upsert((table, id))
                    .content(record.clone())
                    .await
                    .map_err(|e| EvenframeError::database(format!(
                        "Failed to upsert record: {e}"
                    )))?
            } else {
                client
                    .create(table)
                    .content(record.clone())
                    .await
                    .map_err(|e| EvenframeError::database(format!(
                        "Failed to create record during upsert: {e}"
                    )))?
            };

            if let Some(upserted) = upserted
                && let Some(id) = upserted.get("id").and_then(|v| v.as_str())
            {
                ids.push(id.to_string());
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
            format!("SELECT * FROM {} WHERE {}", table, f)
        } else {
            format!("SELECT * FROM {}", table)
        };

        self.execute(&query).await
    }

    async fn count(&self, table: &str, filter: Option<&str>) -> Result<u64> {
        let query = if let Some(f) = filter {
            format!("SELECT count() FROM {} WHERE {} GROUP ALL", table, f)
        } else {
            format!("SELECT count() FROM {} GROUP ALL", table)
        };

        let results = self.execute(&query).await?;

        if let Some(first) = results.first()
            && let Some(count) = first.get("count").and_then(|v| v.as_u64())
        {
            return Ok(count);
        }

        Ok(0)
    }

    async fn delete(&self, table: &str, ids: &[String]) -> Result<()> {
        let client = self.client.as_ref()
            .ok_or_else(|| EvenframeError::database("Not connected to SurrealDB"))?;

        for id in ids {
            // Parse the ID to get table:id format or just id
            let record_id = if id.contains(':') {
                id.clone()
            } else {
                format!("{}:{}", table, id)
            };

            client
                .query(format!("DELETE {}", record_id))
                .await
                .map_err(|e| EvenframeError::database(format!(
                    "Failed to delete record {}: {e}", record_id
                )))?;
        }

        Ok(())
    }

    fn generate_create_table(
        &self,
        table_name: &str,
        config: &TableConfig,
        all_tables: &HashMap<String, TableConfig>,
        objects: &HashMap<String, StructConfig>,
        enums: &HashMap<String, TaggedUnion>,
    ) -> String {
        // Use existing generate_define_statements function
        generate_define_statements(
            table_name,
            config,
            all_tables,
            objects,
            enums,
            false, // full_refresh_mode
        )
    }

    fn generate_create_field(
        &self,
        table_name: &str,
        field: &StructField,
        _objects: &HashMap<String, StructConfig>,
        _enums: &HashMap<String, TaggedUnion>,
    ) -> String {
        // Generate a DEFINE FIELD statement
        let field_type = self.map_field_type(&field.field_type);
        format!(
            "DEFINE FIELD {} ON TABLE {} TYPE {};",
            field.field_name,
            table_name,
            field_type
        )
    }

    fn map_field_type(&self, field_type: &FieldType) -> String {
        self.type_mapper.field_type_to_surql(field_type)
    }

    fn format_value(
        &self,
        field_type: &FieldType,
        value: &serde_json::Value,
    ) -> String {
        to_surreal_string(field_type, value)
    }

    fn generate_relationship_table(&self, edge: &EdgeConfig) -> Vec<String> {
        // For SurrealDB, edges are defined as tables with special structure
        // The actual RELATE statements create relationships
        let mut statements = Vec::new();

        statements.push(format!(
            "DEFINE TABLE {} SCHEMAFULL;",
            edge.edge_name
        ));

        // Define in and out fields for edges
        statements.push(format!(
            "DEFINE FIELD in ON TABLE {} TYPE record;",
            edge.edge_name
        ));
        statements.push(format!(
            "DEFINE FIELD out ON TABLE {} TYPE record;",
            edge.edge_name
        ));

        statements
    }

    async fn create_relationship(
        &self,
        edge_table: &str,
        from_id: &str,
        to_id: &str,
        data: Option<&serde_json::Value>,
    ) -> Result<String> {
        let client = self.client.as_ref()
            .ok_or_else(|| EvenframeError::database("Not connected to SurrealDB"))?;

        let query = if let Some(data) = data {
            format!(
                "RELATE {}->{}->{} CONTENT {}",
                from_id, edge_table, to_id,
                serde_json::to_string(data).unwrap_or_default()
            )
        } else {
            format!("RELATE {}->{}->{}",  from_id, edge_table, to_id)
        };

        let mut response = client
            .query(&query)
            .await
            .map_err(|e| EvenframeError::database(format!(
                "Failed to create relationship: {e}"
            )))?;

        let results: Vec<serde_json::Value> = response
            .take(0)
            .map_err(|e| EvenframeError::database(format!(
                "Failed to parse relationship result: {e}"
            )))?;

        if let Some(first) = results.first()
            && let Some(id) = first.get("id").and_then(|v| v.as_str())
        {
            return Ok(id.to_string());
        }

        Ok(String::new())
    }

    async fn delete_relationship(
        &self,
        edge_table: &str,
        from_id: &str,
        to_id: &str,
    ) -> Result<()> {
        let query = format!(
            "DELETE {} WHERE in = {} AND out = {}",
            edge_table, from_id, to_id
        );
        self.execute(&query).await?;
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
                format!("SELECT * FROM {} WHERE in = {}", edge_table, record_id)
            }
            RelationshipDirection::Incoming => {
                format!("SELECT * FROM {} WHERE out = {}", edge_table, record_id)
            }
            RelationshipDirection::Both => {
                format!(
                    "SELECT * FROM {} WHERE in = {} OR out = {}",
                    edge_table, record_id, record_id
                )
            }
        };

        let results = self.execute(&query).await?;

        let relationships = results
            .into_iter()
            .filter_map(|v| {
                let id = v.get("id")?.as_str()?.to_string();
                let from_id = v.get("in")?.as_str()?.to_string();
                let to_id = v.get("out")?.as_str()?.to_string();
                Some(Relationship {
                    id,
                    from_id,
                    to_id,
                    data: Some(v),
                })
            })
            .collect();

        Ok(relationships)
    }

    async fn begin_transaction(&self) -> Result<Box<dyn Transaction>> {
        Err(EvenframeError::database(
            "SurrealDB transactions are not yet implemented in the provider abstraction"
        ))
    }

    async fn create_embedded_instance(&self) -> Result<Option<Box<dyn DatabaseProvider>>> {
        // SurrealDB supports embedded mode via Surreal<Mem>
        // For now, return None - this will be implemented later
        // when we refactor the comparator to use the provider abstraction
        Ok(None)
    }
}
