// SchemaSync - Database schema synchronization

// Always compiled: pure data types needed by derive macros
#[cfg(feature = "schemasync")]
pub mod compare;
pub mod config;
#[cfg(feature = "schemadump")]
pub mod database;
pub mod define_config;
#[cfg(feature = "schemadump")]
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

/// How mock generation treats a changed table's existing records.
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

// Schemasync orchestrator: requires surrealdb at runtime
#[cfg(feature = "schemasync")]
use crate::{
    config::EvenframeConfig,
    error::{EvenframeError, Result},
    schemasync::compare::SchemaChanges,
    schemasync::config::{ConnectionOverrides, MockOverrides, SchemasyncConfig},
    schemasync::database::surql::{
        define::generate_define_statements,
        execute::{
            Transaction, execute_and_validate, execute_transactions, split_surql_statements,
        },
        optional::NamedTypes,
    },
};
#[cfg(feature = "schemasync")]
use std::collections::BTreeMap;
#[cfg(feature = "schemasync")]
use tracing::{debug, info, trace, warn};

#[cfg(feature = "schemasync")]
use surrealdb::{
    Surreal,
    engine::remote::http::{Client, Http},
    opt::auth::Root,
};

#[cfg(feature = "mockmake")]
use crate::schemasync::mockmake::Mockmaker;
#[cfg(feature = "schemasync")]
use crate::{
    evenframe_log,
    schemasync::compare::SurrealdbComparator,
    schemasync::database::surql::{
        execute::execute_access_query, remove::generate_remove_statements,
    },
    types::{StructConfig, TaggedUnion},
};

/// Rejects settings and annotations that ask for something this build of
/// evenframe was compiled without, naming the setting and the feature.
#[cfg(feature = "schemasync")]
fn check_features(
    config: &crate::schemasync::config::SchemasyncConfig,
    tables: &BTreeMap<String, TableConfig>,
) -> Result<()> {
    #[cfg(not(feature = "mockmake"))]
    {
        let requested = if config.should_generate_mocks {
            Some("should_generate_mocks = true in [schemasync] asks for mock data".to_string())
        } else if !config.plugins.is_empty() {
            Some("[schemasync] plugins configures mock-data plugins".to_string())
        } else {
            tables
                .iter()
                .find(|(_, table)| table.mock_generation_config.is_some())
                .map(|(name, _)| format!("table `{name}` has #[mock_data]"))
        };
        if let Some(requested) = requested {
            return Err(EvenframeError::config(format!(
                "{requested}, but this evenframe was built without the `mockmake` feature, which generates mock data"
            )));
        }
    }
    #[cfg(all(feature = "mockmake", not(feature = "wasm-plugins")))]
    {
        let requested = if !config.plugins.is_empty() {
            Some("[schemasync] plugins configures mock-data plugins".to_string())
        } else {
            table_plugin(tables)
                .map(|(name, plugin)| format!("table `{name}` uses mock-data plugin `{plugin}`"))
        };
        if let Some(requested) = requested {
            return Err(EvenframeError::config(format!(
                "{requested}, but this evenframe was built without the `wasm-plugins` feature"
            )));
        }
    }
    #[cfg(all(feature = "mockmake", feature = "wasm-plugins"))]
    if let Some((name, plugin)) = table_plugin(tables)
        && !config.plugins.contains_key(plugin)
    {
        return Err(EvenframeError::config(format!(
            "table `{name}` uses mock-data plugin `{plugin}`, which [schemasync] plugins does not configure"
        )));
    }
    Ok(())
}

/// The first table whose `#[mock_data]` names a plugin, with the plugin.
#[cfg(feature = "mockmake")]
fn table_plugin(tables: &BTreeMap<String, TableConfig>) -> Option<(&String, &String)> {
    tables.iter().find_map(|(name, table)| {
        let plugin = table.mock_generation_config.as_ref()?.plugin.as_ref()?;
        Some((name, plugin))
    })
}

