//! E2E tests for the complex monetary branding rule.
//!
//! The rule requires ALL of these to be true simultaneously:
//!   1. Struct derives both Serialize AND Deserialize
//!   2. Struct has @monetary annotation
//!   3. Generator is "effect" or "macroforge"
//!   4. Pipeline is "Both" or "Typesync"
//!   5. At least one Decimal/f64/i64 field exists (without @raw)
//!   6. A currency field (String type, "currency" in name) exists
//!
//! When all hold, monetary fields get branded types, the type is renamed,
//! imports are added, currency field gets @iso4217, etc.
//!
//! Run with: cargo test --test complex_rule_e2e_test --features wasm-plugins

#[cfg(feature = "wasm-plugins")]
mod tests {
    use evenframe_core::config::TypePluginConfig;
    use evenframe_core::typesync::plugin::TypePluginManager;
    use evenframe_core::typesync::plugin_types::{TypeKind, TypePluginFieldInfo, TypePluginInput};
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn playground_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn mgr() -> TypePluginManager {
        let mut plugins = HashMap::new();
        plugins.insert(
            "complex".to_string(),
            TypePluginConfig {
                path: ".evenframe/plugins/complex_rule.wasm".to_string(),
            },
        );
        TypePluginManager::new(&plugins, &playground_root()).expect("Should load complex plugin")
    }

    fn f(name: &str, ty: &str) -> TypePluginFieldInfo {
        TypePluginFieldInfo {
            field_name: name.to_string(),
            field_type: ty.to_string(),
            annotations: vec![],
            validators: vec![],
        }
    }

    fn f_ann(name: &str, ty: &str, anns: Vec<&str>) -> TypePluginFieldInfo {
        TypePluginFieldInfo {
            field_name: name.to_string(),
            field_type: ty.to_string(),
            annotations: anns.into_iter().map(|s| s.to_string()).collect(),
            validators: vec![],
        }
    }

    fn f_val(name: &str, ty: &str, vals: Vec<&str>) -> TypePluginFieldInfo {
        TypePluginFieldInfo {
            field_name: name.to_string(),
            field_type: ty.to_string(),
            annotations: vec![],
            validators: vals.into_iter().map(|s| s.to_string()).collect(),
        }
    }

    fn inp(
        name: &str,
        derives: Vec<&str>,
        annotations: Vec<&str>,
        pipeline: &str,
        generator: &str,
        fields: Vec<TypePluginFieldInfo>,
    ) -> TypePluginInput {
        TypePluginInput {
            type_name: name.to_string(),
            kind: TypeKind::Struct,
            rust_derives: derives.into_iter().map(|s| s.to_string()).collect(),
            annotations: annotations.into_iter().map(|s| s.to_string()).collect(),
            pipeline: pipeline.to_string(),
            generator: generator.to_string(),
            fields,
        }
    }

    /// The canonical "everything matches" input
    fn full_match_input(generator: &str) -> TypePluginInput {
        inp(
            "Invoice",
            vec!["Debug", "Clone", "Serialize", "Deserialize"],
            vec!["@monetary"],
            "Both",
            generator,
            vec![
                f("id", "String"),
                f("total", "Decimal"),
                f("tax", "f64"),
                f("line_count", "i64"),
                f("currency_code", "String"),
                f("description", "String"),
            ],
        )
    }

    // ========================================================================
    // Full match: all preconditions met
    // ========================================================================

