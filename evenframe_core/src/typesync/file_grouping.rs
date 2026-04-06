//! Per-file type grouping algorithm.
//!
//! Computes which types go into which files using reverse dependency analysis.
//! Types that are exclusively used by a single other type are co-located with
//! that type. Types used by multiple types get their own file.

use crate::dependency::{analyse_recursion, deps_of};
use crate::types::{StructConfig, TaggedUnion};
use convert_case::{Case, Casing};
use std::collections::{HashMap, HashSet};

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
    pub type_to_group: HashMap<String, usize>,
}

/// Computes the file grouping for all known types.
///
/// Algorithm:
/// 1. Collect all type names (PascalCase) from structs + enums
/// 2. Build forward deps using `deps_of()`
/// 3. Invert to build reverse deps (for each type, who references it?)
/// 4. Find SCCs using `analyse_recursion()` — types in the same SCC stay together
/// 5. For each type T:
///    - If T has exactly 1 reverse dependent AND T is not in a multi-member SCC
///      → co-locate with that dependent
///    - Otherwise → T gets its own file (it's a "primary" type)
/// 6. SCC members that are all exclusively used by one external type
///    → co-locate the whole SCC with that dependent
pub fn compute_file_grouping(
    structs: &HashMap<String, StructConfig>,
    enums: &HashMap<String, TaggedUnion>,
) -> FileOutputPlan {
    // 1. Collect all type names in PascalCase.
    let all_types: HashSet<String> = structs
        .values()
        .map(|s| s.struct_name.to_case(Case::Pascal))
        .chain(enums.values().map(|e| e.enum_name.to_case(Case::Pascal)))
        .collect();

    // 2. Build forward deps.
    let mut forward_deps: HashMap<String, HashSet<String>> = HashMap::new();
    for name in &all_types {
        forward_deps.insert(name.clone(), deps_of(name, structs, enums));
    }

    // 3. Invert to build reverse deps.
    let mut reverse_deps: HashMap<String, HashSet<String>> = HashMap::new();
    for name in &all_types {
        reverse_deps.entry(name.clone()).or_default();
    }
    for (from, tos) in &forward_deps {
        for to in tos {
            reverse_deps
                .entry(to.clone())
                .or_default()
                .insert(from.clone());
        }
    }

    // 4. Analyse recursion to find SCCs.
    let rec = analyse_recursion(structs, enums);

    // Build a map from SCC id → members for multi-member SCCs.
    let mut scc_members: HashMap<usize, Vec<String>> = HashMap::new();
    for (name, &comp_id) in &rec.comp_of {
        if let Some((is_recursive, members)) = rec.meta.get(&comp_id)
            && *is_recursive
            && members.len() > 1
        {
            scc_members.entry(comp_id).or_default();
            if !scc_members[&comp_id].contains(name) {
                scc_members.get_mut(&comp_id).unwrap().push(name.clone());
            }
        }
    }

    // 5. Determine which types are "co-locatable".
    // A type can be co-located if:
    //   - It has exactly 1 reverse dependent
    //   - It is NOT in a multi-member SCC (or the whole SCC is co-locatable)
    let mut co_locate_target: HashMap<String, String> = HashMap::new(); // type → target to co-locate with

    // First handle SCC groups: if ALL members of an SCC are exclusively used by
    // one external type, co-locate the whole SCC with that type.
    for members in scc_members.values() {
        // Collect all external reverse deps for the SCC as a whole.
        let scc_set: HashSet<&String> = members.iter().collect();
        let mut external_users: HashSet<String> = HashSet::new();
        for member in members {
            if let Some(rev) = reverse_deps.get(member) {
                for user in rev {
                    if !scc_set.contains(user) {
                        external_users.insert(user.clone());
                    }
                }
            }
        }
        if external_users.len() == 1 {
            let target = external_users.into_iter().next().unwrap();
            // Don't co-locate if the target is itself in this SCC.
            if !scc_set.contains(&target) {
                for member in members {
                    co_locate_target.insert(member.clone(), target.clone());
                }
            }
        }
    }

    // Then handle individual types not in multi-member SCCs.
    for name in &all_types {
        if co_locate_target.contains_key(name) {
            continue; // Already handled by SCC logic.
        }
        let comp_id = rec.comp_of.get(name);
        let in_multi_scc = comp_id
            .and_then(|c| rec.meta.get(c))
            .map(|(is_rec, members)| *is_rec && members.len() > 1)
            .unwrap_or(false);
        if in_multi_scc {
            continue; // Part of a multi-member SCC that wasn't co-locatable.
        }
        if let Some(rev) = reverse_deps.get(name)
            && rev.len() == 1
        {
            let target = rev.iter().next().unwrap().clone();
            if target != *name {
                co_locate_target.insert(name.clone(), target.clone());
            }
        }
    }

    // Resolve transitive co-location: if A co-locates with B and B co-locates with C,
    // A should co-locate with C (the final primary).
    let resolved_targets = resolve_transitive_colocation(&co_locate_target);

    // 6. Build groups.
    // Primary types = all types NOT in co_locate_target (after resolution).
    let primary_types: Vec<String> = {
        let mut primaries: Vec<String> = all_types
            .iter()
            .filter(|name| !resolved_targets.contains_key(*name))
            .cloned()
            .collect();
        primaries.sort();
        primaries
    };

    let mut groups: Vec<TypeFileGroup> = Vec::new();
    let mut type_to_group: HashMap<String, usize> = HashMap::new();

    for primary in &primary_types {
        let group_idx = groups.len();
        let mut co_located: Vec<String> = resolved_targets
            .iter()
            .filter(|(_, target)| *target == primary)
            .map(|(name, _)| name.clone())
            .collect();
        co_located.sort();

        type_to_group.insert(primary.clone(), group_idx);
        for co in &co_located {
            type_to_group.insert(co.clone(), group_idx);
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
    co_locate_target: &HashMap<String, String>,
) -> HashMap<String, String> {
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
    use super::*;
    use crate::types::{FieldType, StructField};

    fn make_struct(name: &str, fields: Vec<(&str, FieldType)>) -> StructConfig {
        StructConfig {
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
        }
    }

    #[test]
    fn test_exclusive_dependent_co_locates() {
        // User uses Address (exclusively), Post uses nothing.
        // Address should be co-located with User.
        let mut structs = HashMap::new();
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
        let enums = HashMap::new();

        let plan = compute_file_grouping(&structs, &enums);

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
        let mut structs = HashMap::new();
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
        let enums = HashMap::new();

        let plan = compute_file_grouping(&structs, &enums);

        // All three should be in different groups
        assert_ne!(plan.type_to_group["User"], plan.type_to_group["Role"]);
        assert_ne!(plan.type_to_group["Post"], plan.type_to_group["Role"]);
    }

    #[test]
    fn test_plan_example_from_spec() {
        // User (uses Address, Role), Post (uses Role), Address (only by User), Role (shared)
        let mut structs = HashMap::new();
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
        let enums = HashMap::new();

        let plan = compute_file_grouping(&structs, &enums);

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
        let mut structs = HashMap::new();
        structs.insert(
            "A".to_string(),
            make_struct("A", vec![("x", FieldType::String)]),
        );
        structs.insert(
            "B".to_string(),
            make_struct("B", vec![("y", FieldType::I32)]),
        );
        let enums = HashMap::new();

        let plan = compute_file_grouping(&structs, &enums);
        assert_eq!(plan.groups.len(), 2);
    }
}
