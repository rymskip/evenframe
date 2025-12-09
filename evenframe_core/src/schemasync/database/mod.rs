//! Database Provider Abstraction Layer
//!
//! This module provides a database-agnostic interface for SchemaSync operations.
//! It allows Evenframe to work with multiple database backends including:
//! - SurrealDB (default)
//! - PostgreSQL
//! - MySQL/MariaDB
//! - SQLite

pub mod types;

#[cfg(feature = "surrealdb")]
pub mod surql;

#[cfg(feature = "sql")]
pub mod sql;

use async_trait::async_trait;
use std::collections::HashMap;

use crate::error::Result;
use crate::schemasync::{EdgeConfig, TableConfig};
use crate::types::StructField;

pub use types::*;

// Re-export from compare module for backward compatibility
pub use crate::schemasync::compare::SchemaComparator;
#[cfg(feature = "sql")]
pub use crate::schemasync::compare::sql::SqlSchemaComparator;

#[cfg(feature = "surrealdb")]
pub use self::surql::SurrealdbProvider;

/// Core trait that all database providers must implement.
///
/// This trait abstracts database operations to allow SchemaSync to work
/// with multiple database backends. Each provider implements database-specific
/// connection handling, query generation, and schema operations.
#[async_trait]
pub trait DatabaseProvider: Send + Sync {
    // === Provider Identification ===

    /// Returns the name of the database provider (e.g., "surrealdb", "postgres")
    fn name(&self) -> &'static str;

    /// Whether this provider supports native graph queries (edges)
    /// SQL databases return false; SurrealDB returns true
    fn supports_graph_queries(&self) -> bool;

    /// Whether this provider supports embedded/in-memory mode for schema comparison
    fn supports_embedded_mode(&self) -> bool;

    // === Connection Management ===

    /// Establish a connection to the database
    async fn connect(&mut self, config: &DatabaseConfig) -> Result<()>;

    /// Close the database connection
    async fn disconnect(&mut self) -> Result<()>;

    /// Check if the provider is currently connected
    fn is_connected(&self) -> bool;

    // === Schema Operations ===

    /// Export the current database schema
    async fn export_schema(&self) -> Result<SchemaExport>;

    /// Apply schema statements to the database
    async fn apply_schema(&self, statements: &[String]) -> Result<()>;

    /// Get information about a specific table
    async fn get_table_info(&self, table_name: &str) -> Result<Option<TableInfo>>;

    /// List all tables in the database
    async fn list_tables(&self) -> Result<Vec<String>>;

    // === Query Execution ===

    /// Execute a raw query and return results as JSON values
    async fn execute(&self, query: &str) -> Result<Vec<serde_json::Value>>;

    /// Execute multiple queries in a batch
    async fn execute_batch(&self, queries: &[String]) -> Result<Vec<Vec<serde_json::Value>>>;

    // === Data Operations ===

    /// Insert records into a table, returning the generated IDs
    async fn insert(
        &self,
        table: &str,
        records: &[serde_json::Value],
    ) -> Result<Vec<String>>;

    /// Upsert records (insert or update on conflict)
    async fn upsert(
        &self,
        table: &str,
        records: &[serde_json::Value],
    ) -> Result<Vec<String>>;

    /// Select records from a table with optional filter
    async fn select(
        &self,
        table: &str,
        filter: Option<&str>,
    ) -> Result<Vec<serde_json::Value>>;

    /// Count records in a table with optional filter
    async fn count(&self, table: &str, filter: Option<&str>) -> Result<u64>;

    /// Delete records by IDs
    async fn delete(&self, table: &str, ids: &[String]) -> Result<()>;

    // === Schema Generation ===

    /// Generate a CREATE TABLE statement (or equivalent) for the given table config
    fn generate_create_table(
        &self,
        table_name: &str,
        config: &TableConfig,
        all_tables: &HashMap<String, TableConfig>,
        objects: &HashMap<String, crate::types::StructConfig>,
        enums: &HashMap<String, crate::types::TaggedUnion>,
    ) -> String;

    /// Generate a statement to define/create a field
    fn generate_create_field(
        &self,
        table_name: &str,
        field: &StructField,
        objects: &HashMap<String, crate::types::StructConfig>,
        enums: &HashMap<String, crate::types::TaggedUnion>,
    ) -> String;

    /// Map a FieldType to the database's native type string
    fn map_field_type(&self, field_type: &crate::types::FieldType) -> String;

    /// Format a value for use in a query (with proper escaping/quoting)
    fn format_value(
        &self,
        field_type: &crate::types::FieldType,
        value: &serde_json::Value,
    ) -> String;

    // === Relationship/Edge Handling ===

    /// Generate statements to create a relationship/edge table
    /// For SQL: generates JOIN table with foreign keys
    /// For SurrealDB: generates edge table definition
    fn generate_relationship_table(&self, edge: &EdgeConfig) -> Vec<String>;

    /// Create a relationship between two records
    /// For SQL: INSERT into join table
    /// For SurrealDB: RELATE statement
    async fn create_relationship(
        &self,
        edge_table: &str,
        from_id: &str,
        to_id: &str,
        data: Option<&serde_json::Value>,
    ) -> Result<String>;