    #[test]
    fn test_full_match_effect_generator() {
        let mut pm = mgr();
        let result = pm.transform_type("complex", &full_match_input("effect")).unwrap();

        assert!(result.error.is_none());

        // Type renamed
        assert_eq!(
            result.type_name_override,
            Some("InvoiceMonetary".to_string())
        );

        // All three monetary fields overridden with branded Effect type
        for field_name in &["total", "tax", "line_count"] {
            let override_val = result.field_type_overrides.get(*field_name);
            assert!(
                override_val.is_some(),
                "{} should be overridden",
                field_name
            );
            assert!(
                override_val.unwrap().contains("Schema.BigDecimal"),
                "{} should use Schema.BigDecimal. Got: {}",
                field_name,
                override_val.unwrap()
            );
            assert!(
                override_val.unwrap().contains("MonetaryAmount"),
                "{} should be branded. Got: {}",
                field_name,
                override_val.unwrap()
            );
        }

        // Non-monetary fields NOT overridden
        assert!(!result.field_type_overrides.contains_key("id"));
        assert!(!result.field_type_overrides.contains_key("description"));
        assert!(!result.field_type_overrides.contains_key("currency_code"));

        // Monetary fields get @monetary annotation linking to currency field
        for field_name in &["total", "tax", "line_count"] {
            let anns = result.field_annotations.get(*field_name);
            assert!(
                anns.is_some(),
                "{} should have annotations",
                field_name
            );
            let anns = anns.unwrap();
            assert!(
                anns.iter().any(|a| a.contains("currency_field") && a.contains("currency_code")),
                "{} should have @monetary linking to currency_code. Got: {:?}",
                field_name,
                anns
            );
        }

        // Currency field gets @iso4217
        let currency_anns = result.field_annotations.get("currency_code");
        assert!(
            currency_anns
                .map(|v| v.iter().any(|a| a == "@iso4217"))
                .unwrap_or(false),
            "currency_code should get @iso4217. Got: {:?}",
            currency_anns
        );

        // Effect-specific imports
        assert!(
            result.extra_imports.iter().any(|i| i.contains("@effect/schema")),
            "Should import @effect/schema. Got: {:?}",
            result.extra_imports
        );
        assert!(
            result.extra_imports.iter().any(|i| i.contains("BigDecimal")),
            "Should import BigDecimal"
        );

        // Count comment
        assert!(
            result.extra_imports.iter().any(|i| i.contains("3 monetary field(s)")),
            "Should embed count of 3 monetary fields. Got: {:?}",
            result.extra_imports
        );
    }

    #[test]
    fn test_full_match_macroforge_generator() {
        let mut pm = mgr();
        let result = pm.transform_type("complex", &full_match_input("macroforge")).unwrap();

        // Macroforge uses branded intersection type
        let total = result.field_type_overrides.get("total").unwrap();
        assert!(
            total.contains("BigDecimal.BigDecimal") && total.contains("__brand"),
            "Macroforge should use branded intersection. Got: {}",
            total
        );

        // No @effect/schema import for macroforge
        assert!(
            !result.extra_imports.iter().any(|i| i.contains("@effect/schema")),
            "Macroforge should not import @effect/schema"
        );
    }

    // ========================================================================
    // Fail each precondition individually
    // ========================================================================

    #[test]
    fn test_missing_serialize() {
        let mut pm = mgr();
        let i = inp(
            "Invoice",
            vec!["Debug", "Deserialize"], // no Serialize
            vec!["@monetary"],
            "Both",
            "effect",
            vec![f("total", "Decimal"), f("currency_code", "String")],
        );
        let result = pm.transform_type("complex", &i).unwrap();
        assert!(
            result.field_type_overrides.is_empty(),
            "Missing Serialize should prevent rule"
        );
        assert!(result.type_name_override.is_none());
    }

    #[test]
    fn test_missing_deserialize() {
        let mut pm = mgr();
        let i = inp(
            "Invoice",
            vec!["Serialize"], // no Deserialize
            vec!["@monetary"],
            "Both",
            "effect",
            vec![f("total", "Decimal"), f("currency_code", "String")],
        );
        let result = pm.transform_type("complex", &i).unwrap();
        assert!(result.field_type_overrides.is_empty());
    }

    #[test]
    fn test_missing_monetary_annotation() {
        let mut pm = mgr();
        let i = inp(
            "Invoice",
            vec!["Serialize", "Deserialize"],
            vec![], // no @monetary
            "Both",
            "effect",
            vec![f("total", "Decimal"), f("currency_code", "String")],
        );
        let result = pm.transform_type("complex", &i).unwrap();
        assert!(result.field_type_overrides.is_empty());
    }

