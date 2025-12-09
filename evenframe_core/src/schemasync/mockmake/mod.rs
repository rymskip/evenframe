pub mod coordinate;
pub mod field_value;
pub mod field_value_recursive;
pub mod format;
pub mod regex_val_gen;

use crate::{
    compare::surql::SurrealdbComparator,
    coordinate::{
        CoherentDataset, Coordination, CoordinationGroup, CoordinationId, CoordinationPair,
    },
    dependency::sort_tables_by_dependencies,
    evenframe_log,
    mockmake::format::Format,
    schemasync::{
        StructConfig, TableConfig, TaggedUnion, compare::PreservationMode,
        database::surql::access::execute_access_query,
    },
    types::StructField,
    wrappers::EvenframeRecordId,
};
use rand::Rng;
use std::collections::{HashMap, HashSet};
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::engine::remote::http::Client;
use tracing;
use uuid::Uuid;

#[derive(Debug)]
pub struct Mockmaker<'a> {
    db: &'a Surreal<Client>,
    pub(super) tables: &'a HashMap<String, TableConfig>,
    objects: &'a HashMap<String, StructConfig>,
    enums: &'a HashMap<String, TaggedUnion>,
    pub(super) schemasync_config: &'a crate::schemasync::config::SchemasyncConfig,
    pub comparator: Option<SurrealdbComparator<'a>>,

    // Runtime state
    pub(super) id_map: HashMap<String, Vec<String>>,
    pub(super) record_diffs: HashMap<String, i32>,
    filtered_tables: HashMap<String, TableConfig>,
    filtered_objects: HashMap<String, StructConfig>,
    pub coordinated_values: HashMap<CoordinationId, String>,
}

impl<'a> Mockmaker<'a> {
    pub fn new(
        db: &'a Surreal<Client>,
        tables: &'a HashMap<String, TableConfig>,
        objects: &'a HashMap<String, StructConfig>,
        enums: &'a HashMap<String, TaggedUnion>,
        schemasync_config: &'a crate::schemasync::config::SchemasyncConfig,
    ) -> Self {
        Self {
            db,
            tables,
            objects,
            enums,
            schemasync_config,
            comparator: Some(SurrealdbComparator::new(db, schemasync_config)),
            id_map: HashMap::new(),
            record_diffs: HashMap::new(),
            filtered_tables: HashMap::new(),
            filtered_objects: HashMap::new(),
            coordinated_values: HashMap::new(),
        }
    }

    pub async fn run(mut self) -> Result<(), Box<dyn std::error::Error>> {
        tracing::info!("Starting Mockmaker pipeline");

        // Step 1: Generate IDs
        tracing::debug!("Step 1: Generating IDs for mock data");
        self.generate_ids().await?;

        tracing::debug!("Step 2: ??");

        // Step 3: Run remaining mockmaker steps
        tracing::debug!("Step 3: Removing old data based on schema changes");
        self.remove_old_data().await?;

        tracing::debug!("Step 4: Executing access queries");
        self.execute_access().await?;

        tracing::debug!("Step 5: Filtering changed tables and objects");
        self.filter_changes().await?;

        tracing::debug!("Step 6: Generating coordinated values");
        self.generate_coordinated_values();

        tracing::debug!("Step 7: Generating mock data");
        self.generate_mock_data().await?;

        tracing::info!("Mockmaker pipeline completed successfully");
        Ok(())
    }

