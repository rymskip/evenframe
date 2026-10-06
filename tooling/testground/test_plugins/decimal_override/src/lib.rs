//! Test output-rule plugin: annotates Decimal fields with a marker when the
//! struct derives Serialize, and annotates `@internal` fields with a
//! "stripped" marker so the test can verify the plugin observed them.
//!
//! This plugin targets the current `TypeContext` / `OutputRulePluginOutput`
//! API, which exposes the full struct/enum config as JSON. Derive presence and
//! per-field type strings are read via the small helpers below.

use evenframe_plugin::{FieldOverride, OutputRulePluginOutput, define_output_rule_plugin};

define_output_rule_plugin!(|ctx: &TypeContext| {
    let mut output = OutputRulePluginOutput::default();

    let has_serialize = ctx
        .rust_derives()
        .iter()
        .any(|derive| derive == "Serialize");

    for field in ctx.fields() {
        let name = field.field_name().unwrap_or("").to_string();
        let ft = field.field_type_name();

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
        if field
            .annotations()
            .iter()
            .any(|annotation| annotation.contains("@internal"))
        {
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
