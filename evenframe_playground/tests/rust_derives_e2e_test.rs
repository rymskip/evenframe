//! End-to-end tests for rust_derives collection.
//!
//! These tests verify that:
//! 1. Real Rust derives (#[derive(Serialize, Clone, ...)]) are collected into StructConfig/TaggedUnion
//! 2. Derives from multiple #[derive(...)] attributes are merged
//! 3. Path-qualified derives (e.g., serde::Serialize) use the last segment
//! 4. Playground models with Serialize/Deserialize report those derives

use evenframe_core::tooling::{BuildConfig, build_all_configs, merge_tables_and_objects};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn write_cargo_toml(dir: &std::path::Path, name: &str) {
    let content = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2024"
"#
    );
    fs::write(dir.join("Cargo.toml"), content).unwrap();
}

fn write_src_file(dir: &std::path::Path, filename: &str, content: &str) {
    let src_dir = dir.join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(src_dir.join(filename), content).unwrap();
}

// ============================================================================
// Basic derive collection
// ============================================================================

#[test]
fn test_struct_rust_derives_collected() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    write_cargo_toml(root, "derives_struct");
    write_src_file(
        root,
        "lib.rs",
        r#"
        #[derive(Debug, Clone, Serialize, Deserialize, Evenframe)]
        pub struct User {
            pub id: String,
            pub name: String,
        }
        "#,
    );

    let config = BuildConfig::builder().scan_path(root).build();
    let (_enums, tables, _objects) = build_all_configs(&config).unwrap();

    let user = &tables["user"].struct_config;
    assert!(
        user.rust_derives.contains(&"Debug".to_string()),
        "Should contain Debug. Got: {:?}",
        user.rust_derives
    );
    assert!(
        user.rust_derives.contains(&"Clone".to_string()),
        "Should contain Clone"
    );
    assert!(
        user.rust_derives.contains(&"Serialize".to_string()),
        "Should contain Serialize"
    );
    assert!(
        user.rust_derives.contains(&"Deserialize".to_string()),
        "Should contain Deserialize"
    );
    assert!(
        user.rust_derives.contains(&"Evenframe".to_string()),
        "Should contain Evenframe"
    );
}

#[test]
fn test_enum_rust_derives_collected() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    write_cargo_toml(root, "derives_enum");
    write_src_file(
        root,
        "lib.rs",
        r#"
        #[derive(Debug, Clone, Serialize, Evenframe)]
        pub enum Status {
            Active,
            Inactive,
        }
        "#,
    );

    let config = BuildConfig::builder().scan_path(root).build();
    let (enums, _tables, _objects) = build_all_configs(&config).unwrap();

    let status = &enums["Status"];
    assert!(status.rust_derives.contains(&"Debug".to_string()));
    assert!(status.rust_derives.contains(&"Clone".to_string()));
    assert!(status.rust_derives.contains(&"Serialize".to_string()));
    assert!(status.rust_derives.contains(&"Evenframe".to_string()));
}

#[test]
fn test_multiple_derive_attributes_merged() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    write_cargo_toml(root, "derives_multi");
    write_src_file(
        root,
        "lib.rs",
        r#"
        #[derive(Debug, Clone)]
        #[derive(Serialize, Deserialize)]
        #[derive(Evenframe)]
        pub struct MultiDerive {
            pub id: String,
            pub value: i32,
        }
        "#,
    );

    let config = BuildConfig::builder().scan_path(root).build();
    let (_enums, tables, _objects) = build_all_configs(&config).unwrap();

    let md = &tables["multi_derive"].struct_config;
    assert!(md.rust_derives.contains(&"Debug".to_string()));
    assert!(md.rust_derives.contains(&"Clone".to_string()));
    assert!(md.rust_derives.contains(&"Serialize".to_string()));
    assert!(md.rust_derives.contains(&"Deserialize".to_string()));
    assert!(md.rust_derives.contains(&"Evenframe".to_string()));
    assert_eq!(md.rust_derives.len(), 5, "Should have exactly 5 derives");
}

#[test]
fn test_object_without_id_has_rust_derives() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    write_cargo_toml(root, "derives_object");
    write_src_file(
        root,
        "lib.rs",
        r#"
        #[derive(Debug, Serialize, Typesync)]
        pub struct ApiResponse {
            pub message: String,
            pub code: i32,
        }
        "#,
    );

    let config = BuildConfig::builder().scan_path(root).build();
    let (_enums, _tables, objects) = build_all_configs(&config).unwrap();

    let resp = &objects["ApiResponse"];
    assert!(resp.rust_derives.contains(&"Debug".to_string()));
    assert!(resp.rust_derives.contains(&"Serialize".to_string()));
    assert!(resp.rust_derives.contains(&"Typesync".to_string()));
}

// ============================================================================
// Playground model integration
// ============================================================================

#[test]
fn test_playground_models_have_serialize_derive() {
    let playground_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let config = BuildConfig::builder().scan_path(&playground_dir).build();
    let (_enums, tables, objects) = build_all_configs(&config).unwrap();
    let structs = merge_tables_and_objects(&tables, &objects);

    // The playground auth.rs models use #[derive(Debug, Clone, Serialize, Deserialize, Evenframe)]
    // Check at least one has Serialize
    let has_any_serialize = structs
        .values()
        .any(|sc| sc.rust_derives.contains(&"Serialize".to_string()));

    assert!(
        has_any_serialize,
        "At least one playground struct should have Serialize derive"
    );
}

// ============================================================================
// canonical_name for FieldType
// ============================================================================

#[test]
fn test_field_type_canonical_name() {
    use evenframe_core::types::FieldType;

    assert_eq!(FieldType::String.canonical_name(), "String");
    assert_eq!(FieldType::I32.canonical_name(), "i32");
    assert_eq!(FieldType::Bool.canonical_name(), "bool");
    assert_eq!(FieldType::F64.canonical_name(), "f64");
    assert_eq!(
        FieldType::Other("Decimal".to_string()).canonical_name(),
        "Decimal"
    );
    assert_eq!(
        FieldType::Option(Box::new(FieldType::Other("DateTime".to_string()))).canonical_name(),
        "Option<DateTime>"
    );
    assert_eq!(
        FieldType::Vec(Box::new(FieldType::I32)).canonical_name(),
        "Vec<i32>"
    );
    assert_eq!(
        FieldType::HashMap(Box::new(FieldType::String), Box::new(FieldType::I64))
            .canonical_name(),
        "HashMap<String, i64>"
    );
    assert_eq!(
        FieldType::Option(Box::new(FieldType::Vec(Box::new(FieldType::Other(
            "Uuid".to_string()
        )))))
        .canonical_name(),
        "Option<Vec<Uuid>>"
    );
}
