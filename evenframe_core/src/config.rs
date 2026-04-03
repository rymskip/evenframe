use crate::error::{EvenframeError, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
};
use toml;
use tracing::{debug, error, info, trace, warn};

/// Configuration for a single foreign (external crate) type.
/// Defines how a Rust type from an external crate maps to each database
/// and TypeScript target.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ForeignTypeConfig {
    /// Source crate name (for documentation/provenance)
    #[serde(default, rename = "crate")]
    pub crate_name: String,

    /// Rust type names that map to this foreign type
    /// e.g., ["DateTime", "chrono::DateTime"]
    #[serde(default)]
    pub rust_type_names: Vec<String>,

    /// If true, generic params like <Utc> are ignored during parsing
    #[serde(default)]
    pub ignore_generic_params: bool,

    // --- Database schema mappings ---
    #[serde(default)]
    pub surrealdb: String,
    #[serde(default)]
    pub postgres: String,
    #[serde(default)]
    pub mysql: String,
    #[serde(default)]
    pub sqlite: String,

    /// SurrealDB format when field is `id`, e.g. "record<{table_name}>"
    #[serde(default)]
    pub surrealdb_id_format: Option<String>,
    /// SurrealDB format when field is NOT `id`, e.g. "record<any>"
    #[serde(default)]
    pub surrealdb_non_id_format: Option<String>,

    // --- TypeSync mappings ---
    #[serde(default)]
    pub arktype: String,
    #[serde(default)]
    pub effect_schema: String,
    /// The "encoded" representation for Effect TS type declarations
    #[serde(default)]
    pub effect_encoded: String,
    #[serde(default)]
    pub macroforge: String,
    #[serde(default)]
    pub flatbuffers: String,
    #[serde(default)]
    pub protobuf: String,
    /// The wire type for protobuf validation rules
    #[serde(default)]
    pub protobuf_wire_type: String,

    // --- Default values ---
    #[serde(default)]
    pub default_value_ts: String,
    #[serde(default)]
    pub default_value_surql: String,

    // --- SurrealQL value conversion strategy ---
    /// One of: "quoted_string", "datetime", "duration_from_nanos",
    ///         "decimal_number", "record_id", "passthrough"
    #[serde(default)]
    pub surql_value_format: String,

    // --- Mock data generation strategy ---
    /// One of: "datetime", "duration", "timezone", "decimal", "record_id", "string"
    #[serde(default)]
    pub mock_strategy: String,

    // --- Import resolution ---
    /// Macroforge import name (e.g., "DateTime", "BigDecimal"), empty = no import
    #[serde(default)]
    pub macroforge_import: String,
    /// Effect library import name, empty = no import
    #[serde(default)]
    pub effect_import: String,

    // --- Serde format annotation ---
    /// If set, generates `@serde({ format: "..." })` in macroforge output
    #[serde(default)]
    pub serde_format: String,
}

/// Source of truth for type definitions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SourceOfTruth {
    /// Rust structs with #[derive(Evenframe)] or #[apply(...)]
    #[default]
    Rust,
    /// FlatBuffers schema files (.fbs)
    Flatbuffers,
    /// Protocol Buffers schema files (.proto)
    Protobuf,
}

/// Configuration for schema source files (FlatBuffers/Protobuf)
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SourceConfig {
    /// Primary source of truth for type definitions
    #[serde(default)]
    pub primary: SourceOfTruth,

    /// Glob pattern for FlatBuffers schema files (e.g., "./schemas/*.fbs")
    #[serde(default)]
    pub flatbuffers_input: Option<String>,

    /// Glob pattern for Protocol Buffers schema files (e.g., "./schemas/*.proto")
    #[serde(default)]
    pub protobuf_input: Option<String>,

    /// Additional include paths for schema imports
    #[serde(default)]
    pub include_paths: Vec<String>,
}

/// General configuration for Evenframe operations
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct GeneralConfig {
    /// Attribute macro names that expand to include Evenframe derive
    /// These are used with #[apply(...)] and automatically include Evenframe
    #[serde(default)]
    pub apply_aliases: Vec<String>,

    /// Source of truth configuration
    #[serde(default)]
    pub source: SourceConfig,

    /// Path to the .env file, relative to the project root.
    /// Defaults to `.env` in the project root directory.
    #[serde(default)]
    pub env_path: Option<String>,

    /// Foreign type configurations, keyed by canonical type name.
    /// Defines how external Rust types map to database schemas and TypeScript types.
    #[serde(default)]
    pub foreign_types: HashMap<String, ForeignTypeConfig>,
}

