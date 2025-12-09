use crate::{
    compare::SchemaChanges,
    mockmake::Mockmaker,
    schemasync::{
        TableConfig,
        compare::{PreservationMode, collect_referenced_objects},
    },
    types::{FieldType, StructConfig, StructField, TaggedUnion},
};
use std::collections::{HashMap, HashSet};
use tracing;

impl Mockmaker<'_> {
    /// Filter tables and objects to only include those with changes that need to be regenerated.
    ///
    /// This function takes a SchemaChanges instance and the full tables and objects hashmaps,
    /// then returns new hashmaps containing only the tables and fields that have been changed
    /// and need to be regenerated. This version is refactored to handle all conditions for
    /// regeneration in a single pass to avoid overwriting configurations.
    ///
    /// # Arguments
    /// * `schema_changes` - The schema changes detected between old and new schemas.
    /// * `tables` - The full HashMap of table configurations.
    /// * `objects` - The full HashMap of object/struct configurations.
    /// * `enums` - The full HashMap of enum configurations.
    /// * `record_diffs` - Map of table names to record count differences.
    ///
    /// # Returns
    /// A tuple of (filtered_tables, filtered_objects) containing only changed elements.
    pub fn filter_changed_tables_and_objects(
        &self,
        schema_changes: &SchemaChanges,
        tables: &HashMap<String, TableConfig>,
        objects: &HashMap<String, StructConfig>,
        enums: &HashMap<String, TaggedUnion>,
        record_diffs: &HashMap<String, i32>,
    ) -> (HashMap<String, TableConfig>, HashMap<String, StructConfig>) {
        tracing::debug!("Filtering changed tables and objects");

        let mut filtered_tables = HashMap::new();
        let mut filtered_objects = HashMap::new();
        let default_preservation_mode = &self
            .schemasync_config
            .mock_gen_config
            .default_preservation_mode;

        // Pre-index modified tables for efficient O(1) lookups.
        let modified_tables_map: HashMap<_, _> = schema_changes
            .modified_tables
            .iter()
            .map(|tc| (tc.table_name.as_str(), tc))
            .collect();

        // 1. Add all new tables for full regeneration.
        for table_name in &schema_changes.new_tables {
            if let Some(table_config) = tables.get(table_name) {
                tracing::trace!(table = %table_name, "Adding new table for full regeneration");
                filtered_tables.insert(table_name.clone(), table_config.clone());
            }
        }

        // 2. Pre-populate filtered_tables with tables that need more records.
        for (table_name, diff) in record_diffs {
            if *diff > 0
                && let Some(table_config) = tables.get(table_name)
                && !filtered_tables.contains_key(table_name)
            {
                tracing::trace!(
                    table = %table_name,
                    record_diff = diff,
                    "Table needs additional records, setting 'n'"
                );
                let mut modified_config = table_config.clone();
                if let Some(ref mut mock_config) = modified_config.mock_generation_config {
                    mock_config.n = *diff as usize;
                }
                filtered_tables.insert(table_name.clone(), modified_config);
            }
        }

        // 3. Process all tables for schema changes.
        for (table_name, table_config) in tables {
            if schema_changes.new_tables.contains(table_name) {
                continue;
            }

            let preservation_mode = table_config
                .mock_generation_config
                .as_ref()
                .map_or(default_preservation_mode, |config| {
                    &config.preservation_mode
                });

            let table_change = modified_tables_map.get(table_name.as_str()).copied();

            let has_always_regenerate_fields = table_config
                .struct_config
                .fields
                .iter()
                .any(|f| f.always_regenerate);

            match preservation_mode {
                PreservationMode::None => {
                    // For None mode, only include tables that have changes or always_regenerate fields
                    if table_change.is_some() || has_always_regenerate_fields {
                        filtered_tables
                            .entry(table_name.clone())
                            .or_insert_with(|| table_config.clone());
                    }
                }
                PreservationMode::Full | PreservationMode::Smart => {
                    if table_change.is_none() && !has_always_regenerate_fields {
                        continue;
                    }

                    let mut field_map: HashMap<String, StructField> = HashMap::new();
                    let mut parents_with_partial = HashSet::new();
                    let mut fields_to_include = HashSet::new();

                    if let Some(change) = table_change {
                        let removed_fields: HashSet<_> =
                            change.removed_fields.iter().cloned().collect();

                        fields_to_include.extend(change.new_fields.iter().cloned());

                        if matches!(preservation_mode, PreservationMode::Smart) {
                            // Smart mode also includes non-nested modified fields.
                            for mf in &change.modified_fields {
                                if !mf.field_name.contains('.') {
                                    fields_to_include.insert(mf.field_name.clone());
                                }
                            }
                        } else {
                            // Full mode includes all modified fields.
                            fields_to_include.extend(
                                change.modified_fields.iter().map(|f| f.field_name.clone()),
                            );
                        }

                        // Handle nested changes by creating partial parent objects.
                        for field_change in &change.modified_fields {
                            if field_change.field_name.contains('.') {
                                let parent_field_name = field_change
                                    .field_name
                                    .split('.')
                                    .next()
                                    .unwrap_or("")
                                    .to_string();

                                if let Some(nested_field_def) = find_nested_field_def(
                                    &table_config.struct_config,
                                    &field_change.field_name,
                                    objects,
                                ) {
                                    parents_with_partial.insert(parent_field_name.clone());
                                    let partial_parent =
                                        field_map.entry(parent_field_name.clone()).or_insert_with(
                                            || StructField::partial(&parent_field_name),
                                        );

                                    // Add the nested field to the partial parent's struct definition.
                                    if let FieldType::Struct(ref mut fields) =
                                        partial_parent.field_type
                                    {
                                        // De-dupe before pushing
                                        if !fields
                                            .iter()
                                            .any(|(n, _)| n == &nested_field_def.field_name)
                                        {
                                            fields.push((
                                                nested_field_def.field_name.clone(),
                                                nested_field_def.field_type.clone(),
                                            ));
                                        }
                                    }
                                }
                            }
                        }

                        // Add fields marked for direct inclusion or `always_regenerate`.
                        for field in &table_config.struct_config.fields {
                            // Skip if it's a removed field or a parent being handled partially.
                            if removed_fields.contains(&field.field_name)
                                || parents_with_partial.contains(&field.field_name)
                            {
                                continue;
                            }
                            if fields_to_include.contains(&field.field_name)
                                || field.always_regenerate
                            {
                                field_map.insert(field.field_name.clone(), field.clone());
                            }
                        }

                        // Add removed fields as Unit type, giving them final precedence.
                        for removed_field_name in &change.removed_fields {
                            field_map.insert(
                                removed_field_name.clone(),
                                StructField::unit(removed_field_name.clone()),
                            );
                        }
                    } else {
                        // Handle cases with no schema changes but with `always_regenerate` fields.
                        for field in &table_config.struct_config.fields {
                            if field.always_regenerate {
                                field_map.insert(field.field_name.clone(), field.clone());
                            }
                        }
                    }

                    let has_other_changes = table_change.map_or_else(
                        || false,
                        |c| {
                            c.permission_changed
                                || c.schema_type_changed
                                || !c.removed_fields.is_empty()
                                || !c.new_events.is_empty()
                                || !c.removed_events.is_empty()
                        },
                    );

                    if !field_map.is_empty() || has_other_changes {
                        let entry = filtered_tables
                            .entry(table_name.clone())
                            .or_insert_with(|| table_config.clone());
                        entry.struct_config.fields = field_map.into_values().collect();
                    }
                }
            }
        }

        // 4. Collect all referenced objects from the final set of filtered tables.
        let mut processed_objects = HashSet::new();
        let mut objects_to_process = Vec::new();

        for table in filtered_tables.values() {
            for field in &table.struct_config.fields {
                collect_referenced_objects(&field.field_type, &mut objects_to_process, enums);
            }
        }

        while let Some(object_name) = objects_to_process.pop() {
            if processed_objects.contains(&object_name) {
                continue;
            }
            processed_objects.insert(object_name.clone());

            if let Some(object_config) = objects.get(&object_name) {
                filtered_objects.insert(object_name.clone(), object_config.clone());
                for field in &object_config.fields {
                    collect_referenced_objects(&field.field_type, &mut objects_to_process, enums);
                }
            }
        }

        tracing::debug!(
            filtered_table_count = filtered_tables.len(),
            filtered_object_count = filtered_objects.len(),
            "Filtering complete"
        );

        (filtered_tables, filtered_objects)
    }
}

