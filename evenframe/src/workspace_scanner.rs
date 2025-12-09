use evenframe_core::error::{EvenframeError, Result};
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use syn::{Attribute, Item, Meta, parse_file};
use tracing::{debug, info, trace, warn};
use walkdir::WalkDir;

#[allow(unused)]
#[derive(Debug, Clone)]
pub struct EvenframeType {
    pub name: String,
    pub module_path: String,
    pub file_path: String,
    pub kind: TypeKind,
    pub has_id_field: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeKind {
    Struct,
    Enum,
}

pub struct WorkspaceScanner {
    start_path: PathBuf,
    apply_aliases: Vec<String>,
}

impl WorkspaceScanner {
    /// Creates a new WorkspaceScanner that starts scanning from the current directory.
    ///
    /// # Arguments
    ///
    /// * `apply_aliases` - A list of attribute aliases to look for.
    pub fn new(apply_aliases: Vec<String>) -> Result<Self> {
        let start_path = env::current_dir()?;
        Ok(Self::with_path(start_path, apply_aliases))
    }

    /// Creates a new WorkspaceScanner with a specific start path.
    ///
    /// # Arguments
    ///
    /// * `start_path` - The directory to start scanning from. It will search this
    ///   directory and its children for Rust workspaces or standalone crates.
    /// * `apply_aliases` - A list of attribute aliases to look for.
    pub fn with_path(start_path: PathBuf, apply_aliases: Vec<String>) -> Self {
        Self {
            start_path,
            apply_aliases,
        }
    }

    /// Scans for Rust workspaces and collects all Evenframe types within them.
    pub fn scan_for_evenframe_types(&self) -> Result<Vec<EvenframeType>> {
        info!(
            "Starting workspace scan for Evenframe types from path: {:?}",
            self.start_path
        );

        let mut types = Vec::new();
        let mut processed_manifests = HashSet::new();

        // Use WalkDir to efficiently find all Cargo.toml files.
        for entry in WalkDir::new(&self.start_path)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name() == "Cargo.toml")
        {
            let manifest_path = entry.path();
            // Avoid processing the same manifest multiple times.
            if processed_manifests.contains(manifest_path) {
                continue;
            }

            trace!("Found potential manifest: {:?}", manifest_path);
            if let Err(e) =
                self.process_manifest(manifest_path, &mut types, &mut processed_manifests)
            {
                warn!("Failed to process manifest at {:?}: {}", manifest_path, e);
            }
        }

        info!(
            "Workspace scan complete. Found {} Evenframe types",
            types.len()
        );
        debug!(
            "Type breakdown: {} structs, {} enums",
            types.iter().filter(|t| t.kind == TypeKind::Struct).count(),
            types.iter().filter(|t| t.kind == TypeKind::Enum).count()
        );

        Ok(types)
    }

