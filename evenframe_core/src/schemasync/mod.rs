// SchemaSync - Database schema synchronization

// Always compiled: pure data types needed by derive macros
#[cfg(feature = "schemasync")]
pub mod compare;
pub mod config;
#[cfg(feature = "schemasync")]
pub mod database;
pub mod define_config;
#[cfg(feature = "schemasync")]
pub mod dump;
pub mod edge;
pub mod event;
pub mod lint;
pub mod mockmake;
pub mod permissions;
pub mod table;

// Re-export commonly used types (always available)
pub use define_config::DefineConfig;
pub use edge::{Direction, EdgeConfig, Subquery};
pub use event::EventConfig;
pub use mockmake::{coordinate, format};
pub use permissions::PermissionsConfig;
pub use table::{Bm25, IndexConfig, IndexKind, TableConfig, VectorDistance, VectorType};

// PreservationMode - always available (used by MockGenerationConfig data type)
#[derive(Debug, Default, Clone, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum PreservationMode {
    /// No preservation - generate all new data
    None,
    #[default]
    /// Smart preservation - preserve unchanged fields, regenerate modified fields
    Smart,
    /// Full preservation - preserve all existing data, only generate for new fields
    Full,
}

impl quote::ToTokens for PreservationMode {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let variant_tokens = match self {
            PreservationMode::None => {
                quote::quote! { ::evenframe::schemasync::PreservationMode::None }
            }
            PreservationMode::Smart => {
                quote::quote! { ::evenframe::schemasync::PreservationMode::Smart }
            }
            PreservationMode::Full => {
                quote::quote! { ::evenframe::schemasync::PreservationMode::Full }
            }
        };
        tokens.extend(variant_tokens);
    }
}

// Schemasync orchestrator: requires surrealdb at runtime
#[cfg(feature = "schemasync")]
use crate::{
    config::EvenframeConfig,
    error::{EvenframeError, Result},
    schemasync::compare::SchemaChanges,
    schemasync::config::ConnectionOverrides,
    schemasync::database::surql::{
        define::generate_define_statements,
        execute::{execute_and_validate, split_surql_statements},
    },
};
#[cfg(feature = "schemasync")]
use std::collections::BTreeMap;
#[cfg(feature = "schemasync")]
use tracing::{debug, error, info, trace, warn};

#[cfg(feature = "schemasync")]
use surrealdb::{
    Surreal,
    engine::remote::http::{Client, Http},
    opt::auth::Root,
};

#[cfg(feature = "schemasync")]
use crate::{
    evenframe_log,
    schemasync::mockmake::Mockmaker,
    types::{StructConfig, TaggedUnion},
};

#[cfg(feature = "schemasync")]
#[derive(Default)]
pub struct Schemasync<'a> {
    // Input parameters - set via builder methods
    tables: Option<&'a BTreeMap<String, TableConfig>>,
    objects: Option<&'a BTreeMap<String, StructConfig>>,
    enums: Option<&'a BTreeMap<String, TaggedUnion>>,
    registry: Option<&'a crate::types::ForeignTypeRegistry>,

    // Internal state - initialized automatically
    db: Option<Surreal<Client>>,
    schemasync_config: Option<crate::schemasync::config::SchemasyncConfig>,
    /// Owned registry built from config during initialization (used when no external registry is provided)
    owned_registry: Option<crate::types::ForeignTypeRegistry>,
    connection_overrides: ConnectionOverrides,
}

/// Load the config for a command that connects to the database: connection
/// settings may come from `overrides` instead of environment variables, but
/// must be fully resolved one way or the other.
#[cfg(feature = "schemasync")]
pub fn load_connected_config(overrides: &ConnectionOverrides) -> Result<EvenframeConfig> {
    let mut config = EvenframeConfig::new_offline()?;
    config
        .schemasync
        .database
        .apply_connection_overrides(overrides);
    if let Some(var) = config.schemasync.database.unresolved_connection_var() {
        return Err(EvenframeError::EnvVarNotSet(var));
    }
    Ok(config)
}

#[cfg(feature = "schemasync")]
static ACTIVE_CONNECTION: std::sync::RwLock<Option<crate::schemasync::config::DatabaseConfig>> =
    std::sync::RwLock::new(None);

/// The connection settings last used by [`connect_database`] in this
/// process, for code that must reach the same database by other means
/// (e.g. the `surreal import` fallback for oversized statements).
#[cfg(feature = "schemasync")]
pub fn active_connection() -> Option<crate::schemasync::config::DatabaseConfig> {
    ACTIVE_CONNECTION.read().ok().and_then(|c| c.clone())
}

