pub mod coordinate;
#[cfg(feature = "mockmake")]
pub mod field_value;
pub mod format;
#[cfg(all(feature = "mockmake", feature = "wasm-plugins"))]
pub mod plugin;
pub mod plugin_types;
#[cfg(feature = "mockmake")]
pub mod regex_val_gen;
#[cfg(feature = "mockmake")]
mod repoint;
#[cfg(feature = "mockmake")]
pub(crate) mod unique;
#[cfg(feature = "mockmake")]
pub mod validator_gen;

#[cfg(feature = "mockmake")]
use crate::{
    dependency::sort_tables_by_dependencies,
    evenframe_log,
    schemasync::TableConfig,
    schemasync::compare::SchemaChanges,
    schemasync::database::surql::execute::{RPC_SIZE_LIMIT, execute_and_validate, execute_bound},
    schemasync::mockmake::coordinate::{
        CoherentDataset, Coordination, CoordinationGroup, CoordinationId, CoordinationPair,
    },
    types::{FieldType, StructConfig, StructField, TaggedUnion},
};
#[cfg(feature = "mockmake")]
use rand::RngExt;
#[cfg(feature = "mockmake")]
use std::collections::{BTreeMap, BTreeSet};
#[cfg(feature = "mockmake")]
use surrealdb::Surreal;
#[cfg(feature = "mockmake")]
use surrealdb::engine::remote::http::Client;
#[cfg(feature = "mockmake")]
use uuid::Uuid;

/// The mock data one table receives in a run.
#[cfg(feature = "mockmake")]
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

#[cfg(feature = "mockmake")]
#[derive(Debug)]
pub struct Mockmaker<'a> {
    db: &'a Surreal<Client>,
    pub(super) tables: &'a BTreeMap<String, TableConfig>,
    pub(super) objects: &'a BTreeMap<String, StructConfig>,
    enums: &'a BTreeMap<String, TaggedUnion>,
    pub(super) schemasync_config: &'a crate::schemasync::config::SchemasyncConfig,
    pub(super) registry: &'a crate::types::ForeignTypeRegistry,

    // Runtime state
    pub(super) id_map: BTreeMap<String, Vec<String>>,
    /// How many ids at the end of each table's id pool have no record yet.
    pub(super) new_records: BTreeMap<String, usize>,
    /// Existing records beyond each table's record count.
    pub(super) excess_ids: BTreeMap<String, Vec<String>>,
    table_mocks: BTreeMap<String, TableMocks>,
    /// Pre-computed coordinated values, keyed by record index and field.
    pub coordinated_values: coordinate::CoordinatedValues,
    /// Record count for every table, taking precedence over `#[mock_data(n)]`.
    pub count_override: Option<usize>,
    /// The tables each linked type name can point at, resolved once.
    link_targets: std::cell::RefCell<BTreeMap<String, std::rc::Rc<[String]>>>,
    #[cfg(feature = "wasm-plugins")]
    pub(super) plugin_manager: Option<std::cell::RefCell<plugin::PluginManager>>,
}

#[cfg(feature = "mockmake")]
impl<'a> Mockmaker<'a> {
    pub fn new(
        db: &'a Surreal<Client>,
        tables: &'a BTreeMap<String, TableConfig>,
        objects: &'a BTreeMap<String, StructConfig>,
        enums: &'a BTreeMap<String, TaggedUnion>,
        schemasync_config: &'a crate::schemasync::config::SchemasyncConfig,
        registry: &'a crate::types::ForeignTypeRegistry,
    ) -> crate::error::Result<Self> {
        #[cfg(feature = "wasm-plugins")]
        let plugin_manager = if schemasync_config.plugins.is_empty() {
            None
        } else {
            let project_root = crate::config::EvenframeConfig::find_project_root().ok_or_else(|| {
                crate::error::EvenframeError::config(
                    "[schemasync] plugins are resolved against the project root, but no evenframe.toml was found",
                )
            })?;
            Some(std::cell::RefCell::new(plugin::PluginManager::new(
                &schemasync_config.plugins,
                &project_root,
            )?))
        };
        Ok(Self {
            db,
            tables,
            objects,
            enums,
            schemasync_config,
            registry,
            id_map: BTreeMap::new(),
            new_records: BTreeMap::new(),
            excess_ids: BTreeMap::new(),
            table_mocks: BTreeMap::new(),
            coordinated_values: coordinate::CoordinatedValues::default(),
            count_override: None,
            link_targets: std::cell::RefCell::new(BTreeMap::new()),
            #[cfg(feature = "wasm-plugins")]
            plugin_manager,
        })
    }

