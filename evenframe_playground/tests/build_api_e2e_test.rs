//! End-to-end tests for evenframe_build API.
//!
//! These tests verify that:
//! 1. The build API can be used programmatically
//! 2. Generator types produce expected files
//! 3. Generated files contain valid content
//! 4. The builder pattern works correctly
//! 5. Configuration loading works

use evenframe_build::{BuildConfig, GeneratorType, TypeGenerator};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

/// Get the path to the playground root directory (where Cargo.toml is)
fn get_playground_root_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Get the path to the playground's evenframe.toml
fn get_evenframe_toml_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("evenframe.toml")
}

// ============================================================================
// BuildConfig Builder Tests
// ============================================================================

#[test]
fn test_build_config_default() {
    let config = BuildConfig::default();

    assert!(config.arktype, "ArkType should be enabled by default");
    assert!(!config.effect, "Effect should be disabled by default");
    assert!(!config.macroforge, "Macroforge should be disabled by default");
    assert!(!config.flatbuffers, "FlatBuffers should be disabled by default");
    assert!(!config.protobuf, "Protobuf should be disabled by default");
}

#[test]
fn test_build_config_builder_enable_all() {
    let config = BuildConfig::builder().enable_all().build();

    assert!(config.arktype);
    assert!(config.effect);
    assert!(config.macroforge);
    assert!(config.flatbuffers);
    assert!(config.protobuf);
}

#[test]
fn test_build_config_builder_custom_paths() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    let config = BuildConfig::builder()
        .scan_path(get_playground_root_path())
        .output_path(temp_dir.path())
        .build();

    assert_eq!(config.scan_path, get_playground_root_path());
    assert_eq!(config.output_path, temp_dir.path());
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

#[test]
fn test_build_config_builder_flatbuffers_with_namespace() {
    let config = BuildConfig::builder()
        .enable_flatbuffers(Some("com.example.app".to_string()))
        .build();

    assert!(config.flatbuffers);
    assert_eq!(
        config.flatbuffers_namespace,
        Some("com.example.app".to_string())
    );
}

#[test]
fn test_build_config_builder_protobuf_with_package() {
    let config = BuildConfig::builder()
        .enable_protobuf(Some("com.example.proto".to_string()), true)
        .build();

    assert!(config.protobuf);
    assert_eq!(
        config.protobuf_package,
        Some("com.example.proto".to_string())
    );
    assert!(config.protobuf_import_validate);
}

// ============================================================================
// Configuration Loading Tests
// ============================================================================