/// Unified configuration for Evenframe operations
/// This is the root configuration that contains both schemasync and typesync configurations
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EvenframeConfig {
    /// General configuration
    #[serde(default)]
    pub general: GeneralConfig,
    /// Schema synchronization configuration (database operations)
    pub schemasync: crate::schemasync::config::SchemasyncConfig,
    /// Type synchronization configuration (TypeScript/Effect type generation)
    pub typesync: crate::typesync::config::TypesyncConfig,
    /// Path to the config file that was loaded (set at runtime, not from TOML)
    #[serde(skip)]
    pub config_file_path: PathBuf,
}

impl EvenframeConfig {
    /// Best-effort early .env load for use before full config is parsed.
    /// Finds the config file, derives the project root, and loads `.env` from there.
    /// Falls back to `dotenv::dotenv()` if config discovery fails.
    pub fn load_env_early() {
        if let Ok(config_path) = Self::find_config_file() {
            let parent = config_path.parent().unwrap_or(Path::new("."));
            let project_root = if parent.file_name().and_then(|n| n.to_str()) == Some(".evenframe")
            {
                parent.parent().unwrap_or(Path::new("."))
            } else {
                parent
            };
            let _ = dotenv::from_path(project_root.join(".env"));
        } else {
            let _ = dotenv::dotenv();
        }
    }

    /// Load configuration by searching for evenframe.toml in the current
    /// directory and its ancestors.
    pub fn new() -> Result<EvenframeConfig> {
        info!("Loading Evenframe configuration");

        let config_path = Self::find_config_file()?;
        info!("Found configuration file at: {:?}", config_path);

        let contents = fs::read_to_string(&config_path).map_err(|e| {
            error!("Failed to read configuration file: {}", e);
            EvenframeError::from(e)
        })?;

        debug!("Configuration file size: {} bytes", contents.len());

        let mut config: EvenframeConfig = toml::from_str(&contents).map_err(|e| {
            error!("Failed to parse TOML configuration: {}", e);
            EvenframeError::config(e.to_string())
        })?;

        debug!("Successfully parsed TOML configuration");

        // Store config file path early so project_root() works
        config.config_file_path = config_path;

        // Load .env file from configured or default path
        Self::load_env_file(&config);

        // Process environment variable substitutions for all string fields in the config
        debug!("Substituting environment variables in configuration");
        Self::substitute_all_env_vars(&mut config)?;

        // Resolve surql paths
        let project_root = config.project_root().to_path_buf();

        if let crate::schemasync::config::AccessesSource::Path { ref path } =
            config.schemasync.database.accesses
        {
            config.schemasync.database.resolved.access_surql =
                Some(Self::load_surql_from_path(&project_root, path)?);
        }

        if let Some(ref func) = config.schemasync.database.functions {
            config.schemasync.database.resolved.functions_surql =
                Some(Self::load_surql_from_path(&project_root, &func.path)?);
        }

        info!("Configuration loaded successfully");
        debug!(
            "Schemasync enabled: {}, Typesync arktype: {}, effect: {}, macroforge: {}",
            config.schemasync.should_generate_mocks,
            config.typesync.should_generate_arktype_types,
            config.typesync.should_generate_effect_types,
            config.typesync.should_generate_macroforge_types
        );

        Ok(config)
    }

    /// Searches for `.evenframe/config.toml` (preferred) or `evenframe.toml` (fallback)
    /// starting from the current directory and traversing up to the root.
    fn find_config_file() -> Result<PathBuf> {
        let current_dir = env::current_dir()?;
        debug!("Starting config file search from: {:?}", current_dir);

        for path in current_dir.ancestors() {
            // Check .evenframe/config.toml first (preferred location)
            let dotdir_config = path.join(".evenframe").join("config.toml");
            trace!("Checking for config at: {:?}", dotdir_config);
            if dotdir_config.exists() {
                return Ok(dotdir_config);
            }

            // Fall back to evenframe.toml (backwards compatible)
            let legacy_config = path.join("evenframe.toml");
            trace!("Checking for config at: {:?}", legacy_config);
            if legacy_config.exists() {
                return Ok(legacy_config);
            }
        }

        error!("Configuration file not found in any parent directory.");
        Err(EvenframeError::config(
            "Configuration file not found. Expected '.evenframe/config.toml' or 'evenframe.toml' in current or any parent directory.",
        ))
    }

