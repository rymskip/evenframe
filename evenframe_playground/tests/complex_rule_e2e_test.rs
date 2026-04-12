//! E2E tests for the complex monetary-branding output-rule plugin.
//!
//! The rule requires ALL preconditions:
//!   1. Struct derives both Serialize AND Deserialize
//!   2. Struct has @monetary annotation
//!   3. Generator is "effect" or "macroforge"
//!   4. Pipeline is "Both" or "Typesync"
//!   5. At least one Decimal/f64/i64 field exists (without @raw)
//!   6. A currency field (String type, "currency" in name) exists
//!
//! When all hold, monetary fields get `@brand("MonetaryAmount")` and
//! `@monetary(currency_field=X)` annotations, the type gets a
//! `@rename("<Name>Monetary")` annotation, the currency field gets `@iso4217`,
//! etc. (See `test_plugins/complex_rule/src/lib.rs`.)
//!
//! Run with: `cargo test --test complex_rule_e2e_test --features wasm-plugins`

#![cfg(feature = "wasm-plugins")]

use evenframe_core::config::OutputRulePluginConfig;
use evenframe_core::typesync::plugin::OutputRulePluginManager;
use evenframe_core::typesync::plugin_types::{
    OutputRulePluginFieldInfo, OutputRulePluginInput, OutputRulePluginOutput, TypeKind,
};
use std::collections::HashMap;
use std::path::PathBuf;

fn playground_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn mgr() -> OutputRulePluginManager {
    let mut plugins = HashMap::new();
    plugins.insert(
        "complex".to_string(),
        OutputRulePluginConfig {
            path: ".evenframe/plugins/complex_rule.wasm".to_string(),
        },
    );
    OutputRulePluginManager::new(&plugins, &playground_root())
        .expect("failed to load complex_rule plugin")
}

