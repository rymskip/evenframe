use crate::error::{EvenframeError, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};
use tracing::{debug, info, trace, warn};

mod chain;
pub mod workspace;

pub use chain::{chain_ending_at, find_config_chain_from};

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

/// A foreign type's mapping in one TypeScript output.
pub trait TsOutputMapping {
    /// The import the mapped type needs, if any.
    fn import(&self) -> Option<&TsImport>;
}

impl TsOutputMapping for TsMapping {
    fn import(&self) -> Option<&TsImport> {
        self.import.as_ref()
    }
}

impl TsOutputMapping for EffectMapping {
    fn import(&self) -> Option<&TsImport> {
        self.import.as_ref()
    }
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
    pub default_value_surql: String,

    // --- Mock data generation strategy ---
    /// One of: "datetime", "duration", "timezone", "decimal", "record_id", "string"
    #[serde(default)]
    pub mock_strategy: String,

    // --- Endec format annotation ---
    /// If set, generates `@endec({ format: "..." })` in macroforge output
    #[serde(default)]
    pub endec_format: String,
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

/// The foreign type config for evenframe's record link, which a project may
/// configure to own its TypeScript definition.
pub const RECORD_LINK: &str = "RecordLink";

/// The foreign type config for the SurrealDB SDK's `RecordId`, which a record
/// link holds. Evenframe's own `RecordLink` writes its id as this type does.
pub const RECORD_ID: &str = "RecordId";

/// `type_expr` with `{0}`, `{1}` and so on replaced by `params`. Config
/// loading rejects a placeholder no parameter can fill.
pub fn fill(type_expr: &str, params: &[String]) -> String {
    params
        .iter()
        .enumerate()
        .fold(type_expr.to_string(), |filled, (index, param)| {
            filled.replace(&format!("{{{index}}}"), param)
        })
}

/// The generic parameter indices `type_expr` has placeholders for.
pub fn placeholders(type_expr: &str) -> Vec<usize> {
    type_expr
        .split('{')
        .skip(1)
        .filter_map(|rest| rest.split_once('}'))
        .filter_map(|(digits, _)| digits.parse().ok())
        .collect()
}

/// Rejects foreign type entries evenframe cannot use: a placeholder no generic
/// parameter fills, and a `RecordLink` entry that sets more than its
/// TypeScript side, since the record link's schema and mock data are
/// evenframe's own.
pub fn validate_foreign_types(
    foreign_types: &BTreeMap<String, ForeignTypeConfig>,
) -> std::result::Result<(), String> {
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
            endec_format: foreign.endec_format.clone(),
            ..Default::default()
        };
        if record_link && &typescript_only != foreign {
            return Err(format!(
                "foreign_types.{RECORD_LINK} can only set arktype, effect, macroforge, crate and \
                 endec_format: the record link's Rust names, schema and mock data are \
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

    /// The file this entry names, with a relative path joined to
    /// `project_root` and an absolute one taken as is.
    pub fn resolve(&self, project_root: &Path) -> IncludeFile {
        let path = Path::new(self.path());
        IncludeFile {
            path: if path.is_absolute() {
                path.to_path_buf()
            } else {
                project_root.join(path)
            },
            resolve_only: self.resolve_only(),
        }
    }
}

/// A file outside the scan subtree to additionally parse for Evenframe types,
/// with its path resolved to absolute. [`IncludeFileSpec`] is the form the
/// config file takes.
#[derive(Debug, Clone)]
pub struct IncludeFile {
    /// Absolute path to the `.rs` file to parse.
    pub path: PathBuf,
    /// Register the file's types for resolution only (do not emit tables/TS).
    pub resolve_only: bool,
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

    /// Files inside the scan subtree to leave out of it, such as a type a
    /// crate defines that another project of the workspace owns. Paths are
    /// relative to the project root unless absolute.
    #[serde(default)]
    pub exclude_files: Vec<String>,

    /// When true, run `cargo expand` on each crate before scanning for types.
    /// This allows Evenframe to discover types generated by macros.
    /// Requires `cargo-expand` to be installed (`cargo install cargo-expand`).
    #[serde(default)]
    pub expand_macros: bool,

    /// Path to the .env file, relative to the directory of the config file
    /// that sets it. Defaults to `.env` in that config's project root.
    #[serde(default)]
    pub env_path: Option<String>,

    /// The project directories of a workspace, relative to this config's
    /// project root. Each project keeps its own config, which inherits this
    /// one's settings, and one run syncs every project's database and writes
    /// a single typesync output for all of them. Never inherited: it belongs
    /// to the config that declares it.
    #[serde(default)]
    pub projects: Vec<String>,

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
    /// every `.env` the configuration's chain names, or a `.env` found from
    /// the current directory when there is no project yet.
    pub fn load_env_early() {
        match Self::find_config_file() {
            Ok(nearest) => {
                let links: Result<Vec<_>> = chain_ending_at(&nearest)
                    .iter()
                    .map(|path| chain::Link::read(path))
                    .collect();
                match links {
                    Ok(links) => chain::load_envs(&links),
                    // The full load reports it, with the command's context.
                    Err(error) => warn!("Skipped the early .env load: {error}"),
                }
            }
            Err(_) => match dotenvy::dotenv() {
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
        Self::load_from(Self::find_config_file()?, require_connection_env)
    }

    /// Loads the configuration whose nearest file is `config_path`, merged
    /// over every config file in its ancestors (see [`chain_ending_at`]).
    /// With `require_connection_env` false, as for [`Self::new_offline`], the
    /// database connection settings may reference unset variables.
    pub fn load_from(
        config_path: PathBuf,
        require_connection_env: bool,
    ) -> Result<EvenframeConfig> {
        let chain = chain_ending_at(&config_path);
        info!(
            "Loading configuration from {}",
            chain
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(" < ")
        );
        let links = chain
            .iter()
            .map(|path| chain::Link::read(path))
            .collect::<Result<Vec<_>>>()?;
        Self::finish(chain::merge_links(links, require_connection_env)?)
    }

    /// Parses configuration text read from `config_path` on its own, without
    /// the configs above it: loads its `.env`, substitutes environment
    /// references in every string setting, and reads the SurrealQL files the
    /// configuration names.
    pub fn parse(
        contents: &str,
        config_path: PathBuf,
        require_connection_env: bool,
    ) -> Result<EvenframeConfig> {
        let link = chain::Link::parse(config_path, contents.to_string())?;
        Self::finish(chain::merge_links(vec![link], require_connection_env)?)
    }

    /// Reads the SurrealQL files a merged configuration names.
    fn finish(mut config: EvenframeConfig) -> Result<EvenframeConfig> {
        let project_root = config.project_root().to_path_buf();
        if let Some(schemasync) = config.schemasync.as_mut() {
            schemasync.project_root = project_root.clone();
            let database = &mut schemasync.database;
            if let crate::schemasync::config::AccessesSource::Path { ref path } = database.accesses
            {
                database.resolved.access_surql =
                    Some(Self::load_surql_from_path(&project_root, path)?);
            }
            if let Some(ref functions) = database.functions {
                database.resolved.functions_surql =
                    Some(Self::load_surql_from_path(&project_root, &functions.path)?);
            }
            if let Some(ref analyzers) = database.analyzers {
                database.resolved.analyzers_surql =
                    Some(Self::load_surql_from_path(&project_root, &analyzers.path)?);
            }
        }
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

    /// Substitutes environment references in every string under `value`,
    /// whose position in the document is `path`. A value is substituted as
    /// data, never as TOML, so quotes and backslashes in it are kept. When
    /// `offline`, the database connection settings leave references to unset
    /// variables as written.
    fn substitute_strings(value: &mut toml::Value, path: &[&str], offline: bool) -> Result<()> {
        match value {
            toml::Value::String(text) => {
                let lenient = offline
                    && matches!(
                        path,
                        ["schemasync", "database", "url" | "namespace" | "database"]
                    );
                *text = Self::substitute_env_vars_inner(text, !lenient)?;
            }
            toml::Value::Array(items) => {
                for item in items {
                    Self::substitute_strings(item, path, offline)?;
                }
            }
            toml::Value::Table(table) => {
                for (key, item) in table.iter_mut() {
                    let child: Vec<&str> = path.iter().copied().chain([key.as_str()]).collect();
                    Self::substitute_strings(item, &child, offline)?;
                }
            }
            toml::Value::Integer(_)
            | toml::Value::Float(_)
            | toml::Value::Boolean(_)
            | toml::Value::Datetime(_) => {}
        }
        Ok(())
    }

    /// The configuration file: the one chosen with [`Self::use_config_file`],
    /// or else `.evenframe/config.toml` (preferred) or `evenframe.toml`,
    /// searching upward from the current directory.
    pub fn find_config_file() -> Result<PathBuf> {
        if let Some(path) = CONFIG_FILE.get() {
            return Ok(path.clone());
        }
        Self::find_config_file_from(&env::current_dir()?).ok_or_else(|| {
            EvenframeError::config(
                "Configuration file not found. Expected '.evenframe/config.toml' or 'evenframe.toml' in current or any parent directory.",
            )
        })
    }

    /// `.evenframe/config.toml` (preferred) or `evenframe.toml`, in `start` or
    /// the nearest of its ancestors that has one.
    pub fn find_config_file_from(start: &Path) -> Option<PathBuf> {
        debug!("Starting config file search from: {:?}", start);
        start.ancestors().find_map(chain::config_in)
    }

    /// `general.foreign_types` of the configuration that `start` resolves to,
    /// merged over the whole chain, without loading a `.env` or substituting
    /// a variable: for a program that needs the type mappings and nothing
    /// that names its environment, such as a server reading them at runtime.
    pub fn foreign_types_from(start: &Path) -> Result<BTreeMap<String, ForeignTypeConfig>> {
        let nearest = Self::find_config_file_from(start).ok_or_else(|| {
            EvenframeError::config(format!(
                "No .evenframe/config.toml or evenframe.toml in {} or any parent directory",
                start.display()
            ))
        })?;
        let links = chain_ending_at(&nearest)
            .iter()
            .map(|path| chain::Link::read(path))
            .collect::<Result<Vec<_>>>()?;
        chain::merged_foreign_types(links)
    }

    /// The directory a run locks: the project root of the outermost config in
    /// the chain, so runs anywhere in one workspace serialize against each
    /// other. `None` when there is no config.
    pub fn find_lock_root() -> Option<PathBuf> {
        let nearest = Self::find_config_file().ok()?;
        let outermost = chain_ending_at(&nearest).into_iter().next()?;
        Some(Self::project_root_of(&outermost).to_path_buf())
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

    /// `general.include_files`, resolved against the project root.
    pub fn resolved_include_files(&self) -> Vec<IncludeFile> {
        self.general
            .include_files
            .iter()
            .map(|spec| spec.resolve(self.project_root()))
            .collect()
    }

    /// `general.exclude_files`, resolved against the project root.
    pub fn resolved_exclude_files(&self) -> Vec<PathBuf> {
        self.general
            .exclude_files
            .iter()
            .map(|path| self.project_root().join(path).components().collect())
            .collect()
    }

    /// Resolves the .env file path based on config.
    /// If `general.env_path` is set, resolves it relative to the config file's directory.
    /// Otherwise defaults to `<project_root>/.env`.
    pub fn resolve_env_path(&self) -> PathBuf {
        chain::env_path_for(&self.config_file_path, self.general.env_path.as_deref())
    }

    /// Load surql content from a file or directory path, with env var substitution.
    /// If the path points to a file, reads its contents.
    /// If it points to a directory, reads all `*.surql` files sorted by name and concatenates them.
    pub fn load_surql_from_path(project_root: &Path, relative_path: &str) -> Result<String> {
        let full_path = project_root.join(relative_path);
        debug!("Loading surql from path: {:?}", full_path);

        let content = if full_path.is_dir() {
            let mut entries: Vec<_> = fs::read_dir(&full_path)
                .map_err(|error| {
                    EvenframeError::config(format!(
                        "Failed to read directory {:?}: {}",
                        full_path, error
                    ))
                })?
                .filter(|entry| {
                    entry.as_ref().map_or(true, |entry| {
                        entry
                            .path()
                            .extension()
                            .and_then(|extension| extension.to_str())
                            == Some("surql")
                    })
                })
                .collect::<std::io::Result<Vec<_>>>()
                .map_err(|error| {
                    EvenframeError::config(format!(
                        "Failed to read directory {full_path:?}: {error}"
                    ))
                })?;
            entries.sort_by_key(|entry| entry.file_name());

            if entries.is_empty() {
                return Err(EvenframeError::config(format!(
                    "No .surql files found in directory {:?}",
                    full_path
                )));
            }

            let mut combined = String::new();
            for entry in entries {
                let file_content = fs::read_to_string(entry.path()).map_err(|error| {
                    EvenframeError::config(format!("Failed to read {:?}: {}", entry.path(), error))
                })?;
                if !combined.is_empty() {
                    combined.push('\n');
                }
                combined.push_str(&file_content);
            }
            combined
        } else if full_path.is_file() {
            fs::read_to_string(&full_path).map_err(|error| {
                EvenframeError::config(format!("Failed to read {:?}: {}", full_path, error))
            })?
        } else {
            return Err(EvenframeError::config(format!(
                "Surql path does not exist: {:?}",
                full_path
            )));
        };

        Self::substitute_env_vars(&content)
    }
    /// Substitute environment variables in config strings
    /// Supports ${VAR_NAME:-default} syntax
    pub fn substitute_env_vars(value: &str) -> Result<String> {
        Self::substitute_env_vars_inner(value, true)
    }

    /// With `strict` false, a reference to an unset variable without a default
    /// is left in place instead of being an error. Values are substituted in
    /// one pass, so a value that itself contains `${...}` is kept as written.
    fn substitute_env_vars_inner(value: &str, strict: bool) -> Result<String> {
        trace!("Substituting environment variables in: {}", value);
        let mut result = String::with_capacity(value.len());
        let mut copied = 0;
        for reference in ENV_REFERENCE.captures_iter(value) {
            let whole = reference.get_match();
            let name = &reference[1];
            let replacement = match env::var(name) {
                Ok(set) => {
                    debug!("Resolved environment variable: {name}");
                    set
                }
                Err(env::VarError::NotUnicode(_)) => {
                    return Err(EvenframeError::config(format!(
                        "Environment variable {name} is not valid UTF-8"
                    )));
                }
                Err(env::VarError::NotPresent) => match reference.get(2) {
                    Some(default) => {
                        debug!("Environment variable {name} not set, using its default");
                        default.as_str().to_string()
                    }
                    None if !strict => {
                        debug!("Environment variable {name} not set, leaving the reference");
                        continue;
                    }
                    None => return Err(EvenframeError::EnvVarNotSet(name.to_string())),
                },
            };
            result.push_str(&value[copied..whole.start()]);
            result.push_str(&replacement);
            copied = whole.end();
        }
        result.push_str(&value[copied..]);
        Ok(result)
    }
}

/// `${VAR}` or `${VAR:-default}` naming an environment variable (uppercase
/// letters, digits and underscores), so JavaScript template literals like
/// `${foo.bar()}` are left alone.
static ENV_REFERENCE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"\$\{([A-Z_][A-Z0-9_]*)(?::-([^}]*))?\}")
        .expect("the environment reference pattern is valid")
});

#[cfg(test)]
mod tests;
