use crate::{compare::SchemaChanges, mockmake::Mockmaker};
use convert_case::{Case, Casing};
use tracing::{debug, info};

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