    #[test]
    fn test_wrong_generator() {
        let mut pm = mgr();
        let i = inp(
            "Invoice",
            vec!["Serialize", "Deserialize"],
            vec!["@monetary"],
            "Both",
            "arktype", // not effect or macroforge
            vec![f("total", "Decimal"), f("currency_code", "String")],
        );
        let result = pm.transform_type("complex", &i).unwrap();
        assert!(result.field_type_overrides.is_empty());
    }

    #[test]
    fn test_wrong_pipeline() {
        let mut pm = mgr();
        let i = inp(
            "Invoice",
            vec!["Serialize", "Deserialize"],
            vec!["@monetary"],
            "Schemasync", // not Both or Typesync
            "effect",
            vec![f("total", "Decimal"), f("currency_code", "String")],
        );
        let result = pm.transform_type("complex", &i).unwrap();
        assert!(result.field_type_overrides.is_empty());
    }

    #[test]
    fn test_no_monetary_fields() {
        let mut pm = mgr();
        let i = inp(
            "Invoice",
            vec!["Serialize", "Deserialize"],
            vec!["@monetary"],
            "Both",
            "effect",
            vec![
                f("name", "String"),
                f("currency_code", "String"),
            ], // no Decimal/f64/i64
        );
        let result = pm.transform_type("complex", &i).unwrap();
        assert!(result.field_type_overrides.is_empty());
    }

    #[test]
    fn test_no_currency_field() {
        let mut pm = mgr();
        let i = inp(
            "Invoice",
            vec!["Serialize", "Deserialize"],
            vec!["@monetary"],
            "Both",
            "effect",
            vec![
                f("total", "Decimal"),
                f("name", "String"), // no currency field
            ],
        );
        let result = pm.transform_type("complex", &i).unwrap();
        assert!(result.field_type_overrides.is_empty());
    }

    // ========================================================================
    // @raw annotation exempts a monetary field
    // ========================================================================

    #[test]
    fn test_raw_annotated_field_not_overridden() {
        let mut pm = mgr();
        let i = inp(
            "Invoice",
            vec!["Serialize", "Deserialize"],
            vec!["@monetary"],
            "Both",
            "effect",
            vec![
                f("total", "Decimal"),                      // should be overridden
                f_ann("raw_total", "Decimal", vec!["@raw"]), // should NOT be overridden
                f("currency_code", "String"),
            ],
        );
        let result = pm.transform_type("complex", &i).unwrap();
        assert!(result.field_type_overrides.contains_key("total"));
        assert!(
            !result.field_type_overrides.contains_key("raw_total"),
            "@raw should exempt the field"
        );
    }

    // ========================================================================
    // raw_amount is skipped by name
    // ========================================================================

    #[test]
    fn test_raw_amount_skipped() {
        let mut pm = mgr();
        let i = inp(
            "Invoice",
            vec!["Serialize", "Deserialize"],
            vec!["@monetary"],
            "Both",
            "effect",
            vec![
                f("total", "Decimal"),
                f("raw_amount", "Decimal"),
                f("currency_code", "String"),
            ],
        );
        let result = pm.transform_type("complex", &i).unwrap();
        assert!(result.skip_fields.contains(&"raw_amount".to_string()));
        // raw_amount should NOT get a type override since it's skipped
        assert!(!result.field_type_overrides.contains_key("raw_amount"));
    }

    // ========================================================================
    // Type rename: already ends with Monetary
    // ========================================================================

    #[test]
    fn test_no_double_monetary_suffix() {
        let mut pm = mgr();
        let i = inp(
            "InvoiceMonetary",
            vec!["Serialize", "Deserialize"],
            vec!["@monetary"],
            "Both",
            "effect",
            vec![f("total", "Decimal"), f("currency_code", "String")],
        );
        let result = pm.transform_type("complex", &i).unwrap();
        assert!(
            result.type_name_override.is_none(),
            "Should not add Monetary suffix when already present"
        );
    }

