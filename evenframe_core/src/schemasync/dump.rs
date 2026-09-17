//! Offline SurrealQL dumps of the resolved schema, as written by
//! `evenframe schemasync dump`. Nothing here connects to a database.

use crate::schemasync::TableConfig;
use crate::schemasync::compare::surql::analyzers_reference_functions;
use crate::schemasync::config::DatabaseConfig;
use crate::schemasync::database::surql::access::access_definitions_surql;
use crate::schemasync::database::surql::define::generate_define_statements;
use crate::types::{ForeignTypeRegistry, StructConfig, TaggedUnion};
use std::collections::BTreeMap;

/// The `DEFINE TABLE`/`FIELD`/`INDEX`/`EVENT` statements for every table,
/// resolving each table's `output_override` and passing the full
/// table/object/enum context, as schemasync does.
pub fn tables_surql(
    tables: &BTreeMap<String, TableConfig>,
    objects: &BTreeMap<String, StructConfig>,
    enums: &BTreeMap<String, TaggedUnion>,
    registry: &ForeignTypeRegistry,
    allow_scripting: bool,
) -> String {
    tables
        .iter()
        .map(|(table_name, table)| {
            generate_define_statements(
                table_name,
                table.effective(),
                tables,
                objects,
                enums,
                registry,
                allow_scripting,
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Everything schemasync defines, in the order it applies it, so the result
/// can be applied top to bottom: accesses, analyzers, tables (`tables_surql`),
/// then functions, whose parameters may reference tables. When an analyzer
/// uses a `FUNCTION fn::...` preprocessor the functions go before the
/// analyzers instead. Each non-empty section starts with a `-- <name>`
/// comment.
pub fn schema_surql(database: &DatabaseConfig, tables_surql: &str) -> String {
    let resolved = &database.resolved;
    let analyzers = resolved.analyzers_surql.clone().unwrap_or_default();
    let functions = resolved.functions_surql.clone().unwrap_or_default();
    let functions_first = analyzers_reference_functions(&analyzers);

    let mut sections = vec![("Accesses", access_definitions_surql(database))];
    if functions_first {
        sections.push(("Functions", functions.clone()));
    }
    sections.push(("Analyzers", analyzers));
    sections.push(("Tables", tables_surql.to_string()));
    if !functions_first {
        sections.push(("Functions", functions));
    }

    sections
        .into_iter()
        .filter(|(_, body)| !body.trim().is_empty())
        .map(|(title, body)| format!("-- {title}\n{}\n", body.trim()))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schemasync::config::{AccessConfig, AccessType, AccessesSource};

    fn database(analyzers: &str, functions: &str) -> DatabaseConfig {
        let mut database = DatabaseConfig::for_testing();
        database.accesses = AccessesSource::Inline(vec![AccessConfig {
            name: "user".to_string(),
            access_type: AccessType::Bearer,
            table_name: "user".to_string(),
        }]);
        database.resolved.analyzers_surql = Some(analyzers.to_string());
        database.resolved.functions_surql = Some(functions.to_string());
        database
    }

    fn section_order(dump: &str) -> Vec<&str> {
        dump.lines()
            .filter_map(|line| line.strip_prefix("-- "))
            .collect()
    }

    #[test]
    fn sections_follow_apply_order() {
        let dump = schema_surql(
            &database(
                "DEFINE ANALYZER OVERWRITE en TOKENIZERS blank;",
                "DEFINE FUNCTION OVERWRITE fn::greet() { RETURN 'hi' };",
            ),
            "DEFINE TABLE OVERWRITE user SCHEMAFULL;\n",
        );
        assert_eq!(
            section_order(&dump),
            vec!["Accesses", "Analyzers", "Tables", "Functions"]
        );
        assert!(dump.contains("DEFINE ACCESS OVERWRITE user ON DATABASE TYPE BEARER FOR RECORD;"));
        assert!(dump.ends_with("RETURN 'hi' };\n"), "{dump}");
    }

    #[test]
    fn functions_used_by_analyzers_come_first() {
        let dump = schema_surql(
            &database(
                "DEFINE ANALYZER OVERWRITE en FUNCTION fn::strip TOKENIZERS blank;",
                "DEFINE FUNCTION OVERWRITE fn::strip($s: string) { RETURN $s };",
            ),
            "DEFINE TABLE OVERWRITE user SCHEMAFULL;",
        );
        assert_eq!(
            section_order(&dump),
            vec!["Accesses", "Functions", "Analyzers", "Tables"]
        );
    }

    #[test]
    fn empty_sections_are_omitted() {
        let mut database = DatabaseConfig::for_testing();
        database.accesses = AccessesSource::Inline(Vec::new());
        let dump = schema_surql(&database, "DEFINE TABLE OVERWRITE user SCHEMAFULL;");
        assert_eq!(dump, "-- Tables\nDEFINE TABLE OVERWRITE user SCHEMAFULL;\n");
    }
}
