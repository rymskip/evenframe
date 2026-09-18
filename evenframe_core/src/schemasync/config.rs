use crate::schemasync::{PreservationMode, mockmake::coordinate::CoordinationGroup};
use bon::Builder;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tracing::{debug, trace};

/// Configuration for a WASM mock data plugin.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PluginConfig {
    /// Path to the `.wasm` file, relative to project root.
    pub path: String,
    /// Free-form string parameters forwarded to the plugin on every call
    /// (available as `params` on the plugin-side context). Values go through
    /// the same `${VAR}` env substitution as the rest of the config.
    #[serde(default)]
    pub params: BTreeMap<String, String>,
}

/// Configuration for Schemasync operations (database synchronization)
#[derive(Debug, Clone, Deserialize, Serialize, Builder)]
pub struct SchemasyncConfig {
    /// Database connection configuration
    pub database: DatabaseConfig,
    /// Whether to generate mock data
    pub should_generate_mocks: bool,
    /// default mock data generation configuration, overridden by table and field level configs
    #[serde(default)]
    pub mock_gen_config: SchemasyncMockGenConfig,
    /// Performance tuning configuration
    #[serde(default)]
    pub performance: PerformanceConfig,
    /// WASM plugin definitions for mock data generation.
    #[serde(default)]
    #[builder(default)]
    pub plugins: BTreeMap<String, PluginConfig>,
    /// Lint pass configuration
    #[serde(default)]
    #[builder(default)]
    pub lint: LintConfig,
}

/// Configuration for the schemasync lint pass, under `[schemasync.lint]`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct LintConfig {
    /// Silence warnings for `#[define_field_statement(...)]` settings whose
    /// effect cannot be determined from this run's data: settings on a
    /// `resolve_only` type that has an `id` field. This run inlines such a
    /// type (discarding the settings), but the project that owns it may
    /// materialize the table and honor them.
    #[serde(default)]
    pub silence_unverifiable_annotations: bool,
}

/// Database provider type for configuration
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
#[cfg_attr(feature = "cli", derive(clap::ValueEnum))]
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
    /// Access configurations (SurrealDB-specific) - inline or path-based
    #[serde(default)]
    pub accesses: AccessesSource,
    /// Function definitions from .surql files
    #[serde(default)]
    pub functions: Option<FunctionsSource>,
    /// Analyzer definitions (`DEFINE ANALYZER ...`) from .surql files.
    /// Applied before tables so full-text indexes can reference them.
    #[serde(default)]
    pub analyzers: Option<AnalyzersSource>,
    /// Resolved surql content loaded from paths (set at runtime, not from TOML)
    #[serde(skip)]
    pub resolved: ResolvedDatabaseItems,
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

/// Source for access definitions: either inline config or a path to .surql file(s).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum AccessesSource {
    /// Existing format: array of AccessConfig structs
    Inline(Vec<AccessConfig>),
    /// New format: `{ path = "..." }` pointing to .surql file or directory
    Path { path: String },
}

impl Default for AccessesSource {
    fn default() -> Self {
        AccessesSource::Inline(vec![])
    }
}

/// Source for function definitions: a path to .surql file(s).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FunctionsSource {
    pub path: String,
}

/// Source for analyzer definitions: a path to .surql file(s).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnalyzersSource {
    pub path: String,
}

/// Resolved surql content loaded from paths at config init time.
#[derive(Debug, Clone, Default)]
pub struct ResolvedDatabaseItems {
    pub access_surql: Option<String>,
    pub functions_surql: Option<String>,
    pub analyzers_surql: Option<String>,
}

/// Missing keys, and a missing table, take the values of [`Default`].
#[derive(Debug, Clone, Deserialize, Serialize, Builder)]
#[serde(default)]
pub struct SchemasyncMockGenConfig {
    /// overriden by table level  configs
    pub default_record_count: usize,

