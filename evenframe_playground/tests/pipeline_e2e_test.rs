//! End-to-end tests for the Pipeline feature (Typesync / Schemasync derives).
//!
//! These tests verify that:
//! 1. The workspace scanner detects Typesync and Schemasync derives
//! 2. Pipeline metadata is correctly propagated to configs
//! 3. filter_for_typesync / filter_for_schemasync exclude the right types
//! 4. TypeGenerator only emits types that include the typesync pipeline
//! 5. Existing Evenframe derives continue to participate in both pipelines

use evenframe_core::tooling::{
    BuildConfig, TypeGenerator, WorkspaceScanner, build_all_configs, filter_for_schemasync,
    filter_for_typesync, merge_tables_and_objects,
};
use evenframe_core::types::Pipeline;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

// ============================================================================
// Helpers
// ============================================================================

/// Create a minimal Cargo.toml for a fake crate so the scanner can find it.
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

/// Write a Rust source file inside `src/`.
fn write_src_file(dir: &std::path::Path, filename: &str, content: &str) {
    let src_dir = dir.join("src");
    fs::create_dir_all(&src_dir).unwrap();
    fs::write(src_dir.join(filename), content).unwrap();
}

// ============================================================================
// Scanner: detect_pipeline
// ============================================================================

#[test]
fn test_scanner_detects_typesync_derive() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    write_cargo_toml(root, "scan_typesync");
    write_src_file(
        root,
        "lib.rs",
        r#"
        #[derive(Typesync)]
        pub struct ApiResponse {
            pub message: String,
            pub code: i32,
        }
        "#,
    );

    let scanner = WorkspaceScanner::with_path(root.to_path_buf(), vec![], false);
    let types = scanner.scan_for_evenframe_types().unwrap();

    assert_eq!(types.len(), 1);
    assert_eq!(types[0].name, "ApiResponse");
    assert_eq!(types[0].pipeline, Pipeline::Typesync);
}

#[test]
fn test_scanner_detects_schemasync_derive() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    write_cargo_toml(root, "scan_schemasync");
    write_src_file(
        root,
        "lib.rs",
        r#"
        #[derive(Schemasync)]
        pub struct InternalAuditLog {
            pub id: String,
            pub action: String,
        }
        "#,
    );

    let scanner = WorkspaceScanner::with_path(root.to_path_buf(), vec![], false);
    let types = scanner.scan_for_evenframe_types().unwrap();

    assert_eq!(types.len(), 1);
    assert_eq!(types[0].name, "InternalAuditLog");
    assert_eq!(types[0].pipeline, Pipeline::Schemasync);
    assert!(types[0].has_id_field);
}

#[test]
fn test_scanner_detects_evenframe_as_both() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    write_cargo_toml(root, "scan_evenframe");
    write_src_file(
        root,
        "lib.rs",
        r#"
        #[derive(Evenframe)]
        pub struct User {
            pub id: String,
            pub name: String,
        }
        "#,
    );

    let scanner = WorkspaceScanner::with_path(root.to_path_buf(), vec![], false);
    let types = scanner.scan_for_evenframe_types().unwrap();

    assert_eq!(types.len(), 1);
    assert_eq!(types[0].pipeline, Pipeline::Both);
}

#[test]
fn test_scanner_both_derives_yields_pipeline_both() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    write_cargo_toml(root, "scan_both");
    write_src_file(
        root,
        "lib.rs",
        r#"
        #[derive(Typesync, Schemasync)]
        pub struct Widget {
            pub id: String,
            pub label: String,
        }
        "#,
    );

    let scanner = WorkspaceScanner::with_path(root.to_path_buf(), vec![], false);
    let types = scanner.scan_for_evenframe_types().unwrap();

    assert_eq!(types.len(), 1);
    assert_eq!(types[0].pipeline, Pipeline::Both);
}

#[test]
fn test_scanner_enum_with_typesync_derive() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    write_cargo_toml(root, "scan_enum_ts");
    write_src_file(
        root,
        "lib.rs",
        r#"
        #[derive(Typesync)]
        pub enum Color {
            Red,
            Green,
            Blue,
        }
        "#,
    );

    let scanner = WorkspaceScanner::with_path(root.to_path_buf(), vec![], false);
    let types = scanner.scan_for_evenframe_types().unwrap();

    assert_eq!(types.len(), 1);
    assert_eq!(types[0].name, "Color");
    assert_eq!(types[0].pipeline, Pipeline::Typesync);
}

// ============================================================================
// Config builders: pipeline propagation
// ============================================================================