    /// Generate IDs for tables
    pub async fn generate_ids(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        evenframe_log!("", "record_diffs.log");
        tracing::trace!("Starting ID generation for all tables");
        let mut map = HashMap::new();
        let mut record_diffs = HashMap::new();

        // Process tables sequentially to avoid reference issues
        // Since these are just SELECT queries, they should be fast enough
        for (table_name, table_config) in self.tables {
            tracing::trace!(table = %table_name, "Generating IDs for table");

            // Determine desired count from config or default
            let desired_count =
                if let Some(mock_generation_config) = &table_config.mock_generation_config {
                    mock_generation_config.n
                } else {
                    self.schemasync_config.mock_gen_config.default_batch_size
                };

            // Query existing IDs
            let query = format!("SELECT id FROM {table_name};",);
            tracing::trace!("Querying existing IDs {query}");
            let mut response = self.db.query(query).await.expect(
                "Something went wrong getting the ids from the db for mock data generation",
            );
            evenframe_log!(&format!("{:?}", response), "record_diffs.log", true);

            #[derive(serde::Deserialize, Debug)]
            struct IdResponse {
                id: EvenframeRecordId,
            }

            let existing_ids: Vec<IdResponse> = response.take(0).unwrap_or_else(|_| {
                panic!("Something went wrong getting the record ids: {response:?}")
            });

            let mut ids = Vec::new();
            let existing_count = existing_ids.len();

            // Calculate the difference between existing and desired counts
            let record_diff = desired_count as i32 - existing_count as i32;

            tracing::trace!(
                table = %table_name,
                existing_count = existing_count,
                desired_count = desired_count,
                record_diff = record_diff,
                "Calculated record difference"
            );

            // Store the difference in the record_diffs map
            record_diffs.insert(table_name.clone(), record_diff);

            if existing_count >= desired_count {
                // We have enough or more IDs than needed
                // Just use the first desired_count IDs
                for (i, record) in existing_ids.into_iter().enumerate() {
                    if i < desired_count {
                        let id_string = record.id.to_string();
                        ids.push(id_string);
                    } else {
                        // Stop after we have enough
                        break;
                    }
                }
            } else {
                // We need to use existing IDs and generate more
                // First, use all existing IDs
                for record in existing_ids {
                    ids.push(record.id.to_string());
                }

                // Generate additional IDs
                let mut next_id = existing_count + 1;
                while ids.len() < desired_count {
                    ids.push(format!("{table_name}:{next_id}"));
                    next_id += 1;
                }
            }

            // Store with both the original key and snake_case key for easier lookup
            map.insert(table_name.clone(), ids.clone());
        }

        self.id_map = map;
        self.record_diffs = record_diffs;

        tracing::debug!(table_count = self.id_map.len(), "ID generation complete");

        evenframe_log!(
            format!("Record count differences: {:#?}", self.record_diffs),
            "record_diffs.log",
            true
        );

        Ok(())
    }

    /// Remove old data based on schema changes
    pub async fn remove_old_data(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        tracing::trace!("Removing old data based on schema changes");
        let comparator = self.comparator.as_ref().unwrap();
        let schema_changes = comparator.get_schema_changes().unwrap();

        let remove_statements = self.generate_remove_statements(schema_changes);

        tracing::debug!(
            statement_length = remove_statements.len(),
            "Generated remove statements"
        );

        evenframe_log!(&remove_statements, "remove_statements.surql");

        if !remove_statements.is_empty() {
            tracing::trace!("Executing remove statements");
            self.db.query(remove_statements).await?;
        }

        tracing::trace!("Old data removal complete");
        Ok(())
    }

    /// Execute access query on main database
    pub async fn execute_access(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        tracing::trace!("Executing access definitions");
        let comparator = self.comparator.as_ref().unwrap();
        let access_query = comparator.get_access_query();

        tracing::debug!(query_length = access_query.len(), "Executing access query");

        execute_access_query(self.db, access_query).await
    }

    /// Filter changed tables and objects
    pub async fn filter_changes(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        tracing::trace!("Filtering changes based on schema comparison");
        let comparator = self.comparator.as_ref().unwrap();
        let schema_changes = comparator.get_schema_changes().unwrap();

        let (filtered_tables, filtered_objects) =
            if self.schemasync_config.mock_gen_config.full_refresh_mode {
                tracing::debug!("Full refresh mode enabled - using all tables and objects");
                (self.tables.clone(), self.objects.clone())
            } else {
                tracing::debug!("Incremental mode - filtering changed items only");
                self.filter_changed_tables_and_objects(
                    schema_changes,
                    self.tables,
                    self.objects,
                    self.enums,
                    &self.record_diffs,
                )
            };

        self.filtered_tables = filtered_tables;
        self.filtered_objects = filtered_objects;

        tracing::info!(
            filtered_tables = self.filtered_tables.len(),
            filtered_objects = self.filtered_objects.len(),
            "Filtering complete"
        );

        evenframe_log!(
            format!("{:#?}{:#?}", self.filtered_objects, self.filtered_tables),
            "filtered.log"
        );

        Ok(())
    }