    /// Returns the project root directory based on config file location.
    /// - For `evenframe.toml` → parent dir
    /// - For `.evenframe/config.toml` → grandparent dir
    pub fn project_root(&self) -> &Path {
        let parent = self.config_file_path.parent().unwrap_or(Path::new("."));
        if parent.file_name().and_then(|n| n.to_str()) == Some(".evenframe") {
            parent.parent().unwrap_or(Path::new("."))
        } else {
            parent
        }
    }

    /// Resolves the .env file path based on config.
    /// If `general.env_path` is set, resolves it relative to project root.
    /// Otherwise defaults to `<project_root>/.env`.
    pub fn resolve_env_path(&self) -> PathBuf {
        let project_root = self.project_root();
        match &self.general.env_path {
            Some(custom) => project_root.join(custom),
            None => project_root.join(".env"),
        }
    }

    /// Load environment variables from the .env file resolved from config.
    fn load_env_file(config: &EvenframeConfig) {
        let env_path = config.resolve_env_path();
        debug!("Loading environment variables from: {:?}", env_path);
        match dotenv::from_path(&env_path) {
            Ok(_) => info!("Loaded environment variables from {:?}", env_path),
            Err(e) => {
                if env_path.exists() {
                    warn!("Failed to load .env file {:?}: {}", env_path, e);
                } else {
                    debug!("No .env file found at {:?}, skipping", env_path);
                }
            }
        }
    }

    /// Load surql content from a file or directory path, with env var substitution.
    /// If the path points to a file, reads its contents.
    /// If it points to a directory, reads all `*.surql` files sorted by name and concatenates them.
    pub fn load_surql_from_path(project_root: &Path, relative_path: &str) -> Result<String> {
        let full_path = project_root.join(relative_path);
        debug!("Loading surql from path: {:?}", full_path);

        let content = if full_path.is_dir() {
            let mut entries: Vec<_> = fs::read_dir(&full_path)
                .map_err(|e| {
                    EvenframeError::config(format!(
                        "Failed to read directory {:?}: {}",
                        full_path, e
                    ))
                })?
                .filter_map(|entry| entry.ok())
                .filter(|entry| {
                    entry.path().extension().and_then(|ext| ext.to_str()) == Some("surql")
                })
                .collect();
            entries.sort_by_key(|e| e.file_name());

            if entries.is_empty() {
                return Err(EvenframeError::config(format!(
                    "No .surql files found in directory {:?}",
                    full_path
                )));
            }

            let mut combined = String::new();
            for entry in entries {
                let file_content = fs::read_to_string(entry.path()).map_err(|e| {
                    EvenframeError::config(format!("Failed to read {:?}: {}", entry.path(), e))
                })?;
                if !combined.is_empty() {
                    combined.push('\n');
                }
                combined.push_str(&file_content);
            }
            combined
        } else if full_path.is_file() {
            fs::read_to_string(&full_path).map_err(|e| {
                EvenframeError::config(format!("Failed to read {:?}: {}", full_path, e))
            })?
        } else {
            return Err(EvenframeError::config(format!(
                "Surql path does not exist: {:?}",
                full_path
            )));
        };

        Self::substitute_env_vars(&content)
    }
    /// Substitute environment variables across all string fields in the config.
    ///
    /// Serializes the config to TOML, applies env var substitution to the entire
    /// string, then deserializes back. Fields marked `#[serde(skip)]` (like
    /// `config_file_path` and `resolved`) are preserved across the round-trip.
    fn substitute_all_env_vars(config: &mut EvenframeConfig) -> Result<()> {
        let config_file_path = config.config_file_path.clone();
        let resolved = config.schemasync.database.resolved.clone();

        let toml_string = toml::to_string(&config).map_err(|e| {
            EvenframeError::config(format!(
                "Failed to serialize config for env var substitution: {e}"
            ))
        })?;

        let substituted = Self::substitute_env_vars(&toml_string)?;

        let mut new_config: EvenframeConfig = toml::from_str(&substituted).map_err(|e| {
            EvenframeError::config(format!(
                "Failed to re-parse config after env var substitution: {e}"
            ))
        })?;

        new_config.config_file_path = config_file_path;
        new_config.schemasync.database.resolved = resolved;

        *config = new_config;
        Ok(())
    }

