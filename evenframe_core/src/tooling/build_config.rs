//! Build-time configuration for type generation.

use crate::config::ForeignTypeConfig;
use crate::error::EvenframeError;
use crate::typesync::config::{CollisionStrategy, OutputKind, TypesyncConfig, TypesyncOutput};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Configuration for build-time type generation.
#[derive(Debug, Clone)]
pub struct BuildConfig {
    /// Root path to scan for Rust types.
    pub scan_path: PathBuf,

    /// The config file this configuration was loaded from, if any.
    pub config_path: Option<PathBuf>,

    /// Apply aliases for attribute detection (e.g., custom derive macros).
    pub apply_aliases: Vec<String>,

    /// When true, use `cargo expand` to resolve macro-generated types.
    pub expand_macros: bool,

    /// The outputs to generate; each `dir` resolves against `scan_path`.
    pub outputs: Vec<TypesyncOutput>,

    /// How to handle type name collisions across files.
    pub collision_strategy: CollisionStrategy,

    /// Foreign type configurations, keyed by canonical type name.
    pub foreign_types: BTreeMap<String, ForeignTypeConfig>,

    /// Type-transform WASM plugin configurations.
    pub output_rule_plugins: BTreeMap<String, crate::config::OutputRulePluginConfig>,

    /// Synthetic-item WASM plugin configurations. These plugins add new
    /// structs/enums/tables derived from the scanner results.
    pub synthetic_item_plugins: BTreeMap<String, crate::config::SyntheticItemPluginConfig>,

    /// Files outside the scan subtree to additionally parse for Evenframe types.
    /// Paths are already resolved (absolute) relative to the project root.
    pub include_files: Vec<super::IncludeFile>,
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            scan_path: PathBuf::from("."),
            config_path: None,
            apply_aliases: Vec::new(),
            expand_macros: false,
            outputs: vec![TypesyncOutput::new(OutputKind::Arktype, "./src/generated/")],
            collision_strategy: CollisionStrategy::Error,
            foreign_types: BTreeMap::new(),
            output_rule_plugins: BTreeMap::new(),
            synthetic_item_plugins: BTreeMap::new(),
            include_files: Vec::new(),
        }
    }
}

impl BuildConfig {
    /// Creates a new BuildConfig with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads the configuration file the CLI uses: the one
    /// [`EvenframeConfig::find_config_file`](crate::config::EvenframeConfig::find_config_file)
    /// finds from the current directory, so both configs always describe the
    /// same project.
    pub fn discover() -> Result<Self, EvenframeError> {
        Self::from_toml_path(crate::config::EvenframeConfig::find_config_file()?)
    }

    /// Loads configuration from evenframe.toml, for build scripts.
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
        let content = fs::read_to_string(path).map_err(|e| {
            EvenframeError::config_error(format!(
                "Failed to read configuration file {}: {e}",
                path.display()
            ))
        })?;