    pub(super) async fn generate_mock_data(&self) -> Result<(), Box<dyn std::error::Error>> {
        tracing::trace!("Starting mock data generation");

        // Sort tables by dependencies to ensure proper insertion order
        let sorted_table_names =
            sort_tables_by_dependencies(&self.filtered_tables, &self.filtered_objects, self.enums);

        tracing::debug!(
            table_count = sorted_table_names.len(),
            "Tables sorted by dependencies"
        );

        evenframe_log!(
            &format!("Sorted table order: {sorted_table_names:?}"),
            "table_order.log",
            true
        );

        for table_name in &sorted_table_names {
            if let Some(table) = &self.filtered_tables.get(table_name) {
                tracing::trace!(
                    table = %table_name,
                    is_relation = table.relation.is_some(),
                    "Processing table for mock data"
                );

                if self.schemasync_config.should_generate_mocks {
                    let stmts = if table.relation.is_some() {
                        tracing::trace!(table = %table_name, "Generating INSERT statements for relation");
                        self.generate_insert_statements(table_name, table)
                    } else {
                        tracing::trace!(table = %table_name, "Generating UPSERT statements for table");
                        self.generate_upsert_statements(table_name, table)
                    };

                    tracing::debug!(
                        table = %table_name,
                        statement_count = stmts.lines().count(),
                        "Generated mock data statements"
                    );

                    evenframe_log!(&stmts, "all_statements.surql", true);

                    // Execute and validate upsert statements
                    use crate::schemasync::database::surql::execute::execute_and_validate;

                    match execute_and_validate(self.db, &stmts, "UPSERT", table_name).await {
                        Ok(_results) => {
                            tracing::debug!(table = %table_name, "Mock data inserted successfully");
                        }
                        Err(e) => {
                            tracing::error!(
                                table = %table_name,
                                error = %e,
                                "Failed to execute statements"
                            );
                            let error_msg = format!(
                                "Failed to execute upsert statements for table {}: {}",
                                table_name, e
                            );
                            evenframe_log!(&error_msg, "results.log", true);
                            return Err(e);
                        }
                    }
                }
            }
        }
        tracing::info!("Mock data generation complete");
        Ok(())
    }

    // Getter for new_schema so Schemasync can access it
    pub fn get_new_schema(&self) -> Option<&Surreal<Db>> {
        self.comparator.as_ref()?.get_new_schema()
    }

    pub fn random_string(len: usize) -> String {
        use rand::distr::Alphanumeric;
        let mut rng = rand::rng();
        (0..len).map(|_| rng.sample(Alphanumeric) as char).collect()
    }

