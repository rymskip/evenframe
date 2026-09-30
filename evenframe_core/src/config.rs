use crate::error::{EvenframeError, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};
use toml;
use tracing::{debug, info, trace, warn};

/// Where a foreign type's TypeScript definition comes from.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TsImport {
    /// The module to import from: a package name, or a path relative to each
    /// generated file.
    pub from: String,
    /// The name the module exports.
    pub name: String,
    /// Whether to write `import type`. A namespace used as a value, such as
    /// `DateTime.Utc`, needs a plain `import`.
    #[serde(default = "default_true")]
    pub type_only: bool,
}

fn default_true() -> bool {
    true
}

/// A foreign type in a TypeScript output: its type, where `{0}`, `{1}` and so
/// on stand for its generic parameters, and the import it needs when it is
/// not native TypeScript.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TsMapping {
    #[serde(rename = "type")]
    pub type_expr: String,
    #[serde(default)]
    pub import: Option<TsImport>,
}

/// A foreign type in the Effect output: its schema, the type it is encoded
/// as, and the import it needs when it is not part of Effect's `Schema`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EffectMapping {
    #[serde(rename = "type")]
    pub type_expr: String,
    pub encoded: String,
    #[serde(default)]
    pub import: Option<TsImport>,
}

/// Configuration for a single foreign (external crate) type.
/// Defines how a Rust type from an external crate maps to each database
/// and TypeScript target.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ForeignTypeConfig {
    /// Source crate name (for documentation/provenance)
    #[serde(default, rename = "crate")]
    pub crate_name: String,

    /// Rust type names that map to this foreign type
    /// e.g., ["DateTime", "chrono::DateTime"]
    #[serde(default)]
    pub rust_type_names: Vec<String>,

    /// If true, generic params like `<Utc>` are ignored during parsing
    #[serde(default)]
    pub ignore_generic_params: bool,

    // --- Database schema mappings ---
    #[serde(default)]
    pub surrealdb: String,

    /// SurrealDB format when field is `id`, e.g. `record<{table_name}>`
    #[serde(default)]
    pub surrealdb_id_format: Option<String>,
    /// SurrealDB format when field is NOT `id`, e.g. `record<any>`
    #[serde(default)]
    pub surrealdb_non_id_format: Option<String>,

    // --- TypeSync mappings ---
    #[serde(default)]
    pub arktype: Option<TsMapping>,
    #[serde(default)]
    pub effect: Option<EffectMapping>,
    #[serde(default)]
    pub macroforge: Option<TsMapping>,
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

    // --- Serde format annotation ---
    /// If set, generates `@serde({ format: "..." })` in macroforge output
    #[serde(default)]
    pub serde_format: String,
}

fn deserialize_foreign_types<'de, D>(
    deserializer: D,
) -> std::result::Result<BTreeMap<String, ForeignTypeConfig>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let foreign_types = BTreeMap::<String, ForeignTypeConfig>::deserialize(deserializer)?;
    validate_foreign_types(&foreign_types).map_err(serde::de::Error::custom)?;
    Ok(foreign_types)
}

