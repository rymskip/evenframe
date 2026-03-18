//! E2E test for the dealdraft test_user WASM plugin.
//!
//! Verifies that record_index 0 generates the fixed e2e test user credentials
//! and other indices fall back gracefully.
//!
//! Run with: cargo test --test test_user_plugin_e2e_test --features wasm-plugins

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
            "test_user".to_string(),
            PluginConfig {
                path: ".evenframe/plugins/test_user.wasm".to_string(),
            },
        );
        PluginManager::new(&plugins, &playground_root()).expect("Should load test_user plugin")
    }

    fn make_input(field_name: &str, field_type: &str, index: usize) -> PluginFieldInput {
        PluginFieldInput {
            table_name: "user".to_string(),
            field_name: field_name.to_string(),
            field_type: field_type.to_string(),
            record_index: index,
            total_records: 15,
            record_id: format!("user:{}", index),
        }
    }

    // ===== Record index 0: fixed test user =====

    #[test]
    fn test_user_email() {
        let mut pm = create_plugin_manager();
        let result = pm.generate_field_value("test_user", &make_input("email", "Option(String)", 0));
        assert_eq!(result.unwrap(), "'test@example.com'");
    }

    #[test]
    fn test_user_password() {
        let mut pm = create_plugin_manager();
        let result = pm.generate_field_value("test_user", &make_input("password", "Option(String)", 0));
        assert_eq!(result.unwrap(), "'TestPassword123!'");
    }

    #[test]
    fn test_user_first_name() {
        let mut pm = create_plugin_manager();
        let result = pm.generate_field_value("test_user", &make_input("first_name", "String", 0));
        assert_eq!(result.unwrap(), "'Test'");
    }

    #[test]
    fn test_user_last_name() {
        let mut pm = create_plugin_manager();
        let result = pm.generate_field_value("test_user", &make_input("last_name", "String", 0));
        assert_eq!(result.unwrap(), "'User'");
    }

    #[test]
    fn test_user_role() {
        let mut pm = create_plugin_manager();
        let result = pm.generate_field_value("test_user", &make_input("role", "Other(UserRole)", 0));
        assert_eq!(result.unwrap(), "'Administrator'");
    }

    #[test]
    fn test_user_email_verified() {
        let mut pm = create_plugin_manager();
        let result = pm.generate_field_value("test_user", &make_input("email_verified", "Bool", 0));
        assert_eq!(result.unwrap(), "true");
    }

    // ===== Record index > 0: falls back to default generation =====

    #[test]
    fn test_non_zero_index_falls_back() {
        let mut pm = create_plugin_manager();
        let result = pm.generate_field_value("test_user", &make_input("email", "Option(String)", 1));
        assert!(result.is_err(), "Index 1 should return error to trigger fallback");
    }

    #[test]
    fn test_unknown_field_falls_back() {
        let mut pm = create_plugin_manager();
        // settings is a complex nested type — plugin skips it, evenframe handles it
        let result = pm.generate_field_value("test_user", &make_input("settings", "Other(Settings)", 0));
        assert!(result.is_err(), "Complex fields should fall back to evenframe");
    }
}
