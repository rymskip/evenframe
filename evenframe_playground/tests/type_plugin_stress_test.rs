//! Hypercomplex stress tests for output-rule WASM plugins.
//!
//! Covers every edge case: unicode, deeply nested types, empty inputs,
//! massive field counts, all output fields, JSON special chars, error paths,
//! generator/pipeline-specific behavior, enum handling, multi-plugin composition,
//! and rapid sequential invocations.
//!
//! Run with: cargo test --test type_plugin_stress_test --features wasm-plugins

#[cfg(feature = "wasm-plugins")]
mod tests {
    use evenframe_core::config::OutputRulePluginConfig;
    use evenframe_core::typesync::plugin::OutputRulePluginManager;
    use evenframe_core::typesync::plugin_types::{
        TypeKind, OutputRulePluginFieldInfo, OutputRulePluginInput,
    };
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn playground_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn stress_manager() -> OutputRulePluginManager {
        let mut plugins = HashMap::new();
        plugins.insert(
            "stress".to_string(),
            OutputRulePluginConfig {
                path: ".evenframe/plugins/stress_test.wasm".to_string(),
            },
        );
        OutputRulePluginManager::new(&plugins, &playground_root()).expect("Should load stress plugin")
    }

    fn both_managers() -> OutputRulePluginManager {
        let mut plugins = HashMap::new();
        plugins.insert(
            "decimal_override".to_string(),
            OutputRulePluginConfig {
                path: ".evenframe/plugins/decimal_override.wasm".to_string(),
            },
        );
        plugins.insert(
            "stress".to_string(),
            OutputRulePluginConfig {
                path: ".evenframe/plugins/stress_test.wasm".to_string(),
            },
        );
        OutputRulePluginManager::new(&plugins, &playground_root()).expect("Should load both plugins")
    }

    fn field(name: &str, ty: &str) -> OutputRulePluginFieldInfo {
        OutputRulePluginFieldInfo {
            field_name: name.to_string(),
            field_type: ty.to_string(),
            annotations: vec![],
            validators: vec![],
        }
    }

    fn field_with_annotations(name: &str, ty: &str, anns: Vec<&str>) -> OutputRulePluginFieldInfo {
        OutputRulePluginFieldInfo {
            field_name: name.to_string(),
            field_type: ty.to_string(),
            annotations: anns.into_iter().map(|s| s.to_string()).collect(),
            validators: vec![],
        }
    }

    fn field_with_validators(name: &str, ty: &str, vals: Vec<&str>) -> OutputRulePluginFieldInfo {
        OutputRulePluginFieldInfo {
            field_name: name.to_string(),
            field_type: ty.to_string(),
            annotations: vec![],
            validators: vals.into_iter().map(|s| s.to_string()).collect(),
        }
    }

    fn input(
        type_name: &str,
        kind: TypeKind,
        derives: Vec<&str>,
        annotations: Vec<&str>,
        pipeline: &str,
        generator: &str,
        fields: Vec<OutputRulePluginFieldInfo>,
    ) -> OutputRulePluginInput {
        OutputRulePluginInput {
            type_name: type_name.to_string(),
            kind,
            rust_derives: derives.into_iter().map(|s| s.to_string()).collect(),
            annotations: annotations.into_iter().map(|s| s.to_string()).collect(),
            pipeline: pipeline.to_string(),
            generator: generator.to_string(),
            fields,
        }
    }

    // ========================================================================
    // Error handling
    // ========================================================================

    #[test]
    fn test_error_path_returns_error_field() {
        let mut pm = stress_manager();
        let inp = input(
            "PanicType",
            TypeKind::Struct,
            vec![],
            vec![],
            "Both",
            "macroforge",
            vec![field("x", "i32")],
        );
        let result = pm.transform_type("stress", &inp).unwrap();
        assert!(
            result.error.is_some(),
            "PanicType should trigger error. Got: {:?}",
            result
        );
        assert!(result.error.unwrap().contains("Intentional error"));
        // Other fields should be default when error is set
        assert!(result.field_type_overrides.is_empty());
    }