/// Rejects foreign type entries evenframe cannot use: a placeholder no generic
/// parameter fills, and a `RecordLink` entry that sets more than its
/// TypeScript side, since the record link's schema and mock data are
/// evenframe's own.
pub fn validate_foreign_types(
    foreign_types: &BTreeMap<String, ForeignTypeConfig>,
) -> std::result::Result<(), String> {
    use crate::typesync::foreign_ts::{RECORD_LINK, placeholders};
    for (name, foreign) in foreign_types {
        let record_link = name == RECORD_LINK;
        let parameters = usize::from(record_link);
        let types = [
            (
                "arktype",
                foreign.arktype.as_ref().map(|mapping| &mapping.type_expr),
            ),
            (
                "effect",
                foreign.effect.as_ref().map(|mapping| &mapping.type_expr),
            ),
            (
                "effect encoded",
                foreign.effect.as_ref().map(|mapping| &mapping.encoded),
            ),
            (
                "macroforge",
                foreign
                    .macroforge
                    .as_ref()
                    .map(|mapping| &mapping.type_expr),
            ),
        ];
        for (output, type_expr) in types {
            let Some(type_expr) = type_expr else {
                continue;
            };
            if let Some(unfilled) = placeholders(type_expr)
                .into_iter()
                .find(|index| *index >= parameters)
            {
                return Err(format!(
                    "foreign_types.{name}: the {output} type `{type_expr}` uses `{{{unfilled}}}`, \
                     but {name} has {parameters} generic parameter{}",
                    if parameters == 1 { "" } else { "s" }
                ));
            }
        }
        let typescript_only = ForeignTypeConfig {
            crate_name: foreign.crate_name.clone(),
            arktype: foreign.arktype.clone(),
            effect: foreign.effect.clone(),
            macroforge: foreign.macroforge.clone(),
            serde_format: foreign.serde_format.clone(),
            ..Default::default()
        };
        if record_link && &typescript_only != foreign {
            return Err(format!(
                "foreign_types.{RECORD_LINK} can only set arktype, effect, macroforge, crate and \
                 serde_format: the record link's Rust names, schema and mock data are \
                 evenframe's own"
            ));
        }
    }
    Ok(())
}

/// Loads environment variables from the `.env` file at `env_path`. The file
/// is optional; one that exists but cannot be loaded is reported.
fn load_env_from(env_path: &Path) {
    match dotenvy::from_path(env_path) {
        Ok(()) => info!("Loaded environment variables from {:?}", env_path),
        Err(e) if e.not_found() => debug!("No .env file found at {:?}, skipping", env_path),
        Err(e) => warn!("Failed to load .env file {:?}: {e}", env_path),
    }
}

/// A single `include_files` entry: either a bare path string, or a table form
/// `{ path = "...", resolve_only = true }`.
///
/// `resolve_only` registers the file's types for field-type *resolution* only:
/// they are skipped at every emission site (schemasync `DEFINE TABLE`/mock/diff,
/// typesync interface output). This is orthogonal to [`crate::types::Pipeline`]:
/// pipeline chooses *which* pipelines emit a type, while `resolve_only` keeps a
/// type present for resolution in *both* pipelines while emitting it in neither.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
#[serde(untagged)]
pub enum IncludeFileSpec {
    /// Bare path string: the file's types are registered and emitted, as if in-tree.
    Path(String),
    /// Table form: `resolve_only` controls whether the file's types are emitted.
    Spec {
        path: String,
        #[serde(default)]
        resolve_only: bool,
    },
}

impl IncludeFileSpec {
    /// The configured path, relative to the project root unless absolute.
    pub fn path(&self) -> &str {
        match self {
            IncludeFileSpec::Path(p) => p,
            IncludeFileSpec::Spec { path, .. } => path,
        }
    }

    /// Whether the file's types should be registered for resolution only
    /// (not emitted as tables/TS).
    pub fn resolve_only(&self) -> bool {
        match self {
            IncludeFileSpec::Path(_) => false,
            IncludeFileSpec::Spec { resolve_only, .. } => *resolve_only,
        }
    }
}

/// General configuration for Evenframe operations
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct GeneralConfig {
    /// Attribute macro names that expand to include Evenframe derive
    /// These are used with #[apply(...)] and automatically include Evenframe
    #[serde(default)]
    pub apply_aliases: Vec<String>,

    /// Files OUTSIDE the scan subtree to additionally parse for Evenframe types,
    /// so a table field can reference a struct/enum defined in a sibling crate
    /// without duplicating it. Each entry is a bare path string, or a table
    /// `{ path = "...", resolve_only = true }`. Paths are relative to the
    /// project root (the dir containing `.evenframe/` or `evenframe.toml`) unless
    /// absolute.
    #[serde(default)]
    pub include_files: Vec<IncludeFileSpec>,

    /// When true, run `cargo expand` on each crate before scanning for types.
    /// This allows Evenframe to discover types generated by macros.
    /// Requires `cargo-expand` to be installed (`cargo install cargo-expand`).
    #[serde(default)]
    pub expand_macros: bool,

    /// Path to the .env file, relative to the project root.
    /// Defaults to `.env` in the project root directory.
    #[serde(default)]
    pub env_path: Option<String>,

    /// Foreign type configurations, keyed by canonical type name.
    /// Defines how external Rust types map to database schemas and TypeScript types.
    #[serde(default, deserialize_with = "deserialize_foreign_types")]
    pub foreign_types: BTreeMap<String, ForeignTypeConfig>,

    /// Type-transform WASM plugins, keyed by plugin name.
    /// These plugins receive full struct/enum context and can conditionally modify
    /// type generation output (overrides, skips, imports, etc.).
    #[serde(default)]
    pub output_rule_plugins: BTreeMap<String, OutputRulePluginConfig>,

    /// Synthetic-item WASM plugins, keyed by plugin name.
    ///
    /// Unlike `output_rule_plugins`, these plugins *add* new structs, tagged
    /// unions, and database tables derived from the scanner results rather
    /// than overriding existing ones. They run after the rule plugins so
    /// they see the final override state.
    #[serde(default)]
    pub synthetic_item_plugins: BTreeMap<String, SyntheticItemPluginConfig>,
}