fn f(name: &str, ty: &str) -> OutputRulePluginFieldInfo {
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

fn f_ann(name: &str, ty: &str, anns: Vec<&str>) -> OutputRulePluginFieldInfo {
    let mut out = f(name, ty);
    out.annotations = anns.into_iter().map(|s| s.to_string()).collect();
    out
}

fn f_val(name: &str, ty: &str, vals: Vec<&str>) -> OutputRulePluginFieldInfo {
    let mut out = f(name, ty);
    out.validators = vals.into_iter().map(|s| s.to_string()).collect();
    out
}

/// Fluent builder for `OutputRulePluginInput` — keeps the test setup
/// concise without a megadose positional helper.
struct Builder {
    inner: OutputRulePluginInput,
}

impl Builder {
    fn new(name: &str) -> Self {
        Self {
            inner: OutputRulePluginInput {
                type_name: name.to_string(),
                kind: TypeKind::Struct,
                rust_derives: vec![],
                annotations: vec![],
                pipeline: "Both".to_string(),
                generator: "effect".to_string(),
                fields: vec![],
                table_name: String::new(),
                is_relation: false,
                has_explicit_permissions: false,
                has_explicit_events: false,
                has_explicit_mock_data: false,
                existing_macroforge_derives: vec![],
            },
        }
    }

    fn derives(mut self, d: Vec<&str>) -> Self {
        self.inner.rust_derives = d.into_iter().map(|s| s.to_string()).collect();
        self
    }

    fn annotations(mut self, a: Vec<&str>) -> Self {
        self.inner.annotations = a.into_iter().map(|s| s.to_string()).collect();
        self
    }

    fn pipeline(mut self, p: &str) -> Self {
        self.inner.pipeline = p.to_string();
        self
    }

    fn generator(mut self, g: &str) -> Self {
        self.inner.generator = g.to_string();
        self
    }

    fn fields(mut self, f: Vec<OutputRulePluginFieldInfo>) -> Self {
        self.inner.fields = f;
        self
    }

    fn build(self) -> OutputRulePluginInput {
        self.inner
    }
}

fn full_match_input(generator: &str) -> OutputRulePluginInput {
    Builder::new("Invoice")
        .derives(vec!["Debug", "Clone", "Serialize", "Deserialize"])
        .annotations(vec!["@monetary"])
        .generator(generator)
        .fields(vec![
            f("id", "String"),
            f("total", "Decimal"),
            f("tax", "f64"),
            f("line_count", "i64"),
            f("currency_code", "String"),
            f("description", "String"),
        ])
        .build()
}

fn ann(output: &OutputRulePluginOutput, field_name: &str) -> Vec<String> {
    output
        .field_overrides
        .get(field_name)
        .map(|fo| fo.annotations.clone())
        .unwrap_or_default()
}

fn type_annotations(output: &OutputRulePluginOutput) -> &[String] {
    &output.type_override.annotations
}

// ============================================================================
// Full match: all preconditions met
// ============================================================================

#[test]
fn full_match_effect_generator_brands_monetary_fields() {
    let mut pm = mgr();
    let result = pm.transform_type("complex", &full_match_input("effect")).unwrap();
    assert!(result.error.is_none());

    // Type-level rename + generator + count annotations
    assert!(
        type_annotations(&result)
            .iter()
            .any(|a| a == "@rename(\"InvoiceMonetary\")"),
        "expected @rename; got: {:?}",
        type_annotations(&result)
    );
    assert!(
        type_annotations(&result)
            .iter()
            .any(|a| a == "@generator(\"effect\")")
    );
    assert!(
        type_annotations(&result)
            .iter()
            .any(|a| a == "@monetary_count(3)"),
        "expected @monetary_count(3); got: {:?}",
        type_annotations(&result)
    );

    // Every monetary field has @brand + @monetary annotations referring to
    // the discovered currency field.
    for field_name in ["total", "tax", "line_count"] {
        let anns = ann(&result, field_name);
        assert!(
            anns.contains(&"@brand(\"MonetaryAmount\")".to_string()),
            "field `{}` missing @brand annotation; got: {:?}",
            field_name,
            anns
        );
        assert!(
            anns.iter()
                .any(|a| a.contains("currency_field") && a.contains("currency_code")),
            "field `{}` missing @monetary linking to currency_code; got: {:?}",
            field_name,
            anns
        );
    }

    // Non-monetary fields are untouched.
    assert!(!result.field_overrides.contains_key("id"));
    assert!(!result.field_overrides.contains_key("description"));

    // Currency field gets @iso4217.
    assert!(
        ann(&result, "currency_code").contains(&"@iso4217".to_string()),
        "currency_code missing @iso4217; got: {:?}",
        ann(&result, "currency_code")
    );
}

#[test]
fn full_match_macroforge_generator_uses_macroforge_generator_annotation() {
    let mut pm = mgr();
    let result = pm
        .transform_type("complex", &full_match_input("macroforge"))
        .unwrap();

    assert!(
        type_annotations(&result)
            .iter()
            .any(|a| a == "@generator(\"macroforge\")"),
        "expected macroforge generator annotation; got: {:?}",
        type_annotations(&result)
    );
    // @brand still fires for macroforge.
    assert!(ann(&result, "total").contains(&"@brand(\"MonetaryAmount\")".to_string()));
}

// ============================================================================
// Each precondition failing individually blocks the main rule
// ============================================================================

#[test]
fn missing_serialize_blocks_rule() {
    let mut pm = mgr();
    let i = Builder::new("Invoice")
        .derives(vec!["Debug", "Deserialize"])
        .annotations(vec!["@monetary"])
        .fields(vec![f("total", "Decimal"), f("currency_code", "String")])
        .build();
    let result = pm.transform_type("complex", &i).unwrap();
    assert!(
        !type_annotations(&result)
            .iter()
            .any(|a| a.starts_with("@rename("))
    );
    assert!(ann(&result, "total").is_empty());
}

#[test]
fn missing_deserialize_blocks_rule() {
    let mut pm = mgr();
    let i = Builder::new("Invoice")
        .derives(vec!["Serialize"])
        .annotations(vec!["@monetary"])
        .fields(vec![f("total", "Decimal"), f("currency_code", "String")])
        .build();
    let result = pm.transform_type("complex", &i).unwrap();
    assert!(ann(&result, "total").is_empty());
}

#[test]
fn missing_monetary_type_annotation_blocks_rule() {
    let mut pm = mgr();
    let i = Builder::new("Invoice")
        .derives(vec!["Serialize", "Deserialize"])
        .fields(vec![f("total", "Decimal"), f("currency_code", "String")])
        .build();
    let result = pm.transform_type("complex", &i).unwrap();
    assert!(ann(&result, "total").is_empty());
}

#[test]
fn wrong_generator_blocks_rule() {
    let mut pm = mgr();
    let i = Builder::new("Invoice")
        .derives(vec!["Serialize", "Deserialize"])
        .annotations(vec!["@monetary"])
        .generator("arktype")
        .fields(vec![f("total", "Decimal"), f("currency_code", "String")])
        .build();
    let result = pm.transform_type("complex", &i).unwrap();
    assert!(ann(&result, "total").is_empty());
}

#[test]
fn wrong_pipeline_blocks_rule() {
    let mut pm = mgr();
    let i = Builder::new("Invoice")
        .derives(vec!["Serialize", "Deserialize"])
        .annotations(vec!["@monetary"])
        .pipeline("Schemasync")
        .fields(vec![f("total", "Decimal"), f("currency_code", "String")])
        .build();
    let result = pm.transform_type("complex", &i).unwrap();
    assert!(ann(&result, "total").is_empty());
}

#[test]
fn no_monetary_field_blocks_rule() {
    let mut pm = mgr();
    let i = Builder::new("Invoice")
        .derives(vec!["Serialize", "Deserialize"])
        .annotations(vec!["@monetary"])
        .fields(vec![f("name", "String"), f("currency_code", "String")])
        .build();
    let result = pm.transform_type("complex", &i).unwrap();
    assert!(
        !type_annotations(&result)
            .iter()
            .any(|a| a.starts_with("@rename("))
    );
}

#[test]
fn no_currency_field_blocks_rule() {
    let mut pm = mgr();
    let i = Builder::new("Invoice")
        .derives(vec!["Serialize", "Deserialize"])
        .annotations(vec!["@monetary"])
        .fields(vec![f("total", "Decimal"), f("name", "String")])
        .build();
    let result = pm.transform_type("complex", &i).unwrap();
    assert!(ann(&result, "total").is_empty());
}

// ============================================================================
// @raw annotation exempts a monetary field
// ============================================================================

#[test]
fn raw_annotated_field_is_skipped() {
    let mut pm = mgr();
    let i = Builder::new("Invoice")
        .derives(vec!["Serialize", "Deserialize"])
        .annotations(vec!["@monetary"])
        .fields(vec![
            f("total", "Decimal"),
            f_ann("raw_total", "Decimal", vec!["@raw"]),
            f("currency_code", "String"),
        ])
        .build();
    let result = pm.transform_type("complex", &i).unwrap();
    assert!(ann(&result, "total").contains(&"@brand(\"MonetaryAmount\")".to_string()));
    assert!(
        ann(&result, "raw_total").is_empty(),
        "@raw field must not receive any annotation; got: {:?}",
        ann(&result, "raw_total")
    );
}

// ============================================================================
// `raw_amount` is skipped by name
// ============================================================================

#[test]
fn raw_amount_field_is_marked_skipped() {
    let mut pm = mgr();
    let i = Builder::new("Invoice")
        .derives(vec!["Serialize", "Deserialize"])
        .annotations(vec!["@monetary"])
        .fields(vec![
            f("total", "Decimal"),
            f("raw_amount", "Decimal"),
            f("currency_code", "String"),
        ])
        .build();
    let result = pm.transform_type("complex", &i).unwrap();
    assert!(ann(&result, "raw_amount").contains(&"@skip_raw_amount".to_string()));
    assert!(!ann(&result, "raw_amount").contains(&"@brand(\"MonetaryAmount\")".to_string()));
    assert!(ann(&result, "total").contains(&"@brand(\"MonetaryAmount\")".to_string()));
}

// ============================================================================
// Type rename: already ends with Monetary
// ============================================================================

#[test]
fn already_monetary_suffix_is_not_renamed_again() {
    let mut pm = mgr();
    let i = Builder::new("InvoiceMonetary")
        .derives(vec!["Serialize", "Deserialize"])
        .annotations(vec!["@monetary"])
        .fields(vec![f("total", "Decimal"), f("currency_code", "String")])
        .build();
    let result = pm.transform_type("complex", &i).unwrap();
    assert!(
        !type_annotations(&result)
            .iter()
            .any(|a| a.starts_with("@rename(")),
        "type already ending in Monetary should not be renamed; got: {:?}",
        type_annotations(&result)
    );
    // But the other monetary annotations still fire.
    assert!(ann(&result, "total").contains(&"@brand(\"MonetaryAmount\")".to_string()));
}

// ============================================================================
// Always-on rules run regardless of monetary gate
// ============================================================================

#[test]
fn internal_annotation_triggers_skip_marker_without_monetary_gate() {
    let mut pm = mgr();
    let i = Builder::new("Simple")
        .derives(vec!["Debug"])
        .generator("macroforge")
        .fields(vec![
            f("name", "String"),
            f_ann("secret", "String", vec!["@internal"]),
        ])
        .build();
    let result = pm.transform_type("complex", &i).unwrap();
    assert!(ann(&result, "secret").contains(&"@skip_internal".to_string()));
}

#[test]
fn heavily_validated_annotation_fires() {
    let mut pm = mgr();
    let i = Builder::new("Validated")
        .generator("macroforge")
        .fields(vec![
            f_val(
                "email",
                "String",
                vec!["email", "min_length(5)", "max_length(255)"],
            ),
            f_val("name", "String", vec!["min_length(1)"]),
        ])
        .build();
    let result = pm.transform_type("complex", &i).unwrap();
    assert!(
        ann(&result, "email").contains(&"@heavily_validated".to_string()),
        "email with 3 validators should get @heavily_validated; got: {:?}",
        ann(&result, "email")
    );
    assert!(
        !ann(&result, "name").contains(&"@heavily_validated".to_string()),
        "name with 1 validator should not get @heavily_validated"
    );
}

#[test]
fn nested_collection_detection_fires() {
    let mut pm = mgr();
    let i = Builder::new("Order")
        .generator("macroforge")
        .fields(vec![
            f("items", "Vec<LineItem>"),
            f("line_item_ref", "LineItem"),
            f("name", "String"),
        ])
        .build();
    let result = pm.transform_type("complex", &i).unwrap();
    assert!(
        ann(&result, "items")
            .iter()
            .any(|a| a.contains("@nested_collection") && a.contains("LineItem")),
        "Vec<LineItem> should get @nested_collection when LineItem appears as a field type; got: {:?}",
        ann(&result, "items")
    );
}

// ============================================================================
// Typesync pipeline passes the gate
// ============================================================================

#[test]
fn typesync_pipeline_passes_main_rule() {
    let mut pm = mgr();
    let i = Builder::new("Invoice")
        .derives(vec!["Serialize", "Deserialize"])
        .annotations(vec!["@monetary"])
        .pipeline("Typesync")
        .fields(vec![f("total", "Decimal"), f("currency_code", "String")])
        .build();
    let result = pm.transform_type("complex", &i).unwrap();
    assert!(
        ann(&result, "total").contains(&"@brand(\"MonetaryAmount\")".to_string()),
        "Typesync pipeline should pass the gate; got: {:?}",
        ann(&result, "total")
    );
}

// ============================================================================
// Multiple monetary types mixed
// ============================================================================

#[test]
fn mixed_monetary_types_all_get_branded() {
    let mut pm = mgr();
    let i = Builder::new("FinancialRecord")
        .derives(vec!["Serialize", "Deserialize"])
        .annotations(vec!["@monetary"])
        .fields(vec![
            f("decimal_amount", "Decimal"),
            f("float_amount", "f64"),
            f("int_amount", "i64"),
            f("name", "String"),
            f("currency", "String"),
        ])
        .build();
    let result = pm.transform_type("complex", &i).unwrap();

    for field_name in ["decimal_amount", "float_amount", "int_amount"] {
        assert!(
            ann(&result, field_name).contains(&"@brand(\"MonetaryAmount\")".to_string()),
            "{} missing @brand; got: {:?}",
            field_name,
            ann(&result, field_name)
        );
    }
    assert!(!result.field_overrides.contains_key("name"));

    assert!(
        type_annotations(&result)
            .iter()
            .any(|a| a == "@monetary_count(3)"),
        "expected @monetary_count(3); got: {:?}",
        type_annotations(&result)
    );
}

// ============================================================================
// Stability under load
// ============================================================================

#[test]
fn fifty_rapid_calls_stable() {
    let mut pm = mgr();
    for i in 0..50 {
        let name = format!("Type{}", i);
        let input = Builder::new(&name)
            .derives(vec!["Serialize", "Deserialize"])
            .annotations(vec!["@monetary"])
            .generator(if i % 2 == 0 { "effect" } else { "macroforge" })
            .fields(vec![f("amount", "Decimal"), f("currency_code", "String")])
            .build();
        let result = pm.transform_type("complex", &input);
        assert!(result.is_ok(), "call {} failed: {:?}", i, result.err());
        let output = result.unwrap();
        assert!(output.error.is_none());
        assert!(ann(&output, "amount").contains(&"@brand(\"MonetaryAmount\")".to_string()));
    }
}

// ============================================================================
// Kitchen sink
// ============================================================================

#[test]
fn kitchen_sink_everything_at_once() {
    let mut pm = mgr();
    let i = Builder::new("MegaInvoiceDto")
        .derives(vec![
            "Debug",
            "Clone",
            "Serialize",
            "Deserialize",
            "PartialEq",
        ])
        .annotations(vec!["@monetary", "@audit"])
        .generator("effect")
        .fields(vec![
            f("id", "String"),
            f("total", "Decimal"),                              // monetary
            f("tax", "f64"),                                    // monetary
            f("item_count", "i64"),                             // monetary
            f_ann("raw_total", "Decimal", vec!["@raw"]),        // exempted by @raw
            f("raw_amount", "Decimal"),                         // skipped by name
            f_ann("secret_key", "String", vec!["@internal"]),   // @skip_internal
            f("currency_code", "String"),                       // @iso4217
            f("created_at", "String"),
            f_val(
                "email",
                "String",
                vec!["email", "min_length(3)", "max_length(255)"],
            ), // @heavily_validated
            f("items", "Vec<LineItem>"),                        // @nested_collection
            f("metadata", "LineItem"),                          // struct ref
            f("description", "String"),
        ])
        .build();
    let result = pm.transform_type("complex", &i).unwrap();
    assert!(result.error.is_none());

    // Type rename — MegaInvoiceDto does not end with Monetary.
    assert!(
        type_annotations(&result)
            .iter()
            .any(|a| a == "@rename(\"MegaInvoiceDtoMonetary\")")
    );
    assert!(
        type_annotations(&result)
            .iter()
            .any(|a| a == "@generator(\"effect\")")
    );

    // Monetary brand markers.
    for field_name in ["total", "tax", "item_count"] {
        assert!(
            ann(&result, field_name).contains(&"@brand(\"MonetaryAmount\")".to_string()),
            "{} missing @brand",
            field_name
        );
    }

    // @raw exempted.
    assert!(ann(&result, "raw_total").is_empty());

    // Skip markers.
    assert!(ann(&result, "raw_amount").contains(&"@skip_raw_amount".to_string()));
    assert!(ann(&result, "secret_key").contains(&"@skip_internal".to_string()));

    // Currency @iso4217.
    assert!(ann(&result, "currency_code").contains(&"@iso4217".to_string()));

    // @heavily_validated on email.
    assert!(ann(&result, "email").contains(&"@heavily_validated".to_string()));

    // @nested_collection on items.
    assert!(
        ann(&result, "items")
            .iter()
            .any(|a| a.contains("@nested_collection"))
    );

    // Monetary count embedded as a type annotation.
    assert!(
        type_annotations(&result)
            .iter()
            .any(|a| a == "@monetary_count(3)")
    );
}
