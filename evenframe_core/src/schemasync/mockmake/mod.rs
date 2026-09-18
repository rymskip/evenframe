pub mod coordinate;
#[cfg(feature = "schemasync")]
pub mod field_value;
#[cfg(feature = "schemasync")]
pub mod field_value_recursive;
pub mod format;
#[cfg(feature = "wasm-plugins")]
pub mod plugin;
pub mod plugin_types;
#[cfg(feature = "schemasync")]
pub mod regex_val_gen;
#[cfg(feature = "schemasync")]
pub mod validator_gen;

#[cfg(feature = "schemasync")]
use crate::{
    dependency::sort_tables_by_dependencies,
    evenframe_log,
    schemasync::TableConfig,
    schemasync::compare::surql::SurrealdbComparator,
    schemasync::mockmake::coordinate::{
        CoherentDataset, Coordination, CoordinationGroup, CoordinationId, CoordinationPair,
    },
    schemasync::mockmake::format::Format,
    schemasync::{PreservationMode, database::surql::access::execute_access_query},
    types::{StructConfig, StructField, TaggedUnion},
    wrappers::EvenframeRecordId,
};
#[cfg(feature = "schemasync")]
use rand::RngExt;
#[cfg(feature = "schemasync")]
use std::collections::{BTreeMap, BTreeSet};
#[cfg(feature = "schemasync")]
use surrealdb::Surreal;
#[cfg(feature = "schemasync")]
use surrealdb::engine::local::Db;
#[cfg(feature = "schemasync")]
use surrealdb::engine::remote::http::Client;
#[cfg(feature = "schemasync")]
use uuid::Uuid;

#[cfg(feature = "schemasync")]
#[derive(Debug)]
pub struct Mockmaker<'a> {
    db: &'a Surreal<Client>,
    pub(super) tables: &'a BTreeMap<String, TableConfig>,
    pub(super) objects: &'a BTreeMap<String, StructConfig>,
    enums: &'a BTreeMap<String, TaggedUnion>,
    pub(super) schemasync_config: &'a crate::schemasync::config::SchemasyncConfig,
    pub comparator: Option<SurrealdbComparator<'a>>,
    pub(super) registry: &'a crate::types::ForeignTypeRegistry,

    // Runtime state
    pub(super) id_map: BTreeMap<String, Vec<String>>,
    pub(super) record_diffs: BTreeMap<String, i32>,
    filtered_tables: BTreeMap<String, TableConfig>,
    filtered_objects: BTreeMap<String, StructConfig>,
    /// Pre-computed coordinated values, keyed by record index and field.
    pub coordinated_values: BTreeMap<(usize, CoordinationId), String>,
    /// Record count for every table, taking precedence over `#[mock_data(n)]`.
    pub count_override: Option<usize>,
    #[cfg(feature = "wasm-plugins")]
    pub(super) plugin_manager: Option<std::cell::RefCell<plugin::PluginManager>>,
}