    #[test]
    fn test_compute_overrides_skips_error_plugin() {
        let mut pm = stress_manager();
        let inp = input(
            "PanicType",
            TypeKind::Struct,
            vec!["Serialize"],
            vec![],
            "Both",
            "macroforge",
            vec![field("amount", "Decimal")],
        );
        let overrides = pm.compute_overrides(&inp);
        // Error plugin output is skipped, so no overrides
        assert!(
            overrides.field_types.is_empty(),
            "Error plugins should be skipped in compute_overrides"
        );
    }

    // ========================================================================
    // Type name override
    // ========================================================================

    #[test]
    fn test_type_name_override_for_dto() {
        let mut pm = stress_manager();
        let inp = input(
            "UserDto",
            TypeKind::Struct,
            vec![],
            vec![],
            "Both",
            "macroforge",
            vec![field("name", "String")],
        );
        let result = pm.transform_type("stress", &inp).unwrap();
        assert_eq!(
            result.type_name_override,
            Some("UserResponse".to_string()),
            "Types ending in Dto should be renamed"
        );
    }

    #[test]
    fn test_type_name_not_overridden_for_non_dto() {
        let mut pm = stress_manager();
        let inp = input(
            "UserModel",
            TypeKind::Struct,
            vec![],
            vec![],
            "Both",
            "macroforge",
            vec![],
        );
        let result = pm.transform_type("stress", &inp).unwrap();
        assert!(result.type_name_override.is_none());
    }

    // ========================================================================
    // Derive combination matrix
    // ========================================================================

    #[test]
    fn test_serialize_only_overrides_decimal() {
        let mut pm = stress_manager();
        let inp = input(
            "A",
            TypeKind::Struct,
            vec!["Serialize"],
            vec![],
            "Both",
            "macroforge",
            vec![field("val", "Decimal")],
        );
        let result = pm.transform_type("stress", &inp).unwrap();
        assert_eq!(
            result.field_type_overrides.get("val"),
            Some(&"BigDecimal.BigDecimal".to_string())
        );
    }

    #[test]
    fn test_clone_overrides_option_datetime() {
        let mut pm = stress_manager();
        let inp = input(
            "B",
            TypeKind::Struct,
            vec!["Clone"],
            vec![],
            "Both",
            "macroforge",
            vec![field("ts", "Option<DateTime>")],
        );
        let result = pm.transform_type("stress", &inp).unwrap();
        assert_eq!(
            result.field_type_overrides.get("ts"),
            Some(&"DateTime.Utc | null".to_string())
        );
    }

    #[test]
    fn test_debug_overrides_vec_uuid() {
        let mut pm = stress_manager();
        let inp = input(
            "C",
            TypeKind::Struct,
            vec!["Debug"],
            vec![],
            "Both",
            "macroforge",
            vec![field("ids", "Vec<Uuid>")],
        );
        let result = pm.transform_type("stress", &inp).unwrap();
        assert_eq!(
            result.field_type_overrides.get("ids"),
            Some(&"ReadonlyArray<Uuid>".to_string())
        );
    }

    #[test]
    fn test_all_derives_combined() {
        let mut pm = stress_manager();
        let inp = input(
            "D",
            TypeKind::Struct,
            vec!["Debug", "Clone", "Serialize"],
            vec![],
            "Both",
            "macroforge",
            vec![
                field("amount", "Decimal"),
                field("expires", "Option<DateTime>"),
                field("refs", "Vec<Uuid>"),
                field("name", "String"),
            ],
        );
        let result = pm.transform_type("stress", &inp).unwrap();
        assert!(result.field_type_overrides.contains_key("amount"));
        assert!(result.field_type_overrides.contains_key("expires"));
        assert!(result.field_type_overrides.contains_key("refs"));
        assert!(
            !result.field_type_overrides.contains_key("name"),
            "String should not be overridden"
        );
    }

    // ========================================================================
    // HashMap and deeply nested types
    // ========================================================================

    #[test]
    fn test_hashmap_override() {
        let mut pm = stress_manager();
        let inp = input(
            "E",
            TypeKind::Struct,
            vec![],
            vec![],
            "Both",
            "macroforge",
            vec![field("scores", "HashMap<String, i64>")],
        );
        let result = pm.transform_type("stress", &inp).unwrap();
        assert_eq!(
            result.field_type_overrides.get("scores"),
            Some(&"Record<string, number>".to_string())
        );
    }

