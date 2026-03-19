//! Cross-file import resolution for per-file TypeScript output.
//!
//! Determines what import statements each file needs based on cross-group
//! type dependencies.

use crate::dependency::deps_of;
use crate::types::{StructConfig, TaggedUnion};
use crate::typesync::config::FileNamingConvention;
use crate::typesync::file_grouping::{FileOutputPlan, TypeFileGroup};
use convert_case::{Case, Casing};
use std::collections::{BTreeMap, HashMap, HashSet};

/// A single TypeScript import statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportStatement {
    /// Type names to import (e.g. `["User", "UserEncoded"]`).
    pub type_names: Vec<String>,
    /// Relative path without extension (e.g. `"./user"`).
    pub from_path: String,
}

/// Converts a PascalCase type name to a filename (without extension) using the given convention.
pub fn type_name_to_filename(type_name: &str, naming: FileNamingConvention) -> String {
    match naming {
        FileNamingConvention::Pascal => type_name.to_case(Case::Pascal),
        FileNamingConvention::Kebab => type_name.to_case(Case::Kebab),
        FileNamingConvention::Snake => type_name.to_case(Case::Snake),
        FileNamingConvention::Camel => type_name.to_case(Case::Camel),
    }
}

/// Computes the import specifier suffix from a file extension.
/// For `.ts` → no suffix (TypeScript resolves extensionless imports).
/// For `.svelte.ts` → `.svelte` (strip trailing `.ts`).
/// For other compound extensions → strip trailing `.ts`/`.js` if present.
pub fn import_specifier_suffix(file_extension: &str) -> &str {
    if file_extension == ".ts" || file_extension == ".js" {
        ""
    } else {
        file_extension
            .strip_suffix(".ts")
            .or_else(|| file_extension.strip_suffix(".js"))
            .unwrap_or(file_extension)
    }
}

/// Resolves what imports a given file group needs from other groups.
///
/// For each type in the group, collects its `deps_of()`, filters to types NOT in
/// this group, looks up their group, and creates import statements grouped by
/// source file.
pub fn resolve_imports(
    group: &TypeFileGroup,
    plan: &FileOutputPlan,
    structs: &HashMap<String, StructConfig>,
    enums: &HashMap<String, TaggedUnion>,
    naming: FileNamingConvention,
    file_extension: &str,
) -> Vec<ImportStatement> {
    let group_types: HashSet<String> = group.all_types().into_iter().collect();
    let suffix = import_specifier_suffix(file_extension);

    // Collect all external dependencies from all types in this group.
    let mut external_deps: HashSet<String> = HashSet::new();
    for type_name in &group_types {
        for dep in deps_of(type_name, structs, enums) {
            if !group_types.contains(&dep) {
                external_deps.insert(dep);
            }
        }
    }

    // Group external deps by their source file (group index).
    let mut by_group: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for dep in external_deps {
        if let Some(&group_idx) = plan.type_to_group.get(&dep) {
            by_group.entry(group_idx).or_default().push(dep);
        }
    }

    // Build import statements.
    let mut imports: Vec<ImportStatement> = Vec::new();
    for (group_idx, mut type_names) in by_group {
        type_names.sort();
        let primary = &plan.groups[group_idx].primary_type;
        let filename = type_name_to_filename(primary, naming);
        imports.push(ImportStatement {
            type_names,
            from_path: format!("./{}{}", filename, suffix),
        });
    }

    imports
}

/// Formats import statements into TypeScript import lines.
pub fn format_imports(imports: &[ImportStatement]) -> String {
    imports
        .iter()
        .map(|imp| {
            format!(
                "import type {{ {} }} from '{}';",
                imp.type_names.join(", "),
                imp.from_path
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Generates a barrel file (`index.ts`) content that re-exports from all groups.
pub fn generate_barrel_file(
    plan: &FileOutputPlan,
    naming: FileNamingConvention,
    file_extension: &str,
) -> String {
    let suffix = import_specifier_suffix(file_extension);
    let mut lines: Vec<String> = Vec::new();
    for group in &plan.groups {
        let filename = type_name_to_filename(&group.primary_type, naming);
        lines.push(format!("export * from \"./{}{}\";", filename, suffix));
    }
    lines.join("\n")
}

/// Returns the barrel filename for the given extension (e.g. `index.ts` or `index.svelte.ts`).
pub fn barrel_filename(file_extension: &str) -> String {
    format!("index{}", file_extension)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FieldType, StructField};
    use crate::typesync::file_grouping::compute_file_grouping;

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
        }
    }

    #[test]
    fn test_type_name_to_filename() {
        assert_eq!(
            type_name_to_filename("UserProfile", FileNamingConvention::Kebab),
            "user-profile"
        );
        assert_eq!(
            type_name_to_filename("UserProfile", FileNamingConvention::Snake),
            "user_profile"
        );
        assert_eq!(
            type_name_to_filename("UserProfile", FileNamingConvention::Pascal),
            "UserProfile"
        );
        assert_eq!(
            type_name_to_filename("UserProfile", FileNamingConvention::Camel),
            "userProfile"
        );
    }

    #[test]
    fn test_resolve_imports_cross_file() {
        // User uses Role (shared), Address is exclusive to User.
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
        let user_group_idx = plan.type_to_group["User"];
        let user_group = &plan.groups[user_group_idx];

        let imports = resolve_imports(
            user_group,
            &plan,
            &structs,
            &enums,
            FileNamingConvention::Kebab,
            ".ts",
        );

        // User group (User + Address) should import Role from ./role
        assert_eq!(imports.len(), 1);
        assert!(imports[0].type_names.contains(&"Role".to_string()));
        assert_eq!(imports[0].from_path, "./role");
    }

    #[test]
    fn test_format_imports() {
        let imports = vec![
            ImportStatement {
                type_names: vec!["Role".to_string()],
                from_path: "./role".to_string(),
            },
            ImportStatement {
                type_names: vec!["Address".to_string(), "City".to_string()],
                from_path: "./address".to_string(),
            },
        ];

        let formatted = format_imports(&imports);
        assert!(formatted.contains("import type { Role } from './role';"));
        assert!(formatted.contains("import type { Address, City } from './address';"));
    }

    #[test]
    fn test_barrel_file() {
        let plan = FileOutputPlan {
            groups: vec![
                TypeFileGroup {
                    primary_type: "User".to_string(),
                    co_located_types: vec!["Address".to_string()],
                },
                TypeFileGroup {
                    primary_type: "Post".to_string(),
                    co_located_types: vec![],
                },
                TypeFileGroup {
                    primary_type: "Role".to_string(),
                    co_located_types: vec![],
                },
            ],
            type_to_group: HashMap::new(),
        };

        let barrel = generate_barrel_file(&plan, FileNamingConvention::Kebab, ".ts");
        assert!(barrel.contains("export * from \"./user\";"));
        assert!(barrel.contains("export * from \"./post\";"));
        assert!(barrel.contains("export * from \"./role\";"));
    }
}