#[cfg(feature = "schemasync")]
impl<'a> Mockmaker<'a> {
    pub fn new(
        db: &'a Surreal<Client>,
        tables: &'a BTreeMap<String, TableConfig>,
        objects: &'a BTreeMap<String, StructConfig>,
        enums: &'a BTreeMap<String, TaggedUnion>,
        schemasync_config: &'a crate::schemasync::config::SchemasyncConfig,
        registry: &'a crate::types::ForeignTypeRegistry,
    ) -> Self {
        Self {
            db,
            tables,
            objects,
            enums,
            schemasync_config,
            comparator: Some(SurrealdbComparator::new(db, schemasync_config)),
            registry,
            id_map: BTreeMap::new(),
            record_diffs: BTreeMap::new(),
            filtered_tables: BTreeMap::new(),
            filtered_objects: BTreeMap::new(),
            coordinated_values: BTreeMap::new(),
            count_override: None,
            #[cfg(feature = "wasm-plugins")]
            plugin_manager: {
                if schemasync_config.plugins.is_empty() {
                    None
                } else {
                    // Resolve project root from config
                    let project_root = std::env::current_dir().unwrap_or_default();
                    match plugin::PluginManager::new(&schemasync_config.plugins, &project_root) {
                        Ok(pm) => Some(std::cell::RefCell::new(pm)),
                        Err(e) => {
                            tracing::error!("Failed to initialize WASM plugins: {}", e);
                            None
                        }
                    }
                }
            },
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

    /// How many mock records to generate for `table_config`: the count
    /// override, its `#[mock_data(n = ...)]`, or the configured
    /// `default_record_count`. ID pools and INSERT/UPSERT generation must
    /// agree on this, or record links point at records that are never created.
    pub fn record_count(&self, table_config: &TableConfig) -> usize {
        self.count_override.unwrap_or_else(|| {
            table_config
                .mock_generation_config
                .as_ref()
                .map(|c| c.n)
                .unwrap_or(self.schemasync_config.mock_gen_config.default_record_count)
        })
    }

    /// Generate IDs for tables
    pub async fn generate_ids(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        evenframe_log!("", "record_diffs.log");
        tracing::trace!("Starting ID generation for all tables");
        let mut map = BTreeMap::new();
        let mut record_diffs = BTreeMap::new();

        let full_refresh = self.schemasync_config.mock_gen_config.full_refresh_mode;

        // Process tables sequentially to avoid reference issues
        // Since these are just SELECT queries, they should be fast enough
        for (table_name, table_config) in self.tables {
            let table_config = table_config.effective();
            tracing::trace!(table = %table_name, "Generating IDs for table");

            let desired_count = self.record_count(table_config);

            // In full refresh mode, all data will be deleted and recreated.
            // Generate clean sequential IDs instead of reusing stale DB IDs,
            // which may reference records that no longer exist after deletion.
            if full_refresh {
                let ids: Vec<String> = (1..=desired_count)
                    .map(|i| format!("{table_name}:{i}"))
                    .collect();

                tracing::trace!(
                    table = %table_name,
                    desired_count = desired_count,
                    "Full refresh mode - generating fresh sequential IDs"
                );

                record_diffs.insert(table_name.clone(), desired_count as i32);
                map.insert(table_name.clone(), ids);
                continue;
            }

            // Query existing IDs
            let query = format!("SELECT id FROM {table_name};",);
            tracing::trace!("Querying existing IDs {query}");
            let mut response = self.db.query(query).await.expect(
                "Something went wrong getting the ids from the db for mock data generation",
            );
            evenframe_log!(&format!("{:?}", response), "record_diffs.log", true);

            let existing_values: Vec<serde_json::Value> = response.take(0).unwrap_or_default();

            struct IdResponse {
                id: EvenframeRecordId,
            }

            let existing_ids: Vec<IdResponse> = existing_values
                .into_iter()
                .filter_map(|v| {
                    v.get("id")
                        .and_then(|id| id.as_str())
                        .map(|id_str| IdResponse {
                            id: EvenframeRecordId::from(id_str.to_string()),
                        })
                })
                .collect();

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

        // In full refresh mode, delete all records from all tables first
        if self.schemasync_config.mock_gen_config.full_refresh_mode {
            tracing::info!("Full refresh mode - deleting all records from all tables");
            let mut delete_all = String::new();
            for table_name in self.tables.keys() {
                delete_all.push_str(&format!("DELETE {};\n", table_name));
            }

            // Even in full refresh, we must remove fields that were deleted from
            // Rust structs — DEFINE FIELD OVERWRITE only updates existing fields,
            // it does not remove stale ones from the database schema.
            if let Some(comparator) = self.comparator.as_ref()
                && let Some(schema_changes) = comparator.get_schema_changes()
            {
                let remove_stmts = self.generate_remove_statements(schema_changes);
                if !remove_stmts.is_empty() {
                    tracing::info!("Full refresh mode - removing stale fields/tables");
                    delete_all.push_str(&remove_stmts);
                }
            }

            if !delete_all.is_empty() {
                evenframe_log!(&delete_all, "remove_statements.surql");
                self.db.query(delete_all).await?;
            }
            tracing::trace!("Full refresh data deletion complete");
            return Ok(());
        }

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

        execute_access_query(
            self.db,
            access_query,
            &self.schemasync_config.database.database,
        )
        .await
    }

    /// Target the `selected` tables (every table when `None`) that generate
    /// records, without diffing against the database. Other tables only keep
    /// their existing records as link targets, and a targeted table that
    /// links to a table without records is an error. A table generating no
    /// records creates no links, so it is neither targeted nor checked.
    /// Call after [`Self::generate_ids`].
    pub fn select_tables_for_insert(
        &mut self,
        selected: Option<&BTreeSet<String>>,
    ) -> crate::error::Result<()> {
        let is_selected = |name: &str| selected.is_none_or(|s| s.contains(name));

        for (table_name, ids) in self.id_map.iter_mut() {
            if !is_selected(table_name) {
                let new_ids = self
                    .record_diffs
                    .get(table_name)
                    .copied()
                    .unwrap_or(0)
                    .max(0) as usize;
                ids.truncate(ids.len().saturating_sub(new_ids));
            }
        }

        self.filtered_tables = self
            .tables
            .iter()
            .filter(|(name, table)| is_selected(name) && self.record_count(table.effective()) > 0)
            .map(|(name, table)| (name.clone(), table.clone()))
            .collect();
        self.filtered_objects = self.objects.clone();

        for table_name in self.filtered_tables.keys() {
            let dependencies = crate::dependency::collect_table_dependencies(
                table_name,
                self.tables,
                self.objects,
                self.enums,
                &mut BTreeSet::new(),
            );
            for dependency in dependencies {
                if self.id_map.get(&dependency).is_none_or(Vec::is_empty) {
                    return Err(crate::error::EvenframeError::config(format!(
                        "`{table_name}` links to `{dependency}`, which has no records; \
                         include `{dependency}` in the selected tables or insert its records first"
                    )));
                }
            }
        }

        tracing::info!(
            selected_tables = self.filtered_tables.len(),
            "Tables selected for mock data"
        );
        Ok(())
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
                let table = table.effective();
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
                            #[cfg(feature = "dev-mode")]
                            {
                                let error_msg = format!(
                                    "Failed to execute upsert statements for table {}: {}",
                                    table_name, e
                                );
                                evenframe_log!(&error_msg, "results.log", true);
                            }
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
        let mut coordination_map: BTreeMap<String, Vec<(String, Coordination)>> = BTreeMap::new();

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
                        Coordination::OneToOne(field_name) => vec![field_name.clone()],
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
                            CoherentDataset::GeoRadius {
                                latitude,
                                longitude,
                                ..
                            } => [latitude, longitude]
                                .into_iter()
                                .filter(|s| !s.is_empty())
                                .cloned()
                                .collect(),
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

            let mut group_tables = BTreeSet::new();
            let mut group_pairs = Vec::new();

            // Group coordinations by their type and fields
            let mut coordination_by_type: BTreeMap<String, Vec<(String, Coordination)>> =
                BTreeMap::new();

            for (table_name, coordination) in table_coordinations {
                let type_key = match &coordination {
                    Coordination::InitializeEqual(_) => "equal",
                    Coordination::InitializeSequential { .. } => "sequential",
                    Coordination::InitializeSum { .. } => "sum",
                    Coordination::InitializeDerive { .. } => "derive",
                    Coordination::OneToOne(_) => "one_to_one",
                    Coordination::InitializeCoherent(_) => "coherent",
                };

                coordination_by_type
                    .entry(type_key.to_string())
                    .or_default()
                    .push((table_name.clone(), coordination.clone()));

                group_tables.insert(table_name);
            }

            // Create CoordinationPair for each unique coordination
            for typed_coordinations in coordination_by_type.values() {
                // Group coordinations with identical rules
                let mut processed = BTreeSet::new();

                for (_, coordination) in typed_coordinations {
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
                        Coordination::OneToOne(field_name) => vec![field_name.clone()],
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
                            CoherentDataset::GeoRadius {
                                latitude,
                                longitude,
                                ..
                            } => [latitude, longitude]
                                .into_iter()
                                .filter(|s| !s.is_empty())
                                .cloned()
                                .collect(),
                        },
                    };

                    // Create CoordinationId for each field in each table that has this coordination
                    let mut coordinated_fields = Vec::new();
                    for field_name in &field_names {
                        // Check all tables with this coordination type to find which ones have these fields
                        for (t_name, t_coord) in typed_coordinations {
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
                                Coordination::OneToOne(f) => vec![f.clone()],
                                Coordination::InitializeCoherent(d) => match d {
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
                                    } => {
                                        vec![start_date.clone(), end_date.clone()]
                                    }
                                    CoherentDataset::GeoRadius {
                                        latitude,
                                        longitude,
                                        ..
                                    } => [latitude, longitude]
                                        .into_iter()
                                        .filter(|s| !s.is_empty())
                                        .cloned()
                                        .collect(),
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

// Import for MockGenerationConfig (always available, but avoid duplicates with
// the schemasync-gated engine imports above)
#[cfg(not(feature = "schemasync"))]
use crate::schemasync::PreservationMode;
#[cfg(not(feature = "schemasync"))]
use crate::schemasync::mockmake::format::Format;
#[cfg(not(feature = "schemasync"))]
use crate::types::StructField;

/// Unified configuration for mock data generation
/// Combines features from both MockGenerationConfig and merge::MockConfig
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MockGenerationConfig {
    // From original MockGenerationConfig
    pub n: usize,
    pub table_level_override: Option<std::collections::HashMap<StructField, Format>>,
    pub coordination_rules: Vec<crate::schemasync::mockmake::coordinate::Coordination>,
    pub batch_size: usize,
    pub regenerate_fields: Vec<String>,
    pub preservation_mode: PreservationMode,
    /// Name of the WASM plugin to use for table-level mock generation.
    #[serde(default)]
    pub plugin: Option<String>,
}

impl Default for MockGenerationConfig {
    fn default() -> Self {
        // Only mock settings are read, so the connection env vars aren't needed.
        let mock_gen_config = match crate::config::EvenframeConfig::new_offline() {
            Ok(config) => config.schemasync.mock_gen_config,
            Err(e) => {
                tracing::warn!("Using default mock settings; the configuration did not load: {e}");
                crate::schemasync::config::SchemasyncMockGenConfig::default()
            }
        };

        Self {
            n: mock_gen_config.default_record_count,
            table_level_override: None,
            coordination_rules: Vec::new(),
            batch_size: mock_gen_config.default_batch_size,
            regenerate_fields: vec![],
            preservation_mode: mock_gen_config.default_preservation_mode,
            plugin: None,
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
                quote::quote! { ::evenframe::schemasync::PreservationMode::Smart }
            }
            PreservationMode::Full => {
                quote::quote! { ::evenframe::schemasync::PreservationMode::Full }
            }
            PreservationMode::None => {
                quote::quote! { ::evenframe::schemasync::PreservationMode::None }
            }
        };

        // Generate the full config token stream
        let plugin_tokens = match &self.plugin {
            Some(name) => quote::quote! { Some(#name.to_string()) },
            None => quote::quote! { None },
        };

        let config_tokens = quote::quote! {
            MockGenerationConfig {
                n: #n,
                table_level_override: None,
                coordination_rules: #coordination_rules_tokens,
                batch_size: #batch_size,
                regenerate_fields: vec![#(#regenerate_fields.to_string()),*],
                preservation_mode: #preservation_mode_tokens,
                plugin: #plugin_tokens,
            }
        };

        tokens.extend(config_tokens);
    }
}

#[cfg(all(test, feature = "schemasync"))]
mod select_tables_tests {
    use super::*;
    use crate::schemasync::{TableConfig, config::SchemasyncConfig};
    use crate::tooling::{BuildConfig, build_all_configs};
    use std::fs;
    use tempfile::TempDir;

    /// Two tables generating no records that link to each other, and one
    /// generating records that links to one of them.
    fn scanned_tables() -> BTreeMap<String, TableConfig> {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("Cargo.toml"),
            "[package]\nname = \"fixture\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        fs::create_dir_all(tmp.path().join("src")).unwrap();
        fs::write(
            tmp.path().join("src/lib.rs"),
            r#"
#[derive(Evenframe)]
#[mock_data(n = 0)]
pub struct Author { pub id: String, pub favorite: RecordLink<Post> }

#[derive(Evenframe)]
#[mock_data(n = 0)]
pub struct Post { pub id: String, pub author: RecordLink<Author> }

#[derive(Evenframe)]
#[mock_data(n = 2)]
pub struct Comment { pub id: String, pub post: RecordLink<Post> }
"#,
        )
        .unwrap();
        let config = BuildConfig {
            scan_path: tmp.path().to_path_buf(),
            ..BuildConfig::default()
        };
        let (_, tables, _) = build_all_configs(&config).unwrap();
        tables
    }

    /// The tables `select_tables_for_insert` targets over an empty database.
    fn targeted(
        tables: &BTreeMap<String, TableConfig>,
        selected: Option<&[&str]>,
    ) -> crate::error::Result<Vec<String>> {
        let db = Surreal::<Client>::init();
        let config: SchemasyncConfig =
            toml::from_str("should_generate_mocks = true\n[database]\nurl = \"x\"\n").unwrap();
        let objects = BTreeMap::new();
        let enums = BTreeMap::new();
        let registry = crate::types::ForeignTypeRegistry::default();
        let mut mockmaker = Mockmaker::new(&db, tables, &objects, &enums, &config, &registry);
        // What `generate_ids` leaves over an empty database: generated IDs only.
        let id_map = tables
            .iter()
            .map(|(name, table)| {
                let count = mockmaker.record_count(table.effective());
                (
                    name.clone(),
                    (1..=count).map(|i| format!("{name}:{i}")).collect(),
                )
            })
            .collect();
        mockmaker.id_map = id_map;
        let selected: Option<BTreeSet<String>> =
            selected.map(|names| names.iter().map(|n| n.to_string()).collect());
        mockmaker.select_tables_for_insert(selected.as_ref())?;
        Ok(mockmaker.filtered_tables.keys().cloned().collect())
    }

    #[test]
    fn tables_generating_no_records_are_neither_targeted_nor_checked() {
        let tables = scanned_tables();
        assert_eq!(
            targeted(&tables, Some(&["author", "post"])).unwrap(),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_targeted_table_linking_to_an_empty_table_is_an_error() {
        let tables = scanned_tables();
        let err = targeted(&tables, None).unwrap_err().to_string();
        assert!(err.contains("`comment` links to `post`"), "{err}");
    }
}