/// Connect to SurrealDB over HTTP, sign in as root with `SURREALDB_USER` /
/// `SURREALDB_PASSWORD`, and select the configured namespace and database.
#[cfg(feature = "schemasync")]
pub async fn connect_database(
    database: &crate::schemasync::config::DatabaseConfig,
) -> Result<Surreal<Client>> {
    trace!("Database URL: {}", database.url);
    trace!("Database namespace: {}", database.namespace);
    trace!("Database name: {}", database.database);

    // The HTTP engine wants a bare `host:port`; tolerate a configured
    // `http(s)://` scheme (as shipped in .env.example) by stripping it.
    let endpoint = database
        .url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let db = Surreal::new::<Http>(endpoint).await.map_err(|e| {
        EvenframeError::database(format!(
            "There was a problem creating the HTTP surrealdb client: {e}"
        ))
    })?;
    debug!("Created SurrealDB connection");

    let username = std::env::var("SURREALDB_USER")
        .map_err(|_| EvenframeError::EnvVarNotSet("SURREALDB_USER".to_string()))?;
    let password = std::env::var("SURREALDB_PASSWORD")
        .map_err(|_| EvenframeError::EnvVarNotSet("SURREALDB_PASSWORD".to_string()))?;
    debug!("Retrieved database credentials from environment");

    db.signin(Root { username, password }).await.map_err(|e| {
        EvenframeError::database(format!("There was a problem signing in as root: {e}"))
    })?;
    debug!("Successfully signed in to SurrealDB");

    db.use_ns(&database.namespace)
        .use_db(&database.database)
        .await
        .map_err(|e| {
            EvenframeError::database(format!("There was a problem using to the namespace: {e}"))
        })?;
    info!(
        "Connected to database namespace '{}' and database '{}'",
        database.namespace, database.database
    );
    if let Ok(mut active) = ACTIVE_CONNECTION.write() {
        *active = Some(database.clone());
    }
    Ok(db)
}

/// Check database connectivity by loading config, connecting, authenticating,
/// and selecting the configured namespace/database.
#[cfg(feature = "schemasync")]
pub async fn check_database_connectivity() -> Result<()> {
    let config = EvenframeConfig::new()?;

    info!(
        "Connecting to SurrealDB at {}...",
        config.schemasync.database.url
    );
    let db = Surreal::new::<Http>(&config.schemasync.database.url)
        .await
        .map_err(|e| {
            EvenframeError::database(format!(
                "Failed to connect to SurrealDB at {}: {e}",
                config.schemasync.database.url
            ))
        })?;
    info!("    Connection: OK");

    let username = std::env::var("SURREALDB_USER")
        .map_err(|_| EvenframeError::EnvVarNotSet("SURREALDB_USER".to_string()))?;
    let password = std::env::var("SURREALDB_PASSWORD")
        .map_err(|_| EvenframeError::EnvVarNotSet("SURREALDB_PASSWORD".to_string()))?;

    db.signin(Root { username, password })
        .await
        .map_err(|e| EvenframeError::database(format!("Failed to authenticate: {e}")))?;
    info!("    Authentication: OK");

    db.use_ns(&config.schemasync.database.namespace)
        .use_db(&config.schemasync.database.database)
        .await
        .map_err(|e| {
            EvenframeError::database(format!(
                "Failed to select namespace '{}' / database '{}': {e}",
                config.schemasync.database.namespace, config.schemasync.database.database
            ))
        })?;
    info!(
        "    Namespace '{}' / Database '{}': OK",
        config.schemasync.database.namespace, config.schemasync.database.database
    );

    Ok(())
}

#[cfg(feature = "schemasync")]
impl<'a> Schemasync<'a> {
    /// Create a new empty Schemasync instance
    pub fn new() -> Self {
        trace!("Creating new Schemasync instance");
        Self {
            tables: None,
            objects: None,
            enums: None,
            registry: None,
            db: None,
            schemasync_config: None,
            owned_registry: None,
            connection_overrides: ConnectionOverrides::default(),
        }
    }

    /// Builder methods for setting up the parameters
    pub fn with_tables(mut self, tables: &'a BTreeMap<String, TableConfig>) -> Self {
        debug!("Configuring Schemasync with {} tables", tables.len());
        trace!("Table names: {:?}", tables.keys().collect::<Vec<_>>());
        self.tables = Some(tables);
        self
    }

