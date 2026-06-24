//! Stress-test output-rule plugin, ported to the current `TypeContext` /
//! `OutputRulePluginOutput` plugin API.
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
    EventOverride, FieldOverride, OutputRulePluginOutput, PermissionsOverride, TypeContext,
    TypeFieldInfo, define_output_rule_plugin, serde_json,
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

/// A field's `FieldType` rendered back to its source-type string.
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

/// Number of validators declared on a field.
fn validator_count(f: &TypeFieldInfo) -> usize {
    f.node
        .get("validators")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0)
}

define_output_rule_plugin!(|ctx: &TypeContext| {
    let mut output = OutputRulePluginOutput::default();

    let type_name = ctx.type_name().unwrap_or("").to_string();
    let fields = ctx.fields();
    let derives = rust_derives(ctx);

    // ---- Error path: type named "PanicType" triggers an error ----
    if type_name == "PanicType" {
        output.error = Some("Intentional error for PanicType".to_string());
        return output;
    }

    // ---- Type renaming simulated as a type annotation ----
    if type_name.ends_with("Dto") {
        let base = &type_name[..type_name.len() - 3];
        output
            .type_override
            .annotations
            .push(format!("@rename(\"{}Response\")", base));
    }

    // ---- Derive combination matrix → field annotations ----
    let has_serialize = derives.iter().any(|d| d == "Serialize");
    let has_clone = derives.iter().any(|d| d == "Clone");
    let has_debug = derives.iter().any(|d| d == "Debug");

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

    for field in &fields {
        let name = field.field_name().unwrap_or("").to_string();
        let ft = field_type_str(field);
        let anns = field.annotations();

        // Decimal + Serialize → @bigdecimal
        if has_serialize && ft == "Decimal" {
            push_field_annotation(&mut output, &name, "@bigdecimal".to_string());
        }

        // Option<DateTime> + Clone → @datetime_nullable
        if has_clone && ft == "Option<DateTime>" {
            push_field_annotation(&mut output, &name, "@datetime_nullable".to_string());
        }

        // Vec<Uuid> with Debug → @readonly_uuid_array
        if has_debug && ft == "Vec<Uuid>" {
            push_field_annotation(&mut output, &name, "@readonly_uuid_array".to_string());
        }

        // HashMap<String, i64> → @string_number_map
        if ft == "HashMap<String, i64>" {
            push_field_annotation(&mut output, &name, "@string_number_map".to_string());
        }

        // Deeply nested type → @deep_nested
        if ft == "Option<Vec<HashMap<String, Decimal>>>" {
            push_field_annotation(&mut output, &name, "@deep_nested".to_string());
        }

        // ---- Skip markers (since we can't actually skip) ----
        if anns.iter().any(|a| a.contains("@internal")) {
            push_field_annotation(&mut output, &name, "@skip_internal".to_string());
        }
        if anns.iter().any(|a| a.contains("@deprecated")) {
            push_field_annotation(&mut output, &name, "@skip_deprecated".to_string());
        }
        if name == "__private" {
            push_field_annotation(&mut output, &name, "@skip_private".to_string());
        }

        // ---- Readonly/validated ----
        if name.contains("created") || name.contains("updated") {
            push_field_annotation(&mut output, &name, "@readonly".to_string());
        }
        if validator_count(field) > 0 {
            push_field_annotation(&mut output, &name, "@validated".to_string());
        }

        // ---- JSON special characters (verifies wire round-trip) ----
        if name == "json_tricky" {
            push_field_annotation(
                &mut output,
                &name,
                "@tricky(\"value\\\"with\\\\escapes\")".to_string(),
            );
        }
    }

    // ---- Generator-specific type annotations ----
    if ctx.generator() == "arktype" && !output.field_overrides.is_empty() {
        output
            .type_override
            .annotations
            .push("@arktype_generator".to_string());
    }

    // ---- Pipeline-specific annotations ----
    if ctx.pipeline() == "Schemasync" {
        for field in &fields {
            if field_type_str(field).starts_with("Option<") {
                push_field_annotation(
                    &mut output,
                    field.field_name().unwrap_or(""),
                    "@schemasync_option".to_string(),
                );
            }
        }
    }

    // ---- Enum handling ----
    if matches!(ctx, TypeContext::Enum { .. }) {
        output
            .type_override
            .annotations
            .push(format!("@tracked_enum(\"{}\")", type_name));
    }

    // ---- Permissions + events demonstration for Dto-named tables ----
    if ctx.table_name().is_some_and(|t| !t.is_empty()) && type_name.ends_with("Dto") {
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
        output
            .type_override
            .macroforge_derives
            .push("StressGold".to_string());
    }

    output
});
