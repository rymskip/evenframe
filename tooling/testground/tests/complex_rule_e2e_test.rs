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
use evenframe_core::error::EvenframeError;
use evenframe_core::types::{FieldType, StructConfig, StructField};
use evenframe_core::typesync::plugin::OutputRulePluginManager;
use evenframe_core::typesync::plugin_types::{OutputRulePluginInput, OutputRulePluginOutput};
use evenframe_core::validator::{StringValidator, Validator};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[path = "support/plugins.rs"]
mod plugins;

use plugins::build_plugin;

fn testground_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn mgr() -> OutputRulePluginManager {
    let mut plugins = BTreeMap::new();
    plugins.insert(
        "complex".to_string(),
        OutputRulePluginConfig {
            path: build_plugin("complex_rule").to_string_lossy().into_owned(),
        },
    );
    OutputRulePluginManager::new(&plugins, &testground_root())
        .expect("failed to load complex_rule plugin")
}

fn field(name: &str, ty: &str) -> StructField {
    StructField {
        field_name: name.to_string(),
        wire: Default::default(),
        field_type: FieldType::Other(ty.to_string()),
        ..Default::default()
    }
}

fn field_with_annotations(name: &str, ty: &str, anns: Vec<&str>) -> StructField {
    let mut out = field(name, ty);
    out.annotations = anns.into_iter().map(|text| text.to_string()).collect();
    out
}

fn field_with_validators(name: &str, ty: &str, vals: Vec<StringValidator>) -> StructField {
    let mut out = field(name, ty);
    out.validators = vals.into_iter().map(Validator::StringValidator).collect();
    out
}

/// Fluent builder for the struct a plugin input borrows, so test setup stays
/// concise without a long positional helper.
struct Builder {
    name: String,
    derives: Vec<String>,
    annotations: Vec<String>,
    pipeline: String,
    generator: String,
    fields: Vec<StructField>,
}

impl Builder {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            derives: vec![],
            annotations: vec![],
            pipeline: "Both".to_string(),
            generator: "effect".to_string(),
            fields: vec![],
        }
    }

    fn derives(mut self, derives: Vec<&str>) -> Self {
        self.derives = derives.into_iter().map(|text| text.to_string()).collect();
        self
    }

    fn annotations(mut self, annotation: Vec<&str>) -> Self {
        self.annotations = annotation
            .into_iter()
            .map(|text| text.to_string())
            .collect();
        self
    }

    fn pipeline(mut self, pipeline: &str) -> Self {
        self.pipeline = pipeline.to_string();
        self
    }

    fn generator(mut self, generator: &str) -> Self {
        self.generator = generator.to_string();
        self
    }

    fn fields(mut self, field: Vec<StructField>) -> Self {
        self.fields = field;
        self
    }

    fn build(self) -> BuiltStruct {
        BuiltStruct {
            pipeline: self.pipeline,
            generator: self.generator,
            config: StructConfig {
                struct_name: self.name,
                fields: self.fields,
                rust_derives: self.derives,
                annotations: self.annotations,
                ..Default::default()
            },
        }
    }
}

/// A struct the builder made, which the plugin input borrows.
struct BuiltStruct {
    pipeline: String,
    generator: String,
    config: StructConfig,
}

/// The loaded plugin's output for `built`.
fn transform(
    manager: &mut OutputRulePluginManager,
    built: &BuiltStruct,
) -> Result<OutputRulePluginOutput, EvenframeError> {
    let input = OutputRulePluginInput::Struct {
        pipeline: built.pipeline.clone(),
        generator: built.generator.clone(),
        config: &built.config,
    };
    let mut outputs = manager.transform(&input)?;
    assert_eq!(outputs.len(), 1, "one plugin is loaded");
    Ok(outputs.remove(0).1)
}

