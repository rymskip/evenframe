//! End-to-end tests for evenframe_build API.
//!
//! These tests verify that:
//! 1. The build API can be used programmatically
//! 2. Each output kind produces the expected file
//! 3. Generated files contain valid content
//! 4. The builder pattern works correctly
//! 5. Configuration loading works

use evenframe_core::tooling::{BuildConfig, TypeGenerator};
use evenframe_core::typesync::config::{OutputKind, TypesyncOutput};
use evenframe_core::typesync::output::GeneratedFile;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Get the path to the playground root directory (where Cargo.toml is)
fn get_playground_root_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Get the path to the playground's evenframe.toml
fn get_evenframe_toml_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("evenframe.toml")
}

fn output(kind: OutputKind, dir: &Path) -> TypesyncOutput {
    TypesyncOutput::new(kind, dir.to_string_lossy())
}

/// A config scanning the playground and generating `outputs`.
fn config_with(outputs: Vec<TypesyncOutput>) -> BuildConfig {
    BuildConfig::builder()
        .scan_path(get_playground_root_path())
        .outputs(outputs)
        .build()
}

/// Generates the single file `output` produces.
fn generate_one(output: TypesyncOutput) -> GeneratedFile {
    let report = TypeGenerator::new(config_with(vec![output]))
        .generate_all()
        .expect("Generation should succeed");
    assert_eq!(report.files.len(), 1, "Should generate one file");
    report.files.into_iter().next().expect("one file")
}

// ============================================================================
// BuildConfig Builder Tests
// ============================================================================

#[test]
fn test_build_config_default() {
    let config = BuildConfig::default();

    assert_eq!(
        config.outputs,
        vec![TypesyncOutput::new(OutputKind::Arktype, "./src/generated/")],
        "ArkType into ./src/generated/ should be the default output"
    );
}

#[test]
fn test_build_config_builder_custom_paths() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let outputs = vec![output(OutputKind::Arktype, temp_dir.path())];

    let config = config_with(outputs.clone());

    assert_eq!(config.scan_path, get_playground_root_path());
    assert_eq!(config.outputs, outputs);
}

#[test]
fn test_build_config_builder_apply_aliases() {
    let config = BuildConfig::builder()
        .apply_alias("Table")
        .apply_alias("Object")
        .apply_alias("Edge")
        .build();

    assert_eq!(config.apply_aliases.len(), 3);
    assert!(config.apply_aliases.contains(&"Table".to_string()));
    assert!(config.apply_aliases.contains(&"Object".to_string()));
    assert!(config.apply_aliases.contains(&"Edge".to_string()));
}

// ============================================================================
// Configuration Loading Tests
// ============================================================================

#[test]
fn test_load_config_from_toml_path() {
    let toml_path = get_evenframe_toml_path();

    if !toml_path.exists() {
        eprintln!("Skipping test: evenframe.toml not found at {:?}", toml_path);
        return;
    }

    let config = BuildConfig::from_toml_path(&toml_path);
    assert!(config.is_ok(), "Should load config from path: {:?}", config);
}

#[test]
fn test_config_not_found_error() {
    let result = BuildConfig::from_toml_path("/nonexistent/path/evenframe.toml");
    assert!(result.is_err(), "Should fail for nonexistent path");
}

// ============================================================================
// Output Kind Tests
// ============================================================================

#[test]
fn test_output_kind_default_filenames() {
    assert_eq!(OutputKind::Arktype.default_filename(), "arktype.ts");
    assert_eq!(OutputKind::Effect.default_filename(), "bindings.ts");
    assert_eq!(OutputKind::Macroforge.default_filename(), "macroforge.ts");
    assert_eq!(OutputKind::Flatbuffers.default_filename(), "schema.fbs");
    assert_eq!(OutputKind::Protobuf.default_filename(), "schema.proto");
}

// ============================================================================
// Type Generator Tests
// ============================================================================

#[test]
fn test_type_generator_generate_arktype() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // The playground uses #[derive(Evenframe)] directly, no apply_aliases needed
    let generated = generate_one(output(OutputKind::Arktype, temp_dir.path()));

    assert_eq!(generated.kind, OutputKind::Arktype);
    assert!(generated.bytes_written > 0, "Should write some bytes");
    assert!(generated.path.exists(), "Generated file should exist");

    let content = fs::read_to_string(&generated.path).expect("Should read generated file");
    assert!(
        content.contains("arktype"),
        "Generated content should reference arktype"
    );
}