    /// Builds coordination groups from the provided table configs
    pub fn build_coordination_groups(&mut self) -> Vec<CoordinationGroup> {
        let mut coordination_groups = Vec::new();
        let mut coordination_map: HashMap<String, Vec<(String, Coordination)>> = HashMap::new();

        // Extract coordination rules from each table's mock_generation_config
        for (table_name, table_config) in self.tables {
            if let Some(ref mock_config) = table_config.mock_generation_config {
                // Each table may have coordination_rules
                for coordination in &mock_config.coordination_rules {
                    // Extract field names from the coordination enum
                    let field_names = match coordination {
                        Coordination::InitializeEqual(fields) => fields.clone(),
                        Coordination::InitializeSequential { field_names, .. } => {
                            field_names.clone()
                        }
                        Coordination::InitializeSum { field_names, .. } => field_names.clone(),
                        Coordination::InitializeDerive {
                            source_field_names,
                            target_field_name,
                            ..
                        } => {
                            let mut all_fields = source_field_names.clone();
                            all_fields.push(target_field_name.clone());
                            all_fields
                        }
                        Coordination::InitializeCoherent(dataset) => match dataset {
                            CoherentDataset::Address {
                                city,
                                state,
                                zip,
                                country,
                            } => [city, state, zip, country]
                                .into_iter()
                                .filter(|s| !s.is_empty())
                                .cloned()
                                .collect(),
                            CoherentDataset::PersonName {
                                first_name,
                                last_name,
                                full_name,
                            } => [first_name, last_name, full_name]
                                .into_iter()
                                .filter(|s| !s.is_empty())
                                .cloned()
                                .collect(),
                            CoherentDataset::GeoLocation {
                                latitude,
                                longitude,
                                city,
                                country,
                            } => [latitude, longitude, city, country]
                                .into_iter()
                                .filter(|s| !s.is_empty())
                                .cloned()
                                .collect(),
                            CoherentDataset::DateRange {
                                start_date,
                                end_date,
                            } => vec![start_date.clone(), end_date.clone()],
                        },
                    };

                    // Create a unique key for this coordination pattern
                    let mut sorted_fields = field_names.clone();
                    sorted_fields.sort();
                    let coordination_key = format!("{:?}", sorted_fields);

                    // Add this table-coordination pair to the map
                    coordination_map
                        .entry(coordination_key)
                        .or_default()
                        .push((table_name.clone(), coordination.clone()));
                }
            }
        }

        // Now group coordinations that span multiple tables or are within single tables
        for (_coordination_key, table_coordinations) in coordination_map {
            let mut group = CoordinationGroup::builder().id(Uuid::new_v4()).build();

            let mut group_tables = HashSet::new();
            let mut group_pairs = Vec::new();

            // Group coordinations by their type and fields
            let mut coordination_by_type: HashMap<String, Vec<(String, Coordination)>> =
                HashMap::new();

            for (table_name, coordination) in table_coordinations {
                let type_key = match &coordination {
                    Coordination::InitializeEqual(_) => "equal",
                    Coordination::InitializeSequential { .. } => "sequential",
                    Coordination::InitializeSum { .. } => "sum",
                    Coordination::InitializeDerive { .. } => "derive",
                    Coordination::InitializeCoherent(_) => "coherent",
                };

                coordination_by_type
                    .entry(type_key.to_string())
                    .or_default()
                    .push((table_name.clone(), coordination.clone()));

                group_tables.insert(table_name);
            }

            // Create CoordinationPair for each unique coordination
            for (_type_key, typed_coordinations) in coordination_by_type {
                // Group coordinations with identical rules
                let mut processed = HashSet::new();

                for (_table_name, coordination) in &typed_coordinations {
                    let coord_str = format!("{:?}", coordination);
                    if processed.contains(&coord_str) {
                        continue;
                    }
                    processed.insert(coord_str.clone());

                    // Extract field names and create CoordinationId instances
                    let field_names = match coordination {
                        Coordination::InitializeEqual(fields) => fields.clone(),
                        Coordination::InitializeSequential { field_names, .. } => {
                            field_names.clone()
                        }
                        Coordination::InitializeSum { field_names, .. } => field_names.clone(),
                        Coordination::InitializeDerive {
                            source_field_names,
                            target_field_name,
                            ..
                        } => {
                            let mut all_fields = source_field_names.clone();
                            all_fields.push(target_field_name.clone());
                            all_fields
                        }
                        Coordination::InitializeCoherent(dataset) => match dataset {
                            CoherentDataset::Address {
                                city,
                                state,
                                zip,
                                country,
                            } => [city, state, zip, country]
                                .into_iter()
                                .filter(|s| !s.is_empty())
                                .cloned()
                                .collect(),
                            CoherentDataset::PersonName {
                                first_name,
                                last_name,
                                full_name,
                            } => [first_name, last_name, full_name]
                                .into_iter()
                                .filter(|s| !s.is_empty())
                                .cloned()
                                .collect(),
                            CoherentDataset::GeoLocation {
                                latitude,
                                longitude,
                                city,
                                country,
                            } => [latitude, longitude, city, country]
                                .into_iter()
                                .filter(|s| !s.is_empty())
                                .cloned()
                                .collect(),
                            CoherentDataset::DateRange {
                                start_date,
                                end_date,
                            } => vec![start_date.clone(), end_date.clone()],
                        },
                    };

                    // Create CoordinationId for each field in each table that has this coordination
                    let mut coordinated_fields = Vec::new();
                    for field_name in &field_names {
                        // Check all tables with this coordination type to find which ones have these fields
                        for (t_name, t_coord) in &typed_coordinations {
                            // Only add if this table's coordination includes this field
                            let t_fields = match t_coord {
                                Coordination::InitializeEqual(f) => f.clone(),
                                Coordination::InitializeSequential { field_names: f, .. } => {
                                    f.clone()
                                }
                                Coordination::InitializeSum { field_names: f, .. } => f.clone(),
                                Coordination::InitializeDerive {
                                    source_field_names,
                                    target_field_name,
                                    ..
                                } => {
                                    let mut all = source_field_names.clone();
                                    all.push(target_field_name.clone());
                                    all
                                }
                                Coordination::InitializeCoherent(d) => match d {
                                    CoherentDataset::Address {
                                        city,
                                        state,
                                        zip,
                                        country,
                                    } => vec![city, state, zip, country]
                                        .into_iter()
                                        .filter(|s| !s.is_empty())
                                        .cloned()
                                        .collect(),
                                    CoherentDataset::PersonName {
                                        first_name,
                                        last_name,
                                        full_name,
                                    } => vec![first_name, last_name, full_name]
                                        .into_iter()
                                        .filter(|s| !s.is_empty())
                                        .cloned()
                                        .collect(),
                                    CoherentDataset::GeoLocation {
                                        latitude,
                                        longitude,
                                        city,
                                        country,
                                    } => vec![latitude, longitude, city, country]
                                        .into_iter()
                                        .filter(|s| !s.is_empty())
                                        .cloned()
                                        .collect(),
                                    CoherentDataset::DateRange {
                                        start_date,
                                        end_date,
                                    } => {
                                        vec![start_date.clone(), end_date.clone()]
                                    }
                                },
                            };

                            if t_fields.contains(field_name) {
                                coordinated_fields.push(
                                    CoordinationId::builder()
                                        .table_name(t_name.clone())
                                        .field_name(field_name.clone())
                                        .build(),
                                );
                            }
                        }
                    }

                    if !coordinated_fields.is_empty() {
                        // Validate the coordination before creating the pair
                        match coordination.validate(self, &coordinated_fields) {
                            Ok(()) => {
                                let pair = CoordinationPair::builder()
                                    .coordinated_fields(coordinated_fields)
                                    .coordination(coordination.clone())
                                    .build();
                                group_pairs.push(pair);
                            }
                            Err(e) => {
                                // Log detailed error for user to fix
                                tracing::error!(
                                    "Skipping invalid coordination for tables {:?}: {}",
                                    group_tables,
                                    e
                                );
                                evenframe_log!(
                                    format!(
                                        "ERROR: Invalid coordination skipped\nTables: {:?}\nCoordination: {:?}\nError: {}\n",
                                        group_tables, coordination, e
                                    ),
                                    "coordination_validation_errors.log",
                                    true
                                );
                            }
                        }
                    }
                }
            }

            if !group_pairs.is_empty() {
                group.tables = group_tables;
                group.coordination_pairs = group_pairs;
                coordination_groups.push(group);
            }
        }

        coordination_groups
    }
}