    /// Substitute environment variables in config strings
    /// Supports ${VAR_NAME:-default} syntax
    pub fn substitute_env_vars(value: &str) -> Result<String> {
        trace!("Substituting environment variables in: {}", value);
        let mut result = value.to_string();

        // Pattern to match ${VAR_NAME} or ${VAR_NAME:-default}
        // Only matches valid env var names (uppercase letters, digits, underscores)
        // to avoid colliding with JS template literals like ${foo.bar()}
        let re = regex::Regex::new(r"\$\{([A-Z_][A-Z0-9_]*)(?::-([^}]*))?\}")
            .expect("Invalid regex for environment variable substitution");

        for cap in re.captures_iter(value) {
            let var_name = &cap[1];
            let default_value = cap.get(2).map(|m| m.as_str());

            trace!("Looking for environment variable: {}", var_name);

            let replacement = match env::var(var_name) {
                Ok(val) => {
                    debug!("Resolved environment variable: {}", var_name);
                    val
                }
                Err(_) => match default_value {
                    Some(default) => {
                        warn!(
                            "Environment variable {} not set, using default: {}",
                            var_name, default
                        );
                        default.to_string()
                    }
                    None => {
                        error!(
                            "Environment variable {} not set and no default provided",
                            var_name
                        );
                        return Err(EvenframeError::EnvVarNotSet(var_name.to_string()));
                    }
                },
            };

            let full_match = &cap[0];
            result = result.replace(full_match, &replacement);
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // ==================== GeneralConfig Tests ====================

    #[test]
    fn test_general_config_default() {
        let config = GeneralConfig::default();
        assert!(config.apply_aliases.is_empty());
    }

    #[test]
    fn test_general_config_deserialize_empty() {
        let toml_str = "";
        let config: GeneralConfig = toml::from_str(toml_str).unwrap_or_default();
        assert!(config.apply_aliases.is_empty());
    }

    #[test]
    fn test_general_config_deserialize_with_aliases() {
        let toml_str = r#"
            apply_aliases = ["MyAlias", "AnotherAlias"]
        "#;
        let config: GeneralConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.apply_aliases.len(), 2);
        assert_eq!(config.apply_aliases[0], "MyAlias");
        assert_eq!(config.apply_aliases[1], "AnotherAlias");
    }

    #[test]
    fn test_general_config_serialize() {
        let config = GeneralConfig {
            apply_aliases: vec!["Test".to_string()],
            source: SourceConfig::default(),
            env_path: None,
            ..Default::default()
        };
        let toml_str = toml::to_string(&config).unwrap();
        assert!(toml_str.contains("apply_aliases"));
        assert!(toml_str.contains("Test"));
    }

    // ==================== substitute_env_vars Tests ====================

    #[test]
    fn test_substitute_env_vars_basic() {
        temp_env::with_var("TEST_VAR_BASIC", Some("hello"), || {
            let result = EvenframeConfig::substitute_env_vars("${TEST_VAR_BASIC}").unwrap();
            assert_eq!(result, "hello");
        });
    }

    #[test]
    fn test_substitute_env_vars_with_surrounding_text() {
        temp_env::with_var("TEST_VAR_SURROUND", Some("world"), || {
            let result =
                EvenframeConfig::substitute_env_vars("hello ${TEST_VAR_SURROUND}!").unwrap();
            assert_eq!(result, "hello world!");
        });
    }

    #[test]
    fn test_substitute_env_vars_multiple() {
        temp_env::with_vars(
            [
                ("TEST_VAR_MULTI1", Some("foo")),
                ("TEST_VAR_MULTI2", Some("bar")),
            ],
            || {
                let result =
                    EvenframeConfig::substitute_env_vars("${TEST_VAR_MULTI1}:${TEST_VAR_MULTI2}")
                        .unwrap();
                assert_eq!(result, "foo:bar");
            },
        );
    }

    #[test]
    fn test_substitute_env_vars_no_match() {
        let result = EvenframeConfig::substitute_env_vars("no variables here").unwrap();
        assert_eq!(result, "no variables here");
    }

    #[test]
    fn test_substitute_env_vars_empty_string() {
        let result = EvenframeConfig::substitute_env_vars("").unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_substitute_env_vars_url_pattern() {
        temp_env::with_var("TEST_DB_URL", Some("http://localhost:8000"), || {
            let result = EvenframeConfig::substitute_env_vars("${TEST_DB_URL}").unwrap();
            assert_eq!(result, "http://localhost:8000");
        });
    }

    #[test]
    fn test_substitute_env_vars_missing_returns_error() {
        // Clear the env var to make sure it doesn't exist
        // SAFETY: This is a test environment where we control access to env vars
        unsafe {
            std::env::remove_var("DEFINITELY_NOT_SET_VAR_12345");
        }
        let result = EvenframeConfig::substitute_env_vars("${DEFINITELY_NOT_SET_VAR_12345}");
        assert!(result.is_err());
    }

    #[test]
    fn test_substitute_env_vars_with_underscores() {
        temp_env::with_var("TEST_VAR_WITH_UNDERSCORES", Some("value"), || {
            let result =
                EvenframeConfig::substitute_env_vars("${TEST_VAR_WITH_UNDERSCORES}").unwrap();
            assert_eq!(result, "value");
        });
    }

    #[test]
    fn test_substitute_env_vars_with_numbers() {
        temp_env::with_var("TEST_VAR_123", Some("num_value"), || {
            let result = EvenframeConfig::substitute_env_vars("${TEST_VAR_123}").unwrap();
            assert_eq!(result, "num_value");
        });
    }

    #[test]
    fn test_substitute_env_vars_adjacent() {
        temp_env::with_vars([("TEST_ADJ1", Some("a")), ("TEST_ADJ2", Some("b"))], || {
            let result = EvenframeConfig::substitute_env_vars("${TEST_ADJ1}${TEST_ADJ2}").unwrap();
            assert_eq!(result, "ab");
        });
    }

    #[test]
    fn test_substitute_env_vars_preserves_non_matching_braces() {
        let result = EvenframeConfig::substitute_env_vars("{not_a_var}").unwrap();
        assert_eq!(result, "{not_a_var}");
    }

    // ==================== find_config_file Tests ====================
    // NOTE: These tests that use env::set_current_dir should be run with --test-threads=1
    // to avoid race conditions. They are marked with #[ignore] for parallel runs.

    #[test]
    #[ignore = "requires --test-threads=1 due to env::set_current_dir"]
    fn test_find_config_file_not_found() {
        // Create a temp directory without evenframe.toml
        let temp_dir = TempDir::new().unwrap();
        let original_dir = env::current_dir().unwrap();

        // Change to temp directory
        env::set_current_dir(temp_dir.path()).unwrap();

        let result = EvenframeConfig::find_config_file();

        // Restore original directory
        env::set_current_dir(original_dir).unwrap();

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Configuration file not found"));
    }

    #[test]
    #[ignore = "requires --test-threads=1 due to env::set_current_dir"]
    fn test_find_config_file_in_current_dir() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("evenframe.toml");
        fs::write(&config_path, "# test config").unwrap();
        // Canonicalize before changing directory
        let expected_canonical = config_path.canonicalize().unwrap();

        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(temp_dir.path()).unwrap();

        let result = EvenframeConfig::find_config_file();

        env::set_current_dir(original_dir).unwrap();

        assert!(result.is_ok());
        // Use canonicalize to handle symlinks (e.g., /var -> /private/var on macOS)
        let result_canonical = result.unwrap().canonicalize().unwrap();
        assert_eq!(result_canonical, expected_canonical);
    }

    #[test]
    #[ignore = "requires --test-threads=1 due to env::set_current_dir"]
    fn test_find_config_file_in_parent_dir() {
        let temp_dir = TempDir::new().unwrap();
        let child_dir = temp_dir.path().join("child");
        fs::create_dir(&child_dir).unwrap();

        let config_path = temp_dir.path().join("evenframe.toml");
        fs::write(&config_path, "# test config").unwrap();
        // Canonicalize before changing directory
        let expected_canonical = config_path.canonicalize().unwrap();

        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(&child_dir).unwrap();

        let result = EvenframeConfig::find_config_file();

        env::set_current_dir(original_dir).unwrap();

        assert!(result.is_ok());
        // Use canonicalize to handle symlinks
        let result_canonical = result.unwrap().canonicalize().unwrap();
        assert_eq!(result_canonical, expected_canonical);
    }

    #[test]
    #[ignore = "requires --test-threads=1 due to env::set_current_dir"]
    fn test_find_config_file_in_grandparent_dir() {
        let temp_dir = TempDir::new().unwrap();
        let child_dir = temp_dir.path().join("child");
        let grandchild_dir = child_dir.join("grandchild");
        fs::create_dir_all(&grandchild_dir).unwrap();

        let config_path = temp_dir.path().join("evenframe.toml");
        fs::write(&config_path, "# test config").unwrap();
        // Canonicalize before changing directory
        let expected_canonical = config_path.canonicalize().unwrap();

        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(&grandchild_dir).unwrap();

        let result = EvenframeConfig::find_config_file();

        env::set_current_dir(original_dir).unwrap();

        assert!(result.is_ok());
        // Use canonicalize to handle symlinks
        let result_canonical = result.unwrap().canonicalize().unwrap();
        assert_eq!(result_canonical, expected_canonical);
    }

    // ==================== .evenframe/config.toml Discovery Tests ====================

    #[test]
    #[ignore = "requires --test-threads=1 due to env::set_current_dir"]
    fn test_find_config_dotdir_preferred_over_legacy() {
        let temp_dir = TempDir::new().unwrap();

        // Create both config files
        fs::write(temp_dir.path().join("evenframe.toml"), "# legacy").unwrap();
        let dotdir = temp_dir.path().join(".evenframe");
        fs::create_dir(&dotdir).unwrap();
        let dotdir_config = dotdir.join("config.toml");
        fs::write(&dotdir_config, "# preferred").unwrap();
        let expected_canonical = dotdir_config.canonicalize().unwrap();

        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(temp_dir.path()).unwrap();

        let result = EvenframeConfig::find_config_file();

        env::set_current_dir(original_dir).unwrap();

        assert!(result.is_ok());
        let result_canonical = result.unwrap().canonicalize().unwrap();
        assert_eq!(result_canonical, expected_canonical);
    }

    #[test]
    #[ignore = "requires --test-threads=1 due to env::set_current_dir"]
    fn test_find_config_only_dotdir() {
        let temp_dir = TempDir::new().unwrap();

        let dotdir = temp_dir.path().join(".evenframe");
        fs::create_dir(&dotdir).unwrap();
        let dotdir_config = dotdir.join("config.toml");
        fs::write(&dotdir_config, "# only dotdir").unwrap();
        let expected_canonical = dotdir_config.canonicalize().unwrap();

        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(temp_dir.path()).unwrap();

        let result = EvenframeConfig::find_config_file();

        env::set_current_dir(original_dir).unwrap();

        assert!(result.is_ok());
        let result_canonical = result.unwrap().canonicalize().unwrap();
        assert_eq!(result_canonical, expected_canonical);
    }

    #[test]
    fn test_project_root_for_legacy_config() {
        let config = EvenframeConfig {
            general: GeneralConfig::default(),
            schemasync: toml::from_str(
                r#"
                should_generate_mocks = false
                [database]
                provider = "surrealdb"
                url = ""
                namespace = ""
                database = ""
                [mock_gen_config]
                default_record_count = 10
                default_preservation_mode = "Smart"
                default_batch_size = 10
                full_refresh_mode = false
                coordination_groups = []
                [performance]
                embedded_db_memory_limit = "256MB"
                cache_duration_seconds = 60
                use_progressive_loading = false
                "#,
            )
            .unwrap(),
            typesync: toml::from_str(
                r#"
                output_path = "./"
                should_generate_arktype_types = false
                should_generate_effect_types = false
                should_generate_macroforge_types = false
                should_generate_surrealdb_schemas = false
                "#,
            )
            .unwrap(),
            config_file_path: PathBuf::from("/project/evenframe.toml"),
        };
        assert_eq!(config.project_root(), Path::new("/project"));
    }

    #[test]
    fn test_project_root_for_dotdir_config() {
        let config = EvenframeConfig {
            general: GeneralConfig::default(),
            schemasync: toml::from_str(
                r#"
                should_generate_mocks = false
                [database]
                provider = "surrealdb"
                url = ""
                namespace = ""
                database = ""
                [mock_gen_config]
                default_record_count = 10
                default_preservation_mode = "Smart"
                default_batch_size = 10
                full_refresh_mode = false
                coordination_groups = []
                [performance]
                embedded_db_memory_limit = "256MB"
                cache_duration_seconds = 60
                use_progressive_loading = false
                "#,
            )
            .unwrap(),
            typesync: toml::from_str(
                r#"
                output_path = "./"
                should_generate_arktype_types = false
                should_generate_effect_types = false
                should_generate_macroforge_types = false
                should_generate_surrealdb_schemas = false
                "#,
            )
            .unwrap(),
            config_file_path: PathBuf::from("/project/.evenframe/config.toml"),
        };
        assert_eq!(config.project_root(), Path::new("/project"));
    }

    // ==================== load_surql_from_path Tests ====================

    #[test]
    fn test_load_surql_from_file() {
        let temp_dir = TempDir::new().unwrap();
        let surql_path = temp_dir.path().join("test.surql");
        fs::write(&surql_path, "DEFINE FUNCTION fn::test() { RETURN 1; };").unwrap();

        let result = EvenframeConfig::load_surql_from_path(temp_dir.path(), "test.surql");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "DEFINE FUNCTION fn::test() { RETURN 1; };");
    }

