//! The most complex type-transform rule possible.
//!
//! Rule: "Effect-TS Branded Monetary Type"
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
//!   - Override its type to a branded Effect type:
//!     `Schema.BigDecimal.pipe(Schema.brand("MonetaryAmount"))`  (effect)
//!     `BigDecimal.BigDecimal & { readonly __brand: "MonetaryAmount" }`  (macroforge)
//!   - Add a field annotation: `@monetary({ currency_field: "<name of currency field>" })`
//!   - Add the currency field's annotation: `@iso4217`
//!   - Add extra imports for the branded type
//!   - Skip any field named "raw_amount" or annotated @internal
//!   - Rename the type: append "Monetary" suffix if it doesn't already have one
//!
//! Additionally:
//!   - If the struct has a field whose type is `Vec<X>` where X is another struct
//!     name found in the field list (cross-field reference detection), add
//!     `@nested_collection` annotation to that field
//!   - If ANY field has more than 2 validators, add `@heavily_validated` to it
//!   - Count total monetary fields and embed the count in a struct-level comment import

use evenframe_plugin::{TypePluginOutput, define_type_plugin};

define_type_plugin!(|ctx: &TypeContext| {
    let mut output = TypePluginOutput::default();

    // ===== Gate: check all preconditions =====

    let has_serialize = ctx.rust_derives.iter().any(|d| d == "Serialize");
    let has_deserialize = ctx.rust_derives.iter().any(|d| d == "Deserialize");
    let has_monetary_annotation = ctx.annotations.iter().any(|a| a.contains("@monetary"));
    let is_effect = ctx.generator == "effect";
    let is_macroforge = ctx.generator == "macroforge";
    let valid_generator = is_effect || is_macroforge;
    let valid_pipeline = ctx.pipeline == "Both" || ctx.pipeline == "Typesync";

    // Find the currency field (must be a String field with "currency" in the name)
    let currency_field = ctx.fields.iter().find(|f| {
        f.field_name.contains("currency") && f.field_type == "String"
    });

    // Identify monetary field types
    let monetary_types = ["Decimal", "f64", "i64"];
    let has_monetary_field = ctx.fields.iter().any(|f| {
        monetary_types.iter().any(|mt| f.field_type == *mt)
            && !f.annotations.iter().any(|a| a.contains("@raw"))
    });

    // Collect all field names for cross-reference detection
    let _all_field_names: Vec<String> = ctx.fields.iter().map(|f| f.field_name.clone()).collect();
    // Also collect type names that look like struct refs (PascalCase, not primitives)
    let struct_type_names: Vec<String> = ctx.fields.iter()
        .filter_map(|f| {
            let ft = &f.field_type;
            if ft.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
                && !["String", "Decimal", "Uuid", "DateTime", "Url", "Duration"].contains(&ft.as_str())
                && !ft.starts_with("Option<")
                && !ft.starts_with("Vec<")
                && !ft.starts_with("HashMap<")
            {
                Some(ft.clone())
            } else {
                None
            }
        })
        .collect();

    // ===== Always-on rules (no gate) =====

    // Cross-field reference detection: Vec<X> where X appears as another field's type
    for field in &ctx.fields {
        if field.field_type.starts_with("Vec<") && field.field_type.ends_with(">") {
            let inner = &field.field_type[4..field.field_type.len() - 1];
            // Check if inner type name matches any other field's struct type
            if struct_type_names.iter().any(|st| st == inner) {
                output.field_annotations
                    .entry(field.field_name.clone())
                    .or_insert_with(Vec::new)
                    .push(format!("@nested_collection({{ type: \"{}\" }})", inner));
            }
        }
    }

    // Heavily validated detection
    for field in &ctx.fields {
        if field.validators.len() > 2 {
            output.field_annotations
                .entry(field.field_name.clone())
                .or_insert_with(Vec::new)
                .push("@heavily_validated".to_string());
        }
    }

    // Skip @internal fields always
    for field in &ctx.fields {
        if field.annotations.iter().any(|a| a.contains("@internal")) {
            output.skip_fields.push(field.field_name.clone());
        }
    }

    // ===== Main rule: requires ALL preconditions =====

    if !has_serialize
        || !has_deserialize
        || !has_monetary_annotation
        || !valid_generator
        || !valid_pipeline
        || !has_monetary_field
        || currency_field.is_none()
    {
        return output;
    }

    let currency_field_name = currency_field.unwrap().field_name.clone();

    // Rename type if it doesn't already end with "Monetary"
    if !ctx.type_name.ends_with("Monetary") {
        output.type_name_override = Some(format!("{}Monetary", ctx.type_name));
    }

    // Process each monetary field
    let mut monetary_count = 0u32;

    for field in &ctx.fields {
        // Skip raw-annotated fields
        if field.annotations.iter().any(|a| a.contains("@raw")) {
            continue;
        }

        // Skip raw_amount by name
        if field.field_name == "raw_amount" {
            output.skip_fields.push(field.field_name.clone());
            continue;
        }

        let is_monetary = monetary_types.iter().any(|mt| field.field_type == *mt);
        if !is_monetary {
            continue;
        }

        monetary_count += 1;

        // Override type based on generator
        if is_effect {
            output.field_type_overrides.insert(
                field.field_name.clone(),
                "Schema.BigDecimal.pipe(Schema.brand(\"MonetaryAmount\"))".to_string(),
            );
        } else if is_macroforge {
            output.field_type_overrides.insert(
                field.field_name.clone(),
                "BigDecimal.BigDecimal & { readonly __brand: \"MonetaryAmount\" }".to_string(),
            );
        }

        // Add monetary annotation linking to currency field
        output.field_annotations
            .entry(field.field_name.clone())
            .or_insert_with(Vec::new)
            .push(format!(
                "@monetary({{ currency_field: \"{}\" }})",
                currency_field_name
            ));
    }

    // Add @iso4217 to the currency field
    output.field_annotations
        .entry(currency_field_name.clone())
        .or_insert_with(Vec::new)
        .push("@iso4217".to_string());

    // Add imports
    if is_effect {
        output.extra_imports.push(
            "import { Schema } from '@effect/schema';".to_string(),
        );
        output.extra_imports.push(
            "import type { BigDecimal } from 'effect';".to_string(),
        );
    } else if is_macroforge {
        output.extra_imports.push(
            "import type { BigDecimal } from 'effect';".to_string(),
        );
    }

    // Embed monetary field count as a comment import
    output.extra_imports.push(
        format!("// {} monetary field(s) branded in {}", monetary_count, ctx.type_name),
    );

    output
});