    pub fn with_objects(mut self, objects: &'a BTreeMap<String, StructConfig>) -> Self {
        debug!("Configuring Schemasync with {} objects", objects.len());
        trace!("Object names: {:?}", objects.keys().collect::<Vec<_>>());
        self.objects = Some(objects);
        self
    }

    pub fn with_enums(mut self, enums: &'a BTreeMap<String, TaggedUnion>) -> Self {
        debug!("Configuring Schemasync with {} enums", enums.len());
        trace!("Enum names: {:?}", enums.keys().collect::<Vec<_>>());
        self.enums = Some(enums);
        self
    }

    pub fn with_registry(mut self, registry: &'a crate::types::ForeignTypeRegistry) -> Self {
        debug!("Configuring Schemasync with ForeignTypeRegistry");
        self.registry = Some(registry);
        self
    }

    /// Replace the configured connection settings (e.g. from CLI flags).
    pub fn with_connection_overrides(mut self, overrides: ConnectionOverrides) -> Self {
        self.connection_overrides = overrides;
        self
    }

    /// Initialize database connection and config from environment
    async fn initialize(&mut self) -> Result<()> {
        info!("Initializing Schemasync database connection and configuration");
        let config = load_connected_config(&self.connection_overrides)?;
        debug!("Loaded Evenframe configuration successfully");

        let db = connect_database(&config.schemasync.database).await?;

        self.db = Some(db);
        // Build a ForeignTypeRegistry from config if no external registry was provided
        if self.registry.is_none() {
            let registry =
                crate::types::ForeignTypeRegistry::from_config(&config.general.foreign_types);
            debug!("Built ForeignTypeRegistry from EvenframeConfig foreign_types");
            self.owned_registry = Some(registry);
        }
        self.schemasync_config = Some(config.schemasync);
        debug!("Schemasync initialization completed successfully");

        Ok(())
    }

    /// Validate that all required fields are set and return them.
    #[allow(clippy::type_complexity)]
    fn validate(
        &mut self,
    ) -> Result<(
        Surreal<Client>,
        &'a BTreeMap<String, TableConfig>,
        &'a BTreeMap<String, StructConfig>,
        &'a BTreeMap<String, TaggedUnion>,
        crate::schemasync::config::SchemasyncConfig,
    )> {
        debug!("Validating required fields for Schemasync pipeline");
        let db = self
            .db
            .take()
            .ok_or_else(|| EvenframeError::config("Database connection failed to initialize"))?;
        let tables = self
            .tables
            .ok_or_else(|| EvenframeError::config("Tables not provided"))?;
        let objects = self
            .objects
            .ok_or_else(|| EvenframeError::config("Objects not provided"))?;
        let enums = self
            .enums
            .ok_or_else(|| EvenframeError::config("Enums not provided"))?;
        let config = self
            .schemasync_config
            .take()
            .ok_or_else(|| EvenframeError::config("Config failed to initialize"))?;

        if tables.is_empty() {
            return Err(EvenframeError::config(
                "No Evenframe tables found. Ensure your structs have #[derive(Evenframe)] and contain an `id` field.",
            ));
        }

        info!(
            "Pipeline validation completed - {} tables, {} objects, {} enums",
            tables.len(),
            objects.len(),
            enums.len()
        );

        // Surface `#[define_field_statement(...)]` settings that schema
        // generation silently discards. Structs materialized as tables are
        // exempt (their DEFINE FIELDs honor every setting); see the lint
        // module docs for the exact classification rules.
        use crate::schemasync::lint::DiscardedContext;
        for finding in
            crate::schemasync::lint::lint_discarded_field_annotations(tables, objects, enums)
        {
            match &finding.context {
                DiscardedContext::EmbeddedObject => warn!(
                    struct_name = %finding.struct_name,
                    field = %finding.field_name,
                    discarded = ?finding.discarded,
                    "`#[define_field_statement]` on embedded struct field `{}.{}` is ignored: \
                     evenframe inlines embedded structs into the parent field, so per-subfield \
                     settings {:?} are never emitted. Gate the parent field instead, or promote \
                     this struct to its own table.",
                    finding.struct_name, finding.field_name, finding.discarded,
                ),
                DiscardedContext::EnumVariantPayload { enum_name } => warn!(
                    enum_name = %enum_name,
                    variant = %finding.struct_name,
                    field = %finding.field_name,
                    discarded = ?finding.discarded,
                    "`#[define_field_statement]` on enum variant payload field `{}::{}.{}` is \
                     ignored: variant payloads are inlined into the enum's literal type, so \
                     settings {:?} are never emitted (a payload `default` only takes effect on \
                     the enum's default variant). Extract the payload into its own table, or \
                     gate the parent field.",
                    enum_name, finding.struct_name, finding.field_name, finding.discarded,
                ),
                DiscardedContext::ResolveOnlyTable => {
                    if config.lint.silence_unverifiable_annotations {
                        continue;
                    }
                    warn!(
                        struct_name = %finding.struct_name,
                        field = %finding.field_name,
                        discarded = ?finding.discarded,
                        "`#[define_field_statement]` on `{}.{}` has no effect in this run: \
                         `{}` comes from a resolve_only include, so this project inlines it \
                         as an embedded object and discards {:?}. Whether the project that \
                         owns `{}` materializes the table and honors these settings cannot \
                         be determined from this run. Silence with \
                         `silence_unverifiable_annotations = true` under `[schemasync.lint]`.",
                        finding.struct_name, finding.field_name, finding.struct_name,
                        finding.discarded, finding.struct_name,
                    );
                }
            }
        }

        Ok((db, tables, objects, enums, config))
    }