#[test]
fn test_load_config_from_toml_path() {
    let toml_path = get_evenframe_toml_path();

    if !toml_path.exists() {
        eprintln!(
            "Skipping test: evenframe.toml not found at {:?}",
            toml_path
        );
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
// Generator Type Tests
// ============================================================================

#[test]
fn test_generator_type_default_filenames() {
    assert_eq!(GeneratorType::ArkType.default_filename(), "arktype.ts");
    assert_eq!(GeneratorType::Effect.default_filename(), "bindings.ts");
    assert_eq!(GeneratorType::Macroforge.default_filename(), "macroforge.ts");
    assert_eq!(GeneratorType::FlatBuffers.default_filename(), "schema.fbs");
    assert_eq!(GeneratorType::Protobuf.default_filename(), "schema.proto");
}

// ============================================================================
// Type Generator Tests
// ============================================================================

#[test]
fn test_type_generator_generate_arktype() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    // The playground uses #[derive(Evenframe)] directly, no apply_aliases needed
    let config = BuildConfig::builder()
        .scan_path(get_playground_root_path())
        .output_path(temp_dir.path())
        .enable_arktype()
        .disable_effect()
        .disable_macroforge()
        .disable_flatbuffers()
        .disable_protobuf()
        .build();

    let generator = TypeGenerator::new(config);
    let result = generator.generate_arktype();

    assert!(result.is_ok(), "ArkType generation should succeed: {:?}", result);

    let generated = result.unwrap();
    assert_eq!(generated.generator_type, GeneratorType::ArkType);
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

    let config = BuildConfig::builder()
        .scan_path(get_playground_root_path())
        .output_path(temp_dir.path())
        .disable_arktype()
        .enable_effect()
        .disable_macroforge()
        .disable_flatbuffers()
        .disable_protobuf()
        .build();

    let generator = TypeGenerator::new(config);
    let result = generator.generate_effect();

    assert!(result.is_ok(), "Effect generation should succeed: {:?}", result);

    let generated = result.unwrap();
    assert_eq!(generated.generator_type, GeneratorType::Effect);
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

    let config = BuildConfig::builder()
        .scan_path(get_playground_root_path())
        .output_path(temp_dir.path())
        .disable_arktype()
        .disable_effect()
        .enable_macroforge()
        .disable_flatbuffers()
        .disable_protobuf()
        .build();

    let generator = TypeGenerator::new(config);
    let result = generator.generate_macroforge();

    assert!(
        result.is_ok(),
        "Macroforge generation should succeed: {:?}",
        result
    );

    let generated = result.unwrap();
    assert_eq!(generated.generator_type, GeneratorType::Macroforge);
    assert!(generated.bytes_written > 0);
    assert!(generated.path.exists());
}

#[test]
fn test_type_generator_generate_flatbuffers() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    let config = BuildConfig::builder()
        .scan_path(get_playground_root_path())
        .output_path(temp_dir.path())
        .disable_arktype()
        .disable_effect()
        .disable_macroforge()
        .enable_flatbuffers(Some("Playground".to_string()))
        .disable_protobuf()
        .build();

    let generator = TypeGenerator::new(config);
    let result = generator.generate_flatbuffers();

    assert!(
        result.is_ok(),
        "FlatBuffers generation should succeed: {:?}",
        result
    );

    let generated = result.unwrap();
    assert_eq!(generated.generator_type, GeneratorType::FlatBuffers);
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

    let config = BuildConfig::builder()
        .scan_path(get_playground_root_path())
        .output_path(temp_dir.path())
        .disable_arktype()
        .disable_effect()
        .disable_macroforge()
        .disable_flatbuffers()
        .enable_protobuf(Some("playground".to_string()), false)
        .build();

    let generator = TypeGenerator::new(config);
    let result = generator.generate_protobuf();

    assert!(
        result.is_ok(),
        "Protobuf generation should succeed: {:?}",
        result
    );

    let generated = result.unwrap();
    assert_eq!(generated.generator_type, GeneratorType::Protobuf);
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

    let config = BuildConfig::builder()
        .scan_path(get_playground_root_path())
        .output_path(temp_dir.path())
        .enable_all()
        .build();

    let generator = TypeGenerator::new(config);
    let result = generator.generate_all();

    assert!(result.is_ok(), "generate_all should succeed: {:?}", result);

    let report = result.unwrap();

    // Should have generated 5 files (one for each generator type)
    assert_eq!(
        report.files.len(),
        5,
        "Should generate 5 files when all generators are enabled"
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

    // Verify specific files exist
    let arktype_file = temp_dir.path().join("arktype.ts");
    let effect_file = temp_dir.path().join("bindings.ts");
    let macroforge_file = temp_dir.path().join("macroforge.ts");
    let fbs_file = temp_dir.path().join("schema.fbs");
    let proto_file = temp_dir.path().join("schema.proto");

    assert!(arktype_file.exists(), "arktype.ts should exist");
    assert!(effect_file.exists(), "bindings.ts should exist");
    assert!(macroforge_file.exists(), "macroforge.ts should exist");
    assert!(fbs_file.exists(), "schema.fbs should exist");
    assert!(proto_file.exists(), "schema.proto should exist");
}

#[test]
fn test_type_generator_generate_subset() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    let config = BuildConfig::builder()
        .scan_path(get_playground_root_path())
        .output_path(temp_dir.path())
        .enable_arktype()
        .enable_effect()
        .disable_macroforge()
        .disable_flatbuffers()
        .disable_protobuf()
        .build();

    let generator = TypeGenerator::new(config);
    let result = generator.generate_all();

    assert!(result.is_ok(), "generate_all should succeed");

    let report = result.unwrap();
    assert_eq!(report.files.len(), 2, "Should generate 2 files");

    // Verify generator types
    let generator_types: Vec<_> = report.files.iter().map(|f| f.generator_type).collect();
    assert!(generator_types.contains(&GeneratorType::ArkType));
    assert!(generator_types.contains(&GeneratorType::Effect));
    assert!(!generator_types.contains(&GeneratorType::Macroforge));
    assert!(!generator_types.contains(&GeneratorType::FlatBuffers));
    assert!(!generator_types.contains(&GeneratorType::Protobuf));
}