    /// Processes a Cargo.toml file, determines if it's a workspace or a single
    /// crate, and scans the corresponding source files.
    fn process_manifest(
        &self,
        manifest_path: &Path,
        types: &mut Vec<EvenframeType>,
        processed_manifests: &mut HashSet<PathBuf>,
    ) -> Result<()> {
        let manifest_dir = manifest_path
            .parent()
            .ok_or_else(|| EvenframeError::InvalidPath {
                path: manifest_path.to_path_buf(),
            })?;

        let content = fs::read_to_string(manifest_path)?;
        let manifest: toml::Value = toml::from_str(&content)
            .map_err(|e| EvenframeError::parse_error(manifest_path, e.to_string()))?;

        // Check if this is a workspace manifest and scan its members.
        if let Some(workspace) = manifest.get("workspace").and_then(|w| w.as_table()) {
            if let Some(members) = workspace.get("members").and_then(|m| m.as_array()) {
                debug!("Processing workspace at: {:?}", manifest_dir);
                processed_manifests.insert(manifest_path.to_path_buf());

                for member in members.iter().filter_map(|v| v.as_str()) {
                    // Note: For a full implementation, you might use the `glob` crate
                    // to handle patterns like "crates/*". This example handles direct paths.
                    let member_path = manifest_dir.join(member);
                    if member_path.is_dir() {
                        let crate_name = member_path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown_crate");
                        let src_path = member_path.join("src");
                        if src_path.exists() {
                            info!(
                                "Scanning workspace member: {} at {:?}",
                                crate_name, src_path
                            );
                            self.scan_directory(&src_path, types, crate_name, 0)?;
                        } else {
                            warn!(
                                "Workspace member '{}' does not have a 'src' directory.",
                                member
                            );
                        }
                    } else {
                        warn!(
                            "Workspace member path '{}' is not a directory or does not exist.",
                            member
                        );
                    }
                }
            }
        }

        // Also check if this manifest has a [package] section (handles both standalone crates
        // and the case where a crate has an empty [workspace] to exclude from parent workspace).
        if manifest.get("package").is_some() {
            debug!("Processing package at: {:?}", manifest_dir);
            processed_manifests.insert(manifest_path.to_path_buf());
            let crate_name = manifest
                .get("package")
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or_else(|| {
                    manifest_dir
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown_crate")
                });

            let src_path = manifest_dir.join("src");
            if src_path.exists() {
                info!("Scanning crate: {} at {:?}", crate_name, src_path);
                self.scan_directory(&src_path, types, crate_name, 0)?;
            }
        }

        Ok(())
    }

    /// Recursively scans a directory for Rust source files.
    fn scan_directory(
        &self,
        dir: &Path,
        types: &mut Vec<EvenframeType>,
        base_module: &str,
        depth: usize,
    ) -> Result<()> {
        trace!(
            "Scanning directory: {:?}, module: {}, depth: {}",
            dir, base_module, depth
        );

        if depth > 10 {
            return Err(EvenframeError::MaxRecursionDepth {
                depth: 10,
                path: dir.to_path_buf(),
            });
        }

        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.symlink_metadata()?.file_type().is_symlink() {
                debug!("Skipping symlink: {:?}", path);
                continue;
            }

            if path.is_dir() {
                let dir_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if dir_name != "tests" && dir_name != "benches" {
                    let module_path = format!("{}::{}", base_module, dir_name);
                    self.scan_directory(&path, types, &module_path, depth + 1)?;
                }
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                let file_stem = path.file_stem().and_then(|n| n.to_str()).unwrap_or("");

                // FIX: Correctly handle `mod.rs` files.
                if file_stem == "lib" || file_stem == "main" {
                    // Crate root, use the base module path directly.
                    self.scan_rust_file(&path, types, base_module)?;
                } else if path.file_name().and_then(|n| n.to_str()) == Some("mod.rs") {
                    // A `mod.rs` file defines the module for its parent directory.
                    // The `base_module` path is already correct for this case.
                    self.scan_rust_file(&path, types, base_module)?;
                } else {
                    // A regular submodule file (e.g., `user.rs`).
                    let module_path = format!("{}::{}", base_module, file_stem);
                    self.scan_rust_file(&path, types, &module_path)?;
                }
            }
        }
        Ok(())
    }

    /// Parses a single Rust file to find relevant structs and enums.
    fn scan_rust_file(
        &self,
        path: &Path,
        types: &mut Vec<EvenframeType>,
        module_path: &str,
    ) -> Result<()> {
        trace!("Scanning file: {:?}, module: {}", path, module_path);
        let content = fs::read_to_string(path)?;
        let syntax_tree =
            parse_file(&content).map_err(|e| EvenframeError::parse_error(path, e.to_string()))?;

        for item in syntax_tree.items {
            let (attrs, ident, kind, fields) = match item {
                Item::Struct(s) => (s.attrs, s.ident, TypeKind::Struct, Some(s.fields)),
                Item::Enum(e) => (e.attrs, e.ident, TypeKind::Enum, None),
                _ => continue,
            };

            if has_evenframe_derive(&attrs) || self.has_apply_alias(&attrs) {
                let name = ident.to_string();
                let has_id_field = fields.is_some_and(|f| has_id_field(&f));

                debug!(
                    "Found Evenframe {:?} '{}' in module '{}'",
                    kind, name, module_path
                );

                types.push(EvenframeType {
                    name,
                    module_path: module_path.to_string(),
                    file_path: path.to_string_lossy().to_string(),
                    kind,
                    has_id_field,
                });
            }
        }
        Ok(())
    }

    /// Checks for `#[apply(Alias)]` attributes.
    fn has_apply_alias(&self, attrs: &[Attribute]) -> bool {
        self.apply_aliases.iter().any(|alias| {
            attrs.iter().any(|attr| {
                if attr.path().is_ident("apply")
                    && let Meta::List(meta_list) = &attr.meta
                {
                    return meta_list.tokens.to_string() == *alias;
                }

                false
            })
        })
    }
}

