//! Per-file type grouping algorithm.
//!
//! Computes which types go into which files using reverse dependency analysis.
//! Types that are exclusively used by a single other type are co-located with
//! that type. Types used by multiple types get their own file.

use crate::typesync::type_index::TypeIndex;
use std::collections::{BTreeMap, BTreeSet};

/// A group of types that will be emitted into a single file.
#[derive(Debug, Clone)]
pub struct TypeFileGroup {
    /// The type that determines the filename.
    pub primary_type: String,
    /// Exclusive dependents bundled into the same file.
    pub co_located_types: Vec<String>,
}

impl TypeFileGroup {
    /// Returns all type names in this group (primary + co-located).
    pub fn all_types(&self) -> Vec<String> {
        let mut all = vec![self.primary_type.clone()];
        all.extend(self.co_located_types.iter().cloned());
        all
    }
}

/// The complete plan for how types are distributed across files.
#[derive(Debug, Clone)]
pub struct FileOutputPlan {
    /// Ordered list of file groups.
    pub groups: Vec<TypeFileGroup>,
    /// Maps each type name to its group index in `groups`.
    pub type_to_group: BTreeMap<String, usize>,
}

/// Computes the file grouping for every emitted type.
///
/// Algorithm:
/// 1. Take every emitted type (PascalCase) and its forward dependencies
/// 2. Invert them to reverse dependencies (for each type, who references it?)
/// 3. Take the SCCs from the index's recursion analysis; types in the same
///    SCC stay together
/// 4. For each type T:
///    - If T has exactly 1 reverse dependent AND T is not in a multi-member SCC
///      → co-locate with that dependent
///    - Otherwise → T gets its own file (it's a "primary" type)
/// 5. SCC members that are all exclusively used by one external type
///    → co-locate the whole SCC with that dependent
///
/// Types are named by their own struct/enum name, not `effective()`: a
/// synthetic projection whose override redirects to another struct is still
/// its own TS interface and needs its own file. `resolve_only` types are
/// registered for resolution but never emitted, so they get no file.
pub fn compute_file_grouping(index: &TypeIndex) -> FileOutputPlan {
    let all_types = index.emitted();

    let mut reverse_deps: BTreeMap<&String, BTreeSet<&String>> = all_types
        .iter()
        .map(|name| (name, BTreeSet::new()))
        .collect();
    for name in all_types {
        for dependency in index.deps(name) {
            reverse_deps.entry(dependency).or_default().insert(name);
        }
    }

    let recursion = index.recursion();
    let multi_member_sccs: Vec<&Vec<String>> = recursion
        .meta
        .values()
        .filter(|(recursive, members)| *recursive && members.len() > 1)
        .map(|(_, members)| members)
        .collect();
    let in_multi_member_scc = |name: &String| {
        recursion
            .comp_of
            .get(name)
            .and_then(|component| recursion.meta.get(component))
            .is_some_and(|(recursive, members)| *recursive && members.len() > 1)
    };

    // Type → the type it is co-located with.
    let mut co_locate_target: BTreeMap<String, String> = BTreeMap::new();

    // First handle SCC groups: if ALL members of an SCC are exclusively used by
    // one external type, co-locate the whole SCC with that type.
    for members in multi_member_sccs {
        let scc: BTreeSet<&String> = members.iter().collect();
        let external_users: BTreeSet<&String> = members
            .iter()
            .filter_map(|member| reverse_deps.get(member))
            .flatten()
            .filter(|user| !scc.contains(*user))
            .copied()
            .collect();
        if let [target] = external_users.into_iter().collect::<Vec<_>>().as_slice() {
            for member in members {
                co_locate_target.insert(member.clone(), (*target).clone());
            }
        }
    }

    // Then handle individual types not in multi-member SCCs.
    for name in all_types {
        if co_locate_target.contains_key(name) || in_multi_member_scc(name) {
            continue;
        }
        if let Some(users) = reverse_deps.get(name)
            && let [target] = users.iter().collect::<Vec<_>>().as_slice()
            && **target != name
        {
            co_locate_target.insert(name.clone(), (**target).clone());
        }
    }

    // Resolve transitive co-location: if A co-locates with B and B co-locates with C,
    // A should co-locate with C (the final primary).
    let resolved_targets = resolve_transitive_colocation(&co_locate_target);

    // Primary types are every type not co-located with another.
    let mut co_located_with: BTreeMap<&String, Vec<String>> = BTreeMap::new();
    for (name, target) in &resolved_targets {
        co_located_with
            .entry(target)
            .or_default()
            .push(name.clone());
    }

    let mut groups: Vec<TypeFileGroup> = Vec::new();
    let mut type_to_group: BTreeMap<String, usize> = BTreeMap::new();
    for primary in all_types
        .iter()
        .filter(|name| !resolved_targets.contains_key(*name))
    {
        let group_index = groups.len();
        let co_located = co_located_with.remove(primary).unwrap_or_default();
        type_to_group.insert(primary.clone(), group_index);
        for name in &co_located {
            type_to_group.insert(name.clone(), group_index);
        }
        groups.push(TypeFileGroup {
            primary_type: primary.clone(),
            co_located_types: co_located,
        });
    }

    FileOutputPlan {
        groups,
        type_to_group,
    }
}

