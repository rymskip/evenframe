// SchemaSync - Database schema synchronization
pub mod compare;
pub mod config;
pub mod edge;
pub mod mockmake;
pub mod permissions;
pub mod surql;
pub mod table;

use crate::{
    compare::SchemaChanges,
    config::EvenframeConfig,
    error::{EvenframeError, Result},
    schemasync::surql::{define::generate_define_statements, execute::execute_and_validate},
};
use std::collections::HashMap;
use tracing::{debug, error, info, trace};

// Re-export commonly used types
pub use edge::{Direction, EdgeConfig, Subquery};
pub use mockmake::{coordinate, format};
pub use permissions::PermissionsConfig;
pub use surql::{QueryType, define::DefineConfig, generate_query};
use surrealdb::{
    Surreal,
    engine::remote::http::{Client, Http},
    opt::auth::Root,
};
pub use table::TableConfig;

use crate::{
    evenframe_log,
    mockmake::Mockmaker,
    types::{StructConfig, TaggedUnion},
};

#[derive(Default)]
pub struct Schemasync<'a> {
    // Input parameters - set via builder methods
    tables: Option<&'a HashMap<String, TableConfig>>,
    objects: Option<&'a HashMap<String, StructConfig>>,
    enums: Option<&'a HashMap<String, TaggedUnion>>,

    // Internal state - initialized automatically
    db: Option<Surreal<Client>>,
    schemasync_config: Option<crate::schemasync::config::SchemasyncConfig>,
}

impl<'a> Schemasync<'a> {
    /// Create a new empty Schemasync instance
    pub fn new() -> Self {
        trace!("Creating new Schemasync instance");
        Self {
            tables: None,
            objects: None,
            enums: None,
            db: None,
            schemasync_config: None,
        }
    }

    /// Builder methods for setting up the parameters
    pub fn with_tables(mut self, tables: &'a HashMap<String, TableConfig>) -> Self {
        debug!("Configuring Schemasync with {} tables", tables.len());
        trace!("Table names: {:?}", tables.keys().collect::<Vec<_>>());
        self.tables = Some(tables);
        self
    }

    pub fn with_objects(mut self, objects: &'a HashMap<String, StructConfig>) -> Self {
        debug!("Configuring Schemasync with {} objects", objects.len());
        trace!("Object names: {:?}", objects.keys().collect::<Vec<_>>());
        self.objects = Some(objects);
        self
    }

    pub fn with_enums(mut self, enums: &'a HashMap<String, TaggedUnion>) -> Self {
        debug!("Configuring Schemasync with {} enums", enums.len());
        trace!("Enum names: {:?}", enums.keys().collect::<Vec<_>>());
        self.enums = Some(enums);
        self
    }

    /// Initialize database connection and config from environment
    async fn initialize(&mut self) -> Result<()> {
        info!("Initializing Schemasync database connection and configuration");
        dotenv::dotenv().ok();
        debug!("Loaded environment variables from .env file");

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

        db.signin(Root {
            username: &username,
            password: &password,
        })
        .await
        .map_err(|e| {
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
        self.schemasync_config = Some(config.schemasync);
        debug!("Schemasync initialization completed successfully");

        Ok(())
    }

    /// Run the complete schemasync pipeline
    pub async fn run(mut self) -> Result<()> {
        info!("Starting Schemasync pipeline execution");

        // Initialize database and config first
        self.initialize().await?;

        // Validate that all required fields are set
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

        info!(
            "Pipeline validation completed - {} tables, {} objects, {} enums",
            tables.len(),
            objects.len(),
            enums.len()
        );

        evenframe_log!("", "all_statements.surql");
        evenframe_log!("", "results.log");
        evenframe_log!("", "all_define_statements.surql");
        debug!("Initialized logging files");

        debug!("Generating table and field definition statements");
        debug!(
            "Defining tables with full_refresh_mode: {}",
            config.mock_gen_config.full_refresh_mode
        );
        trace!(
            "Table definitions for: {:?}",
            tables.keys().collect::<Vec<_>>()
        );
        let mut define_statements: HashMap<&String, String> = HashMap::new();
        for (table_name, table) in tables {
            define_statements.insert(
                table_name,
                generate_define_statements(
                    table_name,
                    table,
                    tables,
                    objects,
                    enums,
                    config.mock_gen_config.full_refresh_mode,
                ),
            );
        }

        let define_statements_string = define_statements
            .values()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        evenframe_log!(
            &define_statements_string,
            "all_define_statements.surql",
            true
        );
        // Create Mockmaker instance (which contains Comparator)
        info!("Creating Mockmaker instance for data generation and comparison");
        let mut mockmaker = Mockmaker::new(
            db.clone(),
            tables.clone(),
            objects.clone(),
            enums.clone(),
            config.clone(),
        );
        debug!("Mockmaker instance created successfully");

        // Run initial ID generation and comparator setup
        info!("Generating IDs for mock data");
        mockmaker.generate_ids().await?;
        debug!("ID generation completed");

        // Run the comparator pipeline
        info!("Running schema comparison pipeline");
        let comparator = mockmaker.comparator.take().unwrap();
        mockmaker.comparator = Some(comparator.run(&define_statements_string).await?);
        debug!("Schema comparison completed");

        // Continue with the rest of the mockmaker pipeline
        info!("Removing old data from database");
        mockmaker.remove_old_data().await.map_err(|e| {
            error!("Failed to remove old data: {}", e);
            e
        })?;
        debug!("Old data removal completed");

        info!("Executing access control setup");
        mockmaker.execute_access().await.map_err(|e| {
            error!("Failed to execute access setup: {}", e);
            e
        })?;

        let comparator = mockmaker.comparator.take().unwrap();
        let comparator_clone = comparator.clone();
        let schema_changes = comparator.get_schema_changes().unwrap();
        mockmaker.comparator = Some(comparator_clone);

        info!("Defining database tables and schema");
        self.define_tables(&db, define_statements, schema_changes)
            .await
            .map_err(|e| {
                error!("Failed to define tables: {}", e);
                e
            })?;
        debug!("Table definitions completed successfully");

        debug!("Access control setup completed");

        info!("Filtering schema changes");
        mockmaker.filter_changes().await.map_err(|e| {
            error!("Failed to filter changes: {}", e);
            e
        })?;
        debug!("Schema changes filtering completed");

        if self
            .schemasync_config
            .unwrap_or(EvenframeConfig::new()?.schemasync)
            .should_generate_mocks
        {
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
        define_statments: HashMap<&String, String>,
        schema_changes: &SchemaChanges,
    ) -> Result<()> {
        info!("Defining tables based on schema changes");
        debug!(
            "Schema changes before define statement execution: {:?}",
            schema_changes
        );

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
                    let error_msg =
                        format!("Failed to execute define statements for table\n{e}:\n{stmt}",);
                    evenframe_log!(&error_msg, "results.log", true);
                    Err(e.into())
                }
            }
        };

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
                        {
                            execute(table_name, stmt).await?;
                        }
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
                                if table_change
                                    .new_fields
                                    .contains(&norm.to_string())
                                    || table_change
                                        .modified_fields
                                        .iter()
                                        .any(|fc| fc.field_name == norm)
                                {
                                    trace!(
                                        "Defining field: {} on table: {}",
                                        norm, table_name
                                    );
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
}
