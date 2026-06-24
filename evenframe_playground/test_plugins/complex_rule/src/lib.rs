//! The most complex output-rule rule possible, ported to the current
//! `TypeContext` / `OutputRulePluginOutput` API (annotations-only, no type
//! substitution).
//!
//! Rule: "Branded Monetary Type via annotations"
//!
//! IF:
//!   - The struct derives BOTH Serialize AND Deserialize
//!   - AND the struct has an annotation containing "@monetary"
//!   - AND the generator is "effect" or "macroforge"
//!   - AND the pipeline includes typesync (Both or Typesync)
//!   - AND the struct has at least one field of type Decimal, f64, or i64
//!   - AND that monetary field does NOT have a @raw annotation
//!   - AND there exists a "currency" field of type String somewhere in the struct
//!
//! THEN for each qualifying monetary field:
//!   - Add a `@brand("MonetaryAmount")` field annotation
//!   - Add a `@monetary({ currency_field: "..." })` field annotation
//!   - Add `@iso4217` to the currency field
//!   - Add a type-level `@rename("<Name>Monetary")` annotation (if not already)
//!   - Add a type-level `@generator(<effect|macroforge>)` annotation
//!   - Add a type-level `@monetary_count(N)` annotation
//!
//! Always-on rules (no gate):
//!   - Vec<X> where X appears as another field's struct-typed value →
//!     `@nested_collection({ type: "X" })` on that field
//!   - Field with >2 validators → `@heavily_validated` on that field
//!   - @internal field → `@skip_internal` marker on that field

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

    let fields = ctx.fields();
    let type_name = ctx.type_name().unwrap_or("").to_string();
    let derives = rust_derives(ctx);

    // ===== Gate: check all preconditions =====

    let has_serialize = derives.iter().any(|d| d == "Serialize");
    let has_deserialize = derives.iter().any(|d| d == "Deserialize");
    let has_monetary_annotation = ctx.annotations().iter().any(|a| a.contains("@monetary"));
    let is_effect = ctx.generator() == "effect";
    let is_macroforge = ctx.generator() == "macroforge";
    let valid_generator = is_effect || is_macroforge;
    let valid_pipeline = ctx.pipeline() == "Both" || ctx.pipeline() == "Typesync";

    let currency_field_name: Option<String> = fields
        .iter()
        .find(|f| {
            f.field_name().unwrap_or("").contains("currency") && field_type_str(f) == "String"
        })
        .map(|f| f.field_name().unwrap_or("").to_string());

    let monetary_types = ["Decimal", "f64", "i64"];
    let has_monetary_field = fields.iter().any(|f| {
        let ft = field_type_str(f);
        monetary_types.iter().any(|mt| ft == *mt)
            && !f.annotations().iter().any(|a| a.contains("@raw"))
    });

    // Collect struct-like type names for cross-field Vec detection.
    let struct_type_names: Vec<String> = fields
        .iter()
        .filter_map(|f| {
            let ft = field_type_str(f);
            if ft.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
                && !["String", "Decimal", "Uuid", "DateTime", "Url", "Duration"]
                    .contains(&ft.as_str())
                && !ft.starts_with("Option<")
                && !ft.starts_with("Vec<")
                && !ft.starts_with("HashMap<")
            {
                Some(ft)
            } else {
                None
            }
        })
        .collect();

    // ===== Always-on rules (no gate) =====

    for field in &fields {
        let name = field.field_name().unwrap_or("").to_string();
        let ft = field_type_str(field);

        // Vec<X> where X is another field's struct type
        if let Some(rest) = ft.strip_prefix("Vec<")
            && let Some(inner) = rest.strip_suffix('>')
            && struct_type_names.iter().any(|st| st == inner)
        {
            push_field_annotation(
                &mut output,
                &name,
                format!("@nested_collection({{ type: \"{}\" }})", inner),
            );
        }

        if validator_count(field) > 2 {
            push_field_annotation(&mut output, &name, "@heavily_validated".to_string());
        }

        if field.annotations().iter().any(|a| a.contains("@internal")) {
            push_field_annotation(&mut output, &name, "@skip_internal".to_string());
        }
    }

    // ===== Main rule: requires ALL preconditions =====

    if !has_serialize
        || !has_deserialize
        || !has_monetary_annotation
        || !valid_generator
        || !valid_pipeline
        || !has_monetary_field
        || currency_field_name.is_none()
    {
        return output;
    }

    let currency_field_name = currency_field_name.unwrap();

    // Rename type via annotation if it doesn't already end with "Monetary".
    if !type_name.ends_with("Monetary") {
        output
            .type_override
            .annotations
            .push(format!("@rename(\"{}Monetary\")", type_name));
    }

    // Record the active generator at the type level.
    let generator_label = if is_effect { "effect" } else { "macroforge" };
    output
        .type_override
        .annotations
        .push(format!("@generator(\"{}\")", generator_label));

    // Process each monetary field.
    let mut monetary_count: u32 = 0;

    for field in &fields {
        let name = field.field_name().unwrap_or("").to_string();

        // Skip raw-annotated fields.
        if field.annotations().iter().any(|a| a.contains("@raw")) {
            continue;
        }

        // Mark raw_amount as skipped.
        if name == "raw_amount" {
            push_field_annotation(&mut output, &name, "@skip_raw_amount".to_string());
            continue;
        }

        let ft = field_type_str(field);
        let is_monetary = monetary_types.iter().any(|mt| ft == *mt);
        if !is_monetary {
            continue;
        }

        monetary_count += 1;

        push_field_annotation(&mut output, &name, "@brand(\"MonetaryAmount\")".to_string());
        push_field_annotation(
            &mut output,
            &name,
            format!("@monetary({{ currency_field: \"{}\" }})", currency_field_name),
        );
    }

    // Add @iso4217 to the currency field.
    push_field_annotation(&mut output, &currency_field_name, "@iso4217".to_string());

    // Embed monetary field count.
    output
        .type_override
        .annotations
        .push(format!("@monetary_count({})", monetary_count));

    output
});
