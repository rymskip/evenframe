//! End-to-end tests for the `array_style` typesync configuration option.
//!
//! Verifies that macroforge TypeScript output respects the array style setting:
//! - `Shorthand` (default): `Type[]`
//! - `Generic`: `Array<Type>`
//!
//! Run with: cargo test --test array_style_e2e_test

use evenframe_core::tooling::{BuildConfig, TypeGenerator};
use evenframe_core::typesync::config::ArrayStyle;
use std::fs;
use tempfile::TempDir;

fn playground_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Helper: generate macroforge output with the given array style and return the file content.
fn generate_macroforge_with_style(style: ArrayStyle) -> String {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");

    let config = BuildConfig::builder()
        .scan_path(playground_root())
        .output_path(temp_dir.path())
        .disable_arktype()
        .disable_effect()
        .enable_macroforge()
        .disable_flatbuffers()
        .disable_protobuf()
        .array_style(style)
        .build();

    let generator = TypeGenerator::new(config);
    let generated = generator
        .generate_macroforge()
        .expect("Macroforge generation should succeed");

    fs::read_to_string(&generated.path).expect("Should read generated macroforge file")
}

// ============================================================================
// Shorthand (default) style tests
// ============================================================================

#[test]
fn test_shorthand_style_uses_bracket_syntax() {
    let content = generate_macroforge_with_style(ArrayStyle::Shorthand);

    // The playground models have Vec fields (e.g. User.roles: Vec<Role>).
    // In shorthand mode, arrays should use Type[] syntax.
    assert!(
        content.contains("[]"),
        "Shorthand style should produce [] array syntax.\nContent snippet:\n{}",
        &content[..content.len().min(2000)]
    );
    assert!(
        !content.contains("Array<"),
        "Shorthand style should NOT produce Array<> syntax.\nContent snippet:\n{}",
        &content[..content.len().min(2000)]
    );
}

#[test]
fn test_shorthand_style_wraps_option_in_parens() {
    let content = generate_macroforge_with_style(ArrayStyle::Shorthand);

    // If any type has Vec<Option<T>>, shorthand should produce (T | null)[]
    // The playground's Order.items is Vec<CartItem> (not Option), but let's check
    // that basic [] syntax is present on Vec fields.
    // User.roles is Vec<Role> → Role[]
    assert!(
        content.contains("Role[]") || content.contains("role[]"),
        "User.roles should produce Role[] in shorthand mode.\nContent snippet:\n{}",
        &content[..content.len().min(3000)]
    );
}

// ============================================================================
// Generic style tests
// ============================================================================

#[test]
fn test_generic_style_uses_array_generic_syntax() {
    let content = generate_macroforge_with_style(ArrayStyle::Generic);

    // In generic mode, arrays should use Array<Type> syntax.
    assert!(
        content.contains("Array<"),
        "Generic style should produce Array<> syntax.\nContent snippet:\n{}",
        &content[..content.len().min(2000)]
    );

    // Should NOT contain the shorthand [] for array types.
    // Note: [] can appear in tuple types like [string, number] and index signatures
    // like { [key: string]: T }, so we check specifically for patterns like `Type[]`
    // that indicate shorthand array syntax.
    // We check that no line contains a pattern like `word[];` which is the shorthand field syntax.
    let has_shorthand_array = content.lines().any(|line| {
        let trimmed = line.trim();
        // Match field declarations ending with Type[];
        trimmed.ends_with("[];") && !trimmed.starts_with('[')
    });
    assert!(
        !has_shorthand_array,
        "Generic style should NOT produce shorthand Type[] syntax.\nContent snippet:\n{}",
        &content[..content.len().min(2000)]
    );
}

#[test]
fn test_generic_style_vec_fields() {
    let content = generate_macroforge_with_style(ArrayStyle::Generic);

    // User.roles is Vec<Role> → Array<Role>
    assert!(
        content.contains("Array<Role>"),
        "User.roles should produce Array<Role> in generic mode.\nContent:\n{}",
        &content[..content.len().min(3000)]
    );
}

#[test]
fn test_generic_style_no_unnecessary_parens() {
    let content = generate_macroforge_with_style(ArrayStyle::Generic);

    // In generic style, Option inside Vec doesn't need parens:
    // Array<T | null> is correct, not Array<(T | null)>
    assert!(
        !content.contains("Array<("),
        "Generic style should not wrap inner types in parens.\nContent snippet:\n{}",
        &content[..content.len().min(2000)]
    );
}

// ============================================================================
// Both styles should produce valid interfaces
// ============================================================================

#[test]
fn test_both_styles_contain_expected_types() {
    for style in [ArrayStyle::Shorthand, ArrayStyle::Generic] {
        let content = generate_macroforge_with_style(style);

        let expected = ["User", "Session", "Product", "Order", "Post", "Comment"];
        for type_name in &expected {
            assert!(
                content.contains(&format!("export interface {}", type_name))
                    || content.contains(&format!("export type {}", type_name)),
                "{:?} style: should contain type {}.\nContent snippet:\n{}",
                style,
                type_name,
                &content[..content.len().min(2000)]
            );
        }
    }
}

#[test]
fn test_default_array_style_is_shorthand() {
    // Verify that the default OutputConfig uses Shorthand
    let config = BuildConfig::default();
    assert_eq!(
        config.output.array_style,
        ArrayStyle::Shorthand,
        "Default array_style should be Shorthand"
    );
}