    // ========================================================================
    // Always-on rules: cross-field reference, heavy validation, @internal skip
    // ========================================================================

    #[test]
    fn test_internal_skipped_even_without_monetary() {
        let mut pm = mgr();
        let i = inp(
            "Simple",
            vec!["Debug"],
            vec![],
            "Both",
            "macroforge",
            vec![
                f("name", "String"),
                f_ann("secret", "String", vec!["@internal"]),
            ],
        );
        let result = pm.transform_type("complex", &i).unwrap();
        assert!(result.skip_fields.contains(&"secret".to_string()));
    }

    #[test]
    fn test_heavily_validated_annotation() {
        let mut pm = mgr();
        let i = inp(
            "Validated",
            vec![],
            vec![],
            "Both",
            "macroforge",
            vec![
                f_val("email", "String", vec!["email", "min_length(5)", "max_length(255)"]),
                f_val("name", "String", vec!["min_length(1)"]),
            ],
        );
        let result = pm.transform_type("complex", &i).unwrap();
        let email_anns = result.field_annotations.get("email");
        assert!(
            email_anns
                .map(|v| v.iter().any(|a| a == "@heavily_validated"))
                .unwrap_or(false),
            "email with 3 validators should get @heavily_validated. Got: {:?}",
            email_anns
        );
        assert!(
            !result.field_annotations.contains_key("name"),
            "name with 1 validator should not get @heavily_validated"
        );
    }

    #[test]
    fn test_nested_collection_detection() {
        let mut pm = mgr();
        let i = inp(
            "Order",
            vec![],
            vec![],
            "Both",
            "macroforge",
            vec![
                f("items", "Vec<LineItem>"),
                f("line_item_ref", "LineItem"), // struct type reference
                f("name", "String"),
            ],
        );
        let result = pm.transform_type("complex", &i).unwrap();
        let items_anns = result.field_annotations.get("items");
        assert!(
            items_anns
                .map(|v| v.iter().any(|a| a.contains("@nested_collection") && a.contains("LineItem")))
                .unwrap_or(false),
            "Vec<LineItem> should get @nested_collection when LineItem is a struct type. Got: {:?}",
            items_anns
        );
    }

    // ========================================================================
    // Typesync pipeline passes, Schemasync fails
    // ========================================================================

    #[test]
    fn test_typesync_pipeline_passes() {
        let mut pm = mgr();
        let i = inp(
            "Invoice",
            vec!["Serialize", "Deserialize"],
            vec!["@monetary"],
            "Typesync",
            "effect",
            vec![f("total", "Decimal"), f("currency_code", "String")],
        );
        let result = pm.transform_type("complex", &i).unwrap();
        assert!(
            result.field_type_overrides.contains_key("total"),
            "Typesync pipeline should pass the gate"
        );
    }

    // ========================================================================
    // Multiple monetary field types mixed
    // ========================================================================

    #[test]
    fn test_mixed_monetary_types() {
        let mut pm = mgr();
        let i = inp(
            "FinancialRecord",
            vec!["Serialize", "Deserialize"],
            vec!["@monetary"],
            "Both",
            "effect",
            vec![
                f("decimal_amount", "Decimal"),
                f("float_amount", "f64"),
                f("int_amount", "i64"),
                f("name", "String"),
                f("currency", "String"),
            ],
        );
        let result = pm.transform_type("complex", &i).unwrap();

        // All three monetary types should be overridden
        assert!(result.field_type_overrides.contains_key("decimal_amount"));
        assert!(result.field_type_overrides.contains_key("float_amount"));
        assert!(result.field_type_overrides.contains_key("int_amount"));
        assert!(!result.field_type_overrides.contains_key("name"));

        // Count should be 3
        assert!(
            result.extra_imports.iter().any(|i| i.contains("3 monetary field(s)")),
            "Should count 3 monetary fields"
        );
    }

    // ========================================================================
    // Stability under load
    // ========================================================================

