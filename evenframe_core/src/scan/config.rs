//! What a workspace scan reads: where to scan, how to read what it finds,
//! and the outputs a build script or the CLI generates from it.

use crate::config::{EvenframeConfig, ForeignTypeConfig, IncludeFile};
use crate::error::EvenframeError;
use crate::typesync::config::{CollisionStrategy, StructVariants, TsNames, TypesyncOutput};
use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};

/// The configuration a build script runs with: `.evenframe/config.toml` or
/// evenframe.toml, found from `CARGO_MANIFEST_DIR` (if set) or the current
/// directory upward. The database connection settings may reference unset
/// variables, as nothing a build script runs connects.
pub(crate) fn build_script_config() -> Result<EvenframeConfig, EvenframeError> {
    let start_dir = match env::var_os("CARGO_MANIFEST_DIR") {
        Some(manifest_dir) => PathBuf::from(manifest_dir),
        None => env::current_dir()?,
    };
    let path = EvenframeConfig::find_config_file_from(&start_dir).ok_or_else(|| {
        EvenframeError::ConfigNotFound {
            search_start: start_dir.clone(),
        }
    })?;
    EvenframeConfig::load_from(path, false)
}

/// What a workspace scan reads, and the type outputs generated from it.
#[derive(Debug, Clone)]
pub struct ScanConfig {
    /// The TypeScript field naming fallback.
    pub ts_names: TsNames,
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

    /// How a struct variant's fields are written.
    pub struct_variants: StructVariants,

    /// Foreign type configurations, keyed by canonical type name.
    pub foreign_types: BTreeMap<String, ForeignTypeConfig>,

    /// Type-transform WASM plugin configurations.
    pub output_rule_plugins: BTreeMap<String, crate::config::OutputRulePluginConfig>,

    /// Synthetic-item WASM plugin configurations. These plugins add new
    /// structs/enums/tables derived from the scanner results.
    pub synthetic_item_plugins: BTreeMap<String, crate::config::SyntheticItemPluginConfig>,

    /// Files outside the scan subtree to additionally parse for Evenframe types.
    /// Paths are already resolved (absolute) relative to the project root.
    pub include_files: Vec<IncludeFile>,

    /// Files inside the scan subtree to leave out, already resolved (absolute).
    pub exclude_files: Vec<PathBuf>,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            ts_names: TsNames::default(),
            scan_path: PathBuf::from("."),
            config_path: None,
            apply_aliases: Vec::new(),
            expand_macros: false,
            outputs: Vec::new(),
            collision_strategy: CollisionStrategy::Error,
            struct_variants: StructVariants::Named,
            foreign_types: BTreeMap::new(),
            output_rule_plugins: BTreeMap::new(),
            synthetic_item_plugins: BTreeMap::new(),
            include_files: Vec::new(),
            exclude_files: Vec::new(),
        }
    }
}

impl ScanConfig {
    /// Creates a new ScanConfig with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// The build settings of a loaded configuration, so type generation and
    /// the rest of a command read the same, environment-substituted file.
    pub fn from_config(config: &EvenframeConfig) -> Self {
        Self {
            ts_names: config.typesync.ts_names,
            scan_path: config.project_root().to_path_buf(),
            config_path: Some(config.config_file_path.clone()),
            apply_aliases: config.general.apply_aliases.clone(),
            expand_macros: config.general.expand_macros,
            outputs: config.typesync.outputs.clone(),
            collision_strategy: config.typesync.collision_strategy,
            struct_variants: config.typesync.struct_variants,
            foreign_types: config.general.foreign_types.clone(),
            output_rule_plugins: config.general.output_rule_plugins.clone(),
            synthetic_item_plugins: config.general.synthetic_item_plugins.clone(),
            include_files: config.resolved_include_files(),
            exclude_files: config.resolved_exclude_files(),
        }
    }

    /// Loads the configuration file the CLI uses: the one
    /// [`EvenframeConfig::find_config_file`] finds from the current
    /// directory, so both configs always describe the same project.
    pub fn discover() -> Result<Self, EvenframeError> {
        Ok(Self::from_config(&EvenframeConfig::new_offline()?))
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
        Ok(Self::from_config(&build_script_config()?))
    }