    #[test]
    fn test_load_surql_from_directory() {
        let temp_dir = TempDir::new().unwrap();
        let surql_dir = temp_dir.path().join("surql");
        fs::create_dir(&surql_dir).unwrap();
        fs::write(surql_dir.join("01_first.surql"), "-- first").unwrap();
        fs::write(surql_dir.join("02_second.surql"), "-- second").unwrap();
        // Non-surql file should be ignored
        fs::write(surql_dir.join("readme.txt"), "ignore me").unwrap();

        let result = EvenframeConfig::load_surql_from_path(temp_dir.path(), "surql");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "-- first\n-- second");
    }

    #[test]
    fn test_load_surql_nonexistent_path() {
        let temp_dir = TempDir::new().unwrap();
        let result = EvenframeConfig::load_surql_from_path(temp_dir.path(), "nonexistent.surql");
        assert!(result.is_err());
    }

    #[test]
    fn test_load_surql_with_env_var_substitution() {
        let temp_dir = TempDir::new().unwrap();
        let surql_path = temp_dir.path().join("test.surql");
        fs::write(&surql_path, "DEFINE ACCESS test ON DATABASE TYPE JWT ALGORITHM HS256 KEY '${TEST_SURQL_KEY:-default_key}';").unwrap();

        let result = EvenframeConfig::load_surql_from_path(temp_dir.path(), "test.surql");
        assert!(result.is_ok());
        assert!(result.unwrap().contains("default_key"));
    }

    // ==================== EvenframeConfig Serialization Tests ====================

    #[test]
    fn test_evenframe_config_deserialize_minimal() {
        let toml_str = r#"
            [schemasync]
            should_generate_mocks = false

            [schemasync.database]
            provider = "surrealdb"
            url = "http://localhost:8000"
            namespace = "test"
            database = "test"

            [schemasync.mock_gen_config]
            default_record_count = 10
            default_preservation_mode = "Smart"
            default_batch_size = 10
            full_refresh_mode = false
            coordination_groups = []

            [schemasync.performance]
            embedded_db_memory_limit = "256MB"
            cache_duration_seconds = 60
            use_progressive_loading = false

            [typesync]
            output_path = "./generated/"
            should_generate_arktype_types = false
            should_generate_effect_types = false
            should_generate_macroforge_types = false
            should_generate_surrealdb_schemas = false
        "#;

        let config: EvenframeConfig = toml::from_str(toml_str).unwrap();
        assert!(config.general.apply_aliases.is_empty()); // Default
        assert_eq!(config.schemasync.database.url, "http://localhost:8000");
        assert_eq!(config.typesync.output_path, "./generated/");
    }

    #[test]
    fn test_evenframe_config_deserialize_with_general() {
        let toml_str = r#"
            [general]
            apply_aliases = ["MyAlias"]

            [schemasync]
            should_generate_mocks = true

            [schemasync.database]
            provider = "surrealdb"
            url = "http://localhost:8000"
            namespace = "test"
            database = "test"

            [schemasync.mock_gen_config]
            default_record_count = 100
            default_preservation_mode = "Smart"
            default_batch_size = 50
            full_refresh_mode = false
            coordination_groups = []

            [schemasync.performance]
            embedded_db_memory_limit = "1GB"
            cache_duration_seconds = 300
            use_progressive_loading = true

            [typesync]
            output_path = "./types/"
            should_generate_arktype_types = true
            should_generate_effect_types = true
            should_generate_macroforge_types = false
            should_generate_surrealdb_schemas = true
        "#;

        let config: EvenframeConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.general.apply_aliases.len(), 1);
        assert_eq!(config.general.apply_aliases[0], "MyAlias");
        assert!(config.schemasync.should_generate_mocks);
        assert!(config.typesync.should_generate_arktype_types);
    }

    // ==================== EvenframeConfig::new() Integration Tests ====================
    // NOTE: These tests use env::set_current_dir and should be run with --test-threads=1

    #[test]
    #[ignore = "requires --test-threads=1 due to env::set_current_dir"]
    fn test_evenframe_config_new_with_valid_config() {
        let temp_dir = TempDir::new().unwrap();

        // Create a valid evenframe.toml
        let config_content = r#"
            [schemasync]
            should_generate_mocks = false

            [schemasync.database]
            provider = "surrealdb"
            url = "http://localhost:8000"
            namespace = "test_ns"
            database = "test_db"

            [schemasync.mock_gen_config]
            default_record_count = 10
            default_preservation_mode = "Smart"
            default_batch_size = 10
            full_refresh_mode = false
            coordination_groups = []

            [schemasync.performance]
            embedded_db_memory_limit = "256MB"
            cache_duration_seconds = 60
            use_progressive_loading = false

            [typesync]
            output_path = "./output/"
            should_generate_arktype_types = false
            should_generate_effect_types = false
            should_generate_macroforge_types = false
            should_generate_surrealdb_schemas = false
        "#;
        fs::write(temp_dir.path().join("evenframe.toml"), config_content).unwrap();

        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(temp_dir.path()).unwrap();

        let result = EvenframeConfig::new();

        env::set_current_dir(original_dir).unwrap();

        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(config.schemasync.database.namespace, "test_ns");
        assert_eq!(config.schemasync.database.database, "test_db");
    }

    #[test]
    #[ignore = "requires --test-threads=1 due to env::set_current_dir"]
    fn test_evenframe_config_new_invalid_toml() {
        let temp_dir = TempDir::new().unwrap();

        // Create invalid TOML
        fs::write(
            temp_dir.path().join("evenframe.toml"),
            "invalid toml content {{{",
        )
        .unwrap();

        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(temp_dir.path()).unwrap();

        let result = EvenframeConfig::new();

        env::set_current_dir(original_dir).unwrap();

        assert!(result.is_err());
    }

    #[test]
    #[ignore = "requires --test-threads=1 due to env::set_current_dir"]
    fn test_evenframe_config_new_missing_required_fields() {
        let temp_dir = TempDir::new().unwrap();

        // Create TOML missing required fields
        let config_content = r#"
            [general]
            apply_aliases = []
        "#;
        fs::write(temp_dir.path().join("evenframe.toml"), config_content).unwrap();

        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(temp_dir.path()).unwrap();

        let result = EvenframeConfig::new();

        env::set_current_dir(original_dir).unwrap();

        // Should fail due to missing schemasync and typesync sections
        assert!(result.is_err());
    }
}
