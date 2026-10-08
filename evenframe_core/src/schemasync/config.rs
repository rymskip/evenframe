use crate::schemasync::PreservationMode;
use bon::Builder;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tracing::{debug, trace};

/// The database representation of a Rust `Option::None`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OptionNone {
    #[default]
    None,
    Null,
}

impl OptionNone {
    pub fn literal(self) -> &'static str {
        match self {
            Self::None => "NONE",
            Self::Null => "NULL",
        }
    }

    pub fn surql_type(self, inner: &str) -> String {
        match self {
            Self::None => format!("option<{inner}>"),
            Self::Null => format!("null | {inner}"),
        }
    }

    /// Applies the configured absence representation to a typed SDK value.
    #[cfg(feature = "surrealdb-types")]
    pub fn into_value<Item: surrealdb_types::SurrealValue>(
        self,
        item: Item,
    ) -> surrealdb_types::Value {
        let mut value = item.into_value();
        if self == Self::Null {
            Self::null_for_none(&mut value);
        }
        value
    }

    #[cfg(feature = "surrealdb-types")]
    fn null_for_none(value: &mut surrealdb_types::Value) {
        match value {
            surrealdb_types::Value::None => *value = surrealdb_types::Value::Null,
            surrealdb_types::Value::Array(items) => items.iter_mut().for_each(Self::null_for_none),
            surrealdb_types::Value::Object(fields) => {
                fields.values_mut().for_each(Self::null_for_none)
            }
            _ => {}
        }
    }

    /// Reads the configured absence representation using the SDK type's shape.
    #[cfg(feature = "surrealdb-types")]
    pub fn read_value<Item: surrealdb_types::SurrealValue>(
        self,
        mut value: surrealdb_types::Value,
    ) -> Result<Item, surrealdb_types::Error> {
        if self == Self::Null {
            Self::optional_nulls(&mut value, &Item::kind_of());
        }
        Item::from_value(value).map_err(|failure| {
            surrealdb_types::Error::serialization(
                format!("reading {} absence policy: {failure}", self.literal()),
                surrealdb_types::SerializationError::Deserialization,
            )
        })
    }

    #[cfg(feature = "surrealdb-types")]
    fn optional_nulls(value: &mut surrealdb_types::Value, kind: &surrealdb_types::Kind) {
        use surrealdb_types::{Kind, KindLiteral, Value};
        if matches!(value, Value::Null)
            && matches!(kind, Kind::Either(candidates) if candidates.contains(&Kind::None))
        {
            *value = Value::None;
            return;
        }
        match (kind, value) {
            (Kind::None, absent @ Value::Null) => *absent = Value::None,
            (Kind::Either(candidates), value) => {
                for candidate in candidates {
                    let mut converted = value.clone();
                    Self::optional_nulls(&mut converted, candidate);
                    if converted.is_kind(candidate) {
                        *value = converted;
                        break;
                    }
                }
            }
            (Kind::Array(inner, _) | Kind::Set(inner, _), Value::Array(items)) => {
                for item in items.iter_mut() {
                    Self::optional_nulls(item, inner);
                }
            }
            (Kind::Literal(KindLiteral::Array(kinds)), Value::Array(items)) => {
                for (item, kind) in items.iter_mut().zip(kinds) {
                    Self::optional_nulls(item, kind);
                }
            }
            (Kind::Literal(KindLiteral::Object(kinds)), Value::Object(fields)) => {
                for (name, kind) in kinds {
                    if let Some(field) = fields.get_mut(name) {
                        Self::optional_nulls(field, kind);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Options shared by schema and database-default rendering.
#[derive(Debug, Clone, Copy, Default)]
pub struct SurqlOptions {
    pub allow_scripting: bool,
    pub option_none: OptionNone,
}

#[derive(Clone, Copy)]
pub struct SurqlContext<'registry> {
    pub registry: &'registry crate::types::ForeignTypeRegistry,
    pub options: SurqlOptions,
}

impl<'registry> From<&'registry crate::types::ForeignTypeRegistry> for SurqlContext<'registry> {
    fn from(registry: &'registry crate::types::ForeignTypeRegistry) -> Self {
        Self {
            registry,
            options: SurqlOptions::default(),
        }
    }
}

impl From<bool> for SurqlOptions {
    fn from(allow_scripting: bool) -> Self {
        Self {
            allow_scripting,
            ..Self::default()
        }
    }
}

/// Configuration for a WASM mock data plugin.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct SchemasyncConfig {
    /// Database connection configuration
    pub database: DatabaseConfig,
    /// Whether to generate mock data
    pub should_generate_mocks: bool,
    /// The absence representation used by generated schema and mock values.
    #[serde(default)]
    #[builder(default)]
    pub option_none: OptionNone,
    /// default mock data generation configuration, overridden by table and field level configs
    #[serde(default)]
    pub mock_gen_config: SchemasyncMockGenConfig,
    /// WASM plugin definitions for mock data generation.
    #[serde(default)]
    #[builder(default)]
    pub plugins: BTreeMap<String, PluginConfig>,
    /// Lint pass configuration
    #[serde(default)]
    #[builder(default)]
    pub lint: LintConfig,
    /// Warn about each table defined SCHEMALESS because its record holds keys
    /// known only from a value, such as a flattened map's.
    #[serde(default)]
    #[builder(default)]
    pub warn_schemaless: bool,
    /// The project root of the config this came from, which `plugins` paths
    /// are relative to (set at load, not from TOML).
    #[serde(skip)]
    #[builder(default)]
    pub project_root: std::path::PathBuf,
}

/// Configuration for the schemasync lint pass, under `[schemasync.lint]`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LintConfig {
    /// Silence warnings for `#[define_field_statement(...)]` settings whose
    /// effect cannot be determined from this run's data: settings on a
    /// `resolve_only` type that has an `id` field. This run inlines such a
    /// type (discarding the settings), but the project that owns it may
    /// materialize the table and honor them.
    #[serde(default)]
    pub silence_unverifiable_annotations: bool,
}

/// The database schemasync works with. Only SurrealDB is supported; SQL
/// databases are in development on the `wip/sql-providers` branch.
#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DatabaseProvider {
    #[default]
    Surrealdb,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseConfig {
    /// The database kind; only `surrealdb`.
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
}

fn default_timeout() -> u64 {
    60
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
pub struct FunctionsSource {
    pub path: String,
}

/// Source for analyzer definitions: a path to .surql file(s).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
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
#[serde(deny_unknown_fields)]
#[serde(default)]
pub struct SchemasyncMockGenConfig {
    /// overriden by table level  configs
    pub default_record_count: usize,

    /// overriden by table level and field level configs
    pub default_preservation_mode: PreservationMode,

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
    pub fn surql_options(&self) -> SurqlOptions {
        SurqlOptions {
            allow_scripting: self.mock_gen_config.scripting_asserts,
            option_none: self.option_none,
        }
    }

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
}

#[cfg(test)]
mod tests {
    use super::{ConnectionOverrides, DatabaseConfig, MockOverrides, OptionNone, SchemasyncConfig};

    #[test]
    fn optional_absence_defaults_to_none_and_accepts_null() {
        let defaults: SchemasyncConfig =
            toml::from_str("should_generate_mocks = false\n[database]\nurl = \"x\"\n").unwrap();
        assert_eq!(defaults.option_none, OptionNone::None);
        let nullable: SchemasyncConfig = toml::from_str(
            "should_generate_mocks = false\noption_none = \"null\"\n[database]\nurl = \"x\"\n",
        )
        .unwrap();
        assert_eq!(nullable.option_none, OptionNone::Null);
        assert_eq!(nullable.surql_options().option_none, OptionNone::Null);
        assert!(
            toml::from_str::<SchemasyncConfig>(
                "should_generate_mocks = false\noption_none = \"empty\"\n[database]\nurl = \"x\"\n",
            )
            .is_err()
        );
    }

    #[cfg(feature = "surrealdb-types")]
    #[test]
    fn null_policy_translates_nested_typed_option_values() {
        use surrealdb_types::{SurrealValue, Value};
        let original = vec![Some(7_i32), None].into_value();
        assert_eq!(OptionNone::None.into_value(original.clone()), original);
        let converted = OptionNone::Null.into_value(original);
        assert_eq!(
            converted,
            vec![Value::from_t(7_i32), Value::Null].into_value()
        );
        assert_eq!(
            OptionNone::Null
                .read_value::<Vec<Option<i32>>>(converted)
                .unwrap(),
            vec![Some(7), None]
        );
        assert_eq!(
            OptionNone::Null.read_value::<Value>(Value::Null).unwrap(),
            Value::Null
        );
        assert_eq!(
            OptionNone::Null
                .read_value::<Option<Value>>(Value::Null)
                .unwrap(),
            None
        );
        let nested = (vec![None::<Value>], Value::Null);
        assert_eq!(
            OptionNone::Null
                .read_value::<(Vec<Option<Value>>, Value)>(
                    OptionNone::Null.into_value(nested.clone())
                )
                .unwrap(),
            nested
        );
    }

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