    /// Delete a relationship between two records
    async fn delete_relationship(
        &self,
        edge_table: &str,
        from_id: &str,
        to_id: &str,
    ) -> Result<()>;

    /// Get relationships for a record
    async fn get_relationships(
        &self,
        edge_table: &str,
        record_id: &str,
        direction: RelationshipDirection,
    ) -> Result<Vec<Relationship>>;

    // === Transaction Support ===

    /// Begin a transaction (returns a transaction handle)
    async fn begin_transaction(&self) -> Result<Box<dyn Transaction>>;

    // === Embedded Mode (for schema comparison) ===

    /// Create an embedded/in-memory instance for schema comparison
    /// Returns None if the provider doesn't support embedded mode
    async fn create_embedded_instance(&self) -> Result<Option<Box<dyn DatabaseProvider>>>;
}

/// Transaction trait for atomic database operations
#[async_trait]
pub trait Transaction: Send + Sync {
    /// Commit the transaction
    async fn commit(self: Box<Self>) -> Result<()>;

    /// Rollback the transaction
    async fn rollback(self: Box<Self>) -> Result<()>;

    /// Execute a query within the transaction
    async fn execute(&self, query: &str) -> Result<Vec<serde_json::Value>>;
}

/// Database configuration for connecting to a database
#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    /// The type of database provider
    pub provider: ProviderType,

    /// Connection URL (format depends on provider)
    pub url: String,

    /// SurrealDB-specific: namespace
    pub namespace: Option<String>,

    /// SurrealDB-specific: database name
    pub database: Option<String>,

    /// Username for authentication
    pub username: Option<String>,

    /// Password for authentication
    pub password: Option<String>,

    /// Connection timeout in seconds
    pub timeout_secs: u64,

    /// SQL-specific: maximum connection pool size
    pub max_connections: Option<u32>,

    /// SQL-specific: minimum connection pool size
    pub min_connections: Option<u32>,

    /// PostgreSQL-specific: schema name (default: "public")
    pub schema: Option<String>,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            provider: ProviderType::SurrealDb,
            url: String::new(),
            namespace: None,
            database: None,
            username: None,
            password: None,
            timeout_secs: 30,
            max_connections: Some(10),
            min_connections: Some(1),
            schema: Some("public".to_string()),
        }
    }
}

/// Supported database provider types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProviderType {
    #[default]
    SurrealDb,
    Postgres,
    MySql,
    Sqlite,
}

impl std::fmt::Display for ProviderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderType::SurrealDb => write!(f, "surrealdb"),
            ProviderType::Postgres => write!(f, "postgres"),
            ProviderType::MySql => write!(f, "mysql"),
            ProviderType::Sqlite => write!(f, "sqlite"),
        }
    }
}

impl std::str::FromStr for ProviderType {
    type Err = crate::error::EvenframeError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "surrealdb" | "surreal" => Ok(ProviderType::SurrealDb),
            "postgres" | "postgresql" | "pg" => Ok(ProviderType::Postgres),
            "mysql" | "mariadb" => Ok(ProviderType::MySql),
            "sqlite" | "sqlite3" => Ok(ProviderType::Sqlite),
            _ => Err(crate::error::EvenframeError::config(format!(
                "Unknown database provider: {}. Supported: surrealdb, postgres, mysql, sqlite",
                s
            ))),
        }
    }
}

/// Factory function to create a database provider based on configuration
pub fn create_provider(config: &DatabaseConfig) -> Result<Box<dyn DatabaseProvider>> {
    match config.provider {
        #[cfg(feature = "surrealdb")]
        ProviderType::SurrealDb => Ok(Box::new(surql::SurrealdbProvider::new())),

        #[cfg(not(feature = "surrealdb"))]
        ProviderType::SurrealDb => Err(crate::error::EvenframeError::config(
            "SurrealDB support not enabled. Enable the 'surrealdb' feature flag.",
        )),

        #[cfg(feature = "postgres")]
        ProviderType::Postgres => Ok(Box::new(sql::postgres::PostgresProvider::new())),

        #[cfg(not(feature = "postgres"))]
        ProviderType::Postgres => Err(crate::error::EvenframeError::config(
            "PostgreSQL support not enabled. Enable the 'postgres' feature flag.",
        )),

        #[cfg(feature = "mysql")]
        ProviderType::MySql => Ok(Box::new(sql::mysql::MysqlProvider::new())),

        #[cfg(not(feature = "mysql"))]
        ProviderType::MySql => Err(crate::error::EvenframeError::config(
            "MySQL support not enabled. Enable the 'mysql' feature flag.",
        )),

        #[cfg(feature = "sqlite")]
        ProviderType::Sqlite => Ok(Box::new(sql::sqlite::SqliteProvider::new())),

        #[cfg(not(feature = "sqlite"))]
        ProviderType::Sqlite => Err(crate::error::EvenframeError::config(
            "SQLite support not enabled. Enable the 'sqlite' feature flag.",
        )),
    }
}

/// Helper to create a provider and connect in one step
pub async fn connect(config: &DatabaseConfig) -> Result<Box<dyn DatabaseProvider>> {
    let mut provider = create_provider(config)?;
    provider.connect(config).await?;
    Ok(provider)
}