    /// overriden by table level and field level configs
    pub default_preservation_mode: PreservationMode,

    /// overriden by table level and field level configs
    pub default_batch_size: usize,

    #[builder(default)]
    /// global table field coordination, Vec<(BTreeSet<TableName>, Vec<Coordination>)>
    pub coordination_groups: Vec<CoordinationGroup>,

    pub full_refresh_mode: bool,

    /// Controls how field validators that have no native SurrealQL equivalent
    /// (credit-card/Luhn, JSON parseability, Unicode normalization, finiteness,
    /// capitalization) are turned into `DEFINE FIELD ... ASSERT` clauses.
    ///
    /// When `true` (the default) they are emitted as embedded-JavaScript
    /// `ASSERT function($value) { ... }` clauses, which require the SurrealDB
    /// server to run with `--allow-scripting`. When `false`, those validators
    /// contribute no assertion (every native assertion is still emitted), so
    /// the generated schema remains applicable on servers without scripting.
    #[builder(default = true)]
    pub scripting_asserts: bool,
}

impl Default for SchemasyncMockGenConfig {
    fn default() -> Self {
        Self {
            default_record_count: 10,
            default_preservation_mode: PreservationMode::default(),
            default_batch_size: 1000,
            coordination_groups: Vec::new(),
            full_refresh_mode: false,
            scripting_asserts: true,
        }
    }
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            provider: DatabaseProvider::default(),
            url: String::new(),
            namespace: String::new(),
            database: String::new(),
            timeout: default_timeout(),
            accesses: AccessesSource::default(),
            functions: None,
            analyzers: None,
            resolved: ResolvedDatabaseItems::default(),
            max_connections: None,
            min_connections: None,
            schema: None,
        }
    }
}

/// Command-line overrides for mock data generation. Each one only switches
/// a setting on, whatever the config says.
#[derive(Debug, Clone, Default)]
pub struct MockOverrides {
    /// Skip mock data generation.
    pub skip_mocks: bool,
    /// Delete existing data and regenerate everything.
    pub full_refresh: bool,
}

impl SchemasyncConfig {
    /// Apply the settings that `overrides` switches on.
    pub fn apply_mock_overrides(&mut self, overrides: &MockOverrides) {
        if overrides.skip_mocks {
            self.should_generate_mocks = false;
        }
        if overrides.full_refresh {
            self.mock_gen_config.full_refresh_mode = true;
        }
    }
}

/// Command-line replacements for the database connection settings.
#[derive(Debug, Clone, Default)]
pub struct ConnectionOverrides {
    pub url: Option<String>,
    pub namespace: Option<String>,
    pub database: Option<String>,
}

impl DatabaseConfig {
    /// Replace the connection settings that `overrides` provides.
    pub fn apply_connection_overrides(&mut self, overrides: &ConnectionOverrides) {
        if let Some(url) = &overrides.url {
            self.url = url.clone();
        }
        if let Some(namespace) = &overrides.namespace {
            self.namespace = namespace.clone();
        }
        if let Some(database) = &overrides.database {
            self.database = database.clone();
        }
    }

    /// The first environment variable still referenced (`${VAR}`) by the
    /// connection settings, i.e. one that was unset when the config was
    /// loaded offline and wasn't replaced by an override.
    pub fn unresolved_connection_var(&self) -> Option<String> {
        [&self.url, &self.namespace, &self.database]
            .into_iter()
            .find_map(|value| {
                let start = value.find("${")? + 2;
                let name: String = value[start..]
                    .chars()
                    .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
                    .collect();
                (!name.is_empty()).then_some(name)
            })
    }

