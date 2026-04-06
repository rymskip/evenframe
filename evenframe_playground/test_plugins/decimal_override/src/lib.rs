//! Test output-rule plugin: overrides Decimal fields to BigDecimal.BigDecimal
//! when the struct derives Serialize.

use evenframe_plugin::{OutputRulePluginOutput, define_type_plugin};

define_type_plugin!(|ctx: &TypeContext| {
    let mut output = OutputRulePluginOutput::default();

    // Only apply when the struct derives Serialize
    if ctx.rust_derives.iter().any(|d| d == "Serialize") {
        for field in &ctx.fields {
            if field.field_type == "Decimal" {
                output.field_type_overrides.insert(
                    field.field_name.clone(),
                    "BigDecimal.BigDecimal".to_string(),
                );
                output.extra_imports.push(
                    "import type { BigDecimal } from 'effect';".to_string(),
                );
            }
        }
    }

    // Skip fields annotated with @internal
    for field in &ctx.fields {
        if field.annotations.iter().any(|a| a.contains("@internal")) {
            output.skip_fields.push(field.field_name.clone());
        }
    }

    output
});
