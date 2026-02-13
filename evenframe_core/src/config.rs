use crate::error::{EvenframeError, Result};
use serde::{Deserialize, Serialize};
use std::{env, fs, path::PathBuf};
use toml;
use tracing::{debug, error, info, trace, warn};

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
}

impl EvenframeConfig {
    /// Load configuration by searching for evenframe.toml in the current
    /// directory and its ancestors.
    pub fn new() -> Result<EvenframeConfig> {
        info!("Loading Evenframe configuration");
        dotenv::dotenv().ok();
        debug!("Environment variables loaded from .env if present");

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

        // Process environment variable substitutions for all database-related fields
        // TODO: This should find all environment variable references in the config, not just these hardcoded ones
        debug!("Substituting environment variables in configuration");
        config.schemasync.database.url = Self::substitute_env_vars(&config.schemasync.database.url)?;
        config.schemasync.database.namespace =
            Self::substitute_env_vars(&config.schemasync.database.namespace)?;
        config.schemasync.database.database =
            Self::substitute_env_vars(&config.schemasync.database.database)?;
        config.typesync.output_path = Self::substitute_env_vars(&config.typesync.output_path)?;

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

    /// Searches for `evenframe.toml` starting from the current directory
    /// and traversing up to the root.
    fn find_config_file() -> Result<PathBuf> {
        let current_dir = env::current_dir()?;
        debug!("Starting config file search from: {:?}", current_dir);

        for path in current_dir.ancestors() {
            let config_path = path.join("evenframe.toml");
            trace!("Checking for config at: {:?}", config_path);
            if config_path.exists() {
                return Ok(config_path);
            }
        }

        error!("Configuration file 'evenframe.toml' not found in any parent directory.");
        Err(EvenframeError::config(
            "evenframe.toml not found in current or any parent directory.",
        ))
    }
    /// Substitute environment variables in config strings
    /// Supports ${VAR_NAME:-default} syntax
    fn substitute_env_vars(value: &str) -> Result<String> {
        trace!("Substituting environment variables in: {}", value);
        let mut result = value.to_string();

        // Pattern to match ${VAR_NAME} or ${VAR_NAME:-default}
        let re = regex::Regex::new(r"\$\{([^}:]+)(?::-([^}]*))?\}")
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
            let result = EvenframeConfig::substitute_env_vars("hello ${TEST_VAR_SURROUND}!").unwrap();
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
                    EvenframeConfig::substitute_env_vars("${TEST_VAR_MULTI1}:${TEST_VAR_MULTI2}").unwrap();
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
            let result = EvenframeConfig::substitute_env_vars("${TEST_VAR_WITH_UNDERSCORES}").unwrap();
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
        temp_env::with_vars(
            [
                ("TEST_ADJ1", Some("a")),
                ("TEST_ADJ2", Some("b")),
            ],
            || {
                let result = EvenframeConfig::substitute_env_vars("${TEST_ADJ1}${TEST_ADJ2}").unwrap();
                assert_eq!(result, "ab");
            },
        );
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
        assert!(err.to_string().contains("evenframe.toml not found"));
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
