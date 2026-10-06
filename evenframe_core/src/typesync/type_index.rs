//! The types a typesync run generates from, indexed once and shared by every
//! output: lookup by the name a field uses, each type's dependencies, the
//! recursion between them and the order they are defined in.

use crate::dependency::{RecursionInfo, dependency_map, recursion_of};
use crate::error::{EvenframeError, Result};
use crate::types::{FieldType, NewtypeConfig, StructConfig, TaggedUnion};
use crate::typesync::file_grouping::{FileOutputPlan, compute_file_grouping};
use convert_case::{Case, Casing};
use petgraph::algo::toposort;
use petgraph::graphmap::DiGraphMap;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

/// No newtypes, for an index built from structs and enums alone.
static NO_NEWTYPES: BTreeMap<String, NewtypeConfig> = BTreeMap::new();

pub struct TypeIndex<'a> {
    structs: &'a BTreeMap<String, StructConfig>,
    enums: &'a BTreeMap<String, TaggedUnion>,
    newtypes: &'a BTreeMap<String, NewtypeConfig>,
    /// PascalCase name to the newtype by that name.
    newtype_by_name: BTreeMap<String, &'a NewtypeConfig>,
    /// Every struct with its PascalCase name, in map order.
    named_structs: Vec<(String, &'a StructConfig)>,
    /// Every enum with its PascalCase name, in map order.
    named_enums: Vec<(String, &'a TaggedUnion)>,
    /// PascalCase name to every struct by that name, in map order.
    struct_by_name: BTreeMap<String, Vec<&'a StructConfig>>,
    /// PascalCase name to every enum by that name, in map order.
    enum_by_name: BTreeMap<String, Vec<&'a TaggedUnion>>,
    /// PascalCase effective name to the first enum's effective configuration.
    enum_by_effective_name: BTreeMap<String, &'a TaggedUnion>,
    /// The names of every type that is emitted, not only resolved against.
    emitted: BTreeSet<String>,
    /// The PascalCase names of every type's effective configuration.
    effective_names: BTreeSet<String>,
    deps: BTreeMap<String, BTreeSet<String>>,
    recursion: RecursionInfo,
    /// Each type's position in definition order: components that others
    /// depend on come first, and a component's members are sorted by name.
    position: BTreeMap<String, usize>,
    /// Types in definition order.
    ordered: Vec<String>,
    file_plan: OnceLock<FileOutputPlan>,
}

impl<'a> TypeIndex<'a> {
    pub fn new(
        structs: &'a BTreeMap<String, StructConfig>,
        enums: &'a BTreeMap<String, TaggedUnion>,
    ) -> Result<Self> {
        Self::with_newtypes(structs, enums, &NO_NEWTYPES)
    }

    pub fn with_newtypes(
        structs: &'a BTreeMap<String, StructConfig>,
        enums: &'a BTreeMap<String, TaggedUnion>,
        newtypes: &'a BTreeMap<String, NewtypeConfig>,
    ) -> Result<Self> {
        let mut named_structs = Vec::with_capacity(structs.len());
        let mut named_enums = Vec::with_capacity(enums.len());
        let mut struct_by_name: BTreeMap<String, Vec<&StructConfig>> = BTreeMap::new();
        let mut enum_by_name: BTreeMap<String, Vec<&TaggedUnion>> = BTreeMap::new();
        let mut enum_by_effective_name: BTreeMap<String, &TaggedUnion> = BTreeMap::new();
        let mut emitted = BTreeSet::new();
        let mut effective_names = BTreeSet::new();
        for struct_config in structs.values() {
            let name = struct_config.struct_name.to_case(Case::Pascal);
            if !struct_config.resolve_only {
                emitted.insert(name.clone());
            }
            effective_names.insert(struct_config.effective().struct_name.to_case(Case::Pascal));
            struct_by_name
                .entry(name.clone())
                .or_default()
                .push(struct_config);
            named_structs.push((name, struct_config));
        }
        for tagged_union in enums.values() {
            let name = tagged_union.enum_name.to_case(Case::Pascal);
            if !tagged_union.resolve_only {
                emitted.insert(name.clone());
            }
            let effective = tagged_union.effective();
            let effective_name = effective.enum_name.to_case(Case::Pascal);
            effective_names.insert(effective_name.clone());
            enum_by_effective_name
                .entry(effective_name)
                .or_insert(effective);
            enum_by_name
                .entry(name.clone())
                .or_default()
                .push(tagged_union);
            named_enums.push((name, tagged_union));
        }
        let mut newtype_by_name = BTreeMap::new();
        for newtype in newtypes.values() {
            let name = newtype.name.to_case(Case::Pascal);
            if !newtype.resolve_only {
                emitted.insert(name.clone());
            }
            effective_names.insert(name.clone());
            newtype_by_name.entry(name).or_insert(newtype);
        }

        let deps = dependency_map(structs, enums, newtypes);
        let recursion = recursion_of(&deps);
        let ordered = definition_order(&deps, &recursion)?;
        let position = ordered
            .iter()
            .enumerate()
            .map(|(index, name)| (name.clone(), index))
            .collect();
        Ok(Self {
            structs,
            enums,
            newtypes,
            newtype_by_name,
            named_structs,
            named_enums,
            struct_by_name,
            enum_by_name,
            enum_by_effective_name,
            emitted,
            effective_names,
            deps,
            recursion,
            position,
            ordered,
            file_plan: OnceLock::new(),
        })
    }

    pub fn structs(&self) -> &'a BTreeMap<String, StructConfig> {
        self.structs
    }

    pub fn enums(&self) -> &'a BTreeMap<String, TaggedUnion> {
        self.enums
    }

    pub fn newtypes(&self) -> &'a BTreeMap<String, NewtypeConfig> {
        self.newtypes
    }

    /// Every newtype with its PascalCase name, in name order.
    pub fn named_newtypes(&self) -> impl Iterator<Item = (&String, &'a NewtypeConfig)> {
        self.newtype_by_name
            .iter()
            .map(|(name, newtype)| (name, *newtype))
    }

    /// The newtype a field naming `name` refers to.
    pub fn newtype_named(&self, name: &str) -> Option<&'a NewtypeConfig> {
        named(&self.newtype_by_name, name).copied()
    }

    /// The type a value of `field_type` is written as: the inner type of the
    /// newtype it names, through any newtype that holds another.
    pub fn underlying<'t>(&'t self, field_type: &'t FieldType) -> &'t FieldType {
        let mut current = field_type;
        // A chain longer than the newtypes there are has come back on itself.
        for _ in 0..=self.newtype_by_name.len() {
            match current {
                FieldType::Other(name) => match self.newtype_named(name) {
                    Some(newtype) => current = &newtype.inner,
                    None => return current,
                },
                _ => return current,
            }
        }
        current
    }

    /// Every struct with its PascalCase name, in map order.
    pub fn named_structs(&self) -> &[(String, &'a StructConfig)] {
        &self.named_structs
    }

    /// Every enum with its PascalCase name, in map order.
    pub fn named_enums(&self) -> &[(String, &'a TaggedUnion)] {
        &self.named_enums
    }

    /// The first struct a field naming `name` refers to.
    pub fn struct_named(&self, name: &str) -> Option<&'a StructConfig> {
        named(&self.struct_by_name, name).and_then(|found| found.first().copied())
    }

    /// Every struct by the PascalCase name `name`, in map order.
    pub fn structs_named(&self, name: &str) -> &[&'a StructConfig] {
        named(&self.struct_by_name, name).map_or(&[], Vec::as_slice)
    }

    /// The first enum a field naming `name` refers to.
    pub fn enum_named(&self, name: &str) -> Option<&'a TaggedUnion> {
        named(&self.enum_by_name, name).and_then(|found| found.first().copied())
    }

    /// Every enum by the PascalCase name `name`, in map order.
    pub fn enums_named(&self, name: &str) -> &[&'a TaggedUnion] {
        named(&self.enum_by_name, name).map_or(&[], Vec::as_slice)
    }

    /// The effective configuration of the first enum whose effective name is
    /// `name`.
    pub fn effective_enum_named(&self, name: &str) -> Option<&'a TaggedUnion> {
        named(&self.enum_by_effective_name, name).copied()
    }

    /// Every type's PascalCase name: the structs', the enums', then the
    /// newtypes'.
    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.struct_by_name
            .keys()
            .chain(self.enum_by_name.keys())
            .chain(self.newtype_by_name.keys())
    }

    /// The PascalCase names of every type that is emitted.
    pub fn emitted(&self) -> &BTreeSet<String> {
        &self.emitted
    }

    /// The PascalCase names of every type's effective configuration.
    pub fn effective_names(&self) -> &BTreeSet<String> {
        &self.effective_names
    }

    /// The types `name` references directly.
    pub fn deps(&self, name: &str) -> impl Iterator<Item = &String> {
        self.deps.get(name).into_iter().flatten()
    }

    pub fn recursion(&self) -> &RecursionInfo {
        &self.recursion
    }

    /// Every type, in the order its definitions must be written.
    pub fn ordered(&self) -> &[String] {
        &self.ordered
    }

    /// `names` sorted into definition order.
    pub fn in_definition_order<'n>(
        &self,
        names: impl IntoIterator<Item = &'n String>,
    ) -> Vec<&'n String> {
        let mut sorted: Vec<&String> = names.into_iter().collect();
        sorted.sort_by_key(|name| {
            self.position
                .get(name.as_str())
                .copied()
                .unwrap_or(usize::MAX)
        });
        sorted
    }

    /// Which file each type goes to in per-file outputs.
    pub fn file_plan(&self) -> &FileOutputPlan {
        self.file_plan.get_or_init(|| compute_file_grouping(self))
    }
}

