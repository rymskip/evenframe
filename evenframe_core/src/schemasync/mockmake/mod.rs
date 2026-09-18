pub mod coordinate;
#[cfg(feature = "schemasync")]
pub mod field_value;
pub mod format;
#[cfg(feature = "wasm-plugins")]
pub mod plugin;
pub mod plugin_types;
#[cfg(feature = "schemasync")]
pub mod regex_val_gen;
#[cfg(feature = "schemasync")]
mod repoint;
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
    schemasync::{PreservationMode, database::surql::access::execute_access_query},
    types::{FieldType, StructConfig, StructField, TaggedUnion},
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

/// The mock data one table receives in a run.
#[cfg(feature = "schemasync")]
#[derive(Debug, Clone)]
pub struct TableMocks {
    /// How many records to add, with every field. They take the last
    /// `new_records` ids of the table's id pool.
    pub new_records: usize,
    /// The fields rewritten on the existing records: every field, or only
    /// the changed ones under Smart or Full preservation. A removed field is
    /// typed `Unit` and written as NONE, which unsets it. Empty leaves the
    /// existing records untouched.
    pub rewrite_fields: Vec<StructField>,
}

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
    /// How many ids at the end of each table's id pool have no record yet.
    pub(super) new_records: BTreeMap<String, usize>,
    /// Existing records beyond each table's record count.
    pub(super) excess_ids: BTreeMap<String, Vec<String>>,
    table_mocks: BTreeMap<String, TableMocks>,
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
            new_records: BTreeMap::new(),
            excess_ids: BTreeMap::new(),
            table_mocks: BTreeMap::new(),
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

    /// [`crate::types::link_target_tables`] over this run's types.
    pub(super) fn link_target_tables(&self, type_name: &str) -> Vec<String> {
        crate::types::link_target_tables(type_name, self.tables, self.objects, self.enums)
    }

    /// Whether `field_type` is a link all of whose target tables have no
    /// records.
    fn links_only_to_empty_tables(&self, field_type: &FieldType) -> bool {
        let FieldType::RecordLink(inner) = field_type else {
            return false;
        };
        let FieldType::Other(type_name) = inner.as_ref() else {
            return false;
        };
        let targets = self.link_target_tables(type_name);
        !targets.is_empty()
            && targets
                .iter()
                .all(|t| self.id_map.get(t).is_some_and(Vec::is_empty))
    }

    /// Fails when a record this run writes has a link it must fill (not
    /// behind `Option` or `Vec`) but every table it can point at has no
    /// records. Tables with a mock plugin are left to the plugin.
    fn check_required_links(&self) -> crate::error::Result<()> {
        for (table_name, mocks) in &self.table_mocks {
            let table = self.table(table_name)?;
            let has_plugin = table
                .mock_generation_config
                .as_ref()
                .is_some_and(|c| c.plugin.is_some());
            if has_plugin {
                continue;
            }
            let existing = self
                .id_map
                .get(table_name)
                .map_or(0, Vec::len)
                .saturating_sub(mocks.new_records);
            let written_fields = if mocks.new_records > 0 {
                &table.struct_config.fields
            } else if existing > 0 {
                &mocks.rewrite_fields
            } else {
                continue;
            };
            for field in written_fields {
                if let Some(targets) = self.unfillable_link(&field.field_type, &mut BTreeSet::new())
                {
                    return Err(crate::error::EvenframeError::config(format!(
                        "`{table_name}.{}` must link to {}, which has no records; \
                         give it records or make the link optional",
                        field.field_name,
                        targets.join(" or ")
                    )));
                }
            }
        }
        Ok(())
    }

    /// Whether a value of `field_type` cannot be generated because a
    /// required link in it, directly or in a nested object, has nothing to
    /// point at.
    pub(super) fn has_unfillable_link(&self, field_type: &FieldType) -> bool {
        self.unfillable_link(field_type, &mut BTreeSet::new())
            .is_some()
    }

    /// The target tables of a required link inside `field_type` that has
    /// nothing to point at, looking through nested objects.
    fn unfillable_link(
        &self,
        field_type: &FieldType,
        visited: &mut BTreeSet<String>,
    ) -> Option<Vec<String>> {
        match field_type {
            FieldType::RecordLink(inner) => match inner.as_ref() {
                FieldType::Other(type_name) if self.links_only_to_empty_tables(field_type) => {
                    Some(self.link_target_tables(type_name))
                }
                _ => None,
            },
            FieldType::Other(type_name) if visited.insert(type_name.clone()) => self
                .objects
                .get(type_name)?
                .fields
                .iter()
                .find_map(|f| self.unfillable_link(&f.field_type, visited)),
            _ => None,
        }
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

    /// The effective config of `table_name`.
    pub(super) fn table(&self, table_name: &str) -> crate::error::Result<&TableConfig> {
        self.tables
            .get(table_name)
            .map(TableConfig::effective)
            .ok_or_else(|| {
                crate::error::EvenframeError::config(format!("unknown table `{table_name}`"))
            })
    }

    /// The tables defined in the database. A table that is not defined yet
    /// has no records, and selecting from it is an error.
    async fn defined_tables(&self) -> Result<BTreeSet<String>, String> {
        let tables: Vec<String> = self
            .db
            .query("RETURN object::keys((INFO FOR DB).tables);")
            .await
            .and_then(|mut response| response.take(0))
            .map_err(|e| format!("listing the database's tables: {e}"))?;
        Ok(tables.into_iter().collect())
    }

    pub async fn generate_ids(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        evenframe_log!("", "record_diffs.log");
        tracing::trace!("Starting ID generation for all tables");
        let mut map = BTreeMap::new();
        let mut new_records = BTreeMap::new();
        let mut excess_ids = BTreeMap::new();

        let full_refresh = self.schemasync_config.mock_gen_config.full_refresh_mode;
        let defined_tables = self.defined_tables().await?;

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

                new_records.insert(table_name.clone(), desired_count);
                map.insert(table_name.clone(), ids);
                continue;
            }

            // Cast in the query so each id comes back as a SurrealQL record
            // literal, escaped where its key needs it.
            let existing_ids: Vec<String> = if defined_tables.contains(table_name) {
                self.db
                    .query(format!("SELECT VALUE <string> id FROM {table_name};"))
                    .await
                    .and_then(|mut response| response.take(0))
                    .map_err(|e| format!("reading the existing ids of `{table_name}`: {e}"))?
            } else {
                Vec::new()
            };
            let existing_count = existing_ids.len();
            tracing::trace!(
                table = %table_name,
                existing_count = existing_count,
                desired_count = desired_count,
                "Counted existing records"
            );

            // Keep the existing records first, then top up with sequential
            // ids that skip any already taken: deleted records leave gaps, so
            // the next free number is not `existing_count + 1`.
            let taken: BTreeSet<String> = existing_ids.iter().cloned().collect();
            let mut ids = existing_ids;
            let excess = ids.split_off(desired_count.min(existing_count));
            let fresh = (1..)
                .map(|n| format!("{table_name}:{n}"))
                .filter(|id| !taken.contains(id))
                .take(desired_count - ids.len());
            ids.extend(fresh);

            new_records.insert(
                table_name.clone(),
                desired_count.saturating_sub(existing_count),
            );
            // Without mock data, record counts are not managed.
            if self.schemasync_config.should_generate_mocks {
                excess_ids.insert(table_name.clone(), excess);
            }
            map.insert(table_name.clone(), ids);
        }

        self.id_map = map;
        self.new_records = new_records;
        self.excess_ids = excess_ids;

        tracing::debug!(table_count = self.id_map.len(), "ID generation complete");

        evenframe_log!(
            format!(
                "New records: {:#?}\nExcess records: {:#?}",
                self.new_records, self.excess_ids
            ),
            "record_diffs.log",
            true
        );

        Ok(())
    }

    /// Remove old data based on schema changes
    pub async fn remove_old_data(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        tracing::trace!("Removing old data based on schema changes");
        let full_refresh = self.schemasync_config.mock_gen_config.full_refresh_mode;
        let mut statements = String::new();

        // Records are only replaced when mock data is generated.
        if full_refresh && self.schemasync_config.should_generate_mocks {
            let defined_tables = self.defined_tables().await?;
            for table_name in self.tables.keys().filter(|t| defined_tables.contains(*t)) {
                statements.push_str(&format!("DELETE {table_name};\n"));
            }
        }

        // Fields removed from the models are removed in every mode: DEFINE
        // FIELD OVERWRITE does not drop stale ones. Mockmake's full refresh
        // runs without a schema comparison and only clears the tables.
        match self.schema_changes() {
            Ok(schema_changes) => {
                statements.push_str(&self.generate_remove_statements(schema_changes))
            }
            Err(_) if full_refresh => {}
            Err(e) => return Err(e.into()),
        }

        evenframe_log!(&statements, "remove_statements.surql");
        if !statements.is_empty() {
            self.db
                .query(statements)
                .await
                .and_then(|response| response.check())
                .map_err(|e| format!("removing old data: {e}"))?;
        }
        tracing::trace!("Old data removal complete");
        Ok(())
    }

    /// Execute access query on main database
    pub async fn execute_access(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        tracing::trace!("Executing access definitions");
        let access_query = self
            .comparator
            .as_ref()
            .ok_or("the schema comparison has not run")?
            .get_access_query();

        tracing::debug!(query_length = access_query.len(), "Executing access query");

        execute_access_query(
            self.db,
            access_query,
            &self.schemasync_config.database.database,
        )
        .await
    }

    /// Target the `selected` tables (every table when `None`) for mock data,
    /// without diffing against the database. Tables that aren't selected
    /// keep all their records, and only existing ones are link targets. Call
    /// after
    /// [`Self::generate_ids`].
    pub fn select_tables_for_insert(&mut self, selected: Option<&BTreeSet<String>>) {
        let is_selected = |name: &str| selected.is_none_or(|s| s.contains(name));

        for (table_name, ids) in self.id_map.iter_mut() {
            if !is_selected(table_name) {
                let new_ids = self.new_records.get(table_name).copied().unwrap_or(0);
                ids.truncate(ids.len().saturating_sub(new_ids));
                self.excess_ids.remove(table_name);
            }
        }

        self.table_mocks = self
            .tables
            .iter()
            .filter(|(name, _)| is_selected(name))
            .map(|(name, table)| {
                let mocks = TableMocks {
                    new_records: self.new_records.get(name).copied().unwrap_or(0),
                    rewrite_fields: table.effective().struct_config.fields.clone(),
                };
                (name.clone(), mocks)
            })
            .collect();

        tracing::info!(
            selected_tables = self.table_mocks.len(),
            "Tables selected for mock data"
        );
    }

    /// Delete the records beyond each table's count.
    pub async fn remove_excess_records(&self) -> Result<(), Box<dyn std::error::Error>> {
        let deletes = self.excess_record_deletes();
        if deletes.is_empty() {
            return Ok(());
        }
        evenframe_log!(&deletes, "remove_statements.surql");
        self.db
            .query(deletes)
            .await
            .and_then(|response| response.check())
            .map_err(|e| format!("deleting excess records: {e}"))?;
        Ok(())
    }

    /// The schema changes the comparison found.
    fn schema_changes(&self) -> Result<&crate::schemasync::compare::SchemaChanges, String> {
        self.comparator
            .as_ref()
            .and_then(|c| c.get_schema_changes())
            .ok_or_else(|| "the schema comparison has not run".to_string())
    }

    /// Plan the mock data each table receives from the schema changes: new
    /// records up to each table's count, and existing records rewritten where
    /// their fields changed. A full refresh writes every table from scratch.
    pub async fn filter_changes(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        tracing::trace!("Planning mock data from the schema comparison");
        self.table_mocks = if self.schemasync_config.mock_gen_config.full_refresh_mode {
            self.tables
                .keys()
                .map(|name| {
                    let mocks = TableMocks {
                        new_records: self.new_records.get(name).copied().unwrap_or(0),
                        rewrite_fields: Vec::new(),
                    };
                    (name.clone(), mocks)
                })
                .collect()
        } else {
            self.plan_table_mocks(self.schema_changes()?)
        };

        tracing::info!(tables = self.table_mocks.len(), "Mock data planned");
        evenframe_log!(format!("{:#?}", self.table_mocks), "filtered.log");
        Ok(())
    }

    pub(super) async fn generate_mock_data(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.check_required_links()?;
        tracing::trace!("Starting mock data generation");

        // Sort tables by dependencies to ensure proper insertion order
        let planned_tables: BTreeMap<String, TableConfig> = self
            .tables
            .iter()
            .filter(|(name, _)| self.table_mocks.contains_key(*name))
            .map(|(name, table)| (name.clone(), table.clone()))
            .collect();
        let sorted_table_names =
            sort_tables_by_dependencies(&planned_tables, self.objects, self.enums);

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
            if let Some(mocks) = self.table_mocks.get(table_name) {
                tracing::trace!(table = %table_name, "Processing table for mock data");

                if self.schemasync_config.should_generate_mocks {
                    let stmts = self.generate_mock_statements(table_name, mocks)?;

                    tracing::debug!(
                        table = %table_name,
                        statement_count = stmts.lines().count(),
                        "Generated mock data statements"
                    );

                    evenframe_log!(&stmts, "all_statements.surql", true);

                    // Execute and validate upsert statements
                    use crate::schemasync::database::surql::execute::execute_and_validate;

                    match execute_and_validate(self.db, &stmts, "mock data", table_name).await {
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
        // After the new records exist, so every kept id is a record.
        self.repoint_links_to_excess().await?;
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
    pub fn build_coordination_groups(
        &mut self,
    ) -> Result<Vec<CoordinationGroup>, crate::error::EvenframeError> {
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
                        coordination.validate(self, &coordinated_fields).map_err(|e| {
                            crate::error::EvenframeError::validation(format!(
                                "invalid mock data coordination {coordination:?} on {group_tables:?}: {e}"
                            ))
                        })?;
                        group_pairs.push(
                            CoordinationPair::builder()
                                .coordinated_fields(coordinated_fields)
                                .coordination(coordination.clone())
                                .build(),
                        );
                    }
                }
            }

            if !group_pairs.is_empty() {
                group.tables = group_tables;
                group.coordination_pairs = group_pairs;
                coordination_groups.push(group);
            }
        }

        Ok(coordination_groups)
    }
}

// Import for MockGenerationConfig (always available, but avoid duplicates with
// the schemasync-gated engine imports above)
#[cfg(not(feature = "schemasync"))]
use crate::schemasync::PreservationMode;

/// A table's `#[mock_data(...)]` settings.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MockGenerationConfig {
    pub n: usize,
    pub coordination_rules: Vec<crate::schemasync::mockmake::coordinate::Coordination>,
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
            coordination_rules: Vec::new(),
            preservation_mode: mock_gen_config.default_preservation_mode,
            plugin: None,
        }
    }
}

impl quote::ToTokens for MockGenerationConfig {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let n = self.n;
        let coordination_rules = &self.coordination_rules;

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
                coordination_rules: vec![#(#coordination_rules),*],
                preservation_mode: #preservation_mode_tokens,
                plugin: #plugin_tokens,
            }
        };

        tokens.extend(config_tokens);
    }
}
