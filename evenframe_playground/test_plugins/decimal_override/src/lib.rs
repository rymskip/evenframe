//! Test output-rule plugin: annotates Decimal fields with a marker when the
//! struct derives Serialize, and annotates `@internal` fields with a
//! "stripped" marker so the test can verify the plugin observed them.
//!
//! This plugin targets the current `TypeContext` / `OutputRulePluginOutput`
//! API, which exposes the full struct/enum config as JSON. Derive presence and
//! per-field type strings are read via the small helpers below.

use evenframe_plugin::{
    FieldOverride, OutputRulePluginOutput, TypeContext, TypeFieldInfo, define_output_rule_plugin,
    serde_json,
};

/// The struct/enum-level `rust_derives` declared in the Rust source.
fn rust_derives(ctx: &TypeContext) -> Vec<String> {
    let cfg = match ctx {
        TypeContext::Struct { config, .. } | TypeContext::Enum { config, .. } => config,
        TypeContext::Table { struct_config, .. } => struct_config,
    };
    cfg.get("rust_derives")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// A field's `FieldType` rendered back to its source-type string. Unit
/// variants serialize as a bare string (`"String"`); newtype variants such as
/// `Other("Decimal")` serialize as `{"Other":"Decimal"}` — both collapse to the
/// type name the plugin compares against.
fn field_type_str(f: &TypeFieldInfo) -> String {
    match f.node.get("field_type") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Object(o)) => o
            .values()
            .next()
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_default(),
        _ => String::new(),
    }
}

define_output_rule_plugin!(|ctx: &TypeContext| {
    let mut output = OutputRulePluginOutput::default();

    let has_serialize = rust_derives(ctx).iter().any(|d| d == "Serialize");

    for field in ctx.fields() {
        let name = field.field_name().unwrap_or("").to_string();
        let ft = field_type_str(&field);

        // Serialize + Decimal → annotate with @bigdecimal.
        if has_serialize && ft == "Decimal" {
            output
                .field_overrides
                .entry(name.clone())
                .or_insert_with(FieldOverride::default)
                .annotations
                .push("@bigdecimal".to_string());
        }

        // @internal fields get a visible "@internal_stripped" marker.
        if field.annotations().iter().any(|a| a.contains("@internal")) {
            output
                .field_overrides
                .entry(name.clone())
                .or_insert_with(FieldOverride::default)
                .annotations
                .push("@internal_stripped".to_string());
        }
    }

    // Add a type-level annotation when any field-level override fired, so the
    // test can verify type-level emission works too.
    if !output.field_overrides.is_empty() {
        output
            .type_override
            .annotations
            .push("@decimal_override_applied".to_string());
    }

    output
});