#[test]
fn test_type_generator_generate_effect() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    let generated = generate_one(output(OutputKind::Effect, temp_dir.path()));

    assert_eq!(generated.kind, OutputKind::Effect);
    assert!(generated.bytes_written > 0);
    assert!(generated.path.exists());

    let content = fs::read_to_string(&generated.path).expect("Should read generated file");
    assert!(
        content.contains("Schema"),
        "Generated content should reference Effect Schema"
    );
}

#[test]
fn test_type_generator_generate_macroforge() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    let generated = generate_one(output(OutputKind::Macroforge, temp_dir.path()));

    assert_eq!(generated.kind, OutputKind::Macroforge);
    assert!(generated.bytes_written > 0);
    assert!(generated.path.exists());
}

#[test]
fn test_type_generator_generate_flatbuffers() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let mut fbs = output(OutputKind::Flatbuffers, temp_dir.path());
    fbs.namespace = Some("Playground".to_string());

    let generated = generate_one(fbs);

    assert_eq!(generated.kind, OutputKind::Flatbuffers);
    assert!(generated.bytes_written > 0);
    assert!(generated.path.exists());

    let content = fs::read_to_string(&generated.path).expect("Should read generated file");
    assert!(
        content.contains("namespace Playground"),
        "Generated content should contain namespace"
    );
    assert!(
        content.contains("table"),
        "Generated content should contain table definitions"
    );
}

#[test]
fn test_type_generator_generate_protobuf() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let mut proto = output(OutputKind::Protobuf, temp_dir.path());
    proto.package = Some("playground".to_string());

    let generated = generate_one(proto);

    assert_eq!(generated.kind, OutputKind::Protobuf);
    assert!(generated.bytes_written > 0);
    assert!(generated.path.exists());

    let content = fs::read_to_string(&generated.path).expect("Should read generated file");
    assert!(
        content.contains("syntax = \"proto3\""),
        "Generated content should specify proto3 syntax"
    );
    assert!(
        content.contains("package playground"),
        "Generated content should contain package"
    );
    assert!(
        content.contains("message"),
        "Generated content should contain message definitions"
    );
}

// ============================================================================
// Generate All Tests
// ============================================================================

#[test]
fn test_type_generator_generate_all() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let dir = temp_dir.path();

    let config = config_with(vec![
        output(OutputKind::Arktype, dir),
        output(OutputKind::Effect, dir),
        output(OutputKind::Macroforge, dir),
        output(OutputKind::Flatbuffers, dir),
        output(OutputKind::Protobuf, dir),
    ]);
    let report = TypeGenerator::new(config)
        .generate_all()
        .expect("generate_all should succeed");

    // One file for each output
    assert_eq!(
        report.files.len(),
        5,
        "Should generate 5 files when every kind is configured"
    );

    // Verify counts
    assert!(
        report.structs_processed > 0 || report.tables_processed > 0,
        "Should have processed some types"
    );

    // Verify each file was created
    for file in &report.files {
        assert!(file.path.exists(), "File {:?} should exist", file.path);
        assert!(file.bytes_written > 0, "File should have content");
    }

    for name in [
        "arktype.ts",
        "bindings.ts",
        "macroforge.ts",
        "schema.fbs",
        "schema.proto",
    ] {
        assert!(dir.join(name).exists(), "{name} should exist");
    }
}

#[test]
fn test_type_generator_generate_subset() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    let config = config_with(vec![
        output(OutputKind::Arktype, temp_dir.path()),
        output(OutputKind::Effect, temp_dir.path()),
    ]);
    let report = TypeGenerator::new(config)
        .generate_all()
        .expect("generate_all should succeed");

    assert_eq!(report.files.len(), 2, "Should generate 2 files");
    let kinds: Vec<OutputKind> = report.files.iter().map(|f| f.kind).collect();
    assert_eq!(kinds, vec![OutputKind::Arktype, OutputKind::Effect]);
}

// ============================================================================
// Generation Report Tests
// ============================================================================

#[test]
fn test_generation_report_contains_type_counts() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    let config = config_with(vec![output(OutputKind::Arktype, temp_dir.path())]);
    let report = TypeGenerator::new(config)
        .generate_all()
        .expect("Should generate");

    // The playground has multiple types, so these should be non-zero
    println!("Enums processed: {}", report.enums_processed);
    println!("Structs processed: {}", report.structs_processed);
    println!("Tables processed: {}", report.tables_processed);

    // At minimum we should have some types processed
    let total_types = report.enums_processed + report.structs_processed + report.tables_processed;
    assert!(total_types > 0, "Should process at least some types");
}

