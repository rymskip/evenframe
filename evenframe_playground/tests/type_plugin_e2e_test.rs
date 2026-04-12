//! E2E tests for the decimal_override output-rule plugin.
//!
//! The plugin (see `test_plugins/decimal_override/src/lib.rs`) exercises the
//! current `OutputRulePluginOutput` surface: type-level annotations and
//! field-level annotations, keyed off derive presence and field annotations.
//!
//! Run with: `cargo test --test type_plugin_e2e_test --features wasm-plugins`

#![cfg(feature = "wasm-plugins")]

use evenframe_core::config::OutputRulePluginConfig;
use evenframe_core::typesync::plugin::OutputRulePluginManager;
use evenframe_core::typesync::plugin_types::{
    OutputRulePluginFieldInfo, OutputRulePluginInput, TypeKind,
};
use std::collections::HashMap;
use std::path::PathBuf;

fn playground_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn create_plugin_manager() -> OutputRulePluginManager {
    let mut plugins = HashMap::new();
    plugins.insert(
        "decimal_override".to_string(),
        OutputRulePluginConfig {
            path: ".evenframe/plugins/decimal_override.wasm".to_string(),
        },
    );
    OutputRulePluginManager::new(&plugins, &playground_root())
        .expect("failed to load decimal_override plugin")
}

fn field(name: &str, ty: &str) -> OutputRulePluginFieldInfo {
    OutputRulePluginFieldInfo {
        field_name: name.to_string(),
        field_type: ty.to_string(),
        annotations: vec![],
        validators: vec![],
        is_optional: ty.starts_with("Option"),
        record_link_target: None,
        vec_inner_type: None,
        has_explicit_format: false,
        existing_format: None,
        has_explicit_define: false,
    }
}

fn field_with_annotation(name: &str, ty: &str, annotation: &str) -> OutputRulePluginFieldInfo {
    let mut f = field(name, ty);
    f.annotations.push(annotation.to_string());
    f
}

fn struct_input(
    type_name: &str,
    derives: Vec<&str>,
    fields: Vec<OutputRulePluginFieldInfo>,
) -> OutputRulePluginInput {
    OutputRulePluginInput {
        type_name: type_name.to_string(),
        kind: TypeKind::Struct,
        rust_derives: derives.into_iter().map(|s| s.to_string()).collect(),
        annotations: vec![],
        pipeline: "Both".to_string(),
        generator: "macroforge".to_string(),
        fields,
        table_name: String::new(),
        is_relation: false,
        has_explicit_permissions: false,
        has_explicit_events: false,
        has_explicit_mock_data: false,
        existing_macroforge_derives: vec![],
    }
}

// ============================================================================
// Plugin loading
// ============================================================================

#[test]
fn decimal_override_plugin_loads() {
    let _pm = create_plugin_manager();
}

#[test]
fn decimal_override_missing_wasm_errors_at_load() {
    let mut plugins = HashMap::new();
    plugins.insert(
        "missing".to_string(),
        OutputRulePluginConfig {
            path: ".evenframe/plugins/does_not_exist.wasm".to_string(),
        },
    );
    let result = OutputRulePluginManager::new(&plugins, &playground_root());
    assert!(result.is_err(), "missing WASM file must fail at load");
}

// ============================================================================
// Decimal + Serialize → @bigdecimal annotation
// ============================================================================

#[test]
fn decimal_field_gets_bigdecimal_annotation_when_serialize_derived() {
    let mut pm = create_plugin_manager();
    let input = struct_input(
        "Payment",
        vec!["Debug", "Clone", "Serialize", "Deserialize"],
        vec![
            field("id", "String"),
            field("amount", "Decimal"),
            field("currency", "String"),
        ],
    );

    let output = pm
        .transform_type("decimal_override", &input)
        .expect("plugin call must succeed");
    assert!(output.error.is_none(), "plugin reported error: {:?}", output.error);

    let amount_annotations = output
        .field_overrides
        .get("amount")
        .map(|fo| fo.annotations.clone())
        .unwrap_or_default();
    assert!(
        amount_annotations.contains(&"@bigdecimal".to_string()),
        "expected @bigdecimal on `amount`; got: {:?}",
        amount_annotations
    );

    assert!(
        !output.field_overrides.contains_key("id"),
        "non-Decimal fields must not be overridden; got: {:?}",
        output.field_overrides.keys().collect::<Vec<_>>()
    );
    assert!(!output.field_overrides.contains_key("currency"));
}

