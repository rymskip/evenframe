//! End-to-end tests for WASM plugin mock data generation.
//!
//! Tests the PluginManager directly by loading a compiled WASM plugin
//! and verifying it generates expected field values.
//!
//! Run with: cargo test --test wasm_plugin_e2e_test --features wasm-plugins

#[cfg(feature = "wasm-plugins")]
mod tests {
    use evenframe_core::schemasync::config::PluginConfig;
    use evenframe_core::schemasync::mockmake::plugin::PluginManager;
    use evenframe_core::schemasync::mockmake::plugin_types::PluginFieldInput;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn playground_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn create_plugin_manager() -> PluginManager {
        let mut plugins = HashMap::new();
        plugins.insert(
            "hello".to_string(),
            PluginConfig {
                path: ".evenframe/plugins/hello_plugin.wasm".to_string(),
            },
        );
        PluginManager::new(&plugins, &playground_root()).expect("Should load plugin")
    }

    fn make_field_input(field_name: &str, field_type: &str, index: usize) -> PluginFieldInput {
        PluginFieldInput {
            table_name: "test_table".to_string(),
            field_name: field_name.to_string(),
            field_type: field_type.to_string(),
            record_index: index,
            total_records: 10,
            record_id: format!("test_table:{}", index),
        }
    }

    #[test]
    fn test_plugin_loads_successfully() {
        let _pm = create_plugin_manager();
    }

    #[test]
    fn test_plugin_generates_string_field() {
        let mut pm = create_plugin_manager();
        let input = make_field_input("username", "String", 0);
        let result = pm.generate_field_value("hello", &input);
        assert!(result.is_ok(), "Should succeed: {:?}", result);
        let value = result.unwrap();
        assert_eq!(value, "'plugin_username_0'");
    }

    #[test]
    fn test_plugin_generates_string_field_different_index() {
        let mut pm = create_plugin_manager();
        let input = make_field_input("email", "String", 5);
        let result = pm.generate_field_value("hello", &input);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "'plugin_email_5'");
    }

    #[test]
    fn test_plugin_generates_integer_field() {
        let mut pm = create_plugin_manager();
        let input = make_field_input("age", "I32", 3);
        let result = pm.generate_field_value("hello", &input);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "300");
    }

    #[test]
    fn test_plugin_generates_float_field() {
        let mut pm = create_plugin_manager();
        let input = make_field_input("score", "F64", 2);
        let result = pm.generate_field_value("hello", &input);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "3.00");
    }

    #[test]
    fn test_plugin_generates_bool_field() {
        let mut pm = create_plugin_manager();

        let input_even = make_field_input("active", "Bool", 0);
        assert_eq!(
            pm.generate_field_value("hello", &input_even).unwrap(),
            "true"
        );

        let input_odd = make_field_input("active", "Bool", 1);
        assert_eq!(
            pm.generate_field_value("hello", &input_odd).unwrap(),
            "false"
        );
    }

    #[test]
    fn test_plugin_handles_multiple_calls() {
        let mut pm = create_plugin_manager();

        for i in 0..20 {
            let input = make_field_input("name", "String", i);
            let result = pm.generate_field_value("hello", &input);
            assert!(result.is_ok(), "Call {} failed: {:?}", i, result);
            assert_eq!(result.unwrap(), format!("'plugin_name_{}'", i));
        }
    }

    #[test]
    fn test_unknown_plugin_returns_error() {
        let mut pm = create_plugin_manager();
        let input = make_field_input("name", "String", 0);
        let result = pm.generate_field_value("nonexistent", &input);
        assert!(result.is_err());
    }

    #[test]
    fn test_plugin_not_found_fails_at_load() {
        let mut plugins = HashMap::new();
        plugins.insert(
            "missing".to_string(),
            PluginConfig {
                path: ".evenframe/plugins/does_not_exist.wasm".to_string(),
            },
        );
        let result = PluginManager::new(&plugins, &playground_root());
        assert!(result.is_err(), "Should fail for missing WASM file");
    }
}