    /// [`crate::types::link_target_tables`] over this run's types, resolved
    /// once per type name.
    pub(super) fn link_target_tables(&self, type_name: &str) -> std::rc::Rc<[String]> {
        if let Some(targets) = self.link_targets.borrow().get(type_name) {
            return std::rc::Rc::clone(targets);
        }
        let targets: std::rc::Rc<[String]> =
            crate::types::link_target_tables(type_name, self.tables, self.objects, self.enums)
                .into();
        self.link_targets
            .borrow_mut()
            .insert(type_name.to_string(), std::rc::Rc::clone(&targets));
        targets
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
                .all(|table| self.id_map.get(table).is_some_and(Vec::is_empty))
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
                .is_some_and(|config| config.plugin.is_some());
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
                    Some(self.link_target_tables(type_name).to_vec())
                }
                _ => None,
            },
            FieldType::Other(type_name) if visited.insert(type_name.clone()) => self
                .objects
                .get(type_name)?
                .fields
                .iter()
                .find_map(|field| self.unfillable_link(&field.field_type, visited)),
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
                .and_then(|mock_config| mock_config.record_count)
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
    /// The ids of every table's existing records, read in one request. Each
    /// is cast in the query so it comes back as a SurrealQL record literal,
    /// escaped where its key needs it. None are read in full refresh mode,
    /// which deletes them.
    async fn existing_ids(
        &self,
        full_refresh: bool,
    ) -> Result<BTreeMap<String, Vec<String>>, String> {
        if full_refresh {
            return Ok(BTreeMap::new());
        }
        let defined = self.defined_tables().await?;
        let tables: Vec<&String> = self
            .tables
            .keys()
            .filter(|table_name| defined.contains(*table_name))
            .collect();
        if tables.is_empty() {
            return Ok(BTreeMap::new());
        }
        let query: String = tables
            .iter()
            .map(|table_name| format!("SELECT VALUE <string> id FROM {table_name};\n"))
            .collect();
        let mut response = self
            .db
            .query(query)
            .await
            .map_err(|error| format!("reading the existing record ids: {error}"))?;
        tables
            .into_iter()
            .enumerate()
            .map(|(index, table_name)| {
                let ids: Vec<String> = response.take(index).map_err(|error| {
                    format!("reading the existing ids of `{table_name}`: {error}")
                })?;
                Ok((table_name.clone(), ids))
            })
            .collect()
    }

    async fn defined_tables(&self) -> Result<BTreeSet<String>, String> {
        let tables: Vec<String> = self
            .db
            .query("RETURN object::keys((INFO FOR DB).tables);")
            .await
            .and_then(|mut response| response.take(0))
            .map_err(|error| format!("listing the database's tables: {error}"))?;
        Ok(tables.into_iter().collect())
    }

    pub async fn generate_ids(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        evenframe_log!("", "record_diffs.log");
        tracing::trace!("Starting ID generation for all tables");
        let mut map = BTreeMap::new();
        let mut new_records = BTreeMap::new();
        let mut excess_ids = BTreeMap::new();

        let full_refresh = self.schemasync_config.mock_gen_config.full_refresh_mode;
        let mut existing = self.existing_ids(full_refresh).await?;

        for (table_name, table_config) in self.tables {
            let table_config = table_config.effective();
            tracing::trace!(table = %table_name, "Generating IDs for table");

            let desired_count = self.record_count(table_config);

            // In full refresh mode, all data will be deleted and recreated.
            // Generate clean sequential IDs instead of reusing stale DB IDs,
            // which may reference records that no longer exist after deletion.
            if full_refresh {
                let ids: Vec<String> = (1..=desired_count)
                    .map(|number| format!("{table_name}:{number}"))
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

            let existing_ids = existing.remove(table_name).unwrap_or_default();
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
                .map(|number| format!("{table_name}:{number}"))
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
    /// Delete every table's records before a full refresh regenerates them.
    pub async fn clear_records_for_full_refresh(&self) -> Result<(), Box<dyn std::error::Error>> {
        if !self.schemasync_config.mock_gen_config.full_refresh_mode {
            return Ok(());
        }
        let defined_tables = self.defined_tables().await?;
        let statements: String = self
            .tables
            .keys()
            .filter(|table| defined_tables.contains(*table))
            .map(|table| format!("DELETE {table};\n"))
            .collect();
        evenframe_log!(&statements, "remove_statements.surql");
        if !statements.is_empty() {
            execute_and_validate(self.db, &statements, "delete", "full refresh")
                .await
                .map_err(|error| format!("clearing records for a full refresh: {error}"))?;
        }
        Ok(())
    }

    /// Target the `selected` tables (every table when `None`) for mock data,
    /// without diffing against the database. Tables that aren't selected
    /// keep all their records, and only existing ones are link targets. Call
    /// after
    /// [`Self::generate_ids`].
    pub fn select_tables_for_insert(&mut self, selected: Option<&BTreeSet<String>>) {
        let is_selected = |name: &str| selected.is_none_or(|selection| selection.contains(name));

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

    /// Delete the records beyond each table's count: one statement per
    /// table with its ids bound, in as few requests as the size limit allows.
    /// Links are only generated to ids kept in the pool, so no new record
    /// points at an excess one.
    pub async fn remove_excess_records(&self) -> Result<(), Box<dyn std::error::Error>> {
        const DELETE_EXCESS: &str = "DELETE array::map($excess, |$id| <record> $id) RETURN NONE;";
        for (table_name, excess) in self.excess_ids.iter().filter(|(_, ids)| !ids.is_empty()) {
            evenframe_log!(
                format!(
                    "-- {} excess records of {table_name}\n{DELETE_EXCESS}",
                    excess.len()
                ),
                "remove_statements.surql",
                true
            );
            for batch in id_batches(excess) {
                let mut variables = surrealdb::types::Variables::new();
                variables.insert("excess", batch.to_vec());
                execute_bound(self.db, DELETE_EXCESS, &variables, "delete", table_name)
                    .await
                    .map_err(|error| {
                        format!("deleting excess records of `{table_name}`: {error}")
                    })?;
            }
        }
        Ok(())
    }

    /// Plan the mock data each table receives from the schema changes: new
    /// records up to each table's count, and existing records rewritten where
    /// their fields changed. A full refresh writes every table from scratch.
    pub fn filter_changes(&mut self, schema_changes: &SchemaChanges) {
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
            self.plan_table_mocks(schema_changes)
        };

        tracing::info!(tables = self.table_mocks.len(), "Mock data planned");
        evenframe_log!(format!("{:#?}", self.table_mocks), "filtered.log");
    }

    pub(super) async fn generate_mock_data(&self) -> Result<(), Box<dyn std::error::Error>> {
        #[cfg(feature = "wasm-plugins")]
        let started = std::time::Instant::now();
        self.check_required_links()?;
        tracing::trace!("Starting mock data generation");

        // Every table is sorted, so an order that runs through a table without
        // mocks still holds between the tables that have them.
        let sorted_table_names: Vec<String> =
            sort_tables_by_dependencies(self.tables, self.objects, self.enums)
                .into_iter()
                .filter(|name| self.table_mocks.contains_key(name))
                .collect();

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
                    let stmts = self.generate_mock_statements(table_name, mocks).await?;

                    tracing::debug!(
                        table = %table_name,
                        statement_count = stmts.lines().count(),
                        "Generated mock data statements"
                    );

                    evenframe_log!(&stmts, "all_statements.surql", true);

                    match execute_and_validate(self.db, &stmts, "mock data", table_name).await {
                        Ok(_results) => {
                            tracing::debug!(table = %table_name, "Mock data inserted successfully");
                        }
                        Err(error) => {
                            tracing::error!(
                                table = %table_name,
                                error = %error,
                                "Failed to execute statements"
                            );
                            #[cfg(feature = "dev-mode")]
                            {
                                let error_msg = format!(
                                    "Failed to execute upsert statements for table {}: {}",
                                    table_name, error
                                );
                                evenframe_log!(&error_msg, "results.log", true);
                            }
                            return Err(error.into());
                        }
                    }
                }
            }
        }
        // After the new records exist, so every kept id is a record.
        self.repoint_links_to_excess().await?;
        #[cfg(feature = "wasm-plugins")]
        if let Some(plugins) = &self.plugin_manager {
            let usage = plugins.borrow().usage();
            tracing::info!(
                calls = usage.calls,
                plugin_ms = usage.time.as_millis(),
                total_ms = started.elapsed().as_millis(),
                "Mock data plugin time"
            );
        }
        tracing::info!("Mock data generation complete");
        Ok(())
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
                                .filter(|field| !field.is_empty())
                                .cloned()
                                .collect(),
                            CoherentDataset::PersonName {
                                first_name,
                                last_name,
                                full_name,
                            } => [first_name, last_name, full_name]
                                .into_iter()
                                .filter(|field| !field.is_empty())
                                .cloned()
                                .collect(),
                            CoherentDataset::GeoLocation {
                                latitude,
                                longitude,
                                city,
                                country,
                            } => [latitude, longitude, city, country]
                                .into_iter()
                                .filter(|field| !field.is_empty())
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
                                .filter(|field| !field.is_empty())
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
                                .filter(|field| !field.is_empty())
                                .cloned()
                                .collect(),
                            CoherentDataset::PersonName {
                                first_name,
                                last_name,
                                full_name,
                            } => [first_name, last_name, full_name]
                                .into_iter()
                                .filter(|field| !field.is_empty())
                                .cloned()
                                .collect(),
                            CoherentDataset::GeoLocation {
                                latitude,
                                longitude,
                                city,
                                country,
                            } => [latitude, longitude, city, country]
                                .into_iter()
                                .filter(|field| !field.is_empty())
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
                                .filter(|field| !field.is_empty())
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
                                Coordination::InitializeEqual(fields) => fields.clone(),
                                Coordination::InitializeSequential {
                                    field_names: fields,
                                    ..
                                } => fields.clone(),
                                Coordination::InitializeSum {
                                    field_names: fields,
                                    ..
                                } => fields.clone(),
                                Coordination::InitializeDerive {
                                    source_field_names,
                                    target_field_name,
                                    ..
                                } => {
                                    let mut all = source_field_names.clone();
                                    all.push(target_field_name.clone());
                                    all
                                }
                                Coordination::OneToOne(fields) => vec![fields.clone()],
                                Coordination::InitializeCoherent(dataset) => match dataset {
                                    CoherentDataset::Address {
                                        city,
                                        state,
                                        zip,
                                        country,
                                    } => [city, state, zip, country]
                                        .into_iter()
                                        .filter(|field| !field.is_empty())
                                        .cloned()
                                        .collect(),
                                    CoherentDataset::PersonName {
                                        first_name,
                                        last_name,
                                        full_name,
                                    } => [first_name, last_name, full_name]
                                        .into_iter()
                                        .filter(|field| !field.is_empty())
                                        .cloned()
                                        .collect(),
                                    CoherentDataset::GeoLocation {
                                        latitude,
                                        longitude,
                                        city,
                                        country,
                                    } => [latitude, longitude, city, country]
                                        .into_iter()
                                        .filter(|field| !field.is_empty())
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
                                        .filter(|field| !field.is_empty())
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
                        coordination.validate(self, &coordinated_fields).map_err(|error| {
                            crate::error::EvenframeError::validation(format!(
                                "invalid mock data coordination {coordination:?} on {group_tables:?}: {error}"
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

/// `ids` in runs small enough to bind in one request.
#[cfg(feature = "mockmake")]
fn id_batches(ids: &[String]) -> Vec<&[String]> {
    let mut batches = Vec::new();
    let mut start = 0;
    let mut bytes = 0;
    for (index, id) in ids.iter().enumerate() {
        // Each id goes over the wire quoted and comma separated.
        let size = id.len() + 3;
        if bytes + size > RPC_SIZE_LIMIT && index > start {
            batches.push(&ids[start..index]);
            start = index;
            bytes = 0;
        }
        bytes += size;
    }
    if start < ids.len() {
        batches.push(&ids[start..]);
    }
    batches
}

/// A table's `#[mock_data(...)]` settings, as written on the struct.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MockGenerationConfig {
    /// Records to generate; `None` uses the configured `default_record_count`.
    pub record_count: Option<usize>,
    pub coordination_rules: Vec<crate::schemasync::mockmake::coordinate::Coordination>,
    /// Name of the WASM plugin to use for table-level mock generation.
    #[serde(default)]
    pub plugin: Option<String>,
}

impl quote::ToTokens for MockGenerationConfig {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let record_count_tokens = match self.record_count {
            Some(record_count) => quote::quote! { Some(#record_count) },
            None => quote::quote! { None },
        };
        let coordination_rules = &self.coordination_rules;
        let plugin_tokens = match &self.plugin {
            Some(name) => quote::quote! { Some(#name.to_string()) },
            None => quote::quote! { None },
        };

        tokens.extend(quote::quote! {
            MockGenerationConfig {
                record_count: #record_count_tokens,
                coordination_rules: vec![#(#coordination_rules),*],
                plugin: #plugin_tokens,
            }
        });
    }
}

#[cfg(all(test, feature = "mockmake", feature = "scan"))]
mod select_tables_tests {
    use super::{BTreeMap, BTreeSet, Client, Mockmaker, Surreal};
    use crate::scan::{ScanConfig, build_all_configs};
    use crate::schemasync::{TableConfig, config::SchemasyncConfig};
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
        let config = ScanConfig {
            scan_path: tmp.path().to_path_buf(),
            ..ScanConfig::default()
        };
        let (_, tables, _) = build_all_configs(&config).unwrap();
        tables
    }

    /// The records each table selected for insert plans over an empty
    /// database, once its required links are checked.
    fn planned(
        tables: &BTreeMap<String, TableConfig>,
        selected: Option<&[&str]>,
    ) -> crate::error::Result<BTreeMap<String, usize>> {
        let db = Surreal::<Client>::init();
        let config: SchemasyncConfig =
            toml::from_str("should_generate_mocks = true\n[database]\nurl = \"x\"\n").unwrap();
        let objects = BTreeMap::new();
        let enums = BTreeMap::new();
        let registry = crate::types::ForeignTypeRegistry::default();
        let mut mockmaker =
            Mockmaker::new(&db, tables, &objects, &enums, &config, &registry).unwrap();
        // What `generate_ids` leaves over an empty database: generated IDs only.
        let counts: BTreeMap<String, usize> = tables
            .iter()
            .map(|(name, table)| (name.clone(), mockmaker.record_count(table.effective())))
            .collect();
        mockmaker.id_map = counts
            .iter()
            .map(|(name, count)| {
                (
                    name.clone(),
                    (1..=*count)
                        .map(|index| format!("{name}:{index}"))
                        .collect(),
                )
            })
            .collect();
        mockmaker.new_records = counts;
        let selected: Option<BTreeSet<String>> =
            selected.map(|names| names.iter().map(|name| name.to_string()).collect());
        mockmaker.select_tables_for_insert(selected.as_ref());
        mockmaker.check_required_links()?;
        Ok(mockmaker
            .table_mocks
            .iter()
            .map(|(name, mocks)| (name.clone(), mocks.new_records))
            .collect())
    }

    #[test]
    fn tables_generating_no_records_need_nothing_to_link_to() {
        let tables = scanned_tables();
        assert_eq!(
            planned(&tables, Some(&["author", "post"])).unwrap(),
            BTreeMap::from([("author".to_string(), 0), ("post".to_string(), 0)])
        );
    }

    #[test]
    fn a_table_generating_records_needs_its_links_to_have_records() {
        let tables = scanned_tables();
        let error = planned(&tables, None).unwrap_err().to_string();
        assert!(
            error.contains("`comment.post` must link to") && error.contains("which has no records"),
            "{error}"
        );
    }
}

#[cfg(all(test, feature = "mockmake"))]
mod id_batch_tests {
    use super::{RPC_SIZE_LIMIT, id_batches};

    #[test]
    fn ids_split_into_batches_under_the_size_limit() {
        let ids: Vec<String> = (0..200_000)
            .map(|number| format!("record:{number}"))
            .collect();
        let batches = id_batches(&ids);
        assert!(batches.len() > 1);
        for batch in &batches {
            let bytes: usize = batch.iter().map(|id| id.len() + 3).sum();
            assert!(bytes <= RPC_SIZE_LIMIT, "{bytes} bytes");
        }
        assert_eq!(batches.concat(), ids);
    }

    #[test]
    fn few_ids_go_in_one_batch() {
        let ids = vec!["record:1".to_string(), "record:2".to_string()];
        assert_eq!(id_batches(&ids), vec![&ids[..]]);
    }
}