    #[test]
    fn test_deeply_nested_type() {
        let mut pm = stress_manager();
        let inp = input(
            "F",
            TypeKind::Struct,
            vec![],
            vec![],
            "Both",
            "macroforge",
            vec![field("deep", "Option<Vec<HashMap<String, Decimal>>>")],
        );
        let result = pm.transform_type("stress", &inp).unwrap();
        assert_eq!(
            result.field_type_overrides.get("deep"),
            Some(&"Array<Record<string, BigDecimal>> | null".to_string())
        );
    }

    // ========================================================================
    // Skip fields
    // ========================================================================

    #[test]
    fn test_skip_internal_annotated_field() {
        let mut pm = stress_manager();
        let inp = input(
            "G",
            TypeKind::Struct,
            vec![],
            vec![],
            "Both",
            "macroforge",
            vec![
                field("visible", "String"),
                field_with_annotations("secret", "String", vec!["@internal"]),
            ],
        );
        let result = pm.transform_type("stress", &inp).unwrap();
        assert!(result.skip_fields.contains(&"secret".to_string()));
        assert!(!result.skip_fields.contains(&"visible".to_string()));
    }

    #[test]
    fn test_skip_deprecated_annotated_field() {
        let mut pm = stress_manager();
        let inp = input(
            "H",
            TypeKind::Struct,
            vec![],
            vec![],
            "Both",
            "macroforge",
            vec![field_with_annotations("old_field", "i32", vec!["@deprecated"])],
        );
        let result = pm.transform_type("stress", &inp).unwrap();
        assert!(result.skip_fields.contains(&"old_field".to_string()));
    }

    #[test]
    fn test_skip_private_named_field() {
        let mut pm = stress_manager();
        let inp = input(
            "I",
            TypeKind::Struct,
            vec![],
            vec![],
            "Both",
            "macroforge",
            vec![
                field("name", "String"),
                field("__private", "String"),
            ],
        );
        let result = pm.transform_type("stress", &inp).unwrap();
        assert!(result.skip_fields.contains(&"__private".to_string()));
    }

    #[test]
    fn test_multiple_skip_reasons_same_field() {
        let mut pm = stress_manager();
        let inp = input(
            "J",
            TypeKind::Struct,
            vec![],
            vec![],
            "Both",
            "macroforge",
            vec![field_with_annotations(
                "__private",
                "String",
                vec!["@internal", "@deprecated"],
            )],
        );
        let result = pm.transform_type("stress", &inp).unwrap();
        // Field should appear in skip_fields (possibly multiple times, that's fine)
        assert!(
            result.skip_fields.contains(&"__private".to_string()),
            "Should skip for multiple reasons"
        );
    }

    // ========================================================================
    // Field annotations
    // ========================================================================

    #[test]
    fn test_readonly_annotation_on_timestamp_fields() {
        let mut pm = stress_manager();
        let inp = input(
            "K",
            TypeKind::Struct,
            vec![],
            vec![],
            "Both",
            "macroforge",
            vec![
                field("created_at", "String"),
                field("updated_at", "String"),
                field("name", "String"),
            ],
        );
        let result = pm.transform_type("stress", &inp).unwrap();
        assert!(
            result
                .field_annotations
                .get("created_at")
                .map(|v| v.contains(&"@readonly".to_string()))
                .unwrap_or(false),
            "created_at should get @readonly"
        );
        assert!(
            result
                .field_annotations
                .get("updated_at")
                .map(|v| v.contains(&"@readonly".to_string()))
                .unwrap_or(false),
            "updated_at should get @readonly"
        );
        assert!(
            !result.field_annotations.contains_key("name"),
            "name should not get @readonly"
        );
    }

    #[test]
    fn test_validated_annotation_on_fields_with_validators() {
        let mut pm = stress_manager();
        let inp = input(
            "L",
            TypeKind::Struct,
            vec![],
            vec![],
            "Both",
            "macroforge",
            vec![
                field_with_validators("email", "String", vec!["email"]),
                field("name", "String"),
            ],
        );
        let result = pm.transform_type("stress", &inp).unwrap();
        assert!(
            result
                .field_annotations
                .get("email")
                .map(|v| v.contains(&"@validated".to_string()))
                .unwrap_or(false),
            "email with validators should get @validated"
        );
    }