    /// Creates a database configuration suitable for testing with SurrealDB
    pub fn for_testing() -> Self {
        debug!("Creating database configuration for testing environment");
        let config = Self {
            provider: DatabaseProvider::Surrealdb,
            url: "http://localhost:8000".to_string(),
            namespace: "test".to_string(),
            database: "test".to_string(),
            accesses: AccessesSource::Inline(vec![AccessConfig {
                name: "user".to_owned(),
                access_type: AccessType::Record,
                table_name: "user".to_owned(),
            }]),
            functions: None,
            analyzers: None,
            resolved: ResolvedDatabaseItems::default(),
            timeout: 60,
            max_connections: None,
            min_connections: None,
            schema: None,
        };
        trace!(
            "Test database config - URL: {}, namespace: {}, database: {}, timeout: {}s",
            config.url, config.namespace, config.database, config.timeout
        );
        if let AccessesSource::Inline(ref accesses) = config.accesses {
            trace!("Test access configs: {} entries", accesses.len());
        }
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
            accesses: AccessesSource::default(),
            functions: None,
            analyzers: None,
            resolved: ResolvedDatabaseItems::default(),
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
            accesses: AccessesSource::default(),
            functions: None,
            analyzers: None,
            resolved: ResolvedDatabaseItems::default(),
            max_connections: Some(1),
            min_connections: Some(1),
            schema: None,
        }
    }

    /// Convert to provider-specific DatabaseConfig
    #[cfg(feature = "schemasync")]
    pub fn to_provider_config(&self) -> crate::schemasync::database::DatabaseConfig {
        crate::schemasync::database::DatabaseConfig {
            provider: match self.provider {
                DatabaseProvider::Surrealdb => crate::schemasync::database::ProviderType::SurrealDb,
                DatabaseProvider::Postgres => crate::schemasync::database::ProviderType::Postgres,
                DatabaseProvider::Mysql => crate::schemasync::database::ProviderType::MySql,
                DatabaseProvider::Sqlite => crate::schemasync::database::ProviderType::Sqlite,
            },
            url: self.url.clone(),
            namespace: if self.namespace.is_empty() {
                None
            } else {
                Some(self.namespace.clone())
            },
            database: if self.database.is_empty() {
                None
            } else {
                Some(self.database.clone())
            },
            username: None, // Will be loaded from env vars
            password: None, // Will be loaded from env vars
            max_connections: self.max_connections,
            min_connections: self.min_connections,
            schema: self.schema.clone(),
            timeout_secs: self.timeout,
        }
    }
}

/// Missing keys, and a missing table, take the values of [`Default`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_overrides_only_switch_settings_on() {
        let mut config: SchemasyncConfig =
            toml::from_str("should_generate_mocks = true\n[database]\nurl = \"x\"\n").unwrap();

        config.apply_mock_overrides(&MockOverrides::default());
        assert!(config.should_generate_mocks);
        assert!(!config.mock_gen_config.full_refresh_mode);

        config.apply_mock_overrides(&MockOverrides {
            skip_mocks: true,
            full_refresh: true,
        });
        assert!(!config.should_generate_mocks);
        assert!(config.mock_gen_config.full_refresh_mode);
    }

    #[test]
    fn connection_overrides_replace_unresolved_settings() {
        let mut database = DatabaseConfig::for_testing();
        database.url = "${SURREALDB_URL}".to_string();
        database.namespace = "prefix_${SURREALDB_NS}".to_string();
        assert_eq!(
            database.unresolved_connection_var().as_deref(),
            Some("SURREALDB_URL")
        );

        database.apply_connection_overrides(&ConnectionOverrides {
            url: Some("http://localhost:8000".to_string()),
            namespace: None,
            database: None,
        });
        assert_eq!(database.url, "http://localhost:8000");
        assert_eq!(
            database.unresolved_connection_var().as_deref(),
            Some("SURREALDB_NS")
        );

        database.apply_connection_overrides(&ConnectionOverrides {
            url: None,
            namespace: Some("app".to_string()),
            database: Some("main".to_string()),
        });
        assert_eq!(database.unresolved_connection_var(), None);
        assert_eq!(database.database, "main");
    }
}
