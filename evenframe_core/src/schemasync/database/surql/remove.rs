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

/// Generate `REMOVE ANALYZER` statements for analyzers that exist in the
/// database but are no longer declared in the analyzers surql (orphans).
pub fn generate_remove_analyzer_statements(schema_changes: &SchemaChanges) -> String {
    let mut output = String::new();
    if schema_changes.removed_analyzers.is_empty() {
        return output;
    }
    output.push_str("-- Removing analyzers\n");
    for analyzer_name in &schema_changes.removed_analyzers {
        output.push_str(&format!("REMOVE ANALYZER IF EXISTS {};\n", analyzer_name));
    }
    output.push('\n');
    output
}

/// Extract the event name from a `DEFINE EVENT <name> ON TABLE ...` statement.
/// Returns `None` for statements that do not begin with `DEFINE EVENT`.
fn extract_event_name(statement: &str) -> Option<String> {
    let rest = statement
        .trim_start()
        .strip_prefix("DEFINE EVENT")?
        .trim_start();
    let rest = rest
        .strip_prefix("OVERWRITE")
        .or_else(|| rest.strip_prefix("IF NOT EXISTS"))
        .map(str::trim_start)
        .unwrap_or(rest);
    let name = rest.split_whitespace().next()?.trim_matches('`');
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

/// Generate `REMOVE EVENT` statements for events that exist in the database
/// but are no longer declared in Rust. Without this, deleting a `#[event(...)]`
/// attribute leaves the event live in SurrealDB forever.
pub fn generate_remove_event_statements(schema_changes: &SchemaChanges) -> String {
    let mut output = String::new();
    for table_change in &schema_changes.modified_tables {
        if table_change.removed_events.is_empty() {
            continue;
        }
        let table_name = table_change.table_name.to_case(Case::Snake);
        output.push_str(&format!("-- Removing events from table {}\n", table_name));
        for statement in &table_change.removed_events {
            let Some(name) = extract_event_name(statement) else {
                tracing::warn!(
                    table = %table_name,
                    statement = %statement,
                    "Could not extract event name from removed DEFINE EVENT statement; skipping REMOVE"
                );
                continue;
            };
            output.push_str(&format!(
                "REMOVE EVENT IF EXISTS {} ON TABLE {};\n",
                name, table_name
            ));
        }
        output.push('\n');
    }
    output
}

impl Mockmaker<'_> {
    /// DELETE statements for the records beyond each table's count. Links
    /// are only generated to ids kept in the pool, so no new record points at
    /// an excess one.
    pub(crate) fn excess_record_deletes(&self) -> String {
        let mut output = String::new();
        for (table_name, excess) in self.excess_ids.iter().filter(|(_, ids)| !ids.is_empty()) {
            output.push_str(&format!("-- Removing excess records from {table_name}\n"));
            for id in excess {
                output.push_str(&format!("DELETE {id};\n"));
            }
            output.push('\n');
        }
        output
    }

    /// REMOVE statements for what `schema_changes` removed from the schema,
    /// and DELETE statements for the records beyond each table's count.
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

        output.push_str(&self.excess_record_deletes());

        // Process removed indexes before removed fields: SurrealDB rejects
        // `REMOVE FIELD` on a column that still has a live index referencing
        // it, so orphan indexes must be dropped first. Events can reference
        // fields via $before/$after/$value, so drop orphan events here too.
        output.push_str(&generate_remove_index_statements(schema_changes));
        output.push_str(&generate_remove_event_statements(schema_changes));

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

        // Process removed analyzers last: SurrealDB rejects `REMOVE ANALYZER`
        // while any index (including those on tables removed above) still
        // references it.
        output.push_str(&generate_remove_analyzer_statements(schema_changes));

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
            modified_indexes: Vec::new(),
        }
    }

    #[test]
    fn emits_remove_index_for_orphan() {
        let mut tc = empty_table_change("Reaction");
        tc.removed_indexes.push(IndexDefinition {
            name: "idx_reaction_created_at".to_string(),
            columns: vec!["created_at".to_string()],
            unique: false,
            definition: String::new(),
        });

        let changes = SchemaChanges {
            new_tables: Vec::new(),
            removed_tables: Vec::new(),
            modified_tables: vec![tc],
            new_accesses: Vec::new(),
            removed_accesses: Vec::new(),
            modified_accesses: Vec::new(),
            new_analyzers: Vec::new(),
            removed_analyzers: Vec::new(),
            modified_analyzers: Vec::new(),
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
            new_analyzers: Vec::new(),
            removed_analyzers: Vec::new(),
            modified_analyzers: Vec::new(),
        };
        assert!(generate_remove_index_statements(&changes).is_empty());
    }

    #[test]
    fn extracts_event_name_with_and_without_modifiers() {
        assert_eq!(
            extract_event_name("DEFINE EVENT foo ON TABLE bar WHEN true THEN ();"),
            Some("foo".to_string())
        );
        assert_eq!(
            extract_event_name("DEFINE EVENT OVERWRITE foo ON TABLE bar WHEN true THEN ();"),
            Some("foo".to_string())
        );
        assert_eq!(
            extract_event_name("DEFINE EVENT IF NOT EXISTS foo ON TABLE bar WHEN true THEN ();"),
            Some("foo".to_string())
        );
        assert_eq!(
            extract_event_name("DEFINE EVENT `foo` ON TABLE bar WHEN true THEN ();"),
            Some("foo".to_string())
        );
        assert_eq!(extract_event_name("REMOVE EVENT foo ON TABLE bar;"), None);
    }

    #[test]
    fn emits_remove_event_for_orphan() {
        let mut tc = empty_table_change("Attachment");
        tc.removed_events.push(
            "DEFINE EVENT sync_attachment_created ON TABLE attachment WHEN $event = \"CREATE\" THEN ();"
                .to_string(),
        );

        let changes = SchemaChanges {
            new_tables: Vec::new(),
            removed_tables: Vec::new(),
            modified_tables: vec![tc],
            new_accesses: Vec::new(),
            removed_accesses: Vec::new(),
            modified_accesses: Vec::new(),
            new_analyzers: Vec::new(),
            removed_analyzers: Vec::new(),
            modified_analyzers: Vec::new(),
        };

        let out = generate_remove_event_statements(&changes);
        assert!(
            out.contains("REMOVE EVENT IF EXISTS sync_attachment_created ON TABLE attachment;"),
            "missing REMOVE EVENT line; got:\n{out}"
        );
        assert!(out.contains("-- Removing events from table attachment"));
    }

    #[test]
    fn emits_nothing_when_no_orphan_events() {
        let tc = empty_table_change("Attachment");
        let changes = SchemaChanges {
            new_tables: Vec::new(),
            removed_tables: Vec::new(),
            modified_tables: vec![tc],
            new_accesses: Vec::new(),
            removed_accesses: Vec::new(),
            modified_accesses: Vec::new(),
            new_analyzers: Vec::new(),
            removed_analyzers: Vec::new(),
            modified_analyzers: Vec::new(),
        };
        assert!(generate_remove_event_statements(&changes).is_empty());
    }

    #[test]
    fn emits_remove_analyzer_for_orphan() {
        let changes = SchemaChanges {
            new_tables: Vec::new(),
            removed_tables: Vec::new(),
            modified_tables: Vec::new(),
            new_accesses: Vec::new(),
            removed_accesses: Vec::new(),
            modified_accesses: Vec::new(),
            new_analyzers: vec!["kept".to_string()],
            removed_analyzers: vec!["english".to_string()],
            modified_analyzers: vec!["changed".to_string()],
        };
        let out = generate_remove_analyzer_statements(&changes);
        assert_eq!(
            out,
            "-- Removing analyzers\nREMOVE ANALYZER IF EXISTS english;\n\n"
        );
    }
}