    #[test]
    fn test_complex_rule_50_rapid_calls() {
        let mut pm = mgr();
        for i in 0..50 {
            let name = format!("Type{}", i);
            let input = inp(
                &name,
                vec!["Serialize", "Deserialize"],
                vec!["@monetary"],
                "Both",
                if i % 2 == 0 { "effect" } else { "macroforge" },
                vec![
                    f("amount", "Decimal"),
                    f("currency_code", "String"),
                ],
            );
            let result = pm.transform_type("complex", &input);
            assert!(result.is_ok(), "Call {} failed: {:?}", i, result.err());
            let output = result.unwrap();
            assert!(output.error.is_none());
            assert!(output.field_type_overrides.contains_key("amount"));
        }
    }

    // ========================================================================
    // Kitchen sink: everything at once
    // ========================================================================

    #[test]
    fn test_kitchen_sink() {
        let mut pm = mgr();
        let i = inp(
            "MegaInvoiceDto",
            vec!["Debug", "Clone", "Serialize", "Deserialize", "PartialEq"],
            vec!["@monetary", "@audit"],
            "Both",
            "effect",
            vec![
                f("id", "String"),
                f("total", "Decimal"),                          // monetary → branded
                f("tax", "f64"),                                // monetary → branded
                f("item_count", "i64"),                         // monetary → branded
                f_ann("raw_total", "Decimal", vec!["@raw"]),    // exempted by @raw
                f("raw_amount", "Decimal"),                     // skipped by name
                f_ann("secret_key", "String", vec!["@internal"]), // skipped by annotation
                f("currency_code", "String"),                   // gets @iso4217
                f("created_at", "String"),                      // no special treatment
                f_val("email", "String", vec!["email", "min_length(3)", "max_length(255)"]), // @heavily_validated
                f("items", "Vec<LineItem>"),                    // @nested_collection
                f("metadata", "LineItem"),                      // struct type ref for cross-detection
                f("description", "String"),
            ],
        );
        let result = pm.transform_type("complex", &i).unwrap();
        assert!(result.error.is_none());

        // Type rename: Dto → stripped? No, Monetary is appended
        // "MegaInvoiceDto" doesn't end with "Monetary", so it becomes "MegaInvoiceDtoMonetary"
        assert_eq!(
            result.type_name_override,
            Some("MegaInvoiceDtoMonetary".to_string())
        );

        // Monetary overrides (3 fields: total, tax, item_count)
        assert!(result.field_type_overrides.contains_key("total"));
        assert!(result.field_type_overrides.contains_key("tax"));
        assert!(result.field_type_overrides.contains_key("item_count"));

        // @raw exempted
        assert!(!result.field_type_overrides.contains_key("raw_total"));

        // Skips
        assert!(result.skip_fields.contains(&"raw_amount".to_string()));
        assert!(result.skip_fields.contains(&"secret_key".to_string()));

        // Currency @iso4217
        assert!(
            result.field_annotations.get("currency_code")
                .map(|v| v.contains(&"@iso4217".to_string()))
                .unwrap_or(false)
        );

        // @heavily_validated on email (3 validators)
        assert!(
            result.field_annotations.get("email")
                .map(|v| v.iter().any(|a| a == "@heavily_validated"))
                .unwrap_or(false)
        );

        // @nested_collection on items (Vec<LineItem> where LineItem is a struct type)
        assert!(
            result.field_annotations.get("items")
                .map(|v| v.iter().any(|a| a.contains("@nested_collection")))
                .unwrap_or(false)
        );

        // Monetary count = 3 (total, tax, item_count; raw_total exempted, raw_amount skipped)
        assert!(
            result.extra_imports.iter().any(|i| i.contains("3 monetary field(s)")),
            "Should count 3 monetary fields. Imports: {:?}",
            result.extra_imports
        );

        // Effect imports
        assert!(result.extra_imports.iter().any(|i| i.contains("@effect/schema")));
        assert!(result.extra_imports.iter().any(|i| i.contains("BigDecimal")));
    }
}