/// Configuration for a output-rule WASM plugin.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OutputRulePluginConfig {
    /// Path to the `.wasm` file, relative to the project root.
    pub path: String,
}

/// Configuration for a synthetic-item WASM plugin.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SyntheticItemPluginConfig {
    /// Path to the `.wasm` file, relative to the project root.
    pub path: String,
}

/// Unified configuration for Evenframe operations
/// This is the root configuration that contains both schemasync and typesync configurations
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvenframeConfig {
    /// General configuration
    #[serde(default)]
    pub general: GeneralConfig,
    /// Schema synchronization configuration (database operations), when the
    /// project syncs a database.
    #[serde(default)]
    pub schemasync: Option<crate::schemasync::config::SchemasyncConfig>,
    /// Type synchronization configuration (TypeScript/Effect type generation)
    #[serde(default)]
    pub typesync: crate::typesync::config::TypesyncConfig,
    /// Path to the config file that was loaded (set at runtime, not from TOML)
    #[serde(skip)]
    pub config_file_path: PathBuf,
}

/// The configuration file chosen for this process (the CLI's `--config`),
/// used instead of searching upward from the current directory.
static CONFIG_FILE: OnceLock<PathBuf> = OnceLock::new();

impl EvenframeConfig {
    /// The `[schemasync]` section, which every command that reaches the
    /// database needs.
    pub fn require_schemasync(&self) -> Result<&crate::schemasync::config::SchemasyncConfig> {
        self.schemasync.as_ref().ok_or_else(|| {
            EvenframeError::config(format!(
                "{} has no [schemasync] section; add one with a [schemasync.database] table to \
                 sync a database",
                self.config_file_path.display()
            ))
        })
    }

    /// Makes every configuration lookup in this process use `path` instead of
    /// searching upward from the current directory.
    pub fn use_config_file(path: &Path) -> Result<()> {
        let path = std::path::absolute(path).map_err(|e| {
            EvenframeError::config(format!(
                "Failed to resolve configuration path {}: {e}",
                path.display()
            ))
        })?;
        CONFIG_FILE.set(path).map_err(|rejected| {
            EvenframeError::config(format!(
                "Cannot use {}: the configuration file is already chosen",
                rejected.display()
            ))
        })
    }

    /// Best-effort early .env load for use before full config is parsed:
    /// the project's `.env`, or a `.env` found from the current directory
    /// when there is no project yet.
    pub fn load_env_early() {
        match Self::find_project_root() {
            Some(root) => load_env_from(&root.join(".env")),
            None => match dotenvy::dotenv() {
                Ok(path) => debug!("Loaded environment variables from {:?}", path),
                Err(e) if e.not_found() => debug!("No .env file found"),
                Err(e) => warn!("Failed to load .env file: {e}"),
            },
        }
    }

    /// Load configuration by searching for evenframe.toml in the current
    /// directory and its ancestors.
    pub fn new() -> Result<EvenframeConfig> {
        Self::load(true)
    }

    /// Load configuration for work that never connects to the database, such
    /// as `schemasync dump`. The connection settings (`url`, `namespace`,
    /// `database`) may reference environment variables that aren't set: those
    /// references are left unresolved instead of failing. Every other setting
    /// is resolved exactly as by [`EvenframeConfig::new`].
    pub fn new_offline() -> Result<EvenframeConfig> {
        Self::load(false)
    }