/// Resolves transitive co-location chains.
/// If A → B and B → C, resolves to A → C and B → C.
fn resolve_transitive_colocation(
    co_locate_target: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut resolved = co_locate_target.clone();
    // Iterate until stable.
    loop {
        let mut changed = false;
        let snapshot = resolved.clone();
        for (name, target) in resolved.iter_mut() {
            if let Some(next_target) = snapshot.get(target.as_str())
                && name != next_target
            {
                *target = next_target.clone();
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    resolved
}

#[cfg(test)]
mod tests {
    use super::{BTreeMap, TypeIndex, compute_file_grouping};
    use crate::types::{FieldType, StructConfig, StructField};

    fn make_struct(name: &str, fields: Vec<(&str, FieldType)>) -> StructConfig {
        StructConfig {
            resolve_only: false,
            struct_name: name.to_string(),
            fields: fields
                .into_iter()
                .map(|(fname, ftype)| StructField {
                    field_name: fname.to_string(),
                    field_type: ftype,
                    ..Default::default()
                })
                .collect(),
            validators: vec![],
            doccom: None,
            macroforge_derives: vec![],
            annotations: vec![],
            pipeline: crate::types::Pipeline::default(),
            rust_derives: vec![],
            output_override: None,
            raw_attributes: std::collections::BTreeMap::new(),
        }
    }

    #[test]
    fn test_exclusive_dependent_co_locates() {
        // User uses Address (exclusively), Post uses nothing.
        // Address should be co-located with User.
        let mut structs = BTreeMap::new();
        structs.insert(
            "User".to_string(),
            make_struct(
                "User",
                vec![("address", FieldType::Other("Address".to_string()))],
            ),
        );
        structs.insert(
            "Address".to_string(),
            make_struct("Address", vec![("street", FieldType::String)]),
        );
        structs.insert(
            "Post".to_string(),
            make_struct("Post", vec![("title", FieldType::String)]),
        );
        let enums = BTreeMap::new();

        let index = TypeIndex::new(&structs, &enums).unwrap();
        let plan = compute_file_grouping(&index);

        // Address should be co-located with User
        assert_eq!(
            plan.type_to_group[&"User".to_string()],
            plan.type_to_group[&"Address".to_string()]
        );
        // Post should be in its own group
        assert_ne!(
            plan.type_to_group[&"Post".to_string()],
            plan.type_to_group[&"User".to_string()]
        );

        // Find the User group and check co-location
        let user_group_idx = plan.type_to_group[&"User".to_string()];
        let user_group = &plan.groups[user_group_idx];
        assert_eq!(user_group.primary_type, "User");
        assert!(user_group.co_located_types.contains(&"Address".to_string()));
    }

    #[test]
    fn test_shared_type_gets_own_file() {
        // Role is used by both User and Post → gets its own file.
        let mut structs = BTreeMap::new();
        structs.insert(
            "User".to_string(),
            make_struct("User", vec![("role", FieldType::Other("Role".to_string()))]),
        );
        structs.insert(
            "Post".to_string(),
            make_struct("Post", vec![("role", FieldType::Other("Role".to_string()))]),
        );
        structs.insert(
            "Role".to_string(),
            make_struct("Role", vec![("name", FieldType::String)]),
        );
        let enums = BTreeMap::new();

        let index = TypeIndex::new(&structs, &enums).unwrap();
        let plan = compute_file_grouping(&index);

        // All three should be in different groups
        assert_ne!(plan.type_to_group["User"], plan.type_to_group["Role"]);
        assert_ne!(plan.type_to_group["Post"], plan.type_to_group["Role"]);
    }

    #[test]
    fn test_plan_example_from_spec() {
        // User (uses Address, Role), Post (uses Role), Address (only by User), Role (shared)
        let mut structs = BTreeMap::new();
        structs.insert(
            "User".to_string(),
            make_struct(
                "User",
                vec![
                    ("address", FieldType::Other("Address".to_string())),
                    ("role", FieldType::Other("Role".to_string())),
                ],
            ),
        );
        structs.insert(
            "Post".to_string(),
            make_struct("Post", vec![("role", FieldType::Other("Role".to_string()))]),
        );
        structs.insert(
            "Address".to_string(),
            make_struct("Address", vec![("street", FieldType::String)]),
        );
        structs.insert(
            "Role".to_string(),
            make_struct("Role", vec![("name", FieldType::String)]),
        );
        let enums = BTreeMap::new();

        let index = TypeIndex::new(&structs, &enums).unwrap();
        let plan = compute_file_grouping(&index);

        // Address co-located with User
        assert_eq!(plan.type_to_group["User"], plan.type_to_group["Address"]);
        // Role gets own file
        assert_ne!(plan.type_to_group["User"], plan.type_to_group["Role"]);
        // Post gets own file
        assert_ne!(plan.type_to_group["Post"], plan.type_to_group["User"]);

        // 3 groups total: User+Address, Post, Role
        assert_eq!(plan.groups.len(), 3);
    }

    #[test]
    fn test_no_deps_each_gets_own_file() {
        let mut structs = BTreeMap::new();
        structs.insert(
            "A".to_string(),
            make_struct("A", vec![("x", FieldType::String)]),
        );
        structs.insert(
            "B".to_string(),
            make_struct("B", vec![("y", FieldType::I32)]),
        );
        let enums = BTreeMap::new();

        let index = TypeIndex::new(&structs, &enums).unwrap();
        let plan = compute_file_grouping(&index);
        assert_eq!(plan.groups.len(), 2);
    }
}
