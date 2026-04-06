//! End-to-end tests for output-rule WASM plugins.
//!
//! Tests the OutputRulePluginManager by loading a compiled WASM plugin
//! and verifying it returns expected type overrides based on struct context.
//!
//! Run with: cargo test --test type_plugin_e2e_test --features wasm-plugins

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

    fn create_plugin_manager() -> OutputRulePluginManager {
        let mut plugins = HashMap::new();
        plugins.insert(
            "decimal_override".to_string(),
            OutputRulePluginConfig {
                path: ".evenframe/plugins/decimal_override.wasm".to_string(),
            },
        );
        OutputRulePluginManager::new(&plugins, &playground_root()).expect("Should load type plugin")
    }

    fn make_struct_input(
        type_name: &str,
        derives: Vec<&str>,
        fields: Vec<(&str, &str)>,
        generator: &str,
    ) -> OutputRulePluginInput {
        OutputRulePluginInput {
            type_name: type_name.to_string(),
            kind: TypeKind::Struct,
            rust_derives: derives.into_iter().map(|s| s.to_string()).collect(),
            annotations: vec![],
            pipeline: "Both".to_string(),
            generator: generator.to_string(),
            fields: fields
                .into_iter()
                .map(|(name, ty)| OutputRulePluginFieldInfo {
                    field_name: name.to_string(),
                    field_type: ty.to_string(),
                    annotations: vec![],
                    validators: vec![],
                })
                .collect(),
        }
    }

    // ========================================================================
    // Plugin loading
    // ========================================================================

    #[test]
    fn test_type_plugin_loads_successfully() {
        let _pm = create_plugin_manager();
    }

    #[test]
    fn test_type_plugin_not_found_fails_at_load() {
        let mut plugins = HashMap::new();
        plugins.insert(
            "missing".to_string(),
            OutputRulePluginConfig {
                path: ".evenframe/plugins/does_not_exist.wasm".to_string(),
            },
        );
        let result = OutputRulePluginManager::new(&plugins, &playground_root());
        assert!(result.is_err(), "Should fail for missing WASM file");
    }

    // ========================================================================
    // Conditional overrides: Serialize + Decimal
    // ========================================================================

    #[test]
    fn test_decimal_override_when_serialize_derived() {
        let mut pm = create_plugin_manager();

        let input = make_struct_input(
            "Payment",
            vec!["Debug", "Clone", "Serialize", "Deserialize"],
            vec![
                ("id", "String"),
                ("amount", "Decimal"),
                ("currency", "String"),
            ],
            "macroforge",
        );

        let result = pm.transform_type("decimal_override", &input);
        assert!(result.is_ok(), "Should succeed: {:?}", result);

        let output = result.unwrap();

        // Decimal field should be overridden
        assert_eq!(
            output.field_type_overrides.get("amount"),
            Some(&"BigDecimal.BigDecimal".to_string()),
            "Decimal field should be overridden to BigDecimal.BigDecimal"
        );

        // String fields should NOT be overridden
        assert!(
            !output.field_type_overrides.contains_key("id"),
            "String field should not be overridden"
        );
        assert!(
            !output.field_type_overrides.contains_key("currency"),
            "String field should not be overridden"
        );

        // Should have an import
        assert!(
            output
                .extra_imports
                .iter()
                .any(|i| i.contains("BigDecimal")),
            "Should add BigDecimal import"
        );
    }

    #[test]
    fn test_no_override_without_serialize() {
        let mut pm = create_plugin_manager();

        // No Serialize derive
        let input = make_struct_input(
            "InternalPayment",
            vec!["Debug", "Clone"],
            vec![("amount", "Decimal"), ("note", "String")],
            "macroforge",
        );

        let result = pm.transform_type("decimal_override", &input);
        assert!(result.is_ok());

        let output = result.unwrap();

        // No overrides since Serialize is not derived
        assert!(
            output.field_type_overrides.is_empty(),
            "Should not override without Serialize. Got: {:?}",
            output.field_type_overrides
        );
        assert!(
            output.extra_imports.is_empty(),
            "Should not add imports without Serialize"
        );
    }

    #[test]
    fn test_no_override_without_decimal_field() {
        let mut pm = create_plugin_manager();

        let input = make_struct_input(
            "User",
            vec!["Debug", "Serialize"],
            vec![("name", "String"), ("age", "i32")],
            "macroforge",
        );

        let result = pm.transform_type("decimal_override", &input);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(
            output.field_type_overrides.is_empty(),
            "Should not override when no Decimal fields"
        );
    }

    // ========================================================================
    // Skip fields via @internal annotation
    // ========================================================================

    #[test]
    fn test_skip_fields_with_internal_annotation() {
        let mut pm = create_plugin_manager();

        let input = OutputRulePluginInput {
            type_name: "AuditLog".to_string(),
            kind: TypeKind::Struct,
            rust_derives: vec!["Debug".to_string()],
            annotations: vec![],
            pipeline: "Both".to_string(),
            generator: "arktype".to_string(),
            fields: vec![
                OutputRulePluginFieldInfo {
                    field_name: "message".to_string(),
                    field_type: "String".to_string(),
                    annotations: vec![],
                    validators: vec![],
                },
                OutputRulePluginFieldInfo {
                    field_name: "internal_id".to_string(),
                    field_type: "String".to_string(),
                    annotations: vec!["@internal".to_string()],
                    validators: vec![],
                },
            ],
        };

        let result = pm.transform_type("decimal_override", &input);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(
            output.skip_fields.contains(&"internal_id".to_string()),
            "Should skip field with @internal annotation. Got: {:?}",
            output.skip_fields
        );
        assert!(
            !output.skip_fields.contains(&"message".to_string()),
            "Should not skip field without @internal"
        );
    }

    // ========================================================================
    // compute_overrides aggregation
    // ========================================================================

    #[test]
    fn test_compute_overrides_aggregates() {
        let mut pm = create_plugin_manager();

        let input = make_struct_input(
            "Order",
            vec!["Serialize"],
            vec![
                ("total", "Decimal"),
                ("tax", "Decimal"),
                ("name", "String"),
            ],
            "effect",
        );

        let overrides = pm.compute_overrides(&input);

        // Both Decimal fields should have overrides
        assert!(
            overrides
                .field_types
                .contains_key(&("Order".to_string(), "total".to_string())),
            "total should be overridden"
        );
        assert!(
            overrides
                .field_types
                .contains_key(&("Order".to_string(), "tax".to_string())),
            "tax should be overridden"
        );
        assert!(
            !overrides
                .field_types
                .contains_key(&("Order".to_string(), "name".to_string())),
            "name should not be overridden"
        );

        // Should have import(s)
        assert!(
            !overrides.extra_imports.is_empty(),
            "Should have extra imports"
        );
    }

    // ========================================================================
    // Multiple calls (stability)
    // ========================================================================

    #[test]
    fn test_multiple_calls_stable() {
        let mut pm = create_plugin_manager();

        for _ in 0..10 {
            let input = make_struct_input(
                "Widget",
                vec!["Serialize"],
                vec![("price", "Decimal")],
                "macroforge",
            );

            let result = pm.transform_type("decimal_override", &input);
            assert!(result.is_ok());
            assert_eq!(
                result.unwrap().field_type_overrides.get("price"),
                Some(&"BigDecimal.BigDecimal".to_string())
            );
        }
    }

    // ========================================================================
    // Default output for no-op
    // ========================================================================

    #[test]
    fn test_empty_struct_returns_default_output() {
        let mut pm = create_plugin_manager();

        let input = make_struct_input("Empty", vec![], vec![], "macroforge");

        let result = pm.transform_type("decimal_override", &input);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(output.field_type_overrides.is_empty());
        assert!(output.skip_fields.is_empty());
        assert!(output.extra_imports.is_empty());
        assert!(output.error.is_none());
    }
}