    fn load(require_connection_env: bool) -> Result<EvenframeConfig> {
        info!("Loading Evenframe configuration");

        let config_path = Self::find_config_file()?;
        info!("Found configuration file at: {:?}", config_path);

        let contents = fs::read_to_string(&config_path).map_err(|e| {
            EvenframeError::config(format!(
                "Failed to read configuration file {}: {e}",
                config_path.display()
            ))
        })?;

        debug!("Configuration file size: {} bytes", contents.len());

        let mut config: EvenframeConfig = toml::from_str(&contents).map_err(|e| {
            EvenframeError::config(format!(
                "Failed to parse configuration file {}: {e}",
                config_path.display()
            ))
        })?;

        debug!("Successfully parsed TOML configuration");

        // Store config file path early so project_root() works
        config.config_file_path = config_path;

        // Load .env file from configured or default path
        Self::load_env_file(&config);

        // Process environment variable substitutions for all string fields in the config
        debug!("Substituting environment variables in configuration");
        Self::substitute_all_env_vars(&mut config, require_connection_env)?;

        // Resolve surql paths
        let project_root = config.project_root().to_path_buf();

        if let Some(schemasync) = config.schemasync.as_mut() {
            let database = &mut schemasync.database;
            if let crate::schemasync::config::AccessesSource::Path { ref path } = database.accesses
            {
                database.resolved.access_surql =
                    Some(Self::load_surql_from_path(&project_root, path)?);
            }
            if let Some(ref func) = database.functions {
                database.resolved.functions_surql =
                    Some(Self::load_surql_from_path(&project_root, &func.path)?);
            }
            if let Some(ref analyzers) = database.analyzers {
                database.resolved.analyzers_surql =
                    Some(Self::load_surql_from_path(&project_root, &analyzers.path)?);
            }
        }

        info!("Configuration loaded successfully");
        debug!(
            "Mock generation: {}, typesync outputs: {}",
            config
                .schemasync
                .as_ref()
                .is_some_and(|schemasync| schemasync.should_generate_mocks),
            config.typesync.outputs.len()
        );

        Ok(config)
    }

    /// The configuration file: the one chosen with [`Self::use_config_file`],
    /// or else `.evenframe/config.toml` (preferred) or `evenframe.toml`,
    /// searching upward from the current directory.
    pub fn find_config_file() -> Result<PathBuf> {
        if let Some(path) = CONFIG_FILE.get() {
            return Ok(path.clone());
        }
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

        Err(EvenframeError::config(
            "Configuration file not found. Expected '.evenframe/config.toml' or 'evenframe.toml' in current or any parent directory.",
        ))
    }

    /// Locates the project root by the same ancestor walk as config loading,
    /// but without parsing the config, so it is usable before env substitution can
    /// succeed (e.g. for process locking). `None` when no config file exists
    /// in the current directory or any ancestor.
    pub fn find_project_root() -> Option<PathBuf> {
        let config_path = Self::find_config_file().ok()?;
        Some(Self::project_root_of(&config_path).to_path_buf())
    }

    /// The project root for a configuration file:
    /// - For `evenframe.toml` → its directory
    /// - For `.evenframe/config.toml` → the directory containing `.evenframe/`
    pub fn project_root_of(config_path: &Path) -> &Path {
        let parent = config_path.parent().unwrap_or(Path::new("."));
        if parent.file_name().and_then(|n| n.to_str()) == Some(".evenframe") {
            parent.parent().unwrap_or(Path::new("."))
        } else {
            parent
        }
    }

    /// The project root of this configuration's file.
    pub fn project_root(&self) -> &Path {
        Self::project_root_of(&self.config_file_path)
    }