    /// Generate define statements for all tables.
    fn generate_all_define_statements<'b>(
        tables: &'b BTreeMap<String, TableConfig>,
        objects: &BTreeMap<String, StructConfig>,
        enums: &BTreeMap<String, TaggedUnion>,
        full_refresh_mode: bool,
        registry: &crate::types::ForeignTypeRegistry,
        allow_scripting: bool,
    ) -> (BTreeMap<&'b String, String>, String) {
        debug!(
            "Generating table and field definition statements (full_refresh_mode: {}, allow_scripting: {})",
            full_refresh_mode, allow_scripting
        );
        let mut define_statements: BTreeMap<&String, String> = BTreeMap::new();
        for (table_name, table) in tables {
            let table = table.effective();
            define_statements.insert(
                table_name,
                generate_define_statements(
                    table_name,
                    table,
                    tables,
                    objects,
                    enums,
                    registry,
                    allow_scripting,
                ),
            );
        }

        let define_statements_string = define_statements
            .values()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(" ");

        (define_statements, define_statements_string)
    }

    /// Run the comparison pipeline and return schema changes without applying them.
    pub async fn diff(mut self) -> Result<SchemaChanges> {
        info!("Starting schema diff");
        self.initialize().await?;

        let (db, tables, objects, enums, config) = self.validate()?;
        let default_registry = crate::types::ForeignTypeRegistry::default();
        let registry = self
            .registry
            .or(self.owned_registry.as_ref())
            .unwrap_or(&default_registry);

        let (_, define_statements_string) = Self::generate_all_define_statements(
            tables,
            objects,
            enums,
            config.mock_gen_config.full_refresh_mode,
            registry,
            config.mock_gen_config.scripting_asserts,
        );

        let mut mockmaker = Mockmaker::new(&db, tables, objects, enums, &config, registry);
        mockmaker.generate_ids().await?;

        if let Some(ref mut comparator) = mockmaker.comparator {
            comparator.run(&define_statements_string).await?;
        }

        let schema_changes = mockmaker
            .comparator
            .as_ref()
            .and_then(|c| c.get_schema_changes())
            .cloned()
            .ok_or_else(|| EvenframeError::config("Schema changes not computed"))?;

        info!("Schema diff completed");
        Ok(schema_changes)
    }

    /// Connect to the database and generate mock data without applying schema changes.
    pub async fn mock_only(
        mut self,
        count_override: Option<usize>,
        table_filter: Option<Vec<String>>,
    ) -> Result<()> {
        info!("Starting mock-only generation");
        self.initialize().await?;

        let (db, tables, objects, enums, config) = self.validate()?;
        let default_registry = crate::types::ForeignTypeRegistry::default();
        let registry = self
            .registry
            .or(self.owned_registry.as_ref())
            .unwrap_or(&default_registry);

        // Apply table filter if specified
        let owned_filtered: BTreeMap<String, TableConfig>;
        let effective_tables: &BTreeMap<String, TableConfig> =
            if let Some(ref filter) = table_filter {
                owned_filtered = tables
                    .iter()
                    .filter(|(name, _)| filter.contains(name))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                if owned_filtered.is_empty() {
                    return Err(EvenframeError::config(
                        "No tables match the specified filter",
                    ));
                }
                &owned_filtered
            } else {
                tables
            };

        let (_, define_statements_string) = Self::generate_all_define_statements(
            effective_tables,
            objects,
            enums,
            config.mock_gen_config.full_refresh_mode,
            registry,
            config.mock_gen_config.scripting_asserts,
        );

        let mut mockmaker =
            Mockmaker::new(&db, effective_tables, objects, enums, &config, registry);
        mockmaker.count_override = count_override;
        mockmaker.generate_ids().await?;

        if let Some(ref mut comparator) = mockmaker.comparator {
            comparator.run(&define_statements_string).await?;
        }

        mockmaker.filter_changes().await?;
        mockmaker.generate_coordinated_values();
        mockmaker.generate_mock_data().await?;

        info!("Mock-only generation completed successfully");
        Ok(())
    }

    /// Insert mock data into the selected tables (all when `table_filter` is
    /// `None`) of a database whose schema is already in place. Unlike
    /// [`Self::mock_only`] this never diffs or defines anything: each selected
    /// table is brought to its record count (`count_override`, the table's
    /// `#[mock_data(n = ...)]`, or `default_record_count`), regenerating the
    /// records it already has and adding the rest.
    pub async fn insert_mock_data(
        mut self,
        count_override: Option<usize>,
        table_filter: Option<Vec<String>>,
    ) -> Result<()> {
        info!("Inserting mock data");
        self.initialize().await?;

        let (db, tables, objects, enums, mut config) = self.validate()?;
        let default_registry = crate::types::ForeignTypeRegistry::default();
        let registry = self
            .registry
            .or(self.owned_registry.as_ref())
            .unwrap_or(&default_registry);

        // Existing records must stay link targets, and this command exists
        // to generate data.
        config.mock_gen_config.full_refresh_mode = false;
        config.should_generate_mocks = true;

        let selected: Option<std::collections::BTreeSet<String>> = match table_filter {
            None => None,
            Some(names) => {
                let unknown: Vec<&String> =
                    names.iter().filter(|n| !tables.contains_key(*n)).collect();
                if !unknown.is_empty() {
                    let known: Vec<&String> = tables.keys().collect();
                    return Err(EvenframeError::config(format!(
                        "Unknown tables {unknown:?}; known tables: {known:?}"
                    )));
                }
                Some(names.into_iter().collect())
            }
        };

        let mut mockmaker = Mockmaker::new(&db, tables, objects, enums, &config, registry);
        mockmaker.count_override = count_override;
        mockmaker.generate_ids().await?;
        mockmaker.select_tables_for_insert(selected.as_ref())?;
        mockmaker.generate_coordinated_values();
        mockmaker.generate_mock_data().await?;

        info!("Mock data inserted");
        Ok(())
    }

    /// Run the complete schemasync pipeline
    pub async fn run(mut self) -> Result<()> {
        info!("Starting Schemasync pipeline execution");
        self.initialize().await?;

        let (db, tables, objects, enums, config) = self.validate()?;
        let default_registry = crate::types::ForeignTypeRegistry::default();
        let registry = self
            .registry
            .or(self.owned_registry.as_ref())
            .unwrap_or(&default_registry);

        let (define_statements, define_statements_string) = Self::generate_all_define_statements(
            tables,
            objects,
            enums,
            config.mock_gen_config.full_refresh_mode,
            registry,
            config.mock_gen_config.scripting_asserts,
        );

        evenframe_log!("", "all_statements.surql");
        evenframe_log!("", "results.log");
        evenframe_log!("", "all_define_statements.surql");
        evenframe_log!(
            &define_statements_string,
            "all_define_statements.surql",
            true
        );

        // Create Mockmaker instance (which contains Comparator)
        info!("Creating Mockmaker instance for data generation and comparison");
        let mut mockmaker = Mockmaker::new(&db, tables, objects, enums, &config, registry);
        debug!("Mockmaker instance created successfully");

        // Run initial ID generation and comparator setup
        info!("Generating IDs for mock data");
        mockmaker.generate_ids().await?;
        debug!("ID generation completed");

        // Run the comparator pipeline
        info!("Running schema comparison pipeline");
        if let Some(ref mut comparator) = mockmaker.comparator {
            comparator.run(&define_statements_string).await?;
        }
        debug!("Schema comparison completed");

        // Continue with the rest of the mockmaker pipeline
        info!("Removing old data from database");
        mockmaker.remove_old_data().await.map_err(|e| {
            error!("Failed to remove old data: {}", e);
            e
        })?;
        debug!("Old data removal completed");

        // Execution order matters:
        // 1. Access first — defines SIGNUP/SIGNIN on the database (independent of tables)
        // 2. Analyzers second — FULLTEXT indexes on tables reference them
        // 3. Tables third — defines table schemas, fields, indexes, and events
        // 4. Functions last — function params use typed references like `record<site>`
        //    which require the referenced tables to already exist in the database

        info!("Executing access control setup");
        mockmaker.execute_access().await.map_err(|e| {
            error!("Failed to execute access setup: {}", e);
            e
        })?;
        debug!("Access control setup completed");

        info!("Executing analyzer definitions");
        self.execute_analyzers(&db, &config).await.map_err(|e| {
            error!("Failed to execute analyzers: {}", e);
            e
        })?;
        debug!("Analyzer definitions completed");

        let schema_changes = mockmaker
            .comparator
            .as_ref()
            .and_then(|c| c.get_schema_changes())
            .ok_or_else(|| EvenframeError::config("Schema changes not computed"))?;

        info!("Defining database tables and schema");
        self.define_tables(
            &db,
            define_statements,
            schema_changes,
            config.mock_gen_config.full_refresh_mode,
        )
        .await
        .map_err(|e| {
            error!("Failed to define tables: {}", e);
            e
        })?;
        debug!("Table definitions completed successfully");

        info!("Executing function definitions");
        self.execute_functions(&db, &config).await.map_err(|e| {
            error!("Failed to execute functions: {}", e);
            e
        })?;
        debug!("Function definitions completed");

        info!("Filtering schema changes");
        mockmaker.filter_changes().await.map_err(|e| {
            error!("Failed to filter changes: {}", e);
            e
        })?;
        debug!("Schema changes filtering completed");

        if config.should_generate_mocks {
            info!("Generating mock data");
            mockmaker.generate_coordinated_values();
            mockmaker.generate_mock_data().await.map_err(|e| {
                error!("Failed to generate mock data: {}", e);
                e
            })?;
        }

        debug!("Mock data generation completed");

        info!("Schemasync pipeline execution completed successfully");
        Ok(())
    }

    /// Define tables in both schemas (this stays in Schemasync)
    async fn define_tables(
        &self,
        db: &Surreal<Client>,
        define_statments: BTreeMap<&String, String>,
        schema_changes: &SchemaChanges,
        full_refresh_mode: bool,
    ) -> Result<()> {
        info!("Defining tables based on schema changes (full_refresh_mode: {full_refresh_mode})");
        debug!(
            "Schema changes before define statement execution: {:?}",
            schema_changes
        );

        // Validates individual TABLE/FIELD statements (safe to split by ';')
        let execute = async |name, stmt: &str| -> Result<()> {
            let define_result = execute_and_validate(db, stmt, "define", name).await;
            match define_result {
                Ok(_) => {
                    evenframe_log!(
                        &format!("Successfully executed define statements for statements:\n{stmt}",),
                        "results.log",
                        true
                    );
                    Ok(())
                }
                Err(e) => {
                    #[cfg(feature = "dev-mode")]
                    {
                        let error_msg =
                            format!("Failed to execute define statements for table\n{e}:\n{stmt}",);
                        evenframe_log!(&error_msg, "results.log", true);
                    }
                    Err(e.into())
                }
            }
        };

        // Events contain ';' inside { } blocks (e.g. `fn::foo($a, $b);`), so they
        // can't go through execute_and_validate which naively splits by ';' to count
        // expected results. Send event blocks directly via db.query() instead.
        let execute_events = async |table_name: &str, event_block: &str| -> Result<()> {
            debug!("Executing event definitions for table: {}", table_name);
            db.query(event_block).await.map_err(|e| {
                let error_msg = format!(
                    "Failed to execute event definitions for table {}:\n{}\n{}",
                    table_name, e, event_block
                );
                error!("{}", error_msg);
                evenframe_log!(&error_msg, "errors.log", true);
                EvenframeError::database(error_msg)
            })?;
            evenframe_log!(
                &format!(
                    "Successfully executed event definitions for table {}",
                    table_name
                ),
                "results.log",
                true
            );
            Ok(())
        };

        // In full refresh mode, define ALL tables regardless of schema changes
        if full_refresh_mode {
            info!(
                "Full refresh mode - defining all {} tables",
                define_statments.len()
            );
            for (table_name, define_stmt) in &define_statments {
                debug!("Defining table (full refresh): {}", table_name);
                // TABLE and FIELD are single-line statements, safe to split by ';'
                for stmt in split_surql_statements(define_stmt) {
                    let trimmed = stmt.trim_start();
                    if trimmed.starts_with("DEFINE TABLE")
                        || trimmed.starts_with("DEFINE FIELD")
                        || trimmed.starts_with("DEFINE INDEX")
                    {
                        execute(table_name, stmt).await?;
                    }
                }
                // Events are sent as a raw block (bypasses ';'-based validation)
                if let Some(idx) = define_stmt.find("DEFINE EVENT") {
                    execute_events(table_name, &define_stmt[idx..]).await?;
                }
            }
            return Ok(());
        }

        // Process new tables first
        if !schema_changes.new_tables.is_empty() {
            info!("Defining {} new tables", schema_changes.new_tables.len());
            for table_name in &schema_changes.new_tables {
                if let Some(define_stmt) = define_statments.get(table_name) {
                    debug!("Defining new table: {}", table_name);
                    for stmt in split_surql_statements(define_stmt) {
                        let trimmed = stmt.trim_start();
                        if trimmed.starts_with("DEFINE TABLE")
                            || trimmed.starts_with("DEFINE FIELD")
                            || trimmed.starts_with("DEFINE INDEX")
                        {
                            execute(table_name, stmt).await?;
                        }
                    }
                    if let Some(idx) = define_stmt.find("DEFINE EVENT") {
                        execute_events(table_name, &define_stmt[idx..]).await?;
                    }
                }
            }
        }

        // Process modified tables - only define changed fields
        if !schema_changes.modified_tables.is_empty() {
            info!(
                "Processing {} modified tables",
                schema_changes.modified_tables.len()
            );
            for table_change in &schema_changes.modified_tables {
                let table_name = &table_change.table_name;

                if let Some(define_stmt) = define_statments.get(table_name) {
                    debug!("Processing modified table: {}", table_name);

                    // Always redefine the table itself if it has changes
                    for stmt in split_surql_statements(define_stmt) {
                        let trimmed = stmt.trim_start();
                        if trimmed.starts_with("DEFINE TABLE") {
                            debug!("Redefining table structure for: {}", table_name);
                            execute(table_name, stmt).await?;
                        }
                    }

                    // Only define new or modified fields
                    if !table_change.new_fields.is_empty()
                        || !table_change.modified_fields.is_empty()
                    {
                        debug!(
                            "Defining {} new fields and {} modified fields for table {}",
                            table_change.new_fields.len(),
                            table_change.modified_fields.len(),
                            table_name
                        );

                        for stmt in split_surql_statements(define_stmt) {
                            let trimmed = stmt.trim_start();
                            if trimmed.starts_with("DEFINE FIELD") {
                                // Extract field name from the statement, handling optional OVERWRITE
                                // Formats:
                                //   DEFINE FIELD <name> ON TABLE ...
                                //   DEFINE FIELD OVERWRITE <name> ON TABLE ...
                                let mut tokens = trimmed.split_whitespace();
                                let _ = tokens.next(); // DEFINE
                                let _ = tokens.next(); // FIELD
                                let mut name_tok = tokens.next().unwrap_or("");
                                if name_tok.eq_ignore_ascii_case("OVERWRITE") {
                                    name_tok = tokens.next().unwrap_or("");
                                }
                                if name_tok.is_empty() {
                                    continue;
                                }
                                // Normalize backticks and wildcard suffix
                                let mut norm = name_tok.trim_matches('`');
                                if let Some(stripped) = norm.strip_suffix(".*") {
                                    norm = stripped;
                                }

                                // Check if this field is new or modified
                                if table_change.new_fields.contains(&norm.to_string())
                                    || table_change
                                        .modified_fields
                                        .iter()
                                        .any(|fc| fc.field_name == norm)
                                {
                                    trace!("Defining field: {} on table: {}", norm, table_name);
                                    execute(table_name, stmt).await?;
                                } else {
                                    trace!(
                                        "Skipping unchanged field: {} on table: {}",
                                        norm, table_name
                                    );
                                }
                            }
                        }
                    }

                    // Always redefine indexes for modified tables (idempotent with OVERWRITE)
                    for stmt in split_surql_statements(define_stmt) {
                        let trimmed = stmt.trim_start();
                        if trimmed.starts_with("DEFINE INDEX") {
                            execute(table_name, stmt).await?;
                        }
                    }

                    // Define new or changed events
                    if !table_change.new_events.is_empty() {
                        debug!(
                            "Defining {} new/changed events for table {}",
                            table_change.new_events.len(),
                            table_name
                        );

                        for event_stmt in &table_change.new_events {
                            trace!("Defining event on table: {}", table_name);
                            execute_events(table_name, event_stmt).await?;
                        }
                    }
                }
            }
        }

        // Process new accesses if any
        if !schema_changes.new_accesses.is_empty() {
            info!(
                "Defining {} new accesses",
                schema_changes.new_accesses.len()
            );
            // Access definitions would be handled separately if needed
        }

        // Process modified accesses that need recreation
        if !schema_changes.modified_accesses.is_empty() {
            for access_change in &schema_changes.modified_accesses {
                // Check if all changes are ignorable
                let only_ignorable_changes = access_change
                    .changes
                    .iter()
                    .all(|change| change.is_ignorable());

                if !only_ignorable_changes {
                    debug!(
                        "Access {} has non-ignorable changes, needs recreation",
                        access_change.access_name
                    );
                    // Access recreation would be handled here if needed
                }
            }
        }

        Ok(())
    }

    /// Execute analyzer definitions from resolved surql on the live database.
    ///
    /// Analyzers must exist before tables are defined because FULLTEXT indexes
    /// reference them. An analyzer with a `FUNCTION fn::...` preprocessor also
    /// needs its function first, so in that case the functions surql is applied
    /// up front as well (it is re-applied idempotently after the tables).
    async fn execute_analyzers(
        &self,
        db: &Surreal<Client>,
        config: &crate::schemasync::config::SchemasyncConfig,
    ) -> Result<()> {
        if let Some(ref analyzers_surql) = config.database.resolved.analyzers_surql
            && !analyzers_surql.is_empty()
        {
            if crate::schemasync::compare::surql::analyzers_reference_functions(analyzers_surql)
                && let Some(ref functions_surql) = config.database.resolved.functions_surql
                && !functions_surql.is_empty()
            {
                info!("Analyzers reference functions; executing function definitions first");
                let result = execute_and_validate(db, functions_surql, "define", "functions").await;
                if let Err(e) = result {
                    let error_msg = format!(
                        "Failed to execute function definitions required by analyzers: {}\n\
                         Functions used as analyzer preprocessors run before tables are defined. \
                         If a function references tables, move the analyzer's helper function \
                         into the analyzers surql file instead.",
                        e
                    );
                    evenframe_log!(&error_msg, "results.log", true);
                    return Err(EvenframeError::database(error_msg));
                }
            }

            info!("Executing analyzer definitions from surql");
            evenframe_log!(analyzers_surql, "analyzer_definitions.surql");

            let result = execute_and_validate(db, analyzers_surql, "define", "analyzers").await;
            match result {
                Ok(_) => {
                    evenframe_log!(
                        "Successfully executed analyzer definitions",
                        "results.log",
                        true
                    );
                }
                Err(e) => {
                    let error_msg = format!("Failed to execute analyzer definitions: {}", e);
                    evenframe_log!(&error_msg, "results.log", true);
                    return Err(EvenframeError::database(error_msg));
                }
            }
        }
        Ok(())
    }

    /// Execute function definitions from resolved surql on the live database
    async fn execute_functions(
        &self,
        db: &Surreal<Client>,
        config: &crate::schemasync::config::SchemasyncConfig,
    ) -> Result<()> {
        if let Some(ref functions_surql) = config.database.resolved.functions_surql
            && !functions_surql.is_empty()
        {
            info!("Executing function definitions from surql");
            evenframe_log!(functions_surql, "function_definitions.surql");

            let result = execute_and_validate(db, functions_surql, "define", "functions").await;
            match result {
                Ok(_) => {
                    evenframe_log!(
                        "Successfully executed function definitions",
                        "results.log",
                        true
                    );
                }
                Err(e) => {
                    let error_msg = format!("Failed to execute function definitions: {}", e);
                    evenframe_log!(&error_msg, "results.log", true);
                    return Err(EvenframeError::database(error_msg));
                }
            }
        }
        Ok(())
    }
}
