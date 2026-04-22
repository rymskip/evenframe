// SchemaSync - Database schema synchronization

// Always compiled: pure data types needed by derive macros
#[cfg(feature = "schemasync")]
pub mod compare;
pub mod config;
#[cfg(feature = "schemasync")]
pub mod database;
pub mod define_config;
pub mod edge;
pub mod event;
pub mod mockmake;
pub mod permissions;
pub mod table;

// Re-export commonly used types (always available)
pub use define_config::DefineConfig;
pub use edge::{Direction, EdgeConfig, Subquery};
pub use event::EventConfig;
pub use mockmake::{coordinate, format};
pub use permissions::PermissionsConfig;
pub use table::{IndexConfig, TableConfig};

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
#[cfg(feature = "surrealdb")]
use crate::{
    config::EvenframeConfig,
    error::{EvenframeError, Result},
    schemasync::compare::SchemaChanges,
    schemasync::database::surql::{
        define::generate_define_statements, execute::execute_and_validate,
    },
};
#[cfg(feature = "surrealdb")]
use std::collections::BTreeMap;
#[cfg(feature = "surrealdb")]
use tracing::{debug, error, info, trace};

#[cfg(feature = "surrealdb")]
use surrealdb::{
    Surreal,
    engine::remote::http::{Client, Http},
    opt::auth::Root,
};

#[cfg(feature = "surrealdb")]
use crate::{
    evenframe_log,
    schemasync::mockmake::Mockmaker,
    types::{StructConfig, TaggedUnion},
};

#[cfg(feature = "surrealdb")]
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
}

/// Check database connectivity by loading config, connecting, authenticating,
/// and selecting the configured namespace/database.
#[cfg(feature = "surrealdb")]
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

#[cfg(feature = "surrealdb")]
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

    /// Initialize database connection and config from environment
    async fn initialize(&mut self) -> Result<()> {
        info!("Initializing Schemasync database connection and configuration");
        let config = EvenframeConfig::new()?;
        debug!("Loaded Evenframe configuration successfully");
        trace!("Database URL: {}", config.schemasync.database.url);
        trace!(
            "Database namespace: {}",
            config.schemasync.database.namespace
        );
        trace!("Database name: {}", config.schemasync.database.database);

        let db = Surreal::new::<Http>(&config.schemasync.database.url)
            .await
            .map_err(|e| {
                EvenframeError::database(format!(
                    "There was a problem creating the HTTP surrealdb client: {e}"
                ))
            })
            .unwrap();
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

        db.use_ns(&config.schemasync.database.namespace)
            .use_db(&config.schemasync.database.database)
            .await
            .map_err(|e| {
                EvenframeError::database(format!("There was a problem using to the namespace: {e}"))
            })?;
        info!(
            "Connected to database namespace '{}' and database '{}'",
            config.schemasync.database.namespace, config.schemasync.database.database
        );

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

        Ok((db, tables, objects, enums, config))
    }

    /// Generate define statements for all tables.
    fn generate_all_define_statements<'b>(
        tables: &'b BTreeMap<String, TableConfig>,
        objects: &BTreeMap<String, StructConfig>,
        enums: &BTreeMap<String, TaggedUnion>,
        full_refresh_mode: bool,
        registry: &crate::types::ForeignTypeRegistry,
    ) -> (BTreeMap<&'b String, String>, String) {
        debug!(
            "Generating table and field definition statements (full_refresh_mode: {})",
            full_refresh_mode
        );
        let mut define_statements: BTreeMap<&String, String> = BTreeMap::new();
        for (table_name, table) in tables {
            // Skip alias entries with `output_override` — literal override
            // semantics mean the override target appears under its own key
            // and the alias should not produce duplicate DEFINE TABLE output.
            if table.output_override.is_some() {
                continue;
            }
            let table = table.effective();
            define_statements.insert(
                table_name,
                generate_define_statements(
                    table_name,
                    table,
                    tables,
                    objects,
                    enums,
                    full_refresh_mode,
                    registry,
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

        let (db, tables, objects, enums, mut config) = self.validate()?;
        let default_registry = crate::types::ForeignTypeRegistry::default();
        let registry = self
            .registry
            .or(self.owned_registry.as_ref())
            .unwrap_or(&default_registry);

        if let Some(count) = count_override {
            config.mock_gen_config.default_record_count = count;
        }

        // Apply table filter if specified
        let owned_filtered: BTreeMap<String, TableConfig>;
        let effective_tables: &BTreeMap<String, TableConfig> = if let Some(ref filter) = table_filter
        {
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
        );

        let mut mockmaker =
            Mockmaker::new(&db, effective_tables, objects, enums, &config, registry);
        mockmaker.generate_ids().await?;

        if let Some(ref mut comparator) = mockmaker.comparator {
            comparator.run(&define_statements_string).await?;
        }

        mockmaker.filter_changes().await?;
        mockmaker.generate_mock_data().await?;

        info!("Mock-only generation completed successfully");
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
        // 2. Tables second — defines table schemas, fields, and events
        // 3. Functions last — function params use typed references like `record<site>`
        //    which require the referenced tables to already exist in the database

        info!("Executing access control setup");
        mockmaker.execute_access().await.map_err(|e| {
            error!("Failed to execute access setup: {}", e);
            e
        })?;
        debug!("Access control setup completed");

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
                for stmt in define_stmt.split_inclusive(';') {
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
                    for stmt in define_stmt.split_inclusive(';') {
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
                    for stmt in define_stmt.split_inclusive(';') {
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

                        for stmt in define_stmt.split_inclusive(';') {
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
                    for stmt in define_stmt.split_inclusive(';') {
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