    // ========================================================================
    // Generator-specific behavior
    // ========================================================================

    #[test]
    fn test_arktype_specific_import() {
        let mut pm = stress_manager();
        let inp = input(
            "M",
            TypeKind::Struct,
            vec!["Serialize"],
            vec![],
            "Both",
            "arktype",
            vec![field("val", "Decimal")],
        );
        let result = pm.transform_type("stress", &inp).unwrap();
        assert!(
            result
                .extra_imports
                .iter()
                .any(|i| i.contains("arktype")),
            "Should add arktype-specific import. Got: {:?}",
            result.extra_imports
        );
    }

    #[test]
    fn test_macroforge_no_arktype_import() {
        let mut pm = stress_manager();
        let inp = input(
            "N",
            TypeKind::Struct,
            vec!["Serialize"],
            vec![],
            "Both",
            "macroforge",
            vec![field("val", "Decimal")],
        );
        let result = pm.transform_type("stress", &inp).unwrap();
        assert!(
            !result
                .extra_imports
                .iter()
                .any(|i| i.contains("arktype")),
            "macroforge should not get arktype import"
        );
    }

    // ========================================================================
    // Pipeline-specific behavior
    // ========================================================================

    #[test]
    fn test_schemasync_skips_option_fields() {
        let mut pm = stress_manager();
        let inp = input(
            "O",
            TypeKind::Struct,
            vec![],
            vec![],
            "Schemasync",
            "surrealdb",
            vec![
                field("name", "String"),
                field("bio", "Option<String>"),
                field("avatar", "Option<Url>"),
            ],
        );
        let result = pm.transform_type("stress", &inp).unwrap();
        assert!(result.skip_fields.contains(&"bio".to_string()));
        assert!(result.skip_fields.contains(&"avatar".to_string()));
        assert!(!result.skip_fields.contains(&"name".to_string()));
    }

    #[test]
    fn test_non_schemasync_keeps_option_fields() {
        let mut pm = stress_manager();
        let inp = input(
            "P",
            TypeKind::Struct,
            vec![],
            vec![],
            "Typesync",
            "macroforge",
            vec![
                field("name", "String"),
                field("bio", "Option<String>"),
            ],
        );
        let result = pm.transform_type("stress", &inp).unwrap();
        assert!(
            !result.skip_fields.contains(&"bio".to_string()),
            "Typesync should not skip Option fields"
        );
    }

    // ========================================================================
    // Enum handling
    // ========================================================================

    #[test]
    fn test_enum_gets_comment_import() {
        let mut pm = stress_manager();
        let inp = input(
            "Status",
            TypeKind::Enum,
            vec![],
            vec![],
            "Both",
            "macroforge",
            vec![
                field("Active", "Unit"),
                field("Inactive", "Unit"),
            ],
        );
        let result = pm.transform_type("stress", &inp).unwrap();
        assert!(
            result
                .extra_imports
                .iter()
                .any(|i| i.contains("Enum: Status")),
            "Enum should get enum-specific comment. Got: {:?}",
            result.extra_imports
        );
    }

    // ========================================================================
    // JSON special characters
    // ========================================================================

    #[test]
    fn test_json_special_chars_in_override() {
        let mut pm = stress_manager();
        let inp = input(
            "Q",
            TypeKind::Struct,
            vec![],
            vec![],
            "Both",
            "macroforge",
            vec![field("json_tricky", "String")],
        );
        let result = pm.transform_type("stress", &inp).unwrap();
        let override_val = result.field_type_overrides.get("json_tricky");
        assert!(
            override_val.is_some(),
            "json_tricky should be overridden"
        );
        assert!(
            override_val.unwrap().contains("tricky"),
            "Override should contain special chars. Got: {:?}",
            override_val
        );
    }

    // ========================================================================
    // Empty and edge inputs
    // ========================================================================