    /// Resolves `general.include_files` to absolute paths for the workspace
    /// scanner's [`with_extra_files`](crate::tooling::WorkspaceScanner::with_extra_files).
    /// Relative entries are joined to the project root; absolute paths are used
    /// as-is. Mirrors the resolution in
    /// [`BuildConfig::parse_toml`](crate::tooling::BuildConfig) so every scanner
    /// entry point (generate/typesync/schemasync via `BuildConfig`, and
    /// info/validate via `EvenframeConfig`) sees the same included files.
    pub fn resolved_include_files(&self) -> Vec<crate::tooling::IncludeFile> {
        let project_root = self.project_root().to_path_buf();
        self.general
            .include_files
            .iter()
            .map(|spec| {
                let p = PathBuf::from(spec.path());
                let path = if p.is_absolute() {
                    p
                } else {
                    project_root.join(p)
                };
                crate::tooling::IncludeFile {
                    path,
                    resolve_only: spec.resolve_only(),
                }
            })
            .collect()
    }

    /// Resolves the .env file path based on config.
    /// If `general.env_path` is set, resolves it relative to the config file's directory.
    /// Otherwise defaults to `<project_root>/.env`.
    pub fn resolve_env_path(&self) -> PathBuf {
        let config_dir = self.config_file_path.parent().unwrap_or(Path::new("."));
        let raw = match &self.general.env_path {
            Some(custom) => config_dir.join(custom),
            None => self.project_root().join(".env"),
        };
        std::path::absolute(&raw).unwrap_or(raw)
    }

