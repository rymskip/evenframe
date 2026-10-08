//! The types of every project in a workspace, as one set for typesync.
//!
//! A type several projects find (one project includes another's file) is
//! kept once. A project that only resolves it (`resolve_only`) defers to one
//! that emits it, and two projects that both emit it must agree on it.

use crate::config::ForeignTypeConfig;
use crate::error::{EvenframeError, Result};
use crate::schemasync::TableConfig;
use crate::types::{AllConfigs, NewtypeConfig, StructConfig, TaggedUnion};
use std::collections::BTreeMap;

/// A scanned type that a project may emit or only resolve.
trait Emittable {
    fn emitted(&self) -> bool;
    /// Whether typesync writes `self` and `other` alike.
    fn same_types(&self, other: &Self) -> bool;
}

impl Emittable for StructConfig {
    fn emitted(&self) -> bool {
        !self.resolve_only
    }
    fn same_types(&self, other: &Self) -> bool {
        self == other
    }
}

impl Emittable for TaggedUnion {
    fn emitted(&self) -> bool {
        !self.resolve_only
    }
    fn same_types(&self, other: &Self) -> bool {
        self == other
    }
}

impl Emittable for NewtypeConfig {
    fn emitted(&self) -> bool {
        !self.resolve_only
    }
    fn same_types(&self, other: &Self) -> bool {
        self == other
    }
}

/// A table two projects both sync differs in what only each one's database
/// reads (permissions, events, indexes, mocks), while typesync writes just
/// its struct.
impl Emittable for TableConfig {
    fn emitted(&self) -> bool {
        !self.struct_config.resolve_only
    }
    fn same_types(&self, other: &Self) -> bool {
        self.effective().struct_config == other.effective().struct_config
    }
}

/// The types of `projects`, each named, as one set.
pub fn merge_project_types<'a>(
    projects: impl IntoIterator<Item = (&'a str, AllConfigs)>,
) -> Result<AllConfigs> {
    let mut merged = AllConfigs::default();
    let mut owners = Owners::default();
    for (project, configs) in projects {
        let AllConfigs {
            enums,
            tables,
            objects,
            newtypes,
        } = configs;
        merge_kind("enum", project, enums, &mut merged.enums, &mut owners.enums)?;
        merge_kind(
            "table",
            project,
            tables,
            &mut merged.tables,
            &mut owners.tables,
        )?;
        merge_kind(
            "struct",
            project,
            objects,
            &mut merged.objects,
            &mut owners.objects,
        )?;
        merge_kind(
            "newtype",
            project,
            newtypes,
            &mut merged.newtypes,
            &mut owners.newtypes,
        )?;
    }
    Ok(merged)
}

/// Which project each kept type came from, for the error naming both.
#[derive(Default)]
struct Owners<'a> {
    enums: BTreeMap<String, &'a str>,
    tables: BTreeMap<String, &'a str>,
    objects: BTreeMap<String, &'a str>,
    newtypes: BTreeMap<String, &'a str>,
}

fn merge_kind<'a, T: Emittable>(
    kind: &str,
    project: &'a str,
    found: BTreeMap<String, T>,
    kept: &mut BTreeMap<String, T>,
    owners: &mut BTreeMap<String, &'a str>,
) -> Result<()> {
    for (name, definition) in found {
        let Some(existing) = kept.get(&name) else {
            owners.insert(name.clone(), project);
            kept.insert(name, definition);
            continue;
        };
        match (existing.emitted(), definition.emitted()) {
            (true, true) if !existing.same_types(&definition) => {
                let owner = owners.get(&name).copied().unwrap_or_default();
                return Err(EvenframeError::config(format!(
                    "The {kind} `{name}` is emitted by both `{owner}` and `{project}`, and the two \
                     definitions differ. Emit it from one project and include its file in the \
                     other with `resolve_only = true`, or make the two definitions the same"
                )));
            }
            (false, true) => {
                owners.insert(name.clone(), project);
                kept.insert(name, definition);
            }
            (true, true) | (true, false) | (false, false) => {}
        }
    }
    Ok(())
}

/// The foreign types of `projects`, each named, as one set. Two projects may
/// map one foreign type to different database types, since each syncs its
/// own database, but not to different TypeScript.
pub fn merge_foreign_types<'a>(
    projects: impl IntoIterator<Item = (&'a str, &'a BTreeMap<String, ForeignTypeConfig>)>,
) -> Result<BTreeMap<String, ForeignTypeConfig>> {
    let mut merged: BTreeMap<String, ForeignTypeConfig> = BTreeMap::new();
    let mut owners: BTreeMap<&str, &str> = BTreeMap::new();
    for (project, foreign_types) in projects {
        for (name, foreign) in foreign_types {
            match merged.get(name) {
                None => {
                    owners.insert(name, project);
                    merged.insert(name.clone(), foreign.clone());
                }
                Some(existing) if typesync_view(existing) != typesync_view(foreign) => {
                    let owner = owners.get(name.as_str()).copied().unwrap_or_default();
                    return Err(EvenframeError::config(format!(
                        "foreign_types.{name} maps to different TypeScript in `{owner}` and \
                         `{project}`; the workspace writes one typesync output, so set its \
                         TypeScript side in the workspace config"
                    )));
                }
                Some(_) => {}
            }
        }
    }
    Ok(merged)
}

/// `foreign` without the settings only schemasync reads.
fn typesync_view(foreign: &ForeignTypeConfig) -> ForeignTypeConfig {
    ForeignTypeConfig {
        surrealdb: String::new(),
        surrealdb_id_format: None,
        surrealdb_non_id_format: None,
        default_value_surql: String::new(),
        mock_strategy: String::new(),
        ..foreign.clone()
    }
}

#[cfg(test)]
#[path = "workspace_types_tests.rs"]
mod tests;