/// Checks for `#[derive(..., Evenframe, ...)]`.
fn has_evenframe_derive(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if attr.path().is_ident("derive")
            && let Meta::List(meta_list) = &attr.meta
        {
            return meta_list.tokens.to_string().contains("Evenframe");
        }

        false
    })
}

/// Checks if a struct has a field named `id`.
fn has_id_field(fields: &syn::Fields) -> bool {
    if let syn::Fields::Named(fields_named) = fields {
        fields_named
            .named
            .iter()
            .any(|field| field.ident.as_ref().is_some_and(|id| id == "id"))
    } else {
        false
    }
}

/// Helper function to extract unique module paths from the found types.
pub fn _get_unique_modules(types: &[EvenframeType]) -> Vec<String> {
    let mut modules: HashSet<_> = types.iter().map(|t| t.module_path.clone()).collect();
    let unique_modules: Vec<String> = modules.drain().collect();
    debug!(
        "Found {} unique modules from {} types",
        unique_modules.len(),
        types.len()
    );
    trace!("Unique modules: {:?}", unique_modules);
    unique_modules
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;
    use tempfile::TempDir;

    // ==================== TypeKind Tests ====================

    #[test]
    fn test_type_kind_equality() {
        assert_eq!(TypeKind::Struct, TypeKind::Struct);
        assert_eq!(TypeKind::Enum, TypeKind::Enum);
        assert_ne!(TypeKind::Struct, TypeKind::Enum);
    }

    #[test]
    fn test_type_kind_debug() {
        assert_eq!(format!("{:?}", TypeKind::Struct), "Struct");
        assert_eq!(format!("{:?}", TypeKind::Enum), "Enum");
    }

    #[test]
    fn test_type_kind_clone() {
        let kind = TypeKind::Struct;
        let cloned = kind.clone();
        assert_eq!(kind, cloned);
    }

    // ==================== EvenframeType Tests ====================

    #[test]
    fn test_evenframe_type_creation() {
        let ef_type = EvenframeType {
            name: "User".to_string(),
            module_path: "my_crate::models".to_string(),
            file_path: "/path/to/file.rs".to_string(),
            kind: TypeKind::Struct,
            has_id_field: true,
        };

        assert_eq!(ef_type.name, "User");
        assert_eq!(ef_type.module_path, "my_crate::models");
        assert_eq!(ef_type.file_path, "/path/to/file.rs");
        assert_eq!(ef_type.kind, TypeKind::Struct);
        assert!(ef_type.has_id_field);
    }

    #[test]
    fn test_evenframe_type_clone() {
        let ef_type = EvenframeType {
            name: "Order".to_string(),
            module_path: "crate::orders".to_string(),
            file_path: "/orders.rs".to_string(),
            kind: TypeKind::Struct,
            has_id_field: false,
        };

        let cloned = ef_type.clone();
        assert_eq!(ef_type.name, cloned.name);
        assert_eq!(ef_type.module_path, cloned.module_path);
        assert_eq!(ef_type.file_path, cloned.file_path);
        assert_eq!(ef_type.kind, cloned.kind);
        assert_eq!(ef_type.has_id_field, cloned.has_id_field);
    }

    #[test]
    fn test_evenframe_type_debug() {
        let ef_type = EvenframeType {
            name: "Test".to_string(),
            module_path: "crate".to_string(),
            file_path: "/test.rs".to_string(),
            kind: TypeKind::Enum,
            has_id_field: false,
        };

        let debug_str = format!("{:?}", ef_type);
        assert!(debug_str.contains("Test"));
        assert!(debug_str.contains("Enum"));
    }

    // ==================== WorkspaceScanner Tests ====================

    #[test]
    fn test_workspace_scanner_with_path() {
        let path = PathBuf::from("/some/path");
        let aliases = vec!["MyMacro".to_string()];
        let scanner = WorkspaceScanner::with_path(path.clone(), aliases.clone());

        assert_eq!(scanner.start_path, path);
        assert_eq!(scanner.apply_aliases, aliases);
    }

    #[test]
    fn test_workspace_scanner_with_empty_aliases() {
        let path = PathBuf::from("/test");
        let scanner = WorkspaceScanner::with_path(path, vec![]);

        assert!(scanner.apply_aliases.is_empty());
    }

    #[test]
    fn test_workspace_scanner_with_multiple_aliases() {
        let path = PathBuf::from("/test");
        let aliases = vec![
            "Macro1".to_string(),
            "Macro2".to_string(),
            "Macro3".to_string(),
        ];
        let scanner = WorkspaceScanner::with_path(path, aliases.clone());

        assert_eq!(scanner.apply_aliases.len(), 3);
        assert!(scanner.apply_aliases.contains(&"Macro1".to_string()));
        assert!(scanner.apply_aliases.contains(&"Macro2".to_string()));
        assert!(scanner.apply_aliases.contains(&"Macro3".to_string()));
    }

    // ==================== has_evenframe_derive Tests ====================

    #[test]
    fn test_has_evenframe_derive_with_derive_evenframe() {
        let code = r#"
            #[derive(Debug, Clone, Evenframe)]
            struct TestStruct {
                id: String,
            }
        "#;

        let file = syn::parse_file(code).unwrap();
        if let syn::Item::Struct(s) = &file.items[0] {
            assert!(has_evenframe_derive(&s.attrs));
        }
    }

    #[test]
    fn test_has_evenframe_derive_without_evenframe() {
        let code = r#"
            #[derive(Debug, Clone)]
            struct TestStruct {
                id: String,
            }
        "#;

        let file = syn::parse_file(code).unwrap();
        if let syn::Item::Struct(s) = &file.items[0] {
            assert!(!has_evenframe_derive(&s.attrs));
        }
    }

    #[test]
    fn test_has_evenframe_derive_with_no_derive_attr() {
        let code = r#"
            struct TestStruct {
                id: String,
            }
        "#;

        let file = syn::parse_file(code).unwrap();
        if let syn::Item::Struct(s) = &file.items[0] {
            assert!(!has_evenframe_derive(&s.attrs));
        }
    }

    #[test]
    fn test_has_evenframe_derive_only_evenframe() {
        let code = r#"
            #[derive(Evenframe)]
            struct TestStruct {
                id: String,
            }
        "#;

        let file = syn::parse_file(code).unwrap();
        if let syn::Item::Struct(s) = &file.items[0] {
            assert!(has_evenframe_derive(&s.attrs));
        }
    }

    // ==================== has_id_field Tests ====================

    #[test]
    fn test_has_id_field_with_id() {
        let code = r#"
            struct TestStruct {
                id: String,
                name: String,
            }
        "#;

        let file = syn::parse_file(code).unwrap();
        if let syn::Item::Struct(s) = &file.items[0] {
            assert!(has_id_field(&s.fields));
        }
    }

    #[test]
    fn test_has_id_field_without_id() {
        let code = r#"
            struct TestStruct {
                name: String,
                age: i32,
            }
        "#;

        let file = syn::parse_file(code).unwrap();
        if let syn::Item::Struct(s) = &file.items[0] {
            assert!(!has_id_field(&s.fields));
        }
    }

    #[test]
    fn test_has_id_field_tuple_struct() {
        let code = r#"
            struct TestStruct(String, i32);
        "#;

        let file = syn::parse_file(code).unwrap();
        if let syn::Item::Struct(s) = &file.items[0] {
            // Tuple structs have unnamed fields, should return false
            assert!(!has_id_field(&s.fields));
        }
    }

    #[test]
    fn test_has_id_field_unit_struct() {
        let code = r#"
            struct TestStruct;
        "#;

        let file = syn::parse_file(code).unwrap();
        if let syn::Item::Struct(s) = &file.items[0] {
            assert!(!has_id_field(&s.fields));
        }
    }

    #[test]
    fn test_has_id_field_only_id() {
        let code = r#"
            struct TestStruct {
                id: i64,
            }
        "#;

        let file = syn::parse_file(code).unwrap();
        if let syn::Item::Struct(s) = &file.items[0] {
            assert!(has_id_field(&s.fields));
        }
    }

    // ==================== _get_unique_modules Tests ====================

    #[test]
    fn test_get_unique_modules_empty() {
        let types: Vec<EvenframeType> = vec![];
        let modules = _get_unique_modules(&types);
        assert!(modules.is_empty());
    }

    #[test]
    fn test_get_unique_modules_single() {
        let types = vec![EvenframeType {
            name: "User".to_string(),
            module_path: "crate::models".to_string(),
            file_path: "/path.rs".to_string(),
            kind: TypeKind::Struct,
            has_id_field: true,
        }];

        let modules = _get_unique_modules(&types);
        assert_eq!(modules.len(), 1);
        assert!(modules.contains(&"crate::models".to_string()));
    }

    #[test]
    fn test_get_unique_modules_duplicates() {
        let types = vec![
            EvenframeType {
                name: "User".to_string(),
                module_path: "crate::models".to_string(),
                file_path: "/path1.rs".to_string(),
                kind: TypeKind::Struct,
                has_id_field: true,
            },
            EvenframeType {
                name: "Order".to_string(),
                module_path: "crate::models".to_string(),
                file_path: "/path2.rs".to_string(),
                kind: TypeKind::Struct,
                has_id_field: true,
            },
        ];

        let modules = _get_unique_modules(&types);
        assert_eq!(modules.len(), 1);
        assert!(modules.contains(&"crate::models".to_string()));
    }

    #[test]
    fn test_get_unique_modules_different_modules() {
        let types = vec![
            EvenframeType {
                name: "User".to_string(),
                module_path: "crate::models::user".to_string(),
                file_path: "/path1.rs".to_string(),
                kind: TypeKind::Struct,
                has_id_field: true,
            },
            EvenframeType {
                name: "Order".to_string(),
                module_path: "crate::models::order".to_string(),
                file_path: "/path2.rs".to_string(),
                kind: TypeKind::Struct,
                has_id_field: true,
            },
            EvenframeType {
                name: "Status".to_string(),
                module_path: "crate::enums".to_string(),
                file_path: "/path3.rs".to_string(),
                kind: TypeKind::Enum,
                has_id_field: false,
            },
        ];

        let modules = _get_unique_modules(&types);
        assert_eq!(modules.len(), 3);
    }

    // ==================== Filesystem-Based Tests ====================

    fn create_rust_file(dir: &Path, filename: &str, content: &str) -> std::io::Result<()> {
        let file_path = dir.join(filename);
        let mut file = File::create(file_path)?;
        file.write_all(content.as_bytes())?;
        Ok(())
    }

    #[test]
    fn test_scan_rust_file_finds_evenframe_struct() {
        let temp_dir = TempDir::new().unwrap();
        let content = r#"
            #[derive(Debug, Clone, Evenframe)]
            pub struct User {
                pub id: String,
                pub name: String,
            }
        "#;

        create_rust_file(temp_dir.path(), "user.rs", content).unwrap();

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![]);
        let mut types = Vec::new();

        scanner
            .scan_rust_file(
                &temp_dir.path().join("user.rs"),
                &mut types,
                "test_crate::models",
            )
            .unwrap();

        assert_eq!(types.len(), 1);
        assert_eq!(types[0].name, "User");
        assert_eq!(types[0].module_path, "test_crate::models");
        assert_eq!(types[0].kind, TypeKind::Struct);
        assert!(types[0].has_id_field);
    }

    #[test]
    fn test_scan_rust_file_finds_evenframe_enum() {
        let temp_dir = TempDir::new().unwrap();
        let content = r#"
            #[derive(Debug, Clone, Evenframe)]
            pub enum Status {
                Active,
                Inactive,
                Pending,
            }
        "#;

        create_rust_file(temp_dir.path(), "status.rs", content).unwrap();

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![]);
        let mut types = Vec::new();

        scanner
            .scan_rust_file(
                &temp_dir.path().join("status.rs"),
                &mut types,
                "test_crate::enums",
            )
            .unwrap();

        assert_eq!(types.len(), 1);
        assert_eq!(types[0].name, "Status");
        assert_eq!(types[0].module_path, "test_crate::enums");
        assert_eq!(types[0].kind, TypeKind::Enum);
        assert!(!types[0].has_id_field);
    }

    #[test]
    fn test_scan_rust_file_ignores_non_evenframe_types() {
        let temp_dir = TempDir::new().unwrap();
        let content = r#"
            #[derive(Debug, Clone)]
            pub struct RegularStruct {
                pub name: String,
            }

            pub enum RegularEnum {
                A,
                B,
            }
        "#;

        create_rust_file(temp_dir.path(), "regular.rs", content).unwrap();

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![]);
        let mut types = Vec::new();

        scanner
            .scan_rust_file(
                &temp_dir.path().join("regular.rs"),
                &mut types,
                "test_crate",
            )
            .unwrap();

        assert!(types.is_empty());
    }

    #[test]
    fn test_scan_rust_file_finds_multiple_types() {
        let temp_dir = TempDir::new().unwrap();
        let content = r#"
            #[derive(Evenframe)]
            pub struct User {
                pub id: String,
                pub name: String,
            }

            #[derive(Evenframe)]
            pub struct Order {
                pub id: String,
                pub total: f64,
            }

            #[derive(Evenframe)]
            pub enum Status {
                Active,
                Inactive,
            }
        "#;

        create_rust_file(temp_dir.path(), "models.rs", content).unwrap();

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![]);
        let mut types = Vec::new();

        scanner
            .scan_rust_file(
                &temp_dir.path().join("models.rs"),
                &mut types,
                "test_crate::models",
            )
            .unwrap();

        assert_eq!(types.len(), 3);

        let names: Vec<_> = types.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"User"));
        assert!(names.contains(&"Order"));
        assert!(names.contains(&"Status"));
    }

    #[test]
    fn test_scan_rust_file_with_apply_alias() {
        let temp_dir = TempDir::new().unwrap();
        let content = r#"
            #[apply(MyMacro)]
            pub struct User {
                pub id: String,
                pub name: String,
            }
        "#;

        create_rust_file(temp_dir.path(), "user.rs", content).unwrap();

        let scanner = WorkspaceScanner::with_path(
            temp_dir.path().to_path_buf(),
            vec!["MyMacro".to_string()],
        );
        let mut types = Vec::new();

        scanner
            .scan_rust_file(
                &temp_dir.path().join("user.rs"),
                &mut types,
                "test_crate::models",
            )
            .unwrap();

        assert_eq!(types.len(), 1);
        assert_eq!(types[0].name, "User");
    }

    #[test]
    fn test_scan_rust_file_without_matching_apply_alias() {
        let temp_dir = TempDir::new().unwrap();
        let content = r#"
            #[apply(OtherMacro)]
            pub struct User {
                pub id: String,
                pub name: String,
            }
        "#;

        create_rust_file(temp_dir.path(), "user.rs", content).unwrap();

        let scanner = WorkspaceScanner::with_path(
            temp_dir.path().to_path_buf(),
            vec!["MyMacro".to_string()],
        );
        let mut types = Vec::new();

        scanner
            .scan_rust_file(
                &temp_dir.path().join("user.rs"),
                &mut types,
                "test_crate::models",
            )
            .unwrap();

        assert!(types.is_empty());
    }

    #[test]
    fn test_scan_directory_with_src_layout() {
        let temp_dir = TempDir::new().unwrap();
        let src_dir = temp_dir.path().join("src");
        fs::create_dir(&src_dir).unwrap();

        let content = r#"
            #[derive(Evenframe)]
            pub struct User {
                pub id: String,
            }
        "#;

        create_rust_file(&src_dir, "lib.rs", content).unwrap();

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![]);
        let mut types = Vec::new();

        scanner.scan_directory(&src_dir, &mut types, "test_crate", 0).unwrap();

        assert_eq!(types.len(), 1);
        assert_eq!(types[0].name, "User");
        assert_eq!(types[0].module_path, "test_crate");
    }

    #[test]
    fn test_scan_directory_skips_tests_directory() {
        let temp_dir = TempDir::new().unwrap();
        let src_dir = temp_dir.path().join("src");
        let tests_dir = src_dir.join("tests");

        fs::create_dir_all(&tests_dir).unwrap();

        let main_content = r#"
            #[derive(Evenframe)]
            pub struct User {
                pub id: String,
            }
        "#;

        let test_content = r#"
            #[derive(Evenframe)]
            pub struct TestType {
                pub id: String,
            }
        "#;

        create_rust_file(&src_dir, "lib.rs", main_content).unwrap();
        create_rust_file(&tests_dir, "test.rs", test_content).unwrap();

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![]);
        let mut types = Vec::new();

        scanner.scan_directory(&src_dir, &mut types, "test_crate", 0).unwrap();

        // Should only find the main type, not the test type
        assert_eq!(types.len(), 1);
        assert_eq!(types[0].name, "User");
    }

    #[test]
    fn test_scan_directory_skips_benches_directory() {
        let temp_dir = TempDir::new().unwrap();
        let src_dir = temp_dir.path().join("src");
        let benches_dir = src_dir.join("benches");

        fs::create_dir_all(&benches_dir).unwrap();

        let main_content = r#"
            #[derive(Evenframe)]
            pub struct User {
                pub id: String,
            }
        "#;

        let bench_content = r#"
            #[derive(Evenframe)]
            pub struct BenchType {
                pub id: String,
            }
        "#;

        create_rust_file(&src_dir, "lib.rs", main_content).unwrap();
        create_rust_file(&benches_dir, "bench.rs", bench_content).unwrap();

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![]);
        let mut types = Vec::new();

        scanner.scan_directory(&src_dir, &mut types, "test_crate", 0).unwrap();

        // Should only find the main type, not the bench type
        assert_eq!(types.len(), 1);
        assert_eq!(types[0].name, "User");
    }

    #[test]
    fn test_scan_directory_max_recursion_depth() {
        let temp_dir = TempDir::new().unwrap();
        let mut current_dir = temp_dir.path().to_path_buf();

        // Create a deeply nested directory structure
        for i in 0..12 {
            current_dir = current_dir.join(format!("level_{}", i));
            fs::create_dir(&current_dir).unwrap();
        }

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![]);
        let mut types = Vec::new();

        let result = scanner.scan_directory(temp_dir.path(), &mut types, "test_crate", 0);

        // Should hit max recursion depth and return an error
        assert!(result.is_err());
        if let Err(EvenframeError::MaxRecursionDepth { depth, .. }) = result {
            assert_eq!(depth, 10);
        } else {
            panic!("Expected MaxRecursionDepth error");
        }
    }

    #[test]
    fn test_scan_directory_handles_mod_rs() {
        let temp_dir = TempDir::new().unwrap();
        let src_dir = temp_dir.path().join("src");
        let models_dir = src_dir.join("models");

        fs::create_dir_all(&models_dir).unwrap();

        let mod_content = r#"
            #[derive(Evenframe)]
            pub struct ModUser {
                pub id: String,
            }
        "#;

        create_rust_file(&models_dir, "mod.rs", mod_content).unwrap();

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![]);
        let mut types = Vec::new();

        scanner.scan_directory(&src_dir, &mut types, "test_crate", 0).unwrap();

        assert_eq!(types.len(), 1);
        assert_eq!(types[0].name, "ModUser");
        // mod.rs should use the parent directory's module path
        assert_eq!(types[0].module_path, "test_crate::models");
    }

    #[test]
    fn test_scan_directory_handles_submodule_files() {
        let temp_dir = TempDir::new().unwrap();
        let src_dir = temp_dir.path().join("src");

        fs::create_dir(&src_dir).unwrap();

        let user_content = r#"
            #[derive(Evenframe)]
            pub struct User {
                pub id: String,
            }
        "#;

        let order_content = r#"
            #[derive(Evenframe)]
            pub struct Order {
                pub id: String,
            }
        "#;

        create_rust_file(&src_dir, "lib.rs", "").unwrap();
        create_rust_file(&src_dir, "user.rs", user_content).unwrap();
        create_rust_file(&src_dir, "order.rs", order_content).unwrap();

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![]);
        let mut types = Vec::new();

        scanner.scan_directory(&src_dir, &mut types, "test_crate", 0).unwrap();

        assert_eq!(types.len(), 2);

        // Check module paths are correct for submodule files
        let user_type = types.iter().find(|t| t.name == "User").unwrap();
        let order_type = types.iter().find(|t| t.name == "Order").unwrap();

        assert_eq!(user_type.module_path, "test_crate::user");
        assert_eq!(order_type.module_path, "test_crate::order");
    }

    #[test]
    fn test_process_manifest_single_crate() {
        let temp_dir = TempDir::new().unwrap();
        let src_dir = temp_dir.path().join("src");
        fs::create_dir(&src_dir).unwrap();

        let cargo_toml = r#"
            [package]
            name = "my_crate"
            version = "0.1.0"
            edition = "2021"
        "#;

        let lib_content = r#"
            #[derive(Evenframe)]
            pub struct User {
                pub id: String,
            }
        "#;

        create_rust_file(temp_dir.path(), "Cargo.toml", cargo_toml).unwrap();
        create_rust_file(&src_dir, "lib.rs", lib_content).unwrap();

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![]);
        let mut types = Vec::new();
        let mut processed = HashSet::new();

        scanner
            .process_manifest(
                &temp_dir.path().join("Cargo.toml"),
                &mut types,
                &mut processed,
            )
            .unwrap();

        assert_eq!(types.len(), 1);
        assert_eq!(types[0].name, "User");
        assert!(processed.contains(&temp_dir.path().join("Cargo.toml")));
    }

    #[test]
    fn test_scan_for_evenframe_types_empty_directory() {
        let temp_dir = TempDir::new().unwrap();
        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![]);

        let types = scanner.scan_for_evenframe_types().unwrap();

        assert!(types.is_empty());
    }

    #[test]
    fn test_scan_rust_file_struct_without_id() {
        let temp_dir = TempDir::new().unwrap();
        let content = r#"
            #[derive(Evenframe)]
            pub struct Address {
                pub street: String,
                pub city: String,
            }
        "#;

        create_rust_file(temp_dir.path(), "address.rs", content).unwrap();

        let scanner = WorkspaceScanner::with_path(temp_dir.path().to_path_buf(), vec![]);
        let mut types = Vec::new();

        scanner
            .scan_rust_file(
                &temp_dir.path().join("address.rs"),
                &mut types,
                "test_crate",
            )
            .unwrap();

        assert_eq!(types.len(), 1);
        assert_eq!(types[0].name, "Address");
        assert!(!types[0].has_id_field);
    }
}