// ============================================================================
// Content Validation Tests
// ============================================================================

#[test]
fn test_generated_arktype_contains_expected_types() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    let generated = generate_one(output(OutputKind::Arktype, temp_dir.path()));
    let content = fs::read_to_string(&generated.path).expect("Should read");

    // Check for expected type names from the playground models
    let expected_types = ["User", "Session", "Role"];
    let found_types: Vec<&str> = expected_types
        .iter()
        .copied()
        .filter(|type_name| content.contains(type_name))
        .collect();

    println!("Found types in generated ArkType: {:?}", found_types);

    // We should find at least some expected types
    assert!(
        !found_types.is_empty(),
        "Should find at least some expected types in generated content"
    );
}

#[test]
fn test_generated_flatbuffers_has_valid_syntax() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let mut fbs = output(OutputKind::Flatbuffers, temp_dir.path());
    fbs.namespace = Some("Test".to_string());

    let generated = generate_one(fbs);
    let content = fs::read_to_string(&generated.path).expect("Should read");

    // Basic FlatBuffers syntax checks
    assert!(content.contains("namespace"), "Should have namespace");
    assert!(
        content.contains("table") || content.contains("enum"),
        "Should have table or enum definitions"
    );

    // Check for proper field syntax (field_name: type;)
    assert!(content.contains(": "), "Should have field type annotations");
}

#[test]
fn test_generated_protobuf_has_valid_syntax() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let mut proto = output(OutputKind::Protobuf, temp_dir.path());
    proto.package = Some("test".to_string());

    let generated = generate_one(proto);
    let content = fs::read_to_string(&generated.path).expect("Should read");

    // Basic Protocol Buffers syntax checks
    assert!(
        content.contains("syntax = \"proto3\""),
        "Should specify proto3 syntax"
    );
    assert!(content.contains("package"), "Should have package");
    assert!(
        content.contains("message") || content.contains("enum"),
        "Should have message or enum definitions"
    );

    // Check for field numbers
    assert!(content.contains(" = 1;"), "Should have field numbers");
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[test]
fn test_generate_with_invalid_scan_path() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    let config = BuildConfig::builder()
        .scan_path("/nonexistent/path/that/does/not/exist")
        .outputs(vec![output(OutputKind::Arktype, temp_dir.path())])
        .build();

    let result = TypeGenerator::new(config).generate_all();

    // This might succeed with empty results or fail - both are acceptable
    // The important thing is it doesn't panic
    match result {
        Ok(report) => {
            // If it succeeds, it should have processed 0 types
            println!("Generated {} files from invalid path", report.files.len());
        }
        Err(e) => {
            println!("Expected error from invalid path: {:?}", e);
        }
    }
}

#[test]
fn test_generate_creates_output_directory() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let nested_output = temp_dir.path().join("nested").join("output").join("dir");

    // Verify it doesn't exist yet
    assert!(!nested_output.exists());

    let config = config_with(vec![output(OutputKind::Arktype, &nested_output)]);
    let result = TypeGenerator::new(config).generate_all();

    assert!(result.is_ok(), "Should create nested directories");
    assert!(nested_output.exists(), "Output directory should be created");
}

// ============================================================================
// Integration Tests
// ============================================================================

#[test]
fn test_generate_with_config_function() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    let config = config_with(vec![output(OutputKind::Arktype, temp_dir.path())]);

    // Use the top-level generate_with_config function
    let result = evenframe_core::tooling::generate_with_config(config);

    assert!(
        result.is_ok(),
        "generate_with_config should succeed: {:?}",
        result
    );
}

#[test]
fn test_generate_multiple_times() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    let config = config_with(vec![output(OutputKind::Arktype, temp_dir.path())]);

    // Generate multiple times to ensure idempotency
    let result1 = TypeGenerator::new(config.clone()).generate_all();
    assert!(result1.is_ok());

    let result2 = TypeGenerator::new(config).generate_all();
    assert!(result2.is_ok());

    // Both should produce the same number of files
    assert_eq!(result1.unwrap().files.len(), result2.unwrap().files.len());
}
