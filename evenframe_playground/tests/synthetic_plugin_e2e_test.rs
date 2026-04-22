//! End-to-end tests for the synthetic-item WASM plugin type.
//!
//! Run with: cargo test --test synthetic_plugin_e2e_test --features wasm-plugins

#![cfg(feature = "wasm-plugins")]

use evenframe_core::config::SyntheticItemPluginConfig;
use evenframe_core::schemasync::table::TableConfig;
use evenframe_core::types::{FieldType, Pipeline, StructConfig, StructField, TaggedUnion, Variant};
use evenframe_core::typesync::synthetic_plugin::SyntheticItemPluginManager;
use evenframe_core::typesync::synthetic_plugin_types::SyntheticPluginInput;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

// ============================================================================
// WASM plugin build harness
// ============================================================================

fn build_synthetic_test_plugin() -> &'static Path {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT.get_or_init(|| {
        let playground_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let plugin_root = playground_root.join("test_plugins/synthetic_test");
        let installed = playground_root.join(".evenframe/plugins/synthetic_test.wasm");

        if let Some(parent) = installed.parent() {
            std::fs::create_dir_all(parent).expect("failed to create plugins dir");
        }

        let status = Command::new("cargo")
            .args(["build", "--manifest-path"])
            .arg(plugin_root.join("Cargo.toml"))
            .args(["--target", "wasm32-unknown-unknown", "--release"])
            .status()
            .expect("failed to spawn cargo for synthetic_test plugin build");

        assert!(
            status.success(),
            "cargo build failed for synthetic_test plugin. \
             Ensure the wasm32-unknown-unknown target is installed: \
             `rustup target add wasm32-unknown-unknown`"
        );

        let built = plugin_root.join("target/wasm32-unknown-unknown/release/synthetic_test.wasm");
        assert!(
            built.exists(),
            "synthetic_test build succeeded but .wasm not found at {:?}",
            built
        );

        std::fs::copy(&built, &installed).unwrap_or_else(|e| {
            panic!("failed to copy {:?} to {:?}: {}", built, installed, e)
        });

        installed
    })
}

fn playground_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn load_manager() -> SyntheticItemPluginManager {
    let _ = build_synthetic_test_plugin();

    let mut plugins = HashMap::new();
    plugins.insert(
        "synthetic_test".to_string(),
        SyntheticItemPluginConfig {
            path: ".evenframe/plugins/synthetic_test.wasm".to_string(),
        },
    );
    SyntheticItemPluginManager::new(&plugins, &playground_root())
        .expect("failed to load synthetic_test plugin")
}

// ============================================================================
// Input helpers — build real configs, not summaries
// ============================================================================

fn make_struct(name: &str, fields: Vec<StructField>) -> StructConfig {
    StructConfig {
        struct_name: name.to_string(),
        fields,
        validators: vec![],
        doccom: None,
        macroforge_derives: vec![],
        annotations: vec![],
        pipeline: Pipeline::Both,
        rust_derives: vec![],
        output_override: None,
    }
}

fn make_field(name: &str, ft: FieldType) -> StructField {
    StructField {
        field_name: name.to_string(),
        field_type: ft,
        ..Default::default()
    }
}

fn seed_input_with_struct(name: &str) -> SyntheticPluginInput {
    let mut structs = HashMap::new();
    structs.insert(
        name.to_string(),
        make_struct(name, vec![make_field("id", FieldType::String)]),
    );

    let mut enums = HashMap::new();
    enums.insert(
        "Status".to_string(),
        TaggedUnion {
            enum_name: "Status".to_string(),
            variants: vec![
                Variant {
                    name: "Active".to_string(),
                    data: None,
                    doccom: None,
                    annotations: vec![],
                    output_override: None,
                },
                Variant {
                    name: "Inactive".to_string(),
                    data: None,
                    doccom: None,
                    annotations: vec![],
                    output_override: None,
                },
            ],
            representation: Default::default(),
            doccom: None,
            macroforge_derives: vec![],
            annotations: vec![],
            pipeline: Pipeline::Both,
            rust_derives: vec![],
            output_override: None,
        },
    );

    let mut tables = HashMap::new();
    tables.insert(
        "user".to_string(),
        TableConfig {
            table_name: "user".to_string(),
            struct_config: make_struct("User", vec![make_field("id", FieldType::String)]),
            relation: None,
            permissions: None,
            mock_generation_config: None,
            events: vec![],
            indexes: vec![],
            output_override: None,
        },
    );

    SyntheticPluginInput {
        structs,
        enums,
        tables,
    }
}

fn empty_input() -> SyntheticPluginInput {
    SyntheticPluginInput {
        structs: HashMap::new(),
        enums: HashMap::new(),
        tables: HashMap::new(),
    }
}

// ============================================================================
// Happy-path tests
// ============================================================================

#[test]
fn synthetic_plugin_loads_and_returns_all_three_kinds() {
    let mut pm = load_manager();
    let input = seed_input_with_struct("Account");

    let output = pm
        .generate_items("synthetic_test", &input)
        .expect("generate_items must succeed");

    assert!(
        output.error.is_none(),
        "plugin reported error: {:?}",
        output.error
    );
    assert_eq!(output.new_structs.len(), 1, "expected 1 synthetic struct");
    assert_eq!(output.new_enums.len(), 1, "expected 1 synthetic enum");
    assert_eq!(output.new_tables.len(), 1, "expected 1 synthetic table");
}