/// What a pipeline run needs, once [`Schemasync`] has been set up.
#[cfg(feature = "schemasync")]
struct Ready<'a> {
    db: Surreal<Client>,
    tables: &'a BTreeMap<String, TableConfig>,
    objects: &'a BTreeMap<String, StructConfig>,
    enums: &'a BTreeMap<String, TaggedUnion>,
    #[cfg(feature = "mockmake")]
    declared: &'a crate::types::DeclaredTypes,
    config: crate::schemasync::config::SchemasyncConfig,
}

#[cfg(feature = "schemasync")]
#[derive(Default)]
pub struct Schemasync<'a> {
    // Input parameters - set via builder methods
    types: Option<&'a crate::types::SchemasyncTypes>,
    registry: Option<&'a crate::types::ForeignTypeRegistry>,

    // Internal state - initialized automatically
    db: Option<Surreal<Client>>,
    schemasync_config: Option<crate::schemasync::config::SchemasyncConfig>,
    /// Owned registry built from config during initialization (used when no external registry is provided)
    owned_registry: Option<crate::types::ForeignTypeRegistry>,
    connection_overrides: ConnectionOverrides,
    mock_overrides: MockOverrides,
}

/// Load the config for a command that connects to the database: connection
/// settings may come from `overrides` instead of environment variables, but
/// must be fully resolved one way or the other.
#[cfg(feature = "schemasync")]
pub fn load_connected_config(overrides: &ConnectionOverrides) -> Result<EvenframeConfig> {
    let mut config = EvenframeConfig::new_offline()?;
    let mut schemasync = config.require_schemasync()?.clone();
    schemasync.database.apply_connection_overrides(overrides);
    if let Some(var) = schemasync.database.unresolved_connection_var() {
        return Err(EvenframeError::EnvVarNotSet(var));
    }
    config.schemasync = Some(schemasync);
    Ok(config)
}

/// Connect to SurrealDB over HTTP, sign in as root with `SURREALDB_USER` /
/// `SURREALDB_PASSWORD`, and select the configured namespace and database,
/// within the configured `timeout`.
#[cfg(feature = "schemasync")]
pub async fn connect_database(
    database: &crate::schemasync::config::DatabaseConfig,
) -> Result<Surreal<Client>> {
    let timeout = std::time::Duration::from_secs(database.timeout);
    tokio::time::timeout(timeout, open_database(database))
        .await
        .map_err(|_| {
            EvenframeError::database(format!(
                "connecting to SurrealDB at {} took longer than the {}s timeout",
                database.url, database.timeout
            ))
        })?
}

#[cfg(feature = "schemasync")]
async fn open_database(
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
    Ok(db)
}

/// Check database connectivity by loading config, connecting, authenticating,
/// and selecting the configured namespace/database.
#[cfg(feature = "schemasync")]
pub async fn check_database_connectivity() -> Result<()> {
    let config = EvenframeConfig::new()?;
    let database = &config.require_schemasync()?.database;
    info!("Connecting to SurrealDB at {}...", database.url);
    connect_database(database).await?;
    info!(
        "    Connected to namespace '{}' / database '{}'",
        database.namespace, database.database
    );
    Ok(())
}

#[cfg(feature = "schemasync")]
impl<'a> Schemasync<'a> {
    /// Create a new empty Schemasync instance
    pub fn new() -> Self {
        trace!("Creating new Schemasync instance");
        Self {
            types: None,
            registry: None,
            db: None,
            schemasync_config: None,
            owned_registry: None,
            connection_overrides: ConnectionOverrides::default(),
            mock_overrides: MockOverrides::default(),
        }
    }

