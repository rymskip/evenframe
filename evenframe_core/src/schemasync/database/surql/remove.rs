use crate::schemasync::{compare::SchemaChanges, mockmake::Mockmaker};
use convert_case::{Case, Casing};
use tracing::{debug, info};

/// Generate `REMOVE INDEX` statements for indexes that exist in the database
/// but are no longer declared in Rust (orphans). Kept as a free function so it
/// can be unit-tested without standing up a `Mockmaker` (which owns a live
/// `Surreal<Client>`).
pub fn generate_remove_index_statements(schema_changes: &SchemaChanges) -> String {
    let mut output = String::new();
    for table_change in &schema_changes.modified_tables {
        if table_change.removed_indexes.is_empty() {
            continue;
        }
        let table_name = table_change.table_name.to_case(Case::Snake);
        output.push_str(&format!("-- Removing indexes from table {}\n", table_name));
        for index in &table_change.removed_indexes {
            output.push_str(&format!(
                "REMOVE INDEX IF EXISTS {} ON TABLE {};\n",
                index.name, table_name
            ));
        }
        output.push('\n');
    }
    output
}

impl Mockmaker<'_> {
    /// Generate REMOVE statements based on schema changes and record differences
    ///
    /// This function takes a SchemaChanges instance and record differences, and generates
    /// REMOVE statements for tables and fields that have been removed from the schema,
    /// as well as DELETE statements for excess records.
    ///
    /// # Arguments
    /// * `schema_changes` - The schema changes detected between old and new schemas
    /// * `record_diffs` - Map of table names to record count differences
    /// * `id_map` - Map of table names to their existing IDs
    ///
    /// # Returns
    /// A string containing all REMOVE and DELETE statements to be executed
    pub fn generate_remove_statements(&self, schema_changes: &SchemaChanges) -> String {
        info!("Generating remove statements based on schema changes");
        debug!(
            "Schema changes before remove statement gen: {:?}",
            schema_changes
        );
        let mut output = String::new();

        // Process removed accesses first
        if !schema_changes.modified_accesses.is_empty()
            || !schema_changes.removed_accesses.is_empty()
        {
            let mut has_accesses_to_remove = false;

            // Always remove fully removed accesses
            for access_name in &schema_changes.removed_accesses {
                output.push_str(&format!(
                    "REMOVE ACCESS IF EXISTS {} ON DATABASE;\n",
                    access_name
                ));
                has_accesses_to_remove = true;
            }

            // For modified accesses, check if changes are only ignorable (JWT/Issuer key changes)
            for access_change in &schema_changes.modified_accesses {
                // Check if all changes are ignorable (using the enum's is_ignorable method)
                let only_ignorable_changes = access_change
                    .changes
                    .iter()
                    .all(|change| change.is_ignorable());

                // Only remove and recreate if there are changes that aren't ignorable
                if !only_ignorable_changes {
                    output.push_str(&format!(
                        "REMOVE ACCESS IF EXISTS {} ON DATABASE;\n",
                        access_change.access_name
                    ));
                    has_accesses_to_remove = true;
                }
            }

            if has_accesses_to_remove {
                output.push('\n');
            }
        }

        // Process excess records (negative diffs mean we have too many records)
        let mut has_excess_records = false;
        for diff in self.record_diffs.values() {
            if *diff < 0 {
                has_excess_records = true;
                break;
            }
        }

        if has_excess_records {
            output.push_str("-- Removing excess records\n");
            for (table_name, diff) in &self.record_diffs {
                if *diff < 0 {
                    let table_name_snake = table_name.to_case(Case::Snake);
                    let excess_count = diff.unsigned_abs() as usize;

                    // Get the IDs for this table
                    if let Some(table_ids) = self.id_map.get(table_name) {
                        // Delete the last N records (where N = excess_count)
                        // We delete from the end to maintain existing references
                        let ids_to_delete = table_ids
                            .iter()
                            .rev()
                            .take(excess_count)
                            .collect::<Vec<_>>();

                        for id in ids_to_delete {
                            output.push_str(&format!("DELETE {};\n", id));
                        }
                    }
                    output.push_str(&format!(
                        "-- Removed {} excess records from table {}\n",
                        excess_count, table_name_snake
                    ));
                }
            }
            output.push('\n');
        }

        // Process removed indexes before removed fields: SurrealDB rejects
        // `REMOVE FIELD` on a column that still has a live index referencing
        // it, so orphan indexes must be dropped first.
        output.push_str(&generate_remove_index_statements(schema_changes));

        // Process removed fields first (before removing tables)
        for table_change in &schema_changes.modified_tables {
            if !table_change.removed_fields.is_empty() {
                let table_name = table_change.table_name.to_case(Case::Snake);
                output.push_str(&format!("-- Removing fields from table {}\n", table_name));

                for field_name in &table_change.removed_fields {
                    output.push_str(&format!(
                        "REMOVE FIELD IF EXISTS {} ON TABLE {};\n",
                        field_name, table_name
                    ));
                }
                output.push('\n');
            }
        }

        // Process removed tables
        if !schema_changes.removed_tables.is_empty() {
            output.push_str("-- Removing tables\n");
            for table_name in &schema_changes.removed_tables {
                let table_name_snake = table_name.to_case(Case::Snake);
                output.push_str(&format!("REMOVE TABLE IF EXISTS {};\n", table_name_snake));
            }
            output.push('\n');
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schemasync::compare::{IndexDefinition, SchemaChanges, TableChanges};

    fn empty_table_change(name: &str) -> TableChanges {
        TableChanges {
            table_name: name.to_string(),
            new_fields: Vec::new(),
            removed_fields: Vec::new(),
            modified_fields: Vec::new(),
            permission_changed: false,
            schema_type_changed: false,
            new_events: Vec::new(),
            removed_events: Vec::new(),
            new_indexes: Vec::new(),
            removed_indexes: Vec::new(),
        }
    }

    #[test]
    fn emits_remove_index_for_orphan() {
        let mut tc = empty_table_change("Reaction");
        tc.removed_indexes.push(IndexDefinition {
            name: "idx_reaction_created_at".to_string(),
            columns: vec!["created_at".to_string()],
            unique: false,
        });

        let changes = SchemaChanges {
            new_tables: Vec::new(),
            removed_tables: Vec::new(),
            modified_tables: vec![tc],
            new_accesses: Vec::new(),
            removed_accesses: Vec::new(),
            modified_accesses: Vec::new(),
        };

        let out = generate_remove_index_statements(&changes);
        assert!(
            out.contains("REMOVE INDEX IF EXISTS idx_reaction_created_at ON TABLE reaction;"),
            "missing REMOVE INDEX line; got:\n{out}"
        );
        assert!(out.contains("-- Removing indexes from table reaction"));
    }

    #[test]
    fn emits_nothing_when_no_orphans() {
        let tc = empty_table_change("Reaction");
        let changes = SchemaChanges {
            new_tables: Vec::new(),
            removed_tables: Vec::new(),
            modified_tables: vec![tc],
            new_accesses: Vec::new(),
            removed_accesses: Vec::new(),
            modified_accesses: Vec::new(),
        };
        assert!(generate_remove_index_statements(&changes).is_empty());
    }
}