#[test]
fn synthetic_plugin_names_match_plugin_contract() {
    let mut pm = load_manager();
    let output = pm
        .generate_items("synthetic_test", &seed_input_with_struct("Account"))
        .expect("generate_items must succeed");

    let struct_names: Vec<_> = output.new_structs.iter().map(|s| &s.struct_name).collect();
    assert!(
        struct_names.iter().any(|n| *n == "SyntheticAudit"),
        "expected SyntheticAudit, got: {:?}",
        struct_names
    );

    let enum_names: Vec<_> = output.new_enums.iter().map(|e| &e.enum_name).collect();
    assert!(
        enum_names.iter().any(|n| *n == "SyntheticSeverity"),
        "expected SyntheticSeverity, got: {:?}",
        enum_names
    );

    let table_names: Vec<_> = output.new_tables.iter().map(|t| &t.table_name).collect();
    assert!(
        table_names.iter().any(|n| *n == "synthetic_ping"),
        "expected synthetic_ping, got: {:?}",
        table_names
    );
}

#[test]
fn synthetic_plugin_sees_existing_scanner_state() {
    let mut pm = load_manager();

    for seed in ["Account", "Workspace", "Project"] {
        let output = pm
            .generate_items("synthetic_test", &seed_input_with_struct(seed))
            .expect("generate_items must succeed");
        assert_eq!(output.new_structs.len(), 1);
        let audit = &output.new_structs[0];
        let expected_field = format!("{}_audit_note", seed);
        assert!(
            audit.fields.iter().any(|f| f.field_name == expected_field),
            "plugin did not observe seed struct '{}': fields were {:?}",
            seed,
            audit
                .fields
                .iter()
                .map(|f| &f.field_name)
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn synthetic_plugin_noop_when_no_existing_structs() {
    let mut pm = load_manager();
    let output = pm
        .generate_items("synthetic_test", &empty_input())
        .expect("generate_items must succeed on empty input");

    assert!(output.error.is_none());
    assert!(output.new_structs.is_empty());
    assert!(output.new_enums.is_empty());
    assert!(output.new_tables.is_empty());
}

// ============================================================================
// Wire-compatibility round-trips
// ============================================================================

#[test]
fn synthetic_struct_roundtrips_through_core_struct_config() {
    let mut pm = load_manager();
    let output = pm
        .generate_items("synthetic_test", &seed_input_with_struct("Account"))
        .expect("generate_items must succeed");

    let sc = &output.new_structs[0];
    let json = serde_json::to_value(sc).expect("serialize StructConfig");
    let reparsed: StructConfig =
        serde_json::from_value(json).expect("StructConfig must round-trip");
    assert_eq!(reparsed.struct_name, "SyntheticAudit");
    assert_eq!(reparsed.fields.len(), 3);
}

#[test]
fn synthetic_enum_roundtrips_through_core_tagged_union() {
    let mut pm = load_manager();
    let output = pm
        .generate_items("synthetic_test", &seed_input_with_struct("Account"))
        .expect("generate_items must succeed");

    let ec = &output.new_enums[0];
    let json = serde_json::to_value(ec).expect("serialize TaggedUnion");
    let reparsed: TaggedUnion =
        serde_json::from_value(json).expect("TaggedUnion must round-trip");
    assert_eq!(reparsed.enum_name, "SyntheticSeverity");
    assert_eq!(reparsed.variants.len(), 3);
    let variant_names: Vec<_> = reparsed.variants.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(variant_names, ["Info", "Warning", "Critical"]);
}

#[test]
fn synthetic_table_roundtrips_through_core_table_config() {
    let mut pm = load_manager();
    let output = pm
        .generate_items("synthetic_test", &seed_input_with_struct("Account"))
        .expect("generate_items must succeed");

    let tc = &output.new_tables[0];
    let json = serde_json::to_value(tc).expect("serialize TableConfig");
    let reparsed: TableConfig =
        serde_json::from_value(json).expect("TableConfig must round-trip");
    assert_eq!(reparsed.table_name, "synthetic_ping");
    assert_eq!(reparsed.struct_config.struct_name, "SyntheticPing");
    assert_eq!(reparsed.struct_config.fields.len(), 3);
}

// ============================================================================
// Failure-mode tests
// ============================================================================

#[test]
fn synthetic_plugin_missing_wasm_fails_at_load() {
    let mut plugins = HashMap::new();
    plugins.insert(
        "does_not_exist".to_string(),
        SyntheticItemPluginConfig {
            path: ".evenframe/plugins/does_not_exist.wasm".to_string(),
        },
    );
    let result = SyntheticItemPluginManager::new(&plugins, &playground_root());
    assert!(result.is_err(), "missing WASM file must error at load");
    let err = result.err().unwrap().to_string();
    assert!(
        err.contains("does_not_exist"),
        "expected missing-plugin error to mention the plugin name; got: {}",
        err
    );
}

#[test]
fn synthetic_plugin_repeated_calls_are_stable() {
    let mut pm = load_manager();
    let input = seed_input_with_struct("Widget");

    let first = pm.generate_items("synthetic_test", &input).unwrap();
    for _ in 0..10 {
        let again = pm.generate_items("synthetic_test", &input).unwrap();
        assert_eq!(first.new_structs.len(), again.new_structs.len());
        assert_eq!(first.new_enums.len(), again.new_enums.len());
        assert_eq!(first.new_tables.len(), again.new_tables.len());
        assert_eq!(
            first.new_structs[0].struct_name,
            again.new_structs[0].struct_name
        );
    }
}
