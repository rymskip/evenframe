//! Stress-test output-rule plugin, ported to the current plugin API.
//!
//! This exercises every output capability the current
//! `OutputRulePluginOutput` actually supports:
//!
//! - `output.error` for the intentional panic path
//! - `type_override.macroforge_derives` for injected derives
//! - `type_override.annotations` for type-level annotations
//! - `type_override.permissions` for table permissions
//! - `type_override.events` for table events
//! - `field_overrides[name].annotations` for field-level annotations
//!
//! Capabilities removed from the plugin surface (type substitution, skip
//! fields, extra imports, type renaming) are *simulated* via annotation
//! markers so the downstream consumer can still act on them if desired.

use evenframe_plugin::{
    EventOverride, FieldOverride, OutputRulePluginOutput, PermissionsOverride,
    define_output_rule_plugin,
};

define_output_rule_plugin!(|ctx: &TypeContext| {
    let mut output = OutputRulePluginOutput::default();

    // ---- Error path: type named "PanicType" triggers an error ----
    if ctx.type_name == "PanicType" {
        output.error = Some("Intentional error for PanicType".to_string());
        return output;
    }

    // ---- Type renaming simulated as a type annotation ----
    if ctx.type_name.ends_with("Dto") {
        let base = &ctx.type_name[..ctx.type_name.len() - 3];
        output
            .type_override
            .annotations
            .push(format!("@rename(\"{}Response\")", base));
    }

    // ---- Derive combination matrix → field annotations ----
    let has_serialize = ctx.rust_derives.iter().any(|d| d == "Serialize");
    let has_clone = ctx.rust_derives.iter().any(|d| d == "Clone");
    let has_debug = ctx.rust_derives.iter().any(|d| d == "Debug");

    fn push_field_annotation(
        output: &mut OutputRulePluginOutput,
        field_name: &str,
        annotation: String,
    ) {
        output
            .field_overrides
            .entry(field_name.to_string())
            .or_insert_with(FieldOverride::default)
            .annotations
            .push(annotation);
    }

    for field in &ctx.fields {
        // Decimal + Serialize → @bigdecimal
        if has_serialize && field.field_type == "Decimal" {
            push_field_annotation(&mut output, &field.field_name, "@bigdecimal".to_string());
        }

        // Option<DateTime> + Clone → @datetime_nullable
        if has_clone && field.field_type == "Option<DateTime>" {
            push_field_annotation(
                &mut output,
                &field.field_name,
                "@datetime_nullable".to_string(),
            );
        }

        // Vec<Uuid> with Debug → @readonly_uuid_array
        if has_debug && field.field_type == "Vec<Uuid>" {
            push_field_annotation(
                &mut output,
                &field.field_name,
                "@readonly_uuid_array".to_string(),
            );
        }

        // HashMap<String, i64> → @string_number_map
        if field.field_type == "HashMap<String, i64>" {
            push_field_annotation(
                &mut output,
                &field.field_name,
                "@string_number_map".to_string(),
            );
        }

        // Deeply nested type → @deep_nested
        if field.field_type == "Option<Vec<HashMap<String, Decimal>>>" {
            push_field_annotation(&mut output, &field.field_name, "@deep_nested".to_string());
        }

        // ---- Skip markers (since we can't actually skip) ----
        if field.annotations.iter().any(|a| a.contains("@internal")) {
            push_field_annotation(&mut output, &field.field_name, "@skip_internal".to_string());
        }
        if field.annotations.iter().any(|a| a.contains("@deprecated")) {
            push_field_annotation(
                &mut output,
                &field.field_name,
                "@skip_deprecated".to_string(),
            );
        }
        if field.field_name == "__private" {
            push_field_annotation(&mut output, &field.field_name, "@skip_private".to_string());
        }

        // ---- Readonly/validated ----
        if field.field_name.contains("created") || field.field_name.contains("updated") {
            push_field_annotation(&mut output, &field.field_name, "@readonly".to_string());
        }
        if !field.validators.is_empty() {
            push_field_annotation(&mut output, &field.field_name, "@validated".to_string());
        }

        // ---- JSON special characters (verifies wire round-trip) ----
        if field.field_name == "json_tricky" {
            push_field_annotation(
                &mut output,
                &field.field_name,
                "@tricky(\"value\\\"with\\\\escapes\")".to_string(),
            );
        }
    }

    // ---- Generator-specific type annotations ----
    if ctx.generator == "arktype" && !output.field_overrides.is_empty() {
        output
            .type_override
            .annotations
            .push("@arktype_generator".to_string());
    }

    // ---- Pipeline-specific annotations ----
    if ctx.pipeline == "Schemasync" {
        for field in &ctx.fields {
            if field.field_type.starts_with("Option<") {
                push_field_annotation(
                    &mut output,
                    &field.field_name,
                    "@schemasync_option".to_string(),
                );
            }
        }
    }

    // ---- Enum handling ----
    if ctx.kind == "Enum" {
        output
            .type_override
            .annotations
            .push(format!("@tracked_enum(\"{}\")", ctx.type_name));
    }

    // ---- Permissions + events demonstration for Dto-named tables ----
    if !ctx.table_name.is_empty() && ctx.type_name.ends_with("Dto") {
        output.type_override.permissions = Some(PermissionsOverride {
            select: "FULL".to_string(),
            create: "WHERE $auth != NONE".to_string(),
            update: "WHERE $auth != NONE".to_string(),
            delete: "WHERE $auth.role = 'admin'".to_string(),
        });
        output.type_override.events.push(EventOverride {
            name: "dto_audit".to_string(),
            statement: "CREATE audit SET table = $table, action = $event, at = time::now()"
                .to_string(),
        });
    }

    // ---- Macroforge derive injection demo ----
    if has_serialize && has_clone && has_debug {
        output.type_override.macroforge_derives.push("StressGold".to_string());
    }

    output
});