/// The entry for a field's type name, which is PascalCase in all but rare
/// cases, so the conversion only runs on a miss.
fn named<'m, T>(by_name: &'m BTreeMap<String, T>, name: &str) -> Option<&'m T> {
    by_name
        .get(name)
        .or_else(|| by_name.get(&name.to_case(Case::Pascal)))
}

/// Every type in definition order: the strongly connected components sorted
/// so each comes after the components it depends on, and each component's
/// members by name.
fn definition_order(
    deps: &BTreeMap<String, BTreeSet<String>>,
    recursion: &RecursionInfo,
) -> Result<Vec<String>> {
    let mut condensation = DiGraphMap::<usize, ()>::new();
    // Every component is a node, so a type with no dependency edges is still emitted.
    for &component in recursion.meta.keys() {
        condensation.add_node(component);
    }
    let component_of = |name: &String| {
        recursion.comp_of.get(name).copied().ok_or_else(|| {
            EvenframeError::type_sync(format!("`{name}` is missing from the dependency graph"))
        })
    };
    for (_, members) in recursion.meta.values() {
        for member in members {
            let from = component_of(member)?;
            for dependency in deps.get(member).into_iter().flatten() {
                let to = component_of(dependency)?;
                if from != to {
                    // An edge A -> B means "A depends on B".
                    condensation.add_edge(from, to, ());
                }
            }
        }
    }
    let mut components = toposort(&condensation, None).map_err(|cycle| {
        EvenframeError::type_sync(format!(
            "type dependency components form a cycle at component {}",
            cycle.node_id()
        ))
    })?;
    // `toposort` puts dependents first; definitions need dependencies first.
    components.reverse();
    let mut ordered = Vec::new();
    for component in components {
        let mut members = recursion
            .meta
            .get(&component)
            .map(|(_, members)| members.clone())
            .ok_or_else(|| {
                EvenframeError::type_sync(format!("component {component} has no members"))
            })?;
        members.sort();
        ordered.extend(members);
    }
    Ok(ordered)
}
