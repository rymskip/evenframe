//! Build-time configuration for type generation.

use crate::error::EvenframeError;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Configuration for build-time type generation.
#[derive(Debug, Clone)]
pub struct BuildConfig {
    /// Root path to scan for Rust types.
    pub scan_path: PathBuf,

    /// Output directory for generated files.
    pub output_path: PathBuf,

    /// Apply aliases for attribute detection (e.g., custom derive macros).
    pub apply_aliases: Vec<String>,

    /// Generate ArkType schema.
    pub arktype: bool,

    /// Generate Effect-TS schema.
    pub effect: bool,

    /// Generate Macroforge types.
    pub macroforge: bool,

    /// Generate FlatBuffers schema.
    pub flatbuffers: bool,

    /// Generate Protocol Buffers schema.
    pub protobuf: bool,

    /// FlatBuffers namespace (e.g., "com.example.app").
    pub flatbuffers_namespace: Option<String>,

    /// Protocol Buffers package name (e.g., "com.example.app").
    pub protobuf_package: Option<String>,

    /// Whether to import validate.proto for Protocol Buffers.
    pub protobuf_import_validate: bool,
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            scan_path: PathBuf::from("."),
            output_path: PathBuf::from("./src/generated/"),
            apply_aliases: Vec::new(),
            arktype: true,
            effect: false,
            macroforge: false,
            flatbuffers: false,
            protobuf: false,
            flatbuffers_namespace: None,
            protobuf_package: None,
            protobuf_import_validate: false,
        }
    }
}

impl BuildConfig {
    /// Creates a new BuildConfig with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads configuration from evenframe.toml.
    ///
    /// Searches for evenframe.toml starting from `CARGO_MANIFEST_DIR` (if set)
    /// or the current directory, walking upward to the filesystem root.
    ///
    /// # Errors
    ///
    /// Returns `EvenframeError::ConfigNotFound` if no evenframe.toml is found.
    /// Returns `EvenframeError::Config` if the file cannot be parsed.
    pub fn from_toml() -> Result<Self, EvenframeError> {
        let start_dir = env::var("CARGO_MANIFEST_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        Self::from_toml_search(&start_dir)
    }

    /// Loads configuration from a specific evenframe.toml file.
    pub fn from_toml_path(path: impl AsRef<Path>) -> Result<Self, EvenframeError> {
        let path = path.as_ref();
        let content = fs::read_to_string(path)?;

        Self::parse_toml(&content, path)
    }

    /// Searches for evenframe.toml starting from the given directory.
    fn from_toml_search(start_dir: &Path) -> Result<Self, EvenframeError> {
        let mut current = start_dir.to_path_buf();

        loop {
            let config_path = current.join("evenframe.toml");
            if config_path.exists() {
                return Self::from_toml_path(&config_path);
            }

            if !current.pop() {
                return Err(EvenframeError::ConfigNotFound {
                    search_start: start_dir.to_path_buf(),
                });
            }
        }
    }

    /// Parses TOML content into BuildConfig.
    fn parse_toml(content: &str, path: &Path) -> Result<Self, EvenframeError> {
        let value: toml::Value =
            toml::from_str(content).map_err(|e| EvenframeError::config_error(e.to_string()))?;

        let mut config = Self::default();

        // Parse [general] section
        if let Some(general) = value.get("general").and_then(|v| v.as_table())
            && let Some(aliases) = general.get("apply_aliases").and_then(|v| v.as_array())
        {
            config.apply_aliases = aliases
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect();
        }

        // Parse [typesync] section
        if let Some(typesync) = value.get("typesync").and_then(|v| v.as_table()) {
            if let Some(output) = typesync.get("output_path").and_then(|v| v.as_str()) {
                // Resolve relative paths from the config file's directory
                let base_dir = path.parent().unwrap_or(Path::new("."));
                config.output_path = base_dir.join(output);
            }

            if let Some(v) = typesync.get("should_generate_arktype_types") {
                config.arktype = v.as_bool().unwrap_or(false);
            }

            if let Some(v) = typesync.get("should_generate_effect_types") {
                config.effect = v.as_bool().unwrap_or(false);
            }

            if let Some(v) = typesync.get("should_generate_macroforge_types") {
                config.macroforge = v.as_bool().unwrap_or(false);
            }

            if let Some(v) = typesync.get("should_generate_flatbuffers_types") {
                config.flatbuffers = v.as_bool().unwrap_or(false);
            }

            if let Some(v) = typesync.get("should_generate_protobuf_types") {
                config.protobuf = v.as_bool().unwrap_or(false);
            }

            if let Some(ns) = typesync.get("flatbuffers_namespace").and_then(|v| v.as_str()) {
                config.flatbuffers_namespace = Some(ns.to_string());
            }

            if let Some(pkg) = typesync.get("protobuf_package").and_then(|v| v.as_str()) {
                config.protobuf_package = Some(pkg.to_string());
            }

            if let Some(v) = typesync.get("protobuf_import_validate") {
                config.protobuf_import_validate = v.as_bool().unwrap_or(false);
            }
        }

        // Set scan_path to the directory containing the config file
        if let Some(parent) = path.parent() {
            config.scan_path = parent.to_path_buf();
        }

        Ok(config)
    }

    /// Creates a builder for programmatic configuration.
    pub fn builder() -> BuildConfigBuilder {
        BuildConfigBuilder::new()
    }
}

/// Builder for creating BuildConfig programmatically.
#[derive(Debug, Clone, Default)]
pub struct BuildConfigBuilder {
    config: BuildConfig,
}

impl BuildConfigBuilder {
    /// Creates a new builder with default configuration.
    pub fn new() -> Self {
        Self {
            config: BuildConfig::default(),
        }
    }