#[test]
fn test_config_builder_propagates_pipeline_to_struct() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    write_cargo_toml(root, "cfg_struct");
    write_src_file(
        root,
        "lib.rs",
        r#"
        #[derive(Schemasync)]
        pub struct DbOnlyRecord {
            pub id: String,
            pub data: String,
        }

        #[derive(Typesync)]
        pub struct TsOnlyDto {
            pub value: String,
        }

        #[derive(Evenframe)]
        pub struct SharedModel {
            pub id: String,
            pub name: String,
        }
        "#,
    );

    let config = BuildConfig::builder().scan_path(root).build();
    let (enums, tables, objects) = build_all_configs(&config).unwrap();

    // DbOnlyRecord has id → table
    assert!(tables.contains_key("db_only_record"));
    assert_eq!(
        tables["db_only_record"].struct_config.pipeline,
        Pipeline::Schemasync
    );

    // TsOnlyDto has no id → object
    assert!(objects.contains_key("TsOnlyDto"));
    assert_eq!(objects["TsOnlyDto"].pipeline, Pipeline::Typesync);

    // SharedModel has id → table
    assert!(tables.contains_key("shared_model"));
    assert_eq!(
        tables["shared_model"].struct_config.pipeline,
        Pipeline::Both
    );

    assert!(enums.is_empty());
}

#[test]
fn test_config_builder_propagates_pipeline_to_enum() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    write_cargo_toml(root, "cfg_enum");
    write_src_file(
        root,
        "lib.rs",
        r#"
        #[derive(Typesync)]
        pub enum FrontendTheme {
            Light,
            Dark,
        }

        #[derive(Schemasync)]
        pub enum DbStatus {
            Active,
            Archived,
        }

        #[derive(Evenframe)]
        pub enum SharedEnum {
            A,
            B,
        }
        "#,
    );

    let config = BuildConfig::builder().scan_path(root).build();
    let (enums, _tables, _objects) = build_all_configs(&config).unwrap();

    assert_eq!(enums["FrontendTheme"].pipeline, Pipeline::Typesync);
    assert_eq!(enums["DbStatus"].pipeline, Pipeline::Schemasync);
    assert_eq!(enums["SharedEnum"].pipeline, Pipeline::Both);
}

// ============================================================================
// Filtering
// ============================================================================

#[test]
fn test_filter_for_typesync_excludes_schemasync_only() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    write_cargo_toml(root, "filter_ts");
    write_src_file(
        root,
        "lib.rs",
        r#"
        #[derive(Schemasync)]
        pub struct DbOnly {
            pub id: String,
            pub secret: String,
        }

        #[derive(Typesync)]
        pub struct TsOnly {
            pub label: String,
        }

        #[derive(Evenframe)]
        pub struct Shared {
            pub id: String,
            pub name: String,
        }

        #[derive(Schemasync)]
        pub enum DbEnum {
            X,
            Y,
        }

        #[derive(Typesync)]
        pub enum TsEnum {
            A,
            B,
        }

        #[derive(Evenframe)]
        pub enum SharedEnum {
            P,
            Q,
        }
        "#,
    );

    let config = BuildConfig::builder().scan_path(root).build();
    let (enums, tables, objects) = build_all_configs(&config).unwrap();
    let (ts_enums, ts_tables, ts_objects) = filter_for_typesync(enums, tables, objects);

    // Typesync filter should include Typesync and Both, exclude Schemasync
    assert!(
        !ts_tables.contains_key("db_only"),
        "Schemasync-only table should be excluded from typesync"
    );
    assert!(
        ts_tables.contains_key("shared"),
        "Both-pipeline table should be included in typesync"
    );
    assert!(
        ts_objects.contains_key("TsOnly"),
        "Typesync-only object should be included"
    );

    assert!(
        !ts_enums.contains_key("DbEnum"),
        "Schemasync-only enum should be excluded from typesync"
    );
    assert!(
        ts_enums.contains_key("TsEnum"),
        "Typesync enum should be included"
    );
    assert!(
        ts_enums.contains_key("SharedEnum"),
        "Both-pipeline enum should be included in typesync"
    );
}

#[test]
fn test_filter_for_schemasync_excludes_typesync_only() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    write_cargo_toml(root, "filter_ss");
    write_src_file(
        root,
        "lib.rs",
        r#"
        #[derive(Schemasync)]
        pub struct DbOnly {
            pub id: String,
            pub secret: String,
        }

        #[derive(Typesync)]
        pub struct TsOnly {
            pub label: String,
        }

        #[derive(Evenframe)]
        pub struct Shared {
            pub id: String,
            pub name: String,
        }

        #[derive(Typesync)]
        pub enum TsEnum {
            A,
            B,
        }

        #[derive(Schemasync)]
        pub enum DbEnum {
            X,
            Y,
        }
        "#,
    );

    let config = BuildConfig::builder().scan_path(root).build();
    let (enums, tables, objects) = build_all_configs(&config).unwrap();
    let (ss_enums, ss_tables, ss_objects) = filter_for_schemasync(enums, tables, objects);

    // Schemasync filter should include Schemasync and Both, exclude Typesync
    assert!(
        ss_tables.contains_key("db_only"),
        "Schemasync-only table should be included"
    );
    assert!(
        ss_tables.contains_key("shared"),
        "Both-pipeline table should be included in schemasync"
    );
    assert!(
        !ss_objects.contains_key("TsOnly"),
        "Typesync-only object should be excluded from schemasync"
    );

    assert!(
        !ss_enums.contains_key("TsEnum"),
        "Typesync-only enum should be excluded from schemasync"
    );
    assert!(
        ss_enums.contains_key("DbEnum"),
        "Schemasync enum should be included"
    );
}