    /// Load environment variables from the .env file resolved from config.
    fn load_env_file(config: &EvenframeConfig) {
        load_env_from(&config.resolve_env_path());
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
    ///
    /// With `require_connection_env` false, the database connection settings
    /// are substituted leniently: references to unset variables stay as-is.
    fn substitute_all_env_vars(
        config: &mut EvenframeConfig,
        require_connection_env: bool,
    ) -> Result<()> {
        let config_file_path = config.config_file_path.clone();
        let mut resolved = config
            .schemasync
            .as_ref()
            .map(|schemasync| schemasync.database.resolved.clone())
            .unwrap_or_default();

        // Taken out of the strict round-trip below and substituted leniently
        let connection = match (require_connection_env, config.schemasync.as_mut()) {
            (false, Some(schemasync)) => {
                let database = &mut schemasync.database;
                Some((
                    std::mem::take(&mut database.url),
                    std::mem::take(&mut database.namespace),
                    std::mem::take(&mut database.database),
                ))
            }
            _ => None,
        };

        // The TOML round-trip below only substitutes vars that appear in
        // config string fields; it can't reach surql content loaded from
        // disk into `resolved`. Substitute those explicitly so DDL like
        // `WITH JWT URL '${OIDC_JWKS_URL:-…}'` reaches SurrealDB resolved,
        // since SurrealDB itself does no env-var expansion.
        if let Some(ref surql) = resolved.access_surql {
            resolved.access_surql = Some(Self::substitute_env_vars(surql)?);
        }
        if let Some(ref surql) = resolved.functions_surql {
            resolved.functions_surql = Some(Self::substitute_env_vars(surql)?);
        }
        if let Some(ref surql) = resolved.analyzers_surql {
            resolved.analyzers_surql = Some(Self::substitute_env_vars(surql)?);
        }

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
        if let Some(schemasync) = new_config.schemasync.as_mut() {
            let db = &mut schemasync.database;
            db.resolved = resolved;
            if let Some((url, namespace, database)) = connection {
                db.url = Self::substitute_env_vars_inner(&url, false)?;
                db.namespace = Self::substitute_env_vars_inner(&namespace, false)?;
                db.database = Self::substitute_env_vars_inner(&database, false)?;
            }
        }

        *config = new_config;
        Ok(())
    }

    /// Substitute environment variables in config strings
    /// Supports ${VAR_NAME:-default} syntax
    pub fn substitute_env_vars(value: &str) -> Result<String> {
        Self::substitute_env_vars_inner(value, true)
    }

    /// With `strict` false, a reference to an unset variable without a default
    /// is left in place instead of being an error.
    fn substitute_env_vars_inner(value: &str, strict: bool) -> Result<String> {
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
                        debug!(
                            "Environment variable {} not set, using default: {}",
                            var_name, default
                        );
                        default.to_string()
                    }
                    None if !strict => {
                        debug!(
                            "Environment variable {} not set, leaving the reference unresolved",
                            var_name
                        );
                        continue;
                    }
                    None => {
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
    use crate::typesync::config::{OutputKind, TypesyncOutput};
    use tempfile::TempDir;

    fn foreign_types(entries: &str) -> std::result::Result<GeneralConfig, toml::de::Error> {
        toml::from_str(&format!("foreign_types = {{ {entries} }}"))
    }

    #[test]
    fn a_record_link_entry_sets_only_its_typescript_side() {
        let owned = foreign_types(
            "RecordLink = { macroforge = { type = \"RecordLink<{0}>\", import = { from = \"./index\", name = \"RecordLink\" } } }",
        )
        .unwrap();
        assert!(owned.foreign_types.contains_key("RecordLink"));
        let error = foreign_types("RecordLink = { surrealdb = \"record\" }")
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(
            error.contains("foreign_types.RecordLink can only set arktype, effect, macroforge"),
            "{error}"
        );
    }

    #[test]
    fn a_placeholder_needs_a_generic_parameter() {
        let error = foreign_types("Money = { macroforge = { type = \"Money<{0}>\" } }")
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(
            error.contains(
                "the macroforge type `Money<{0}>` uses `{0}`, but Money has 0 generic parameters"
            ),
            "{error}"
        );
    }

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
    fn a_config_may_leave_out_either_pipeline() {
        let config: EvenframeConfig = toml::from_str("[general]\napply_aliases = []\n").unwrap();
        assert!(config.schemasync.is_none());
        assert!(config.typesync.outputs.is_empty());
        let config = EvenframeConfig {
            config_file_path: PathBuf::from("/project/evenframe.toml"),
            ..config
        };
        let error = config.require_schemasync().unwrap_err().to_string();
        assert!(
            error.contains("/project/evenframe.toml has no [schemasync] section"),
            "{error}"
        );
    }

    #[test]
    fn test_project_root_for_legacy_config() {
        let config = EvenframeConfig {
            general: GeneralConfig::default(),
            schemasync: None,
            typesync: crate::typesync::config::TypesyncConfig::default(),
            config_file_path: PathBuf::from("/project/evenframe.toml"),
        };
        assert_eq!(config.project_root(), Path::new("/project"));
    }

    #[test]
    fn test_project_root_for_dotdir_config() {
        let config = EvenframeConfig {
            general: GeneralConfig::default(),
            schemasync: None,
            typesync: crate::typesync::config::TypesyncConfig::default(),
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
            full_refresh_mode = false


            [typesync]
            output = { kind = "arktype", dir = "./generated/" }
        "#;

        let config: EvenframeConfig = toml::from_str(toml_str).unwrap();
        assert!(config.general.apply_aliases.is_empty()); // Default
        assert_eq!(
            config.require_schemasync().unwrap().database.url,
            "http://localhost:8000"
        );
        assert_eq!(
            config.typesync.outputs,
            vec![TypesyncOutput::new(OutputKind::Arktype, "./generated/")]
        );
    }

    #[test]
    fn unknown_keys_are_rejected() {
        let base = r#"
            [schemasync]
            should_generate_mocks = false

            [schemasync.database]
            url = "http://localhost:8000"

            [typesync]
            output = { kind = "arktype", dir = "./generated/" }
        "#;
        assert!(toml::from_str::<EvenframeConfig>(base).is_ok());

        for (unknown, key) in [
            (
                "[schemasync.mock_gen_config]\ndefault_batch_size = 10",
                "default_batch_size",
            ),
            (
                "[schemasync.performance]\ncache_duration_seconds = 60",
                "performance",
            ),
            ("[general]\napply_alias = [\"Typo\"]", "apply_alias"),
            ("[unknown_section]\nkey = 1", "unknown_section"),
        ] {
            let error = toml::from_str::<EvenframeConfig>(&format!("{base}\n{unknown}"))
                .expect_err(unknown)
                .to_string();
            assert!(error.contains(key), "{error}");
        }

        let output_typo = base.replace(
            r#"dir = "./generated/" }"#,
            r#"dir = "./generated/", barel_file = true }"#,
        );
        let error = toml::from_str::<EvenframeConfig>(&output_typo)
            .expect_err("a typo in an output")
            .to_string();
        assert!(error.contains("barel_file"), "{error}");
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
            full_refresh_mode = false


            [typesync]
            outputs = [
                { kind = "arktype", dir = "./types/arktype" },
                { kind = "effect", dir = "./types/effect" },
            ]
        "#;

        let config: EvenframeConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.general.apply_aliases.len(), 1);
        assert_eq!(config.general.apply_aliases[0], "MyAlias");
        assert!(config.require_schemasync().unwrap().should_generate_mocks);
        let kinds: Vec<OutputKind> = config.typesync.outputs.iter().map(|o| o.kind).collect();
        assert_eq!(kinds, vec![OutputKind::Arktype, OutputKind::Effect]);
    }

    #[test]
    fn struct_variants_default_to_named_and_accept_inline() {
        let parse = |typesync: &str| toml::from_str::<EvenframeConfig>(typesync);
        let named = parse("[typesync]\noutputs = []\n").unwrap();
        assert_eq!(
            named.typesync.struct_variants,
            crate::typesync::config::StructVariants::Named
        );
        let inline = parse("[typesync]\noutputs = []\nstruct_variants = \"inline\"\n").unwrap();
        assert_eq!(
            inline.typesync.struct_variants,
            crate::typesync::config::StructVariants::Inline
        );
        assert!(parse("[typesync]\nstruct_variants = \"flattened\"\n").is_err());
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
            full_refresh_mode = false


            [typesync]
            outputs = []
        "#;
        fs::write(temp_dir.path().join("evenframe.toml"), config_content).unwrap();

        let original_dir = env::current_dir().unwrap();
        env::set_current_dir(temp_dir.path()).unwrap();

        let result = EvenframeConfig::new();

        env::set_current_dir(original_dir).unwrap();

        assert!(result.is_ok());
        let config = result.unwrap();
        assert_eq!(
            config.require_schemasync().unwrap().database.namespace,
            "test_ns"
        );
        assert_eq!(
            config.require_schemasync().unwrap().database.database,
            "test_db"
        );
    }

    fn config_with_env_refs(url: &str, output_path: &str) -> EvenframeConfig {
        let content = format!(
            r#"
            [schemasync]
            should_generate_mocks = false

            [schemasync.database]
            provider = "surrealdb"
            url = "{url}"
            namespace = "${{EF_TEST_OFFLINE_UNSET_NS}}"
            database = "${{EF_TEST_OFFLINE_UNSET_DB:-fallback_db}}"

            [schemasync.mock_gen_config]
            default_record_count = 10
            default_preservation_mode = "Smart"
            full_refresh_mode = false


            [typesync]
            output = {{ kind = "arktype", dir = "{output_path}" }}
            "#
        );
        toml::from_str(&content).unwrap()
    }

    #[test]
    fn offline_substitution_leaves_unset_connection_vars_unresolved() {
        let mut config = config_with_env_refs("${EF_TEST_OFFLINE_UNSET_URL}", "./output/");

        let strict = EvenframeConfig::substitute_all_env_vars(&mut config.clone(), true);
        assert!(
            matches!(strict, Err(EvenframeError::EnvVarNotSet(_))),
            "online loading must still require the connection variables"
        );

        EvenframeConfig::substitute_all_env_vars(&mut config, false).unwrap();
        let database = &config.require_schemasync().unwrap().database;
        assert_eq!(database.url, "${EF_TEST_OFFLINE_UNSET_URL}");
        assert_eq!(database.namespace, "${EF_TEST_OFFLINE_UNSET_NS}");
        assert_eq!(database.database, "fallback_db");
    }

    #[test]
    fn offline_substitution_still_requires_other_vars() {
        let mut config = config_with_env_refs(
            "${EF_TEST_OFFLINE_UNSET_URL}",
            "${EF_TEST_OFFLINE_UNSET_OUTPUT}",
        );
        let result = EvenframeConfig::substitute_all_env_vars(&mut config, false);
        assert!(
            matches!(&result, Err(EvenframeError::EnvVarNotSet(name)) if name == "EF_TEST_OFFLINE_UNSET_OUTPUT"),
            "unexpected result: {result:?}"
        );
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
