//! Resolves the type paths scanned fields name to the definitions they refer
//! to: through each module's own items, its `use` declarations, its glob
//! imports and the re-exports of the modules a path passes through, and by
//! name where no parsed module leads to a definition.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use syn::{Item, UseTree};

/// How many re-exports a path may pass through before resolution gives up,
/// which also ends a cycle of re-exports.
const MAX_REEXPORT_DEPTH: usize = 32;

/// The crates a path into the standard library starts with. Such a path never
/// names a scanned definition.
const STANDARD_CRATES: [&str; 3] = ["std", "core", "alloc"];

/// What a module brings into scope for name resolution.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleScope {
    /// Each name a `use` declaration brings in, with the absolute path it
    /// names.
    pub imports: BTreeMap<String, String>,
    /// The absolute paths of the modules a `use path::*` imports from.
    pub globs: Vec<String>,
    /// The names of the module's own types and child modules.
    pub items: BTreeSet<String>,
}

impl ModuleScope {
    /// The scope `items` give the module at `module_path`.
    pub fn of(items: &[Item], module_path: &str) -> Self {
        let module_path = rust_path(module_path);
        let mut scope = Self {
            items: items.iter().filter_map(declared_name).collect(),
            ..Self::default()
        };
        for item in items {
            if let Item::Use(item_use) = item {
                let prefix = if item_use.leading_colon.is_some() {
                    vec![String::new()]
                } else {
                    Vec::new()
                };
                scope.add_use_tree(&item_use.tree, prefix, &module_path);
            }
        }
        scope
    }

    fn add_use_tree(&mut self, tree: &UseTree, prefix: Vec<String>, module_path: &str) {
        match tree {
            UseTree::Path(path) => {
                let mut prefix = prefix;
                prefix.push(path.ident.to_string());
                self.add_use_tree(&path.tree, prefix, module_path);
            }
            UseTree::Name(name) => {
                let ident = name.ident.to_string();
                self.add_import(ident.clone(), prefix, &ident, module_path);
            }
            UseTree::Rename(rename) => {
                self.add_import(
                    rename.rename.to_string(),
                    prefix,
                    &rename.ident.to_string(),
                    module_path,
                );
            }
            UseTree::Glob(_) => {
                if let Some(path) = absolute(&prefix, module_path, &self.items) {
                    self.globs.push(path);
                }
            }
            UseTree::Group(group) => {
                for tree in &group.items {
                    self.add_use_tree(tree, prefix.clone(), module_path);
                }
            }
        }
    }

    /// Records that `alias` names `prefix::ident`, where an `ident` of `self`
    /// names the module `prefix` itself.
    fn add_import(&mut self, alias: String, prefix: Vec<String>, ident: &str, module_path: &str) {
        let alias = if ident == "self" {
            match prefix.last() {
                Some(last) if alias == "self" => last.clone(),
                _ => alias,
            }
        } else {
            alias
        };
        let mut segments = prefix;
        if ident != "self" {
            segments.push(ident.to_string());
        }
        if alias == "_" {
            return;
        }
        if let Some(path) = absolute(&segments, module_path, &self.items) {
            self.imports.insert(alias, path);
        }
    }
}

/// `segments`, written in the module at `module_path` whose own items are
/// `items`, as an absolute path. A leading empty segment marks a path that
/// is already absolute.
fn absolute(segments: &[String], module_path: &str, items: &BTreeSet<String>) -> Option<String> {
    let (first, rest) = segments.split_first()?;
    let base = match first.as_str() {
        "" => return Some(rest.join("::")),
        "crate" => crate_root(module_path).to_string(),
        "self" => module_path.to_string(),
        "super" => parent(module_path)?.to_string(),
        name if items.contains(name) => format!("{module_path}::{name}"),
        name => name.to_string(),
    };
    let mut path = base;
    for segment in rest {
        match segment.as_str() {
            "super" => path = parent(&path)?.to_string(),
            "self" => {}
            segment => {
                path.push_str("::");
                path.push_str(segment);
            }
        }
    }
    Some(path)
}

/// The name of a type or module `item` declares.
fn declared_name(item: &Item) -> Option<String> {
    let ident = match item {
        Item::Struct(item) => &item.ident,
        Item::Enum(item) => &item.ident,
        Item::Union(item) => &item.ident,
        Item::Type(item) => &item.ident,
        Item::Mod(item) => &item.ident,
        Item::Trait(item) => &item.ident,
        _ => return None,
    };
    Some(ident.to_string())
}

/// `module_path` as Rust paths name it: a crate named with hyphens is
/// imported with underscores.
pub fn rust_path(module_path: &str) -> String {
    module_path.replace('-', "_")
}

fn crate_root(module_path: &str) -> &str {
    module_path.split("::").next().unwrap_or(module_path)
}

fn parent(path: &str) -> Option<&str> {
    path.rsplit_once("::").map(|(parent, _)| parent)
}

/// What a type path resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// The scanned definition at this index.
    Definition(usize),
    /// A type no scanned definition provides, by its absolute path.
    External(String),
    /// A path no resolution reaches, whose name several scanned definitions
    /// share.
    Ambiguous(Vec<usize>),
}

/// Every scanned definition by absolute path, and every module's scope.
pub struct TypePaths<'a> {
    scopes: &'a BTreeMap<String, ModuleScope>,
    definitions: BTreeMap<String, usize>,
    by_name: BTreeMap<String, Vec<usize>>,
}