#[test]
fn decimal_field_is_untouched_without_serialize() {
    let mut pm = create_plugin_manager();
    let input = struct_input(
        "InternalPayment",
        vec!["Debug", "Clone"], // no Serialize
        vec![field("amount", "Decimal"), field("note", "String")],
    );

    let output = pm
        .transform_type("decimal_override", &input)
        .expect("plugin call must succeed");
    assert!(
        output.field_overrides.is_empty(),
        "plugin must not annotate anything without Serialize; got: {:?}",
        output.field_overrides
    );
    assert!(output.type_override.annotations.is_empty());
}

#[test]
fn non_decimal_field_is_untouched_even_with_serialize() {
    let mut pm = create_plugin_manager();
    let input = struct_input(
        "User",
        vec!["Debug", "Serialize"],
        vec![field("name", "String"), field("age", "I32")],
    );

    let output = pm
        .transform_type("decimal_override", &input)
        .expect("plugin call must succeed");
    assert!(
        output.field_overrides.is_empty(),
        "plugin must not touch non-Decimal fields; got: {:?}",
        output.field_overrides
    );
}

// ============================================================================
// Internal field markers
// ============================================================================

#[test]
fn internal_annotated_field_gets_stripped_marker() {
    let mut pm = create_plugin_manager();
    let input = struct_input(
        "AuditLog",
        vec!["Debug"],
        vec![
            field("message", "String"),
            field_with_annotation("internal_id", "String", "@internal"),
        ],
    );

    let output = pm
        .transform_type("decimal_override", &input)
        .expect("plugin call must succeed");

    let internal_annotations = output
        .field_overrides
        .get("internal_id")
        .map(|fo| fo.annotations.clone())
        .unwrap_or_default();
    assert!(
        internal_annotations.contains(&"@internal_stripped".to_string()),
        "expected @internal_stripped on `internal_id`; got: {:?}",
        internal_annotations
    );
    assert!(!output.field_overrides.contains_key("message"));
}

// ============================================================================
// Type-level annotation surfaces when any field-level override fires
// ============================================================================

#[test]
fn type_level_annotation_appears_when_any_override_fires() {
    let mut pm = create_plugin_manager();
    let input = struct_input(
        "Order",
        vec!["Serialize"],
        vec![field("total", "Decimal"), field("note", "String")],
    );

    let output = pm
        .transform_type("decimal_override", &input)
        .expect("plugin call must succeed");

    assert!(
        output
            .type_override
            .annotations
            .contains(&"@decimal_override_applied".to_string()),
        "expected type-level marker; got: {:?}",
        output.type_override.annotations
    );
}

#[test]
fn type_level_annotation_absent_when_no_override_fires() {
    let mut pm = create_plugin_manager();
    let input = struct_input(
        "Empty",
        vec!["Debug"],
        vec![field("name", "String")],
    );

    let output = pm
        .transform_type("decimal_override", &input)
        .expect("plugin call must succeed");
    assert!(output.type_override.annotations.is_empty());
    assert!(output.field_overrides.is_empty());
}

// ============================================================================
// Stability
// ============================================================================

#[test]
fn plugin_is_stable_across_repeated_calls() {
    let mut pm = create_plugin_manager();
    let input = struct_input(
        "Widget",
        vec!["Serialize"],
        vec![field("price", "Decimal")],
    );

    let first = pm
        .transform_type("decimal_override", &input)
        .expect("first call");
    for _ in 0..10 {
        let again = pm
            .transform_type("decimal_override", &input)
            .expect("repeat call");
        assert_eq!(
            first.field_overrides.get("price").map(|f| f.annotations.clone()),
            again.field_overrides.get("price").map(|f| f.annotations.clone()),
        );
    }
}

#[test]
fn empty_struct_returns_empty_output() {
    let mut pm = create_plugin_manager();
    let input = struct_input("Blank", vec![], vec![]);
    let output = pm
        .transform_type("decimal_override", &input)
        .expect("empty input must succeed");

    assert!(output.error.is_none());
    assert!(output.field_overrides.is_empty());
    assert!(output.type_override.annotations.is_empty());
    assert!(output.type_override.macroforge_derives.is_empty());
    assert!(output.type_override.events.is_empty());
    assert!(output.type_override.permissions.is_none());
}