    /// The types to synchronize, as `AllConfigs::into_schemasync` gives them.
    pub fn with_types(mut self, types: &'a crate::types::SchemasyncTypes) -> Self {
        debug!(
            "Configuring Schemasync with {} tables, {} objects, {} enums",
            types.tables.len(),
            types.objects.len(),
            types.enums.len()
        );
        trace!("Table names: {:?}", types.tables.keys().collect::<Vec<_>>());
        self.types = Some(types);
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

    /// Override the configured mock generation settings (e.g. from CLI flags).
    pub fn with_mock_overrides(mut self, overrides: MockOverrides) -> Self {
        self.mock_overrides = overrides;
        self
    }

    /// Initialize database connection and config from environment
    async fn initialize(&mut self) -> Result<()> {
        info!("Initializing Schemasync database connection and configuration");
        let config = load_connected_config(&self.connection_overrides)?;
        let mut schemasync = config.require_schemasync()?.clone();
        schemasync.apply_mock_overrides(&self.mock_overrides);
        debug!("Loaded Evenframe configuration successfully");

        let db = connect_database(&schemasync.database).await?;

        self.db = Some(db);
        // Build a ForeignTypeRegistry from config if no external registry was provided
        if self.registry.is_none() {
            let registry =
                crate::types::ForeignTypeRegistry::from_config(&config.general.foreign_types);
            debug!("Built ForeignTypeRegistry from EvenframeConfig foreign_types");
            self.owned_registry = Some(registry);
        }
        self.schemasync_config = Some(schemasync);
        debug!("Schemasync initialization completed successfully");

        Ok(())
    }

    /// Validate that all required fields are set and return them.
    fn validate(&mut self) -> Result<Ready<'a>> {
        debug!("Validating required fields for Schemasync pipeline");
        let db = self
            .db
            .take()
            .ok_or_else(|| EvenframeError::config("Database connection failed to initialize"))?;
        let crate::types::SchemasyncTypes {
            tables,
            objects,
            enums,
            declared,
        } = self
            .types
            .ok_or_else(|| EvenframeError::config("Types not provided"))?;
        let config = self
            .schemasync_config
            .take()
            .ok_or_else(|| EvenframeError::config("Config failed to initialize"))?;

        if tables.is_empty() {
            return Err(EvenframeError::config(
                "No Evenframe tables found. Ensure your structs have #[derive(Evenframe)] and contain an `id` field.",
            ));
        }
        check_features(&config, tables)?;

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

        if config.warn_schemaless {
            for (name, table) in tables {
                if table.effective().struct_config.is_open() {
                    warn!(
                        table = %name,
                        "`{name}` is defined SCHEMALESS: its record holds keys known only from a \
                         value, such as a flattened map's, so the schema cannot list them",
                    );
                }
            }
        }
        for finding in crate::schemasync::lint::lint_unasserted_newtypes(declared) {
            warn!(
                location = %finding.location,
                newtype = %finding.newtype,
                "`{}` holds the newtype `{}` below its own value, where the schema does not \
                 assert the newtype's validators: the database accepts a value failing them, \
                 and reading that record back fails.",
                finding.location, finding.newtype,
            );
        }

        Ok(Ready {
            db,
            tables,
            objects,
            enums,
            #[cfg(feature = "mockmake")]
            declared,
            config,
        })
    }

