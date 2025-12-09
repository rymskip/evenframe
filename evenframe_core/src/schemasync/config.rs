use crate::{coordinate::CoordinationGroup, schemasync::compare::PreservationMode};
use bon::Builder;
use serde::{Deserialize, Serialize};
use tracing::{debug, trace};

/// Configuration for Schemasync operations (database synchronization)
#[derive(Debug, Clone, Deserialize, Serialize, Builder)]
pub struct SchemasyncConfig {
    /// Database connection configuration
    pub database: DatabaseConfig,
    /// Whether to generate mock data
    pub should_generate_mocks: bool,
    /// default mock data generation configuration, overridden by table and field level configs
    pub mock_gen_config: SchemasyncMockGenConfig,
    /// Performance tuning configuration
    pub performance: PerformanceConfig,
}

/// Database provider type for configuration
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DatabaseProvider {
    /// SurrealDB (default)
    #[default]
    Surrealdb,
    /// PostgreSQL
    Postgres,
    /// MySQL
    Mysql,
    /// SQLite
    Sqlite,
}

impl std::fmt::Display for DatabaseProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DatabaseProvider::Surrealdb => write!(f, "surrealdb"),
            DatabaseProvider::Postgres => write!(f, "postgres"),
            DatabaseProvider::Mysql => write!(f, "mysql"),
            DatabaseProvider::Sqlite => write!(f, "sqlite"),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DatabaseConfig {
    /// Database provider type (surrealdb, postgres, mysql, sqlite)
    #[serde(default)]
    pub provider: DatabaseProvider,
    /// Database connection URL
    pub url: String,
    /// SurrealDB namespace (only used for SurrealDB)
    #[serde(default)]
    pub namespace: String,
    /// Database name (SurrealDB) or schema name (PostgreSQL)
    #[serde(default)]
    pub database: String,
    /// Connection timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout: u64,
    /// Access configurations (SurrealDB-specific)
    #[serde(default)]
    pub accesses: Vec<AccessConfig>,
    /// Maximum number of connections in the pool (SQL databases)
    #[serde(default)]
    pub max_connections: Option<u32>,
    /// Minimum number of connections in the pool (SQL databases)
    #[serde(default)]
    pub min_connections: Option<u32>,
    /// Schema name for PostgreSQL (defaults to "public")
    #[serde(default)]
    pub schema: Option<String>,
}

fn default_timeout() -> u64 {
    60
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AccessConfig {
    pub name: String,
    pub access_type: AccessType,
    pub table_name: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub enum AccessType {
    System,
    Record,
    Bearer,
    Jwt,
}

#[derive(Debug, Clone, Deserialize, Serialize, Builder)]
pub struct SchemasyncMockGenConfig {
    /// overriden by table level  configs
    pub default_record_count: usize,

    /// overriden by table level and field level configs
    pub default_preservation_mode: PreservationMode,

    /// overriden by table level and field level configs
    pub default_batch_size: usize,

    #[builder(default)]
    /// global table field coordination, Vec<(HashSet<TableName>, Vec<Coordination>)>
    pub coordination_groups: Vec<CoordinationGroup>,

    pub full_refresh_mode: bool,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            provider: DatabaseProvider::default(),
            url: String::new(),
            namespace: String::new(),
            database: String::new(),
            timeout: default_timeout(),
            accesses: vec![],
            max_connections: None,
            min_connections: None,
            schema: None,
        }
    }
}

impl DatabaseConfig {
    /// Creates a database configuration suitable for testing with SurrealDB
    pub fn for_testing() -> Self {
        debug!("Creating database configuration for testing environment");
        let config = Self {
            provider: DatabaseProvider::Surrealdb,
            url: "http://localhost:8000".to_string(),
            namespace: "test".to_string(),
            database: "test".to_string(),
            accesses: vec![AccessConfig {
                name: "user".to_owned(),
                access_type: AccessType::Record,
                table_name: "user".to_owned(),
            }],
            timeout: 60,
            max_connections: None,
            min_connections: None,
            schema: None,
        };
        trace!(
            "Test database config - URL: {}, namespace: {}, database: {}, timeout: {}s",
            config.url, config.namespace, config.database, config.timeout
        );
        trace!("Test access configs: {} entries", config.accesses.len());
        config
    }

    /// Creates a database configuration for PostgreSQL testing
    pub fn for_postgres_testing(url: &str) -> Self {
        debug!("Creating PostgreSQL database configuration for testing");
        Self {
            provider: DatabaseProvider::Postgres,
            url: url.to_string(),
            namespace: String::new(),
            database: String::new(),
            timeout: 60,
            accesses: vec![],
            max_connections: Some(5),
            min_connections: Some(1),
            schema: Some("public".to_string()),
        }
    }

    /// Creates a database configuration for SQLite testing
    pub fn for_sqlite_testing(path: &str) -> Self {
        debug!("Creating SQLite database configuration for testing");
        Self {
            provider: DatabaseProvider::Sqlite,
            url: format!("sqlite:{}", path),
            namespace: String::new(),
            database: String::new(),
            timeout: 60,
            accesses: vec![],
            max_connections: Some(1),
            min_connections: Some(1),
            schema: None,
        }
    }

    /// Convert to provider-specific DatabaseConfig
    pub fn to_provider_config(&self) -> crate::schemasync::database::DatabaseConfig {
        crate::schemasync::database::DatabaseConfig {
            provider: match self.provider {
                DatabaseProvider::Surrealdb => crate::schemasync::database::ProviderType::SurrealDb,
                DatabaseProvider::Postgres => crate::schemasync::database::ProviderType::Postgres,
                DatabaseProvider::Mysql => crate::schemasync::database::ProviderType::MySql,
                DatabaseProvider::Sqlite => crate::schemasync::database::ProviderType::Sqlite,
            },
            url: self.url.clone(),
            namespace: if self.namespace.is_empty() { None } else { Some(self.namespace.clone()) },
            database: if self.database.is_empty() { None } else { Some(self.database.clone()) },
            username: None, // Will be loaded from env vars
            password: None, // Will be loaded from env vars
            max_connections: self.max_connections,
            min_connections: self.min_connections,
            schema: self.schema.clone(),
            timeout_secs: self.timeout,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    pub embedded_db_memory_limit: String,
    pub cache_duration_seconds: u64,
    pub use_progressive_loading: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MockMode {
    Smart,
    RegenerateAll,
    PreserveAll,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegenerateFieldsConfig {
    pub always: Vec<String>,
}

impl Default for RegenerateFieldsConfig {
    fn default() -> Self {
        debug!("Creating default regenerate fields configuration");
        let config = Self {
            always: vec!["updated_at".to_string(), "created_at".to_string()],
        };
        trace!("Default regenerate fields: {:?}", config.always);
        config
    }
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        debug!("Creating default performance configuration");
        let config = Self {
            embedded_db_memory_limit: "1GB".to_string(),
            cache_duration_seconds: 300,
            use_progressive_loading: true,
        };
        trace!(
            "Default performance config - memory: {}, cache: {}s, progressive: {}",
            config.embedded_db_memory_limit,
            config.cache_duration_seconds,
            config.use_progressive_loading
        );
        config
    }
}
