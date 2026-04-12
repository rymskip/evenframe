//! Test output-rule plugin: annotates Decimal fields with a marker when the
//! struct derives Serialize, and annotates `@internal` fields with a
//! "stripped" marker so the test can verify the plugin observed them.
//!
//! This plugin targets the current `OutputRulePluginOutput` API, which only
//! supports type-level derive/annotation/permission/event injection and
//! field-level annotation injection. It does NOT do type substitution or
//! skip fields — those capabilities no longer exist in the plugin surface.

use evenframe_plugin::{FieldOverride, OutputRulePluginOutput, define_output_rule_plugin};

define_output_rule_plugin!(|ctx: &TypeContext| {
    let mut output = OutputRulePluginOutput::default();

    let has_serialize = ctx.rust_derives.iter().any(|d| d == "Serialize");

    for field in &ctx.fields {
        // Serialize + Decimal → annotate with @bigdecimal.
        if has_serialize && field.field_type == "Decimal" {
            output
                .field_overrides
                .entry(field.field_name.clone())
                .or_insert_with(FieldOverride::default)
                .annotations
                .push("@bigdecimal".to_string());
        }

        // @internal fields get a visible "@internal_stripped" marker.
        if field.annotations.iter().any(|a| a.contains("@internal")) {
            output
                .field_overrides
                .entry(field.field_name.clone())
                .or_insert_with(FieldOverride::default)
                .annotations
                .push("@internal_stripped".to_string());
        }
    }

    // Add a type-level annotation when any monetary override fired, so the
    // test can verify type-level emission works too.
    if !output.field_overrides.is_empty() {
        output
            .type_override
            .annotations
            .push("@decimal_override_applied".to_string());
    }

    output
});
