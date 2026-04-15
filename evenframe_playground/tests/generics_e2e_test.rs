//! End-to-end tests for Generic types and enum instantiations.
//!
//! Verifies how Evenframe handles:
//! 1. Built-in generics (Option, Vec, etc.)
//! 2. Custom generic instantiations (MyEnum<i32>)
//! 3. Deriving Evenframe on generic types (expected to be limited/unsupported)

use evenframe_core::tooling::{BuildConfig, build_all_configs};
use evenframe_core::types::FieldType;
use std::fs;
use tempfile::TempDir;

// ============================================================================
// Helpers
// ============================================================================

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
// Tests
// ============================================================================

#[test]
fn test_built_in_generics_are_parsed_correctly() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    write_cargo_toml(root, "builtin_generics");
    write_src_file(
        root,
        "lib.rs",
        r#"
        #[derive(Typesync)]
        pub struct Container {
            pub opt: Option<String>,
            pub list: Vec<i32>,
            pub map: std::collections::HashMap<String, f64>,
        }
        "#,
    );

    let config = BuildConfig::builder().scan_path(root).build();
    let (_enums, _tables, objects) = build_all_configs(&config).unwrap();

    let container = &objects["Container"];
    
    // Option<String>
    let opt_field = container.fields.iter().find(|f| f.field_name == "opt").unwrap();
    assert!(matches!(opt_field.field_type, FieldType::Option(ref inner) if matches!(**inner, FieldType::String)));

    // Vec<i32>
    let list_field = container.fields.iter().find(|f| f.field_name == "list").unwrap();
    assert!(matches!(list_field.field_type, FieldType::Vec(ref inner) if matches!(**inner, FieldType::I32)));

    // HashMap<String, f64>
    let map_field = container.fields.iter().find(|f| f.field_name == "map").unwrap();
    assert!(matches!(map_field.field_type, FieldType::HashMap(ref k, ref v) 
        if matches!(**k, FieldType::String) && matches!(**v, FieldType::F64)));
}

#[test]
fn test_custom_generic_instantiations_are_collapsed() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    write_cargo_toml(root, "custom_generics");
    write_src_file(
        root,
        "lib.rs",
        r#"
        #[derive(Typesync)]
        pub struct Usage {
            pub int_enum: Result<i32, String>,
            pub custom: MyGeneric<f64>,
        }
        "#,
    );

    let config = BuildConfig::builder().scan_path(root).build();
    let (_enums, _tables, objects) = build_all_configs(&config).unwrap();

    let usage = &objects["Usage"];
    
    // Result<i32, String> -> collapsed to "Result"
    let result_field = usage.fields.iter().find(|f| f.field_name == "int_enum").unwrap();
    assert_eq!(result_field.field_type.canonical_name(), "Result");
    assert!(matches!(result_field.field_type, FieldType::Other(ref name) if name == "Result"));

    // MyGeneric<f64> -> collapsed to "MyGeneric"
    let custom_field = usage.fields.iter().find(|f| f.field_name == "custom").unwrap();
    assert_eq!(custom_field.field_type.canonical_name(), "MyGeneric");
    assert!(matches!(custom_field.field_type, FieldType::Other(ref name) if name == "MyGeneric"));
}

#[test]
fn test_scanner_with_generic_definition() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    write_cargo_toml(root, "generic_def");
    write_src_file(
        root,
        "lib.rs",
        r#"
        #[derive(Typesync)]
        pub enum MyGenericEnum<T> {
            Data(T),
            None,
        }

        #[derive(Typesync)]
        pub struct Wrapper {
            pub instantiated: MyGenericEnum<i32>,
        }
        "#,
    );

    let config = BuildConfig::builder().scan_path(root).build();
    let (enums, _tables, objects) = build_all_configs(&config).unwrap();

    // The scanner should still find the types, but the instantiation will be collapsed.
    let wrapper = &objects["Wrapper"];
    let instantiated_field = wrapper.fields.iter().find(|f| f.field_name == "instantiated").unwrap();
    
    // It should be collapsed to the base name "MyGenericEnum"
    assert_eq!(instantiated_field.field_type.canonical_name(), "MyGenericEnum");

    // The enum definition itself should be found
    assert!(enums.contains_key("MyGenericEnum"));
}