    /// Loads configuration from a specific evenframe.toml file. The database
    /// connection settings may reference unset variables, as type generation
    /// never connects.
    pub fn from_toml_path(path: impl AsRef<Path>) -> Result<Self, EvenframeError> {
        let config = EvenframeConfig::load_from(path.as_ref().to_path_buf(), false)?;
        Ok(Self::from_config(&config))
    }

    /// Creates a builder for programmatic configuration.
    pub fn builder() -> ScanConfigBuilder {
        ScanConfigBuilder::new()
    }
}

/// Builder for creating ScanConfig programmatically.
#[derive(Debug, Clone, Default)]
pub struct ScanConfigBuilder {
    config: ScanConfig,
}

impl ScanConfigBuilder {
    /// Creates a new builder with default configuration.
    pub fn new() -> Self {
        Self {
            config: ScanConfig::default(),
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

    /// Sets the outputs to generate. No output is selected implicitly.
    pub fn outputs(mut self, outputs: Vec<TypesyncOutput>) -> Self {
        self.config.outputs = outputs;
        self
    }

    /// Sets the TypeScript field naming fallback.
    pub fn ts_names(mut self, ts_names: TsNames) -> Self {
        self.config.ts_names = ts_names;
        self
    }

    /// Builds the final ScanConfig.
    pub fn build(self) -> ScanConfig {
        self.config
    }
}

#[cfg(test)]
mod tests {
    use super::{EvenframeConfig, EvenframeError, PathBuf, ScanConfig, TsNames, TypesyncOutput};
    use crate::typesync::config::OutputKind;

    fn parse_at(content: &str, config_path: &str) -> Result<ScanConfig, EvenframeError> {
        let config = EvenframeConfig::parse(content, PathBuf::from(config_path), false)?;
        Ok(ScanConfig::from_config(&config))
    }

    fn parse(content: &str) -> Result<ScanConfig, EvenframeError> {
        parse_at(content, "/nonexistent-evenframe-project/evenframe.toml")
    }

    #[test]
    fn default_config_requires_explicit_outputs() {
        assert!(ScanConfig::default().outputs.is_empty());
    }

    #[test]
    fn ts_names_policy_is_parsed_and_forwarded() {
        assert_eq!(
            parse("[typesync]").expect("default naming").ts_names,
            TsNames::Default
        );
        assert_eq!(
            parse("[typesync]\nts_names = \"default\"")
                .expect("explicit default naming")
                .ts_names,
            TsNames::Default
        );
        assert_eq!(
            parse("[typesync]\nts_names = \"respect_serde\"")
                .expect("serde naming")
                .ts_names,
            TsNames::RespectSerde
        );
        assert!(parse("[typesync]\nts_names = \"snake_case\"").is_err());
    }

    #[test]
    fn builder_sets_paths_aliases_outputs_and_naming() {
        let outputs = vec![TypesyncOutput::new(OutputKind::Effect, "/custom/output")];
        let config = ScanConfig::builder()
            .scan_path("/custom/scan")
            .apply_alias("MyMacro")
            .apply_alias("OtherMacro")
            .outputs(outputs.clone())
            .ts_names(TsNames::RespectSerde)
            .build();

        assert_eq!(config.scan_path, PathBuf::from("/custom/scan"));
        assert_eq!(config.apply_aliases, vec!["MyMacro", "OtherMacro"]);
        assert_eq!(config.outputs, outputs);
        assert_eq!(config.ts_names, TsNames::RespectSerde);
    }

    #[test]
    fn single_output_is_read_in_full() {
        let config = parse(
            r#"
[general]
apply_aliases = ["MyMacro"]

[typesync]
output = { kind = "macroforge", dir = "./generated", mode = "per_file", file_extension = ".svelte.ts", import_extension = "js", macros = { Form = "@app/forms" }, default_derives = ["Default", "Encode", "Form"] }
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
            "output = { kind = \"effect\", dir = \"g\", macros = { Form = \"@app/forms\" } }",
            "output = { kind = \"effect\", dir = \"g\", default_derives = [\"Encode\"] }",
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
  "../auth/src/lib/policy.rs",
  { path = "/abs/shared/ids.rs", resolve_only = true },
]
"#;

        let config = parse_at(toml_content, "/proj/.evenframe/config.toml")
            .expect("Should parse successfully");

        assert_eq!(config.include_files.len(), 2);
        // Relative path joined to project root (/proj); `.evenframe/` stripped.
        assert_eq!(
            config.include_files[0].path,
            PathBuf::from("/proj/../auth/src/lib/policy.rs")
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