/// Helper to find a nested field definition by traversing a path like "a.b.c".
fn find_nested_field_def(
    parent_struct: &StructConfig,
    path: &str,
    objects: &HashMap<String, StructConfig>,
) -> Option<StructField> {
    let mut segments = path.split('.');
    let root_field_name = segments.next()?;
    let root_field = parent_struct
        .fields
        .iter()
        .find(|f| f.field_name == root_field_name)?;

    // If there are no more segments, the root field is what we were looking for.
    if segments.clone().next().is_none() {
        return Some(root_field.clone());
    }

    resolve_path(&root_field.field_type, segments, objects)
}

/// Recursive helper to resolve a path of segments against a field type.
fn resolve_path(
    field_type: &FieldType,
    mut segments: std::str::Split<'_, char>,
    objects: &HashMap<String, StructConfig>,
) -> Option<StructField> {
    // First, get the current segment we are trying to resolve.
    let segment = segments.next()?;

    // Unwrap container types to get to the traversable type.
    let mut current_type = field_type;
    loop {
        match current_type {
            FieldType::Option(inner)
            | FieldType::Vec(inner)
            | FieldType::OrderedFloat(inner)
            | FieldType::RecordLink(inner) => {
                current_type = inner;
            }
            // For maps, we assume traversal into the value type.
            FieldType::HashMap(_, value_type) | FieldType::BTreeMap(_, value_type) => {
                current_type = value_type;
            }
            _ => break,
        }
    }

    // Now, match on the actual, unwrapped type.
    let found_field = match current_type {
        FieldType::Other(struct_name) => {
            let struct_def = objects.get(struct_name)?;
            struct_def
                .fields
                .iter()
                .find(|f| f.field_name == segment)?
                .clone()
        }
        FieldType::Struct(fields) => {
            let (field_name, field_type) = fields.iter().find(|(name, _)| name == segment)?;
            // Create a temporary StructField for recursion.
            StructField {
                field_name: field_name.clone(),
                field_type: field_type.clone(),
                ..StructField::unit("".to_string()) // Use unit as a placeholder for non-relevant fields
            }
        }
        // These types cannot be traversed by name.
        _ => return None,
    };

    // If there are more segments, recurse. Otherwise, we're done.
    if segments.clone().next().is_none() {
        Some(found_field)
    } else {
        resolve_path(&found_field.field_type, segments, objects)
    }
}