impl<'a> TypePaths<'a> {
    /// Indexes `definitions`, each an absolute path, by the order given.
    pub fn new(
        scopes: &'a BTreeMap<String, ModuleScope>,
        definitions: impl IntoIterator<Item = String>,
    ) -> Self {
        let mut index = BTreeMap::new();
        let mut by_name: BTreeMap<String, Vec<usize>> = BTreeMap::new();
        for (position, path) in definitions.into_iter().enumerate() {
            let name = path.rsplit("::").next().unwrap_or(&path).to_string();
            by_name.entry(name).or_default().push(position);
            index.insert(path, position);
        }
        Self {
            scopes,
            definitions: index,
            by_name,
        }
    }

    /// What `path`, written in the module at `module_path`, names.
    pub fn resolve(&self, module_path: &str, path: &str) -> Target {
        let module_path = rust_path(module_path);
        let candidates = self.candidates(&module_path, path);
        if let Some(definition) = candidates
            .iter()
            .find_map(|candidate| self.lookup(candidate, 0))
        {
            return Target::Definition(definition);
        }
        let absolute = candidates
            .last()
            .cloned()
            .unwrap_or_else(|| path.to_string());
        if STANDARD_CRATES.contains(&crate_root(&absolute)) {
            return Target::External(absolute);
        }
        // A path resolution cannot follow, such as a re-export in a file the
        // scan never parsed, still names a definition its name singles out.
        let name = absolute.rsplit("::").next().unwrap_or(&absolute);
        match self.by_name.get(name).map(Vec::as_slice) {
            Some([only]) => Target::Definition(*only),
            Some(several) if !several.is_empty() => Target::Ambiguous(several.to_vec()),
            _ => Target::External(absolute),
        }
    }

    /// The absolute paths `path` may name from `module_path`, the most
    /// specific first and the path as an external one last.
    fn candidates(&self, module_path: &str, path: &str) -> Vec<String> {
        let segments: Vec<String> = path.split("::").map(str::to_string).collect();
        let Some(first) = segments.first() else {
            return Vec::new();
        };
        let scope = self.scopes.get(module_path);
        let rest = segments[1..].join("::");
        let join = |base: &str| {
            if rest.is_empty() {
                base.to_string()
            } else {
                format!("{base}::{rest}")
            }
        };
        match first.as_str() {
            "" => vec![rest.clone()],
            "crate" | "self" | "super" => absolute(&segments, module_path, &BTreeSet::new())
                .into_iter()
                .collect(),
            name => {
                if let Some(scope) = scope {
                    if scope.items.contains(name) {
                        return vec![format!("{module_path}::{path}")];
                    }
                    if let Some(import) = scope.imports.get(name) {
                        return vec![join(import)];
                    }
                }
                let child = format!("{module_path}::{name}");
                if self.scopes.contains_key(&child) {
                    return vec![format!("{module_path}::{path}")];
                }
                scope
                    .map(|scope| scope.globs.iter().map(|glob| format!("{glob}::{path}")))
                    .into_iter()
                    .flatten()
                    .chain(std::iter::once(path.to_string()))
                    .collect()
            }
        }
    }

    /// The definition at `absolute`, following the re-exports of the module
    /// it names an item of.
    fn lookup(&self, absolute: &str, depth: usize) -> Option<usize> {
        if let Some(definition) = self.definitions.get(absolute) {
            return Some(*definition);
        }
        if depth >= MAX_REEXPORT_DEPTH {
            return None;
        }
        let (module_path, name) = absolute.rsplit_once("::")?;
        let scope = self.scopes.get(module_path)?;
        if let Some(import) = scope.imports.get(name)
            && import != absolute
            && let Some(definition) = self.lookup(import, depth + 1)
        {
            return Some(definition);
        }
        scope
            .globs
            .iter()
            .find_map(|glob| self.lookup(&format!("{glob}::{name}"), depth + 1))
    }
}

#[cfg(test)]
mod tests {
    use super::{BTreeMap, ModuleScope, Target, TypePaths};

    fn scope(source: &str, module_path: &str) -> ModuleScope {
        let file = syn::parse_file(source).expect("source parses");
        ModuleScope::of(&file.items, module_path)
    }

    #[test]
    fn use_declarations_import_absolute_paths() {
        let scope = scope(
            r#"
            mod billing;
            use crate::models::auth::{self, Status as AuthStatus};
            use super::shared::Address;
            use billing::Invoice;
            use ::serde::Serialize;
            use chrono::DateTime;
            use self::billing::*;
            "#,
            "shop::models::orders",
        );
        let imported = |alias: &str| scope.imports.get(alias).map(String::as_str);
        assert_eq!(imported("auth"), Some("shop::models::auth"));
        assert_eq!(imported("AuthStatus"), Some("shop::models::auth::Status"));
        assert_eq!(imported("Address"), Some("shop::models::shared::Address"));
        assert_eq!(
            imported("Invoice"),
            Some("shop::models::orders::billing::Invoice")
        );
        assert_eq!(imported("Serialize"), Some("serde::Serialize"));
        assert_eq!(imported("DateTime"), Some("chrono::DateTime"));
        assert_eq!(
            scope.globs,
            vec!["shop::models::orders::billing".to_string()]
        );
    }

    #[test]
    fn a_hyphenated_crate_is_named_with_underscores() {
        let scopes = BTreeMap::from([(
            "my_shop::orders".to_string(),
            scope("use crate::models::Status;", "my-shop::orders"),
        )]);
        let paths = TypePaths::new(&scopes, ["my_shop::models::Status".to_string()]);
        assert_eq!(
            paths.resolve("my-shop::orders", "Status"),
            Target::Definition(0)
        );
    }

    #[test]
    fn standard_library_paths_never_name_a_definition() {
        let scopes = BTreeMap::new();
        let paths = TypePaths::new(&scopes, ["shop::Duration".to_string()]);
        assert_eq!(
            paths.resolve("shop::orders", "std::time::Duration"),
            Target::External("std::time::Duration".to_string())
        );
    }
}
