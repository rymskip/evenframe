use crate::{
    schemasync::compare::{SchemaChanges, TableChanges},
    schemasync::mockmake::{Mockmaker, TableMocks},
    schemasync::{PreservationMode, TableConfig},
    types::StructField,
};
use std::collections::{BTreeMap, BTreeSet};

impl Mockmaker<'_> {
    /// The mock data each table receives after `schema_changes`: new records
    /// up to its count with every field, and its existing records rewritten
    /// where their fields changed. What gets rewritten follows the table's
    /// preservation mode: `None` regenerates every field of a changed table,
    /// `Smart` writes new and modified fields, `Full` only new ones, and both
    /// unset removed fields. `always_regenerate` fields are rewritten in
    /// every mode.
    pub fn plan_table_mocks(&self, schema_changes: &SchemaChanges) -> BTreeMap<String, TableMocks> {
        tracing::debug!("Planning mock data for changed tables");
        let default_preservation_mode = &self
            .schemasync_config
            .mock_gen_config
            .default_preservation_mode;
        let modified_tables: BTreeMap<_, _> = schema_changes
            .modified_tables
            .iter()
            .map(|tc| (tc.table_name.as_str(), tc))
            .collect();

        let mut plans = BTreeMap::new();
        for (table_name, table_config) in self.tables {
            let table_config = table_config.effective();
            let new_records = self.new_records.get(table_name).copied().unwrap_or(0);
            let rewrite_fields = if schema_changes.new_tables.contains(table_name) {
                Vec::new()
            } else {
                let preservation_mode = table_config
                    .mock_generation_config
                    .as_ref()
                    .map_or(default_preservation_mode, |config| {
                        &config.preservation_mode
                    });
                rewritten_fields(
                    table_config,
                    modified_tables.get(table_name.as_str()).copied(),
                    preservation_mode,
                )
            };

            if new_records > 0 || !rewrite_fields.is_empty() {
                plans.insert(
                    table_name.clone(),
                    TableMocks {
                        new_records,
                        rewrite_fields,
                    },
                );
            }
        }
        plans
    }
}

/// The fields rewritten on a table's existing records. A change inside a
/// field (`field[*]`) rewrites the whole field.
fn rewritten_fields(
    table_config: &TableConfig,
    table_change: Option<&TableChanges>,
    preservation_mode: &PreservationMode,
) -> Vec<StructField> {
    let fields = &table_config.struct_config.fields;
    let Some(change) = table_change else {
        return fields
            .iter()
            .filter(|f| f.always_regenerate)
            .cloned()
            .collect();
    };
    if matches!(preservation_mode, PreservationMode::None) {
        return fields.clone();
    }

    let modified = change
        .modified_fields
        .iter()
        .map(|c| c.field_name.as_str())
        .filter(|_| matches!(preservation_mode, PreservationMode::Smart));
    let changed: BTreeSet<&str> = change
        .new_fields
        .iter()
        .chain(&change.removed_fields)
        .map(String::as_str)
        .chain(modified)
        .map(|path| path.split(['.', '[']).next().unwrap_or(path))
        .collect();

    let mut rewritten: BTreeMap<&str, StructField> = fields
        .iter()
        .filter(|f| f.always_regenerate || changed.contains(f.field_name.as_str()))
        .map(|f| (f.field_name.as_str(), f.clone()))
        .collect();
    // A changed name the model no longer has was removed.
    for name in changed {
        rewritten
            .entry(name)
            .or_insert_with(|| StructField::unit(name.to_string()));
    }
    rewritten.into_values().collect()
}