fn full_match_input(generator: &str) -> BuiltStruct {
    Builder::new("Invoice")
        .derives(vec!["Debug", "Clone", "Serialize", "Deserialize"])
        .annotations(vec!["@monetary"])
        .generator(generator)
        .fields(vec![
            field("id", "String"),
            field("total", "Decimal"),
            field("tax", "f64"),
            field("line_count", "i64"),
            field("currency_code", "String"),
            field("description", "String"),
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
    let result = transform(&mut pm, &full_match_input("effect")).unwrap();
    assert!(result.error.is_none());

    // Type-level rename + generator + count annotations
    assert!(
        type_annotations(&result)
            .iter()
            .any(|annotation| annotation == "@rename(\"InvoiceMonetary\")"),
        "expected @rename; got: {:?}",
        type_annotations(&result)
    );
    assert!(
        type_annotations(&result)
            .iter()
            .any(|annotation| annotation == "@generator(\"effect\")")
    );
    assert!(
        type_annotations(&result)
            .iter()
            .any(|annotation| annotation == "@monetary_count(3)"),
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
                .any(|annotation| annotation.contains("currency_field")
                    && annotation.contains("currency_code")),
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
    let result = transform(&mut pm, &full_match_input("macroforge")).unwrap();

    assert!(
        type_annotations(&result)
            .iter()
            .any(|annotation| annotation == "@generator(\"macroforge\")"),
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
    let built = Builder::new("Invoice")
        .derives(vec!["Debug", "Deserialize"])
        .annotations(vec!["@monetary"])
        .fields(vec![
            field("total", "Decimal"),
            field("currency_code", "String"),
        ])
        .build();
    let result = transform(&mut pm, &built).unwrap();
    assert!(
        !type_annotations(&result)
            .iter()
            .any(|annotation| annotation.starts_with("@rename("))
    );
    assert!(ann(&result, "total").is_empty());
}

#[test]
fn missing_deserialize_blocks_rule() {
    let mut pm = mgr();
    let built = Builder::new("Invoice")
        .derives(vec!["Serialize"])
        .annotations(vec!["@monetary"])
        .fields(vec![
            field("total", "Decimal"),
            field("currency_code", "String"),
        ])
        .build();
    let result = transform(&mut pm, &built).unwrap();
    assert!(ann(&result, "total").is_empty());
}

#[test]
fn missing_monetary_type_annotation_blocks_rule() {
    let mut pm = mgr();
    let built = Builder::new("Invoice")
        .derives(vec!["Serialize", "Deserialize"])
        .fields(vec![
            field("total", "Decimal"),
            field("currency_code", "String"),
        ])
        .build();
    let result = transform(&mut pm, &built).unwrap();
    assert!(ann(&result, "total").is_empty());
}

#[test]
fn wrong_generator_blocks_rule() {
    let mut pm = mgr();
    let built = Builder::new("Invoice")
        .derives(vec!["Serialize", "Deserialize"])
        .annotations(vec!["@monetary"])
        .generator("arktype")
        .fields(vec![
            field("total", "Decimal"),
            field("currency_code", "String"),
        ])
        .build();
    let result = transform(&mut pm, &built).unwrap();
    assert!(ann(&result, "total").is_empty());
}

#[test]
fn wrong_pipeline_blocks_rule() {
    let mut pm = mgr();
    let built = Builder::new("Invoice")
        .derives(vec!["Serialize", "Deserialize"])
        .annotations(vec!["@monetary"])
        .pipeline("Schemasync")
        .fields(vec![
            field("total", "Decimal"),
            field("currency_code", "String"),
        ])
        .build();
    let result = transform(&mut pm, &built).unwrap();
    assert!(ann(&result, "total").is_empty());
}

#[test]
fn no_monetary_field_blocks_rule() {
    let mut pm = mgr();
    let built = Builder::new("Invoice")
        .derives(vec!["Serialize", "Deserialize"])
        .annotations(vec!["@monetary"])
        .fields(vec![
            field("name", "String"),
            field("currency_code", "String"),
        ])
        .build();
    let result = transform(&mut pm, &built).unwrap();
    assert!(
        !type_annotations(&result)
            .iter()
            .any(|annotation| annotation.starts_with("@rename("))
    );
}

#[test]
fn no_currency_field_blocks_rule() {
    let mut pm = mgr();
    let built = Builder::new("Invoice")
        .derives(vec!["Serialize", "Deserialize"])
        .annotations(vec!["@monetary"])
        .fields(vec![field("total", "Decimal"), field("name", "String")])
        .build();
    let result = transform(&mut pm, &built).unwrap();
    assert!(ann(&result, "total").is_empty());
}

// ============================================================================
// @raw annotation exempts a monetary field
// ============================================================================

#[test]
fn raw_annotated_field_is_skipped() {
    let mut pm = mgr();
    let built = Builder::new("Invoice")
        .derives(vec!["Serialize", "Deserialize"])
        .annotations(vec!["@monetary"])
        .fields(vec![
            field("total", "Decimal"),
            field_with_annotations("raw_total", "Decimal", vec!["@raw"]),
            field("currency_code", "String"),
        ])
        .build();
    let result = transform(&mut pm, &built).unwrap();
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
    let built = Builder::new("Invoice")
        .derives(vec!["Serialize", "Deserialize"])
        .annotations(vec!["@monetary"])
        .fields(vec![
            field("total", "Decimal"),
            field("raw_amount", "Decimal"),
            field("currency_code", "String"),
        ])
        .build();
    let result = transform(&mut pm, &built).unwrap();
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
    let built = Builder::new("InvoiceMonetary")
        .derives(vec!["Serialize", "Deserialize"])
        .annotations(vec!["@monetary"])
        .fields(vec![
            field("total", "Decimal"),
            field("currency_code", "String"),
        ])
        .build();
    let result = transform(&mut pm, &built).unwrap();
    assert!(
        !type_annotations(&result)
            .iter()
            .any(|annotation| annotation.starts_with("@rename(")),
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
    let built = Builder::new("Simple")
        .derives(vec!["Debug"])
        .generator("macroforge")
        .fields(vec![
            field("name", "String"),
            field_with_annotations("secret", "String", vec!["@internal"]),
        ])
        .build();
    let result = transform(&mut pm, &built).unwrap();
    assert!(ann(&result, "secret").contains(&"@skip_internal".to_string()));
}

#[test]
fn heavily_validated_annotation_fires() {
    let mut pm = mgr();
    let built = Builder::new("Validated")
        .generator("macroforge")
        .fields(vec![
            field_with_validators(
                "email",
                "String",
                vec![
                    StringValidator::Email,
                    StringValidator::MinLength(5),
                    StringValidator::MaxLength(255),
                ],
            ),
            field_with_validators("name", "String", vec![StringValidator::MinLength(1)]),
        ])
        .build();
    let result = transform(&mut pm, &built).unwrap();
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
    let built = Builder::new("Order")
        .generator("macroforge")
        .fields(vec![
            field("items", "Vec<LineItem>"),
            field("line_item_ref", "LineItem"),
            field("name", "String"),
        ])
        .build();
    let result = transform(&mut pm, &built).unwrap();
    assert!(
        ann(&result, "items")
            .iter()
            .any(|annotation| annotation.contains("@nested_collection")
                && annotation.contains("LineItem")),
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
    let built = Builder::new("Invoice")
        .derives(vec!["Serialize", "Deserialize"])
        .annotations(vec!["@monetary"])
        .pipeline("Typesync")
        .fields(vec![
            field("total", "Decimal"),
            field("currency_code", "String"),
        ])
        .build();
    let result = transform(&mut pm, &built).unwrap();
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
    let built = Builder::new("FinancialRecord")
        .derives(vec!["Serialize", "Deserialize"])
        .annotations(vec!["@monetary"])
        .fields(vec![
            field("decimal_amount", "Decimal"),
            field("float_amount", "f64"),
            field("int_amount", "i64"),
            field("name", "String"),
            field("currency", "String"),
        ])
        .build();
    let result = transform(&mut pm, &built).unwrap();

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
            .any(|annotation| annotation == "@monetary_count(3)"),
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
    for index in 0..50 {
        let name = format!("Type{}", index);
        let input = Builder::new(&name)
            .derives(vec!["Serialize", "Deserialize"])
            .annotations(vec!["@monetary"])
            .generator(if index % 2 == 0 {
                "effect"
            } else {
                "macroforge"
            })
            .fields(vec![
                field("amount", "Decimal"),
                field("currency_code", "String"),
            ])
            .build();
        let result = transform(&mut pm, &input);
        assert!(result.is_ok(), "call {} failed: {:?}", index, result.err());
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
    let built = Builder::new("MegaInvoiceDto")
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
            field("id", "String"),
            field("total", "Decimal"),  // monetary
            field("tax", "f64"),        // monetary
            field("item_count", "i64"), // monetary
            field_with_annotations("raw_total", "Decimal", vec!["@raw"]), // exempted by @raw
            field("raw_amount", "Decimal"), // skipped by name
            field_with_annotations("secret_key", "String", vec!["@internal"]), // @skip_internal
            field("currency_code", "String"), // @iso4217
            field("created_at", "String"),
            field_with_validators(
                "email",
                "String",
                vec![
                    StringValidator::Email,
                    StringValidator::MinLength(3),
                    StringValidator::MaxLength(255),
                ],
            ), // @heavily_validated
            field("items", "Vec<LineItem>"), // @nested_collection
            field("metadata", "LineItem"),   // struct ref
            field("description", "String"),
        ])
        .build();
    let result = transform(&mut pm, &built).unwrap();
    assert!(result.error.is_none());

    // Type rename: MegaInvoiceDto does not end with Monetary.
    assert!(
        type_annotations(&result)
            .iter()
            .any(|annotation| annotation == "@rename(\"MegaInvoiceDtoMonetary\")")
    );
    assert!(
        type_annotations(&result)
            .iter()
            .any(|annotation| annotation == "@generator(\"effect\")")
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
            .any(|annotation| annotation.contains("@nested_collection"))
    );

    // Monetary count embedded as a type annotation.
    assert!(
        type_annotations(&result)
            .iter()
            .any(|annotation| annotation == "@monetary_count(3)")
    );
}