// ============================================================================
// TypeGenerator: pipeline-aware output
// ============================================================================

#[test]
fn test_typegenerator_excludes_schemasync_only_types() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let out_dir = root.join("out");
    fs::create_dir_all(&out_dir).unwrap();

    write_cargo_toml(root, "gen_pipeline");
    write_src_file(
        root,
        "lib.rs",
        r#"
        #[derive(Schemasync)]
        pub struct InternalSecret {
            pub id: String,
            pub key: String,
        }

        #[derive(Typesync)]
        pub struct ApiResponse {
            pub message: String,
            pub code: i32,
        }

        #[derive(Evenframe)]
        pub struct User {
            pub id: String,
            pub name: String,
        }

        #[derive(Evenframe)]
        pub enum Status {
            Active,
            Inactive,
        }

        #[derive(Schemasync)]
        pub enum InternalStatus {
            Pending,
            Processing,
        }
        "#,
    );

    let config = BuildConfig::builder()
        .scan_path(root)
        .output_path(&out_dir)
        .enable_arktype()
        .enable_effect()
        .build();

    let generator = TypeGenerator::new(config);
    let report = generator.generate_all().unwrap();

    assert!(!report.files.is_empty(), "Should generate at least one file");

    // Read all generated TypeScript content
    let mut all_ts_content = String::new();
    for file in &report.files {
        let content = fs::read_to_string(&file.path).unwrap();
        all_ts_content.push_str(&content);
    }

    // Types with Typesync or Both pipeline should appear
    assert!(
        all_ts_content.contains("ApiResponse"),
        "Typesync-only type should appear in generated TypeScript"
    );
    assert!(
        all_ts_content.contains("User"),
        "Both-pipeline type should appear in generated TypeScript"
    );
    assert!(
        all_ts_content.contains("Status"),
        "Both-pipeline enum should appear in generated TypeScript"
    );

    // Schemasync-only types should NOT appear
    assert!(
        !all_ts_content.contains("InternalSecret"),
        "Schemasync-only type should NOT appear in generated TypeScript"
    );
    assert!(
        !all_ts_content.contains("InternalStatus"),
        "Schemasync-only enum should NOT appear in generated TypeScript"
    );
}

// ============================================================================
// merge_tables_and_objects preserves pipeline
// ============================================================================

#[test]
fn test_merge_tables_and_objects_preserves_pipeline() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    write_cargo_toml(root, "merge_pipeline");
    write_src_file(
        root,
        "lib.rs",
        r#"
        #[derive(Typesync)]
        pub struct TypesyncTable {
            pub id: String,
            pub data: String,
        }

        #[derive(Schemasync)]
        pub struct SchemasyncTable {
            pub id: String,
            pub data: String,
        }

        #[derive(Typesync)]
        pub struct TypesyncObject {
            pub value: String,
        }
        "#,
    );

    let config = BuildConfig::builder().scan_path(root).build();
    let (_enums, tables, objects) = build_all_configs(&config).unwrap();
    let merged = merge_tables_and_objects(&tables, &objects);

    assert_eq!(merged["TypesyncTable"].pipeline, Pipeline::Typesync);
    assert_eq!(merged["SchemasyncTable"].pipeline, Pipeline::Schemasync);
    assert_eq!(merged["TypesyncObject"].pipeline, Pipeline::Typesync);
}

// ============================================================================
// Existing playground models: backward compatibility
// ============================================================================

#[test]
fn test_existing_playground_models_are_pipeline_both() {
    let playground_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let config = BuildConfig::builder().scan_path(&playground_dir).build();
    let (enums, tables, objects) = build_all_configs(&config).unwrap();

    // All existing models use #[derive(Evenframe)] so they should be Pipeline::Both
    for (name, table) in &tables {
        assert_eq!(
            table.struct_config.pipeline,
            Pipeline::Both,
            "Existing table '{name}' should have Pipeline::Both"
        );
    }
    for (name, obj) in &objects {
        assert_eq!(
            obj.pipeline,
            Pipeline::Both,
            "Existing object '{name}' should have Pipeline::Both"
        );
    }
    for (name, en) in &enums {
        assert_eq!(
            en.pipeline,
            Pipeline::Both,
            "Existing enum '{name}' should have Pipeline::Both"
        );
    }
}

// ============================================================================
// Pipeline helpers
// ============================================================================

#[test]
fn test_pipeline_includes_methods() {
    assert!(Pipeline::Both.includes_typesync());
    assert!(Pipeline::Both.includes_schemasync());
    assert!(Pipeline::Typesync.includes_typesync());
    assert!(!Pipeline::Typesync.includes_schemasync());
    assert!(!Pipeline::Schemasync.includes_typesync());
    assert!(Pipeline::Schemasync.includes_schemasync());
}

#[test]
fn test_pipeline_default_is_both() {
    assert_eq!(Pipeline::default(), Pipeline::Both);
}
