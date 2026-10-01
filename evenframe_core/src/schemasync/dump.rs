//! Offline SurrealQL dumps of the resolved schema, as written by
//! `evenframe schemasync dump` and by build scripts. Nothing here connects to
//! a database.

use crate::config::EvenframeConfig;
use crate::error::{EvenframeError, Result};
use crate::schemasync::TableConfig;
use crate::schemasync::config::DatabaseConfig;
use crate::schemasync::database::surql::access::access_definitions_surql;
use crate::schemasync::database::surql::define::generate_define_statements;
use crate::types::{ForeignTypeRegistry, StructConfig, TaggedUnion};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// What a dump holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DumpScope {
    /// The `DEFINE TABLE`/`FIELD`/`INDEX`/`EVENT` statements only.
    Tables,
    /// Everything schemasync defines, in the order it applies it.
    Schema,
}

impl DumpScope {
    /// Where a dump of this scope is written unless told otherwise.
    pub fn default_path(self, project_root: &Path) -> PathBuf {
        let file = match self {
            DumpScope::Tables => "tables.surql",
            DumpScope::Schema => "schema.surql",
        };
        project_root.join(".evenframe").join("surql").join(file)
    }
}

/// The SurrealQL of `scope` for the scanned types, with `config`'s foreign
/// types, scripting setting and database definitions.
pub fn dump_surql(
    config: &EvenframeConfig,
    tables: &BTreeMap<String, TableConfig>,
    objects: &BTreeMap<String, StructConfig>,
    enums: &BTreeMap<String, TaggedUnion>,
    scope: DumpScope,
) -> Result<String> {
    let schemasync = config.require_schemasync()?;
    let registry = ForeignTypeRegistry::from_config(&config.general.foreign_types);
    let tables = tables_surql(
        tables,
        objects,
        enums,
        &registry,
        schemasync.mock_gen_config.scripting_asserts,
    )?;
    match scope {
        DumpScope::Tables => Ok(tables),
        DumpScope::Schema => schema_surql(&schemasync.database, &tables),
    }
}

/// Writes `surql` to `path`, creating its directory. An unchanged file is
/// left untouched, so a build script rerunning does not bump its mtime.
pub fn write_dump(path: &Path, surql: &str) -> Result<()> {
    if fs::read(path).is_ok_and(|existing| existing == surql.as_bytes()) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            EvenframeError::config(format!(
                "Failed to create output directory {}: {error}",
                parent.display()
            ))
        })?;
    }
    fs::write(path, surql).map_err(|error| {
        EvenframeError::config(format!(
            "Failed to write schema dump to {}: {error}",
            path.display()
        ))
    })
}

/// Whether any `DEFINE ANALYZER` in `surql` uses a `FUNCTION fn::...`
/// preprocessor, which must exist before the analyzer is defined.
pub fn analyzers_reference_functions(surql: &str) -> bool {
    surql.to_uppercase().contains("FUNCTION FN::")
}

/// The `DEFINE TABLE`/`FIELD`/`INDEX`/`EVENT` statements for every table,
/// resolving each table's `output_override` and passing the full
/// table/object/enum context, as schemasync does.
pub fn tables_surql(
    tables: &BTreeMap<String, TableConfig>,
    objects: &BTreeMap<String, StructConfig>,
    enums: &BTreeMap<String, TaggedUnion>,
    registry: &ForeignTypeRegistry,
    allow_scripting: bool,
) -> Result<String> {
    Ok(tables
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
        .collect::<Result<Vec<_>>>()?
        .join("\n"))
}

/// Everything schemasync defines, in the order it applies it, so the result
/// can be applied top to bottom: accesses, analyzers, tables (`tables_surql`),
/// then functions, whose parameters may reference tables. When an analyzer
/// uses a `FUNCTION fn::...` preprocessor the functions go before the
/// analyzers instead. Each non-empty section starts with a `-- <name>`
/// comment.
pub fn schema_surql(database: &DatabaseConfig, tables_surql: &str) -> Result<String> {
    let resolved = &database.resolved;
    let analyzers = resolved.analyzers_surql.clone().unwrap_or_default();
    let functions = resolved.functions_surql.clone().unwrap_or_default();
    let functions_first = analyzers_reference_functions(&analyzers);

    let mut sections = vec![("Accesses", access_definitions_surql(database)?)];
    if functions_first {
        sections.push(("Functions", functions.clone()));
    }
    sections.push(("Analyzers", analyzers));
    sections.push(("Tables", tables_surql.to_string()));
    if !functions_first {
        sections.push(("Functions", functions));
    }

    Ok(sections
        .into_iter()
        .filter(|(_, body)| !body.trim().is_empty())
        .map(|(title, body)| format!("-- {title}\n{}\n", body.trim()))
        .collect::<Vec<_>>()
        .join("\n"))
}

#[cfg(test)]
mod tests {
    use super::{DatabaseConfig, analyzers_reference_functions, schema_surql};
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
    fn detects_function_preprocessors_in_analyzers() {
        assert!(analyzers_reference_functions(
            "DEFINE ANALYZER a FUNCTION fn::strip TOKENIZERS blank;"
        ));
        assert!(!analyzers_reference_functions(
            "DEFINE ANALYZER a TOKENIZERS blank;"
        ));
    }

    #[test]
    fn sections_follow_apply_order() {
        let dump = schema_surql(
            &database(
                "DEFINE ANALYZER OVERWRITE en TOKENIZERS blank;",
                "DEFINE FUNCTION OVERWRITE fn::greet() { RETURN 'hi' };",
            ),
            "DEFINE TABLE OVERWRITE user SCHEMAFULL;\n",
        )
        .unwrap();
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
        )
        .unwrap();
        assert_eq!(
            section_order(&dump),
            vec!["Accesses", "Functions", "Analyzers", "Tables"]
        );
    }

    #[test]
    fn empty_sections_are_omitted() {
        let mut database = DatabaseConfig::for_testing();
        database.accesses = AccessesSource::Inline(Vec::new());
        let dump = schema_surql(&database, "DEFINE TABLE OVERWRITE user SCHEMAFULL;").unwrap();
        assert_eq!(dump, "-- Tables\nDEFINE TABLE OVERWRITE user SCHEMAFULL;\n");
    }
}