        Self::parse_toml(&content, path)
    }

    /// Searches for `.evenframe/config.toml` (preferred) or `evenframe.toml` (fallback)
    /// starting from the given directory.
    fn from_toml_search(start_dir: &Path) -> Result<Self, EvenframeError> {
        let mut current = start_dir.to_path_buf();

        loop {
            // Check .evenframe/config.toml first (preferred)
            let dotdir_config = current.join(".evenframe").join("config.toml");
            if dotdir_config.exists() {
                return Self::from_toml_path(&dotdir_config);
            }

            // Fall back to evenframe.toml
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

        let mut config = Self {
            config_path: Some(path.to_path_buf()),
            ..Self::default()
        };

        // Captured from [general] but resolved below, once `project_root` is known.
        let mut include_specs: Vec<crate::config::IncludeFileSpec> = Vec::new();

        // Parse [general] section
        if let Some(general) = value.get("general") {
            let general_config: crate::config::GeneralConfig =
                general.clone().try_into().map_err(|e| {
                    EvenframeError::config_error(format!("Failed to parse [general]: {e}"))
                })?;

            config.apply_aliases = general_config.apply_aliases;
            config.expand_macros = general_config.expand_macros;
            config.foreign_types = general_config.foreign_types;
            config.output_rule_plugins = general_config.output_rule_plugins;
            config.synthetic_item_plugins = general_config.synthetic_item_plugins;
            include_specs = general_config.include_files;
        }

        let project_root = crate::config::EvenframeConfig::project_root_of(path);

        // Resolve `include_files` paths relative to the project root (absolute as-is).
        config.include_files = include_specs
            .iter()
            .map(|spec| {
                let p = PathBuf::from(spec.path());
                let path = if p.is_absolute() {
                    p
                } else {
                    project_root.join(p)
                };
                super::IncludeFile {
                    path,
                    resolve_only: spec.resolve_only(),
                }
            })
            .collect();

        if let Some(typesync) = value.get("typesync") {
            let typesync: TypesyncConfig = typesync.clone().try_into().map_err(|e| {
                EvenframeError::config_error(format!("Failed to parse [typesync]: {e}"))
            })?;
            config.outputs = typesync.outputs;
            config.collision_strategy = typesync.collision_strategy;
        }

        // Set scan_path to the project root
        config.scan_path = project_root.to_path_buf();

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

    /// Enables or disables macro expansion via `cargo expand`.
    pub fn expand_macros(mut self, enabled: bool) -> Self {
        self.config.expand_macros = enabled;
        self
    }

    /// Sets foreign type configurations.
    pub fn foreign_types(mut self, foreign_types: BTreeMap<String, ForeignTypeConfig>) -> Self {
        self.config.foreign_types = foreign_types;
        self
    }

    /// Sets the outputs to generate, replacing the default ArkType output.
    pub fn outputs(mut self, outputs: Vec<TypesyncOutput>) -> Self {
        self.config.outputs = outputs;
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

    fn parse(content: &str) -> Result<BuildConfig, EvenframeError> {
        BuildConfig::parse_toml(content, Path::new("/proj/evenframe.toml"))
    }

    #[test]
    fn default_config_generates_arktype() {
        assert_eq!(
            BuildConfig::default().outputs,
            vec![TypesyncOutput::new(OutputKind::Arktype, "./src/generated/")]
        );
    }

    #[test]
    fn builder_sets_paths_aliases_and_outputs() {
        let outputs = vec![TypesyncOutput::new(OutputKind::Effect, "/custom/output")];
        let config = BuildConfig::builder()
            .scan_path("/custom/scan")
            .apply_alias("MyMacro")
            .apply_alias("OtherMacro")
            .outputs(outputs.clone())
            .build();

        assert_eq!(config.scan_path, PathBuf::from("/custom/scan"));
        assert_eq!(config.apply_aliases, vec!["MyMacro", "OtherMacro"]);
        assert_eq!(config.outputs, outputs);
    }

    #[test]
    fn single_output_is_read_in_full() {
        let config = parse(
            r#"
[general]
apply_aliases = ["MyMacro"]

[typesync]
output = { kind = "macroforge", dir = "./generated", mode = "per_file", file_extension = ".svelte.ts", import_extension = "js" }
"#,
        )
        .unwrap();
        assert_eq!(config.apply_aliases, vec!["MyMacro"]);
        insta::assert_debug_snapshot!(config.outputs);
    }

    #[test]
    fn outputs_array_keeps_each_kinds_settings() {
        let config = parse(
            r#"
[typesync]
outputs = [
  { kind = "arktype", dir = "./generated/arktype", file = "schemas.ts" },
  { kind = "protobuf", dir = "./proto", package = "com.example", import_validate = true },
]
"#,
        )
        .unwrap();
        insta::assert_debug_snapshot!(config.outputs);
    }

    #[test]
    fn invalid_typesync_configs_are_rejected() {
        let errors: Vec<String> = [
            "should_generate_arktype_types = true",
            "output = { kind = \"arcktype\", dir = \"g\" }",
            "output = { kind = \"arktype\", dir = \"g\" }\noutputs = [{ kind = \"effect\", dir = \"e\" }]",
            "output = { kind = \"arktype\", dir = \"g\", mode = \"per_file\" }",
            "output = { kind = \"effect\", dir = \"g\", package = \"com.example\" }",
            "output = { kind = \"effect\", dir = \"g\", mode = \"per_file\", file = \"x.ts\" }",
            "output = { kind = \"macroforge\", dir = \"g\", file_naming = \"kebabcase\" }",
            "collision_strategy = \"autorename\"",
        ]
        .iter()
        .map(|body| {
            parse(&format!("[typesync]\n{body}\n"))
                .unwrap_err()
                .to_string()
        })
        .collect();
        insta::assert_snapshot!(errors.join("\n\n"));
    }

    #[test]
    fn include_files_resolve_against_the_project_root() {
        // Both the bare-string and the `{ path, resolve_only }` table forms,
        // resolved relative to the project root (parent of `.evenframe/`).
        let toml_content = r#"
[general]
include_files = [
  "../idp/src/lib/policy.rs",
  { path = "/abs/shared/ids.rs", resolve_only = true },
]
"#;

        let config =
            BuildConfig::parse_toml(toml_content, Path::new("/proj/.evenframe/config.toml"))
                .expect("Should parse successfully");

        assert_eq!(config.include_files.len(), 2);
        // Relative path joined to project root (/proj); `.evenframe/` stripped.
        assert_eq!(
            config.include_files[0].path,
            PathBuf::from("/proj/../idp/src/lib/policy.rs")
        );
        assert!(!config.include_files[0].resolve_only);
        // Absolute path used as-is; `resolve_only` carried through.
        assert_eq!(
            config.include_files[1].path,
            PathBuf::from("/abs/shared/ids.rs")
        );
        assert!(config.include_files[1].resolve_only);
        assert_eq!(config.scan_path, PathBuf::from("/proj"));
    }
}