/// Unified configuration for mock data generation
/// Combines features from both MockGenerationConfig and merge::MockConfig
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MockGenerationConfig {
    // From original MockGenerationConfig
    pub n: usize,
    pub table_level_override: Option<HashMap<StructField, Format>>,
    pub coordination_rules: Vec<crate::schemasync::mockmake::coordinate::Coordination>,
    pub batch_size: usize,
    pub regenerate_fields: Vec<String>,
    pub preservation_mode: PreservationMode,
}

impl Default for MockGenerationConfig {
    fn default() -> Self {
        // Try to load config, fall back to hardcoded defaults if unavailable
        let (n, batch_size, preservation_mode) = match crate::config::EvenframeConfig::new() {
            Ok(config) => (
                config.schemasync.mock_gen_config.default_record_count,
                config.schemasync.mock_gen_config.default_batch_size,
                config.schemasync.mock_gen_config.default_preservation_mode,
            ),
            Err(_) => {
                // Fall back to reasonable defaults if config can't be loaded
                (10, 1000, PreservationMode::Smart)
            }
        };

        Self {
            n,
            table_level_override: None,
            coordination_rules: Vec::new(),
            batch_size,
            regenerate_fields: vec![],
            preservation_mode,
        }
    }
}

impl quote::ToTokens for MockGenerationConfig {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let n = self.n;
        let batch_size = self.batch_size;

        // Convert coordination rules to tokens
        let coordination_rules_tokens = if self.coordination_rules.is_empty() {
            quote::quote! { vec![] }
        } else {
            // We need to serialize coordination rules properly
            // For now, just create an empty vec as coordination rules need their own ToTokens impl
            quote::quote! { vec![] }
        };

        // Convert regenerate fields to tokens
        let regenerate_fields = &self.regenerate_fields;

        // Convert preservation mode to tokens
        let preservation_mode_tokens = match &self.preservation_mode {
            PreservationMode::Smart => {
                quote::quote! { ::evenframe::schemasync::compare::PreservationMode::Smart }
            }
            PreservationMode::Full => {
                quote::quote! { ::evenframe::schemasync::compare::PreservationMode::Full }
            }
            PreservationMode::None => {
                quote::quote! { ::evenframe::schemasync::compare::PreservationMode::None }
            }
        };

        // Generate the full config token stream
        let config_tokens = quote::quote! {
            MockGenerationConfig {
                n: #n,
                table_level_override: None,
                coordination_rules: #coordination_rules_tokens,
                batch_size: #batch_size,
                regenerate_fields: vec![#(#regenerate_fields.to_string()),*],
                preservation_mode: #preservation_mode_tokens,
            }
        };

        tokens.extend(config_tokens);
    }
}
