//! Offline SurrealQL dumps of the resolved schema, as written by
//! `evenframe schemasync dump` and by build scripts. Nothing here connects to
//! a database.

use crate::config::EvenframeConfig;
use crate::error::{EvenframeError, Result};
use crate::schemasync::config::DatabaseConfig;
use crate::schemasync::database::surql::access::access_definitions_surql;
use crate::schemasync::database::surql::define::generate_define_statements;
use crate::schemasync::database::surql::shape::DefineContext;
use crate::types::{ForeignTypeRegistry, SchemasyncTypes};
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
/// types and database definitions.
pub fn dump_surql(
    config: &EvenframeConfig,
    types: &SchemasyncTypes,
    scope: DumpScope,
) -> Result<String> {
    let schemasync = config.require_schemasync()?;
    let registry = ForeignTypeRegistry::from_config(&config.general.foreign_types);
    let tables = tables_surql(types, &registry, schemasync.surql_options())?;
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

/// The `DEFINE TABLE`/`FIELD`/`INDEX`/`EVENT` statements for every table,
/// resolving each table's `output_override` and passing the full
/// table/object/enum context and the declared types, as schemasync does.
pub fn tables_surql(
    types: &SchemasyncTypes,
    registry: &ForeignTypeRegistry,
    options: crate::schemasync::config::SurqlOptions,
) -> Result<String> {
    let context = DefineContext {
        tables: &types.tables,
        objects: &types.objects,
        enums: &types.enums,
        declared: &types.declared,
        registry,
        options,
    };
    Ok(types
        .tables
        .iter()
        .map(|(table_name, table)| {
            generate_define_statements(table_name, table.effective(), &context)
        })
        .collect::<Result<Vec<_>>>()?
        .join("\n"))
}

/// Everything schemasync defines, in the order it applies it, so the result
/// can be applied top to bottom: accesses, functions (which analyzers and
/// PERMISSIONS clauses call), analyzers, then tables (`tables_surql`). Each
/// non-empty section starts with a `-- <name>` comment.
pub fn schema_surql(database: &DatabaseConfig, tables_surql: &str) -> Result<String> {
    let resolved = &database.resolved;
    let sections = [
        ("Accesses", access_definitions_surql(database)?),
        (
            "Functions",
            resolved.functions_surql.clone().unwrap_or_default(),
        ),
        (
            "Analyzers",
            resolved.analyzers_surql.clone().unwrap_or_default(),
        ),
        ("Tables", tables_surql.to_string()),
    ];

    Ok(sections
        .into_iter()
        .filter(|(_, body)| !body.trim().is_empty())
        .map(|(title, body)| format!("-- {title}\n{}\n", body.trim()))
        .collect::<Vec<_>>()
        .join("\n"))
}

#[cfg(test)]
mod tests {
    use super::{DatabaseConfig, schema_surql};
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
        )
        .unwrap();
        assert_eq!(
            section_order(&dump),
            vec!["Accesses", "Functions", "Analyzers", "Tables"]
        );
        assert!(dump.contains("DEFINE ACCESS OVERWRITE user ON DATABASE TYPE BEARER FOR RECORD;"));
        assert!(
            dump.ends_with("DEFINE TABLE OVERWRITE user SCHEMAFULL;\n"),
            "{dump}"
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