    /// Sets the scan path for finding Rust types.
    pub fn scan_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.config.scan_path = path.into();
        self
    }

    /// Sets the output path for generated files.
    pub fn output_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.config.output_path = path.into();
        self
    }

    /// Adds an apply alias for attribute detection.
    pub fn apply_alias(mut self, alias: impl Into<String>) -> Self {
        self.config.apply_aliases.push(alias.into());
        self
    }

    /// Sets multiple apply aliases.
    pub fn apply_aliases(mut self, aliases: Vec<String>) -> Self {
        self.config.apply_aliases = aliases;
        self
    }

    /// Enables ArkType schema generation.
    pub fn enable_arktype(mut self) -> Self {
        self.config.arktype = true;
        self
    }

    /// Disables ArkType schema generation.
    pub fn disable_arktype(mut self) -> Self {
        self.config.arktype = false;
        self
    }

    /// Enables Effect-TS schema generation.
    pub fn enable_effect(mut self) -> Self {
        self.config.effect = true;
        self
    }

    /// Disables Effect-TS schema generation.
    pub fn disable_effect(mut self) -> Self {
        self.config.effect = false;
        self
    }

    /// Enables Macroforge type generation.
    pub fn enable_macroforge(mut self) -> Self {
        self.config.macroforge = true;
        self
    }

    /// Disables Macroforge type generation.
    pub fn disable_macroforge(mut self) -> Self {
        self.config.macroforge = false;
        self
    }

    /// Enables FlatBuffers schema generation with optional namespace.
    pub fn enable_flatbuffers(mut self, namespace: Option<String>) -> Self {
        self.config.flatbuffers = true;
        self.config.flatbuffers_namespace = namespace;
        self
    }

    /// Disables FlatBuffers schema generation.
    pub fn disable_flatbuffers(mut self) -> Self {
        self.config.flatbuffers = false;
        self
    }

    /// Enables Protocol Buffers schema generation with options.
    pub fn enable_protobuf(mut self, package: Option<String>, import_validate: bool) -> Self {
        self.config.protobuf = true;
        self.config.protobuf_package = package;
        self.config.protobuf_import_validate = import_validate;
        self
    }

    /// Disables Protocol Buffers schema generation.
    pub fn disable_protobuf(mut self) -> Self {
        self.config.protobuf = false;
        self
    }

    /// Enables all generators.
    pub fn enable_all(mut self) -> Self {
        self.config.arktype = true;
        self.config.effect = true;
        self.config.macroforge = true;
        self.config.flatbuffers = true;
        self.config.protobuf = true;
        self
    }

    /// Disables all generators.
    pub fn disable_all(mut self) -> Self {
        self.config.arktype = false;
        self.config.effect = false;
        self.config.macroforge = false;
        self.config.flatbuffers = false;
        self.config.protobuf = false;
        self
    }

    /// Builds the final BuildConfig.
    pub fn build(self) -> BuildConfig {
        self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = BuildConfig::default();
        assert!(config.arktype);
        assert!(!config.effect);
        assert!(!config.macroforge);
        assert!(!config.flatbuffers);
        assert!(!config.protobuf);
    }

    #[test]
    fn test_builder_enable_arktype() {
        let config = BuildConfig::builder().enable_arktype().build();
        assert!(config.arktype);
    }

    #[test]
    fn test_builder_enable_all() {
        let config = BuildConfig::builder().enable_all().build();
        assert!(config.arktype);
        assert!(config.effect);
        assert!(config.macroforge);
        assert!(config.flatbuffers);
        assert!(config.protobuf);
    }

    #[test]
    fn test_builder_custom_paths() {
        let config = BuildConfig::builder()
            .scan_path("/custom/scan")
            .output_path("/custom/output")
            .build();

        assert_eq!(config.scan_path, PathBuf::from("/custom/scan"));
        assert_eq!(config.output_path, PathBuf::from("/custom/output"));
    }

    #[test]
    fn test_builder_apply_aliases() {
        let config = BuildConfig::builder()
            .apply_alias("MyMacro")
            .apply_alias("OtherMacro")
            .build();

        assert_eq!(config.apply_aliases.len(), 2);
        assert!(config.apply_aliases.contains(&"MyMacro".to_string()));
        assert!(config.apply_aliases.contains(&"OtherMacro".to_string()));
    }

    #[test]
    fn test_parse_toml_basic() {
        let toml_content = r#"
[general]
apply_aliases = ["MyMacro"]

[typesync]
output_path = "./generated/"
should_generate_arktype_types = true
should_generate_effect_types = true
"#;

        let config = BuildConfig::parse_toml(toml_content, Path::new("/test/evenframe.toml"))
            .expect("Should parse successfully");

        assert!(config.arktype);
        assert!(config.effect);
        assert_eq!(config.apply_aliases, vec!["MyMacro".to_string()]);
    }
}