    /// Generate define statements for all tables.
    fn generate_all_define_statements<'b>(
        tables: &'b BTreeMap<String, TableConfig>,
        objects: &BTreeMap<String, StructConfig>,
        enums: &BTreeMap<String, TaggedUnion>,
        full_refresh_mode: bool,
        registry: &crate::types::ForeignTypeRegistry,
        options: impl Into<crate::schemasync::config::SurqlOptions>,
    ) -> Result<(BTreeMap<&'b String, String>, String)> {
        let options = options.into();
        debug!(
            "Generating table and field definition statements (full_refresh_mode: {}, allow_scripting: {})",
            full_refresh_mode, options.allow_scripting
        );
        let mut define_statements: BTreeMap<&String, String> = BTreeMap::new();
        for (table_name, table) in tables {
            let table = table.effective();
            define_statements.insert(
                table_name,
                generate_define_statements(
                    table_name, table, tables, objects, enums, registry, options,
                )?,
            );
        }

        let define_statements_string = define_statements
            .values()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(" ");

        Ok((define_statements, define_statements_string))
    }

    /// Run the comparison pipeline and return schema changes without applying them.
    pub async fn diff(mut self) -> Result<SchemaChanges> {
        info!("Starting schema diff");
        self.initialize().await?;

        let Ready {
            db,
            tables,
            objects,
            enums,
            config,
            ..
        } = self.validate()?;
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
            config.surql_options(),
        )?;

        let mut comparator = SurrealdbComparator::new(&db, &config);
        comparator.run(&define_statements_string).await?;
        let schema_changes = comparator
            .get_schema_changes()
            .cloned()
            .ok_or_else(|| EvenframeError::config("Schema changes not computed"))?;

        info!("Schema diff completed");
        Ok(schema_changes)
    }

    /// Connect to the database and generate mock data without applying schema changes.
    #[cfg(feature = "mockmake")]
    pub async fn mock_only(
        mut self,
        count_override: Option<usize>,
        table_filter: Option<Vec<String>>,
    ) -> Result<()> {
        info!("Starting mock-only generation");
        self.initialize().await?;

        let Ready {
            db,
            tables,
            objects,
            enums,
            declared,
            config,
        } = self.validate()?;
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
            config.surql_options(),
        )?;

        let mut mockmaker = Mockmaker::new(
            &db,
            effective_tables,
            objects,
            enums,
            declared,
            &config,
            registry,
        )?;
        mockmaker.count_override = count_override;
        mockmaker.generate_ids().await?;

        let mut comparator = SurrealdbComparator::new(&db, &config);
        comparator.run(&define_statements_string).await?;
        let schema_changes = comparator
            .get_schema_changes()
            .ok_or_else(|| EvenframeError::config("Schema changes not computed"))?;

        mockmaker.filter_changes(schema_changes);
        mockmaker.generate_coordinated_values()?;
        mockmaker.generate_mock_data().await?;

        info!("Mock-only generation completed successfully");
        Ok(())
    }

    /// Insert mock data into the selected tables (all when `table_filter` is
    /// `None`) of a database whose schema is already in place. Unlike
    /// [`Self::mock_only`] this never diffs or defines anything: each selected
    /// table is brought to its record count (`count_override`, the table's
    /// `#[mock_data(n = ...)]`, or `default_record_count`), regenerating the
    /// records it keeps, adding the rest and deleting any beyond it. With
    /// `full_refresh_mode`,
    /// every table's records are deleted and all tables are regenerated.
    #[cfg(feature = "mockmake")]
    pub async fn insert_mock_data(
        mut self,
        count_override: Option<usize>,
        table_filter: Option<Vec<String>>,
    ) -> Result<()> {
        info!("Inserting mock data");
        self.initialize().await?;

        let Ready {
            db,
            tables,
            objects,
            enums,
            declared,
            config,
        } = self.validate()?;
        let default_registry = crate::types::ForeignTypeRegistry::default();
        let registry = self
            .registry
            .or(self.owned_registry.as_ref())
            .unwrap_or(&default_registry);

        if !config.should_generate_mocks {
            return Err(EvenframeError::config(
                "should_generate_mocks is false in [schemasync], so no mock data is inserted"
                    .to_string(),
            ));
        }
        let full_refresh = config.mock_gen_config.full_refresh_mode;
        if full_refresh && table_filter.is_some() {
            return Err(EvenframeError::config(
                "--tables limits which tables get mock data, but full_refresh_mode \
                 regenerates every table"
                    .to_string(),
            ));
        }

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

        let mut mockmaker =
            Mockmaker::new(&db, tables, objects, enums, declared, &config, registry)?;
        mockmaker.count_override = count_override;
        mockmaker.generate_ids().await?;
        mockmaker.clear_records_for_full_refresh().await?;
        mockmaker.select_tables_for_insert(selected.as_ref());
        mockmaker.remove_excess_records().await?;
        mockmaker.generate_coordinated_values()?;
        mockmaker.generate_mock_data().await?;

        info!("Mock data inserted");
        Ok(())
    }

    /// Run the complete schemasync pipeline
    pub async fn run(mut self) -> Result<()> {
        info!("Starting Schemasync pipeline execution");
        self.initialize().await?;

        let Ready {
            db,
            tables,
            objects,
            enums,
            #[cfg(feature = "mockmake")]
            declared,
            config,
            ..
        } = self.validate()?;
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
            config.surql_options(),
        )?;

        evenframe_log!("", "all_statements.surql");
        evenframe_log!("", "results.log");
        evenframe_log!("", "all_define_statements.surql");
        evenframe_log!(
            &define_statements_string,
            "all_define_statements.surql",
            true
        );

        #[cfg(feature = "mockmake")]
        let mockmaker = if config.should_generate_mocks {
            let mut mockmaker =
                Mockmaker::new(&db, tables, objects, enums, declared, &config, registry)?;
            mockmaker.generate_ids().await?;
            Some(mockmaker)
        } else {
            None
        };

        info!("Running schema comparison pipeline");
        let mut comparator = SurrealdbComparator::new(&db, &config);
        comparator.run(&define_statements_string).await?;
        let schema_changes = comparator
            .get_schema_changes()
            .ok_or_else(|| EvenframeError::config("Schema changes not computed"))?;

        #[cfg(feature = "mockmake")]
        if let Some(mockmaker) = &mockmaker {
            mockmaker
                .clear_records_for_full_refresh()
                .await
                .map_err(|e| EvenframeError::SchemaSync(format!("Failed to clear records: {e}")))?;
            mockmaker.remove_excess_records().await.map_err(|e| {
                EvenframeError::SchemaSync(format!("Failed to remove excess records: {e}"))
            })?;
        }

        info!("Removing what the models no longer define");
        let remove_statements = generate_remove_statements(schema_changes);
        evenframe_log!(&remove_statements, "remove_statements.surql");
        if !remove_statements.is_empty() {
            execute_and_validate(&db, &remove_statements, "remove", "old schema")
                .await
                .map_err(|e| {
                    EvenframeError::SchemaSync(format!("Failed to remove old schema: {e}"))
                })?;
        }

        // Execution order matters:
        // 1. Access first: defines SIGNUP/SIGNIN on the database (independent of tables)
        // 2. Analyzers second: FULLTEXT indexes on tables reference them
        // 3. Tables third: defines table schemas, fields, indexes, and events
        // 4. Functions last: function params use typed references like `record<site>`
        //    which require the referenced tables to already exist in the database

        info!("Executing access control setup");
        execute_access_query(
            &db,
            comparator.get_access_query(),
            &config.database.database,
        )
        .await
        .map_err(|e| EvenframeError::SchemaSync(format!("Failed to execute access setup: {e}")))?;

        info!("Executing analyzer definitions");
        self.execute_analyzers(&db, &config)
            .await
            .map_err(|e| EvenframeError::SchemaSync(format!("Failed to execute analyzers: {e}")))?;

        info!("Defining database tables and schema");
        self.define_tables(
            &db,
            define_statements,
            schema_changes,
            &config,
            tables,
            &NamedTypes { objects, enums },
        )
        .await
        .map_err(|e| EvenframeError::SchemaSync(format!("Failed to define tables: {e}")))?;

        info!("Executing function definitions");
        self.execute_functions(&db, &config)
            .await
            .map_err(|e| EvenframeError::SchemaSync(format!("Failed to execute functions: {e}")))?;

        #[cfg(feature = "mockmake")]
        if let Some(mut mockmaker) = mockmaker {
            info!("Generating mock data");
            mockmaker.filter_changes(schema_changes);
            mockmaker.generate_coordinated_values()?;
            mockmaker.generate_mock_data().await.map_err(|e| {
                EvenframeError::SchemaSync(format!("Failed to generate mock data: {e}"))
            })?;
        }

        info!("Schemasync pipeline execution completed successfully");
        Ok(())
    }

    /// Defines the changed tables on the database, one transaction per table,
    /// so a table that fails is left as it was while the others apply.
    async fn define_tables(
        &self,
        db: &Surreal<Client>,
        define_statements: BTreeMap<&String, String>,
        schema_changes: &SchemaChanges,
        config: &SchemasyncConfig,
        tables: &BTreeMap<String, TableConfig>,
        types: &NamedTypes<'_>,
    ) -> Result<()> {
        info!(
            full_refresh_mode = config.mock_gen_config.full_refresh_mode,
            "Defining tables based on schema changes"
        );
        debug!(
            "Schema changes before define statement execution: {:?}",
            schema_changes
        );

        let mut transactions = Vec::new();
        if config.mock_gen_config.full_refresh_mode {
            for (table_name, block) in &define_statements {
                transactions.push(DefineParts::split(block).whole_table(table_name));
            }
        } else {
            for table_name in &schema_changes.new_tables {
                if let Some(block) = define_statements.get(table_name) {
                    transactions.push(DefineParts::split(block).whole_table(table_name));
                }
            }
            for table_change in &schema_changes.modified_tables {
                if let Some(block) = define_statements.get(&table_change.table_name) {
                    let mut transaction = DefineParts::split(block).changes(table_change);
                    if let Some(table) = tables.get(&table_change.table_name) {
                        transaction.statements.extend(option_absence_changes(
                            table_change,
                            table,
                            types,
                            config.option_none,
                        )?);
                    }
                    transactions.push(transaction);
                }
            }
        }
        info!("Defining {} tables", transactions.len());
        execute_transactions(db, &transactions, "define").await
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
            if crate::schemasync::dump::analyzers_reference_functions(analyzers_surql)
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

/// One table's define block, split into its statements once.
#[cfg(feature = "schemasync")]
struct DefineParts {
    table: Vec<String>,
    /// Each `DEFINE FIELD` with the field it defines.
    fields: Vec<(String, String)>,
    indexes: Vec<String>,
    events: Vec<String>,
}

#[cfg(feature = "schemasync")]
impl DefineParts {
    fn split(block: &str) -> Self {
        let mut parts = DefineParts {
            table: Vec::new(),
            fields: Vec::new(),
            indexes: Vec::new(),
            events: Vec::new(),
        };
        for statement in split_surql_statements(block) {
            if statement.starts_with("DEFINE TABLE") {
                parts.table.push(statement);
            } else if statement.starts_with("DEFINE FIELD") {
                if let Some(name) = defined_field_name(&statement) {
                    parts.fields.push((name.to_string(), statement));
                }
            } else if statement.starts_with("DEFINE INDEX") {
                parts.indexes.push(statement);
            } else if statement.starts_with("DEFINE EVENT") {
                parts.events.push(statement);
            }
        }
        parts
    }

    /// Every statement of the table, for a new table or a full refresh.
    fn whole_table(self, table_name: &str) -> Transaction {
        let statements = self
            .table
            .into_iter()
            .chain(self.fields.into_iter().map(|(_, statement)| statement))
            .chain(self.indexes)
            .chain(self.events)
            .collect();
        Transaction {
            label: table_name.to_string(),
            statements,
        }
    }

    /// The statements a modified table needs: the table itself, its new and
    /// modified fields, every index (each is an idempotent `OVERWRITE`) and
    /// its new or changed events.
    fn changes(self, table_change: &crate::schemasync::compare::TableChanges) -> Transaction {
        let changed_fields: std::collections::BTreeSet<&str> = table_change
            .new_fields
            .iter()
            .map(String::as_str)
            .chain(
                table_change
                    .modified_fields
                    .iter()
                    .map(|change| change.field_name.as_str()),
            )
            .collect();
        let statements = self
            .table
            .into_iter()
            .chain(
                self.fields
                    .into_iter()
                    .filter(|(name, _)| changed_fields.contains(name.as_str()))
                    .map(|(_, statement)| statement),
            )
            .chain(self.indexes)
            .chain(table_change.new_events.iter().cloned())
            .collect();
        Transaction {
            label: table_change.table_name.clone(),
            statements,
        }
    }
}

/// The UPDATE translating changed fields to their configured absence
/// representation, run once the fields are redefined. One statement sets
/// every such field, since each write checks the whole record.
#[cfg(feature = "schemasync")]
fn option_absence_changes(
    table_change: &crate::schemasync::compare::TableChanges,
    table: &TableConfig,
    types: &NamedTypes<'_>,
    target: crate::schemasync::config::OptionNone,
) -> Result<Option<String>> {
    let mut assignments = Vec::new();
    for change in &table_change.modified_fields {
        if match target {
            crate::schemasync::config::OptionNone::None => !change.old_type.contains("null"),
            crate::schemasync::config::OptionNone::Null => {
                !change.old_type.contains("option<") && !change.old_type.contains("none")
            }
        } {
            continue;
        }
        let Some(field) = table
            .struct_config
            .fields
            .iter()
            .map(|field| field.effective())
            .find(|field| field.db_name() == change.field_name)
        else {
            continue;
        };
        let place = crate::schemasync::table::surql_ident(field.db_name());
        if let Some(rewritten) = types.option_absence(&field.field_type, &place, target)? {
            assignments.push(format!("{place} = {rewritten}"));
        }
    }
    Ok((!assignments.is_empty()).then(|| {
        format!(
            "UPDATE {} SET {};",
            crate::schemasync::table::surql_ident(&table_change.table_name),
            assignments.join(", ")
        )
    }))
}

/// The field a `DEFINE FIELD [OVERWRITE] <name> ON ...` statement defines,
/// without backticks or an array wildcard suffix.
#[cfg(feature = "schemasync")]
fn defined_field_name(statement: &str) -> Option<&str> {
    let mut tokens = statement.split_whitespace().skip(2);
    let mut name = tokens.next()?;
    if name.eq_ignore_ascii_case("OVERWRITE") {
        name = tokens.next()?;
    }
    let name = name.trim_matches('`');
    Some(name.strip_suffix(".*").unwrap_or(name))
}

#[cfg(all(test, feature = "schemasync"))]
mod define_parts_tests {
    use super::DefineParts;

    #[test]
    fn event_comments_do_not_change_definition_boundaries() {
        let block = r#"
            DEFINE TABLE sample SCHEMAFULL;
            DEFINE FIELD value ON TABLE sample TYPE int;
            DEFINE EVENT first ON TABLE sample WHEN $event = 'UPDATE' THEN {
                -- The owner's value; an unmatched { must not affect splitting.
                IF $after.value > 0 { RETURN true; };
            };
            DEFINE EVENT second ON TABLE sample WHEN $event = 'CREATE' THEN {
                RETURN 'a literal with -- and ; and an escaped \' quote';
            };
        "#;
        let transaction = DefineParts::split(block).whole_table("sample");
        assert_eq!(transaction.statements.len(), 4);
        assert!(transaction.statements[2].contains("IF $after.value > 0 { RETURN true; };"));
        assert!(transaction.statements[2].trim_end().ends_with("};"));
        assert!(transaction.statements[3].starts_with("DEFINE EVENT second"));
        assert!(transaction.statements[3].contains("a literal with -- and ;"));
    }
}

#[cfg(all(test, feature = "mockmake", feature = "wasm-plugins"))]
mod check_features_tests {
    use super::{BTreeMap, TableConfig, check_features};
    use crate::schemasync::config::SchemasyncConfig;
    use crate::schemasync::mockmake::MockGenerationConfig;

    #[derive(serde::Deserialize)]
    struct Fixture {
        table_config: TableConfig,
    }

    fn tables_using(plugin: &str) -> BTreeMap<String, TableConfig> {
        let fixture: Fixture =
            serde_json::from_str(include_str!("../../tests/specs/surrealql/basic_table.json"))
                .unwrap();
        let mut table = fixture.table_config;
        table.mock_generation_config = Some(MockGenerationConfig {
            record_count: Some(1),
            coordination_rules: Vec::new(),
            plugin: Some(plugin.to_string()),
        });
        BTreeMap::from([("user".to_string(), table)])
    }

    fn config(plugins: &str) -> SchemasyncConfig {
        toml::from_str(&format!(
            "should_generate_mocks = true\n[database]\nurl = \"x\"\n{plugins}"
        ))
        .unwrap()
    }

    #[test]
    fn a_table_plugin_must_be_configured() {
        let error = check_features(&config(""), &tables_using("names"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("`names`"), "{error}");
        let configured = config("[plugins.names]\npath = \"names.wasm\"\n");
        assert!(check_features(&configured, &tables_using("names")).is_ok());
    }
}
