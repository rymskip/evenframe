//! Stress-test output-rule plugin.
//!
//! Exercises every output field and handles edge cases:
//! - Unicode type names and field names
//! - Deeply nested generic types
//! - Empty input (no fields, no derives)
//! - Massive field counts
//! - Every output feature: overrides, skips, imports, annotations, type_name_override, error
//! - JSON special characters in strings
//! - Conflicting overrides (last wins at host level)

use evenframe_plugin::{OutputRulePluginOutput, define_type_plugin};

define_type_plugin!(|ctx: &TypeContext| {
    let mut output = OutputRulePluginOutput::default();

    // ---- Error path: type named "PanicType" triggers an error ----
    if ctx.type_name == "PanicType" {
        output.error = Some("Intentional error for PanicType".to_string());
        return output;
    }

    // ---- Type name override: rename types ending with "Dto" ----
    if ctx.type_name.ends_with("Dto") {
        let base = &ctx.type_name[..ctx.type_name.len() - 3];
        output.type_name_override = Some(format!("{}Response", base));
    }

    // ---- Field type overrides based on derive combinations ----
    let has_serialize = ctx.rust_derives.iter().any(|d| d == "Serialize");
    let has_clone = ctx.rust_derives.iter().any(|d| d == "Clone");
    let has_debug = ctx.rust_derives.iter().any(|d| d == "Debug");

    for field in &ctx.fields {
        // Decimal + Serialize → BigDecimal
        if has_serialize && field.field_type == "Decimal" {
            output.field_type_overrides.insert(
                field.field_name.clone(),
                "BigDecimal.BigDecimal".to_string(),
            );
            output.extra_imports.push(
                "import type { BigDecimal } from 'effect';".to_string(),
            );
        }

        // DateTime + Clone → DateTimeIso
        if has_clone && field.field_type == "Option<DateTime>" {
            output.field_type_overrides.insert(
                field.field_name.clone(),
                "DateTime.Utc | null".to_string(),
            );
        }

        // Vec<Uuid> with Debug → readonly array
        if has_debug && field.field_type == "Vec<Uuid>" {
            output.field_type_overrides.insert(
                field.field_name.clone(),
                "ReadonlyArray<Uuid>".to_string(),
            );
        }

        // HashMap override
        if field.field_type == "HashMap<String, i64>" {
            output.field_type_overrides.insert(
                field.field_name.clone(),
                "Record<string, number>".to_string(),
            );
        }

        // Deeply nested type
        if field.field_type == "Option<Vec<HashMap<String, Decimal>>>" {
            output.field_type_overrides.insert(
                field.field_name.clone(),
                "Array<Record<string, BigDecimal>> | null".to_string(),
            );
        }

        // ---- Skip fields ----
        // Skip @internal annotated fields
        if field.annotations.iter().any(|a| a.contains("@internal")) {
            output.skip_fields.push(field.field_name.clone());
        }
        // Skip @deprecated annotated fields
        if field.annotations.iter().any(|a| a.contains("@deprecated")) {
            output.skip_fields.push(field.field_name.clone());
        }
        // Skip fields named exactly "__private"
        if field.field_name == "__private" {
            output.skip_fields.push(field.field_name.clone());
        }

        // ---- Field annotations ----
        // Add @readonly to fields with "created" or "updated" in name
        if field.field_name.contains("created") || field.field_name.contains("updated") {
            output.field_annotations
                .entry(field.field_name.clone())
                .or_insert_with(Vec::new)
                .push("@readonly".to_string());
        }

        // Fields with validators get a @validated annotation
        if !field.validators.is_empty() {
            output.field_annotations
                .entry(field.field_name.clone())
                .or_insert_with(Vec::new)
                .push("@validated".to_string());
        }
    }

    // ---- Generator-specific behavior ----
    if ctx.generator == "arktype" {
        // Add arktype-specific import
        if !output.field_type_overrides.is_empty() {
            output.extra_imports.push(
                "import { type } from 'arktype';".to_string(),
            );
        }
    }

    // ---- Pipeline-specific behavior ----
    if ctx.pipeline == "Schemasync" {
        // For schemasync-only types, skip all Option fields
        for field in &ctx.fields {
            if field.field_type.starts_with("Option<") {
                output.skip_fields.push(field.field_name.clone());
            }
        }
    }

    // ---- Enum handling ----
    if ctx.kind == "Enum" {
        // Add enum-specific import
        output.extra_imports.push(
            format!("// Enum: {}", ctx.type_name),
        );
    }

    // ---- JSON special characters in output ----
    // Field named "json_tricky" gets a type with special chars
    for field in &ctx.fields {
        if field.field_name == "json_tricky" {
            output.field_type_overrides.insert(
                field.field_name.clone(),
                r#"string & { readonly __brand: "tricky\"value" }"#.to_string(),
            );
        }
    }

    output
});