// ============================================================================
// Generation Report Tests
// ============================================================================

#[test]
fn test_generation_report_contains_type_counts() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    let config = BuildConfig::builder()
        .scan_path(get_playground_root_path())
        .output_path(temp_dir.path())
        .enable_arktype()
        .build();

    let generator = TypeGenerator::new(config);
    let report = generator.generate_all().expect("Should generate");

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

    let config = BuildConfig::builder()
        .scan_path(get_playground_root_path())
        .output_path(temp_dir.path())
        .enable_arktype()
        .build();

    let generator = TypeGenerator::new(config);
    let generated = generator.generate_arktype().expect("Should generate");

    let content = fs::read_to_string(&generated.path).expect("Should read");

    // Check for expected type names from the playground models
    let expected_types = ["User", "Session", "Role"];
    let mut found_types = Vec::new();

    for type_name in &expected_types {
        if content.contains(type_name) {
            found_types.push(*type_name);
        }
    }

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

    let config = BuildConfig::builder()
        .scan_path(get_playground_root_path())
        .output_path(temp_dir.path())
        .enable_flatbuffers(Some("Test".to_string()))
        .build();

    let generator = TypeGenerator::new(config);
    let generated = generator.generate_flatbuffers().expect("Should generate");

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

    let config = BuildConfig::builder()
        .scan_path(get_playground_root_path())
        .output_path(temp_dir.path())
        .enable_protobuf(Some("test".to_string()), false)
        .build();

    let generator = TypeGenerator::new(config);
    let generated = generator.generate_protobuf().expect("Should generate");

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
        .output_path(temp_dir.path())
        .enable_arktype()
        .build();

    let generator = TypeGenerator::new(config);
    let result = generator.generate_all();

    // This might succeed with empty results or fail - both are acceptable
    // The important thing is it doesn't panic
    match result {
        Ok(report) => {
            // If it succeeds, it should have processed 0 types
            println!(
                "Generated {} files from invalid path",
                report.files.len()
            );
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

    let config = BuildConfig::builder()
        .scan_path(get_playground_root_path())
        .output_path(&nested_output)
        .enable_arktype()
        .build();

    let generator = TypeGenerator::new(config);
    let result = generator.generate_all();

    assert!(result.is_ok(), "Should create nested directories");
    assert!(nested_output.exists(), "Output directory should be created");
}

// ============================================================================
// Integration Tests
// ============================================================================

#[test]
fn test_generate_with_config_function() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    let config = BuildConfig::builder()
        .scan_path(get_playground_root_path())
        .output_path(temp_dir.path())
        .enable_arktype()
        .build();

    // Use the top-level generate_with_config function
    let result = evenframe_build::generate_with_config(config);

    assert!(
        result.is_ok(),
        "generate_with_config should succeed: {:?}",
        result
    );
}

#[test]
fn test_generate_multiple_times() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    let config = BuildConfig::builder()
        .scan_path(get_playground_root_path())
        .output_path(temp_dir.path())
        .enable_arktype()
        .build();

    // Generate multiple times to ensure idempotency
    let generator = TypeGenerator::new(config.clone());

    let result1 = generator.generate_all();
    assert!(result1.is_ok());

    let generator2 = TypeGenerator::new(config);
    let result2 = generator2.generate_all();
    assert!(result2.is_ok());

    // Both should produce the same number of files
    assert_eq!(result1.unwrap().files.len(), result2.unwrap().files.len());
}
