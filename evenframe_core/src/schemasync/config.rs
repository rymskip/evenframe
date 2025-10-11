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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DatabaseConfig {
    pub url: String,
    pub namespace: String,
    pub database: String,
    pub timeout: u64,
    pub accesses: Vec<AccessConfig>,
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

impl DatabaseConfig {
    /// Creates a database configuration suitable for testing
    pub fn for_testing() -> Self {
        debug!("Creating database configuration for testing environment");
        let config = Self {
            url: "http://localhost:8000".to_string(),
            namespace: "test".to_string(),
            database: "test".to_string(),
            accesses: vec![AccessConfig {
                name: "user".to_owned(),
                access_type: AccessType::Record,
                table_name: "user".to_owned(),
            }],
            timeout: 60,
        };
        trace!(
            "Test database config - URL: {}, namespace: {}, database: {}, timeout: {}s",
            config.url, config.namespace, config.database, config.timeout
        );
        trace!("Test access configs: {} entries", config.accesses.len());
        config
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