    #[test]
    fn test_empty_struct_no_fields_no_derives() {
        let mut pm = stress_manager();
        let inp = input(
            "Empty",
            TypeKind::Struct,
            vec![],
            vec![],
            "Both",
            "macroforge",
            vec![],
        );
        let result = pm.transform_type("stress", &inp).unwrap();
        assert!(result.field_type_overrides.is_empty());
        assert!(result.skip_fields.is_empty());
        assert!(result.field_annotations.is_empty());
        assert!(result.error.is_none());
    }

    #[test]
    fn test_single_field_struct() {
        let mut pm = stress_manager();
        let inp = input(
            "Single",
            TypeKind::Struct,
            vec!["Serialize"],
            vec![],
            "Both",
            "macroforge",
            vec![field("x", "Decimal")],
        );
        let result = pm.transform_type("stress", &inp).unwrap();
        assert_eq!(result.field_type_overrides.len(), 1);
    }

    // ========================================================================
    // Massive field count
    // ========================================================================

    #[test]
    fn test_100_fields() {
        let mut pm = stress_manager();
        let fields: Vec<OutputRulePluginFieldInfo> = (0..100)
            .map(|i| {
                if i % 3 == 0 {
                    field(&format!("decimal_{}", i), "Decimal")
                } else {
                    field(&format!("string_{}", i), "String")
                }
            })
            .collect();

        let inp = input(
            "BigStruct",
            TypeKind::Struct,
            vec!["Serialize"],
            vec![],
            "Both",
            "macroforge",
            fields,
        );
        let result = pm.transform_type("stress", &inp).unwrap();

        // 34 Decimal fields (0, 3, 6, ..., 99)
        let decimal_overrides: Vec<_> = result
            .field_type_overrides
            .keys()
            .filter(|k| k.starts_with("decimal_"))
            .collect();
        assert_eq!(
            decimal_overrides.len(),
            34,
            "Should override all 34 Decimal fields. Got: {}",
            decimal_overrides.len()
        );
    }

    #[test]
    fn test_500_fields_performance() {
        let mut pm = stress_manager();
        let fields: Vec<OutputRulePluginFieldInfo> = (0..500)
            .map(|i| field(&format!("f_{}", i), "String"))
            .collect();

        let inp = input(
            "HugeStruct",
            TypeKind::Struct,
            vec![],
            vec![],
            "Both",
            "macroforge",
            fields,
        );
        let result = pm.transform_type("stress", &inp);
        assert!(result.is_ok(), "Should handle 500 fields without error");
    }

    // ========================================================================
    // Multi-plugin composition
    // ========================================================================

    #[test]
    fn test_two_plugins_both_apply() {
        let mut pm = both_managers();
        let inp = input(
            "ComposedDto",
            TypeKind::Struct,
            vec!["Serialize"],
            vec![],
            "Both",
            "macroforge",
            vec![
                field("price", "Decimal"),
                field("name", "String"),
            ],
        );

        // Both plugins should process; compute_overrides merges them
        let overrides = pm.compute_overrides(&inp);

        // Both plugins override Decimal → BigDecimal.BigDecimal
        assert!(
            overrides
                .field_types
                .contains_key(&("ComposedDto".to_string(), "price".to_string())),
            "Decimal field should be overridden by at least one plugin"
        );

        // Stress plugin should set type_name_override for Dto suffix
        assert!(
            overrides
                .type_name_overrides
                .contains_key("ComposedDto"),
            "Dto type should get name override"
        );
        assert_eq!(
            overrides.type_name_overrides.get("ComposedDto"),
            Some(&"ComposedResponse".to_string())
        );
    }

    #[test]
    fn test_two_plugins_extra_imports_accumulate() {
        let mut pm = both_managers();
        let inp = input(
            "MultiImport",
            TypeKind::Struct,
            vec!["Serialize"],
            vec![],
            "Both",
            "macroforge",
            vec![field("val", "Decimal")],
        );
        let overrides = pm.compute_overrides(&inp);

        // Both plugins add BigDecimal import, so we should have at least 2
        let bigdecimal_imports: Vec<_> = overrides
            .extra_imports
            .iter()
            .filter(|i| i.contains("BigDecimal"))
            .collect();
        assert!(
            bigdecimal_imports.len() >= 2,
            "Both plugins should contribute imports. Got: {:?}",
            overrides.extra_imports
        );
    }

    // ========================================================================
    // Rapid sequential calls (WASM memory stability)
    // ========================================================================

    #[test]
    fn test_rapid_sequential_calls() {
        let mut pm = stress_manager();

        for i in 0..50 {
            let inp = input(
                &format!("Rapid{}", i),
                TypeKind::Struct,
                vec!["Serialize", "Clone", "Debug"],
                vec![],
                "Both",
                "macroforge",
                vec![
                    field("amount", "Decimal"),
                    field("expires", "Option<DateTime>"),
                    field("ids", "Vec<Uuid>"),
                ],
            );
            let result = pm.transform_type("stress", &inp);
            assert!(
                result.is_ok(),
                "Call {} failed: {:?}",
                i,
                result.err()
            );
            let output = result.unwrap();
            assert!(output.field_type_overrides.contains_key("amount"));
            assert!(output.field_type_overrides.contains_key("expires"));
            assert!(output.field_type_overrides.contains_key("ids"));
        }
    }

    // ========================================================================
    // Alternating error and success
    // ========================================================================

    #[test]
    fn test_alternating_error_and_success() {
        let mut pm = stress_manager();

        for i in 0..20 {
            let type_name = if i % 2 == 0 { "PanicType" } else { "NormalType" };
            let inp = input(
                type_name,
                TypeKind::Struct,
                vec![],
                vec![],
                "Both",
                "macroforge",
                vec![field("x", "i32")],
            );
            let result = pm.transform_type("stress", &inp);
            assert!(result.is_ok(), "Call {} should not crash: {:?}", i, result);
            if i % 2 == 0 {
                assert!(result.unwrap().error.is_some());
            } else {
                assert!(result.unwrap().error.is_none());
            }
        }
    }

    // ========================================================================
    // Combined everything: overrides + skips + annotations + imports + rename
    // ========================================================================

    #[test]
    fn test_everything_combined() {
        let mut pm = stress_manager();
        let inp = input(
            "KitchenSinkDto",
            TypeKind::Struct,
            vec!["Debug", "Clone", "Serialize"],
            vec!["@audit"],
            "Both",
            "arktype",
            vec![
                field("amount", "Decimal"),                       // override
                field("expires", "Option<DateTime>"),             // override
                field("ids", "Vec<Uuid>"),                        // override
                field("scores", "HashMap<String, i64>"),          // override
                field("created_at", "String"),                    // annotation
                field_with_validators("email", "String", vec!["email"]), // annotation
                field_with_annotations("secret", "String", vec!["@internal"]), // skip
                field("__private", "i32"),                        // skip
                field("normal", "bool"),                          // untouched
                field("json_tricky", "String"),                   // JSON special chars
            ],
        );

        let result = pm.transform_type("stress", &inp).unwrap();
        assert!(result.error.is_none());

        // Type name override
        assert_eq!(
            result.type_name_override,
            Some("KitchenSinkResponse".to_string())
        );

        // Field type overrides
        assert!(result.field_type_overrides.contains_key("amount"));
        assert!(result.field_type_overrides.contains_key("expires"));
        assert!(result.field_type_overrides.contains_key("ids"));
        assert!(result.field_type_overrides.contains_key("scores"));
        assert!(result.field_type_overrides.contains_key("json_tricky"));
        assert!(!result.field_type_overrides.contains_key("normal"));

        // Skips
        assert!(result.skip_fields.contains(&"secret".to_string()));
        assert!(result.skip_fields.contains(&"__private".to_string()));

        // Annotations
        assert!(
            result
                .field_annotations
                .get("created_at")
                .map(|v| v.contains(&"@readonly".to_string()))
                .unwrap_or(false)
        );
        assert!(
            result
                .field_annotations
                .get("email")
                .map(|v| v.contains(&"@validated".to_string()))
                .unwrap_or(false)
        );

        // Imports (BigDecimal + arktype)
        assert!(result.extra_imports.iter().any(|i| i.contains("BigDecimal")));
        assert!(result.extra_imports.iter().any(|i| i.contains("arktype")));
    }
}
