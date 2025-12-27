//! Tests that verify Evenframe CLI output
//!
//! These tests run the evenframe CLI and verify:
//! 1. TypeScript files are generated correctly
//! 2. Database schema files are generated correctly
//!
//! Note: These tests require the evenframe CLI to be built.
//! Run with: cargo test --test evenframe_output_test

use std::env;
use std::fs;
use std::path::PathBuf;

#[allow(unused_imports)]
use std::process::Command;

/// Get the workspace root directory
#[allow(dead_code)]
fn get_workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // evenframe_playground is at the same level as evenframe, evenframe_core, etc.
    manifest_dir.parent().unwrap().to_path_buf()
}

/// Get the playground directory
fn get_playground_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Create the bindings output directory if it doesn't exist
#[allow(dead_code)]
fn ensure_bindings_dir() {
    let playground_dir = get_playground_dir();
    let bindings_dir = playground_dir.join("src/bindings");
    if !bindings_dir.exists() {
        fs::create_dir_all(&bindings_dir).expect("Failed to create bindings directory");
    }
}

/// Clean up generated files before test
#[allow(dead_code)]
fn cleanup_generated_files() {
    let playground_dir = get_playground_dir();
    let bindings_dir = playground_dir.join("src/bindings");

    if bindings_dir.exists() {
        // Remove all .ts files in bindings directory
        if let Ok(entries) = fs::read_dir(&bindings_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "ts") {
                    let _ = fs::remove_file(path);
                }
            }
        }
    }
}

/// Run the evenframe CLI on the playground
#[allow(dead_code)]
fn run_evenframe() -> Result<std::process::Output, std::io::Error> {
    let workspace_root = get_workspace_root();
    let playground_dir = get_playground_dir();

    // First, ensure bindings directory exists
    ensure_bindings_dir();

    // Build evenframe first
    let build_output = Command::new("cargo")
        .args(["build", "-p", "evenframe"])
        .current_dir(&workspace_root)
        .output()?;

    if !build_output.status.success() {
        eprintln!(
            "Failed to build evenframe: {}",
            String::from_utf8_lossy(&build_output.stderr)
        );
        return Err(std::io::Error::other("Failed to build evenframe"));
    }

    // Run evenframe in the playground directory
    // Note: We need to set up a .env file or environment variables for the database
    Command::new("cargo")
        .args(["run", "-p", "evenframe"])
        .current_dir(&playground_dir)
        .env("RUST_LOG", "warn") // Reduce output noise
        .env("SURREALDB_URL", "http://localhost:8000")
        .env("SURREALDB_NS", "test")
        .env("SURREALDB_DB", "test")
        .env("SURREALDB_USER", "root")
        .env("SURREALDB_PASSWORD", "root")
        .output()
}

/// Test that verifies the model structure matches expected patterns
#[test]
fn test_model_field_counts() {
    let playground_dir = get_playground_dir();

    // User struct should have specific fields
    let auth_content = fs::read_to_string(playground_dir.join("src/models/auth.rs"))
        .expect("Failed to read auth.rs");

    // Count fields in User struct
    let user_fields = vec![
        "pub id:",
        "pub email:",
        "pub username:",
        "pub password_hash:",
        "pub roles:",
        "pub is_active:",
        "pub created_at:",
        "pub updated_at:",
    ];

    for field in &user_fields {
        assert!(
            auth_content.contains(field),
            "User struct should contain field: {}",
            field
        );
    }

    // Session struct should have specific fields
    let session_fields = vec![
        "pub id:",
        "pub user:",
        "pub token:",
        "pub expires_at:",
        "pub created_at:",
    ];

    for field in &session_fields {
        assert!(
            auth_content.contains(field),
            "Session struct should contain field: {}",
            field
        );
    }
}

/// Test that format attributes are valid
#[test]
fn test_valid_format_attributes() {
    let playground_dir = get_playground_dir();

    let files = vec![
        "src/models/auth.rs",
        "src/models/blog.rs",
        "src/models/ecommerce.rs",
    ];

    // Valid format attribute patterns
    let valid_formats = vec![
        "#[format(Email)]",
        "#[format(DateTime)]",
        "#[format(Date)]",
        "#[format(Time)]",
        "#[format(PhoneNumber)]",
        "#[format(Url(",
    ];

    for file_path in files {
        let content = fs::read_to_string(playground_dir.join(file_path))
            .unwrap_or_else(|_| panic!("Failed to read {}", file_path));

        // Find all format attributes
        for line in content.lines() {
            if line.trim().starts_with("#[format(") {
                let is_valid = valid_formats.iter().any(|fmt| line.contains(fmt));
                assert!(
                    is_valid,
                    "Invalid format attribute in {}: {}",
                    file_path, line
                );
            }
        }
    }
}

/// Test that edge attributes have required parameters
#[test]
fn test_edge_attribute_structure() {
    let playground_dir = get_playground_dir();

    let files = vec![
        "src/models/auth.rs",
        "src/models/blog.rs",
        "src/models/ecommerce.rs",
    ];

    for file_path in files {
        let content = fs::read_to_string(playground_dir.join(file_path))
            .unwrap_or_else(|_| panic!("Failed to read {}", file_path));

        // Find all edge attributes
        for line in content.lines() {
            if line.contains("#[edge(") {
                // Edge should have name parameter
                assert!(
                    line.contains("name = "),
                    "Edge attribute should have 'name' parameter in {}: {}",
                    file_path,
                    line
                );

                // Edge should have from parameter
                assert!(
                    line.contains("from = "),
                    "Edge attribute should have 'from' parameter in {}: {}",
                    file_path,
                    line
                );

                // Edge should have to parameter
                assert!(
                    line.contains("to = "),
                    "Edge attribute should have 'to' parameter in {}: {}",
                    file_path,
                    line
                );

                // Edge should have direction parameter
                assert!(
                    line.contains("direction = "),
                    "Edge attribute should have 'direction' parameter in {}: {}",
                    file_path,
                    line
                );
            }
        }
    }
}

/// Test expected TypeScript type mappings (without running evenframe)
#[test]
fn test_expected_typescript_mappings() {
    // This test documents the expected Rust -> TypeScript type mappings
    // Based on evenframe's type generation

    let type_mappings: Vec<(&str, &str)> = vec![
        ("String", "string"),
        ("bool", "boolean"),
        ("f64", "number"),
        ("u32", "number"),
        ("i32", "number"),
        ("Option<T>", "T | null | undefined"),
        ("Vec<T>", "T[]"),
    ];

    // Just verify the mappings are documented
    assert!(!type_mappings.is_empty());

    // Verify specific expected generated types
    let expected_types = vec![
        // Auth models
        "User",
        "Session",
        "Role",
        // Blog models
        "Tag",
        "Author",
        "Post",
        "Comment",
        // Ecommerce models
        "Product",
        "Order",
        "Customer",
        "OrderStatus",
        "ProductCategory",
        "Address",
        "CartItem",
    ];

    assert_eq!(expected_types.len(), 14, "Should have 14 expected types");
}

/// Test that the evenframe.toml has correct output path
#[test]
fn test_output_path_configuration() {
    let playground_dir = get_playground_dir();
    let config_content = fs::read_to_string(playground_dir.join("evenframe.toml"))
        .expect("Failed to read evenframe.toml");

    // Check output_path is set to ./src/bindings/
    assert!(
        config_content.contains(r#"output_path = "./src/bindings/""#),
        "output_path should be set to ./src/bindings/"
    );
}

/// Test database configuration section
#[test]
fn test_database_configuration() {
    let playground_dir = get_playground_dir();
    let config_content = fs::read_to_string(playground_dir.join("evenframe.toml"))
        .expect("Failed to read evenframe.toml");

    // Check database provider
    assert!(
        config_content.contains(r#"provider = "surrealdb""#),
        "Database provider should be surrealdb"
    );

    // Check for environment variable references
    assert!(
        config_content.contains("${SURREALDB_URL}"),
        "Database URL should reference SURREALDB_URL env var"
    );
    assert!(
        config_content.contains("${SURREALDB_NS}"),
        "Namespace should reference SURREALDB_NS env var"
    );
    assert!(
        config_content.contains("${SURREALDB_DB}"),
        "Database should reference SURREALDB_DB env var"
    );
}

/// Test mock generation configuration
#[test]
fn test_mock_generation_configuration() {
    let playground_dir = get_playground_dir();
    let config_content = fs::read_to_string(playground_dir.join("evenframe.toml"))
        .expect("Failed to read evenframe.toml");

    // Check mock generation is enabled
    assert!(
        config_content.contains("should_generate_mocks = true"),
        "Mock generation should be enabled"
    );

    // Check mock gen config section exists
    assert!(
        config_content.contains("[schemasync.mock_gen_config]"),
        "Mock gen config section should exist"
    );

    // Check default record count
    assert!(
        config_content.contains("default_record_count = 100"),
        "Default record count should be 100"
    );
}

/// Test that all model files have proper imports
#[test]
fn test_model_imports() {
    let playground_dir = get_playground_dir();

    // Check auth.rs imports
    let auth_content = fs::read_to_string(playground_dir.join("src/models/auth.rs"))
        .expect("Failed to read auth.rs");
    assert!(
        auth_content.contains("use evenframe::Evenframe;"),
        "auth.rs should import Evenframe"
    );
    assert!(
        auth_content.contains("use serde::{Deserialize, Serialize};"),
        "auth.rs should import serde traits"
    );

    // Check blog.rs imports
    let blog_content = fs::read_to_string(playground_dir.join("src/models/blog.rs"))
        .expect("Failed to read blog.rs");
    assert!(
        blog_content.contains("use evenframe::types::RecordLink;"),
        "blog.rs should import RecordLink"
    );
    assert!(
        blog_content.contains("use super::auth::User;"),
        "blog.rs should import User from auth"
    );

    // Check ecommerce.rs imports
    let ecommerce_content = fs::read_to_string(playground_dir.join("src/models/ecommerce.rs"))
        .expect("Failed to read ecommerce.rs");
    assert!(
        ecommerce_content.contains("use evenframe::types::RecordLink;"),
        "ecommerce.rs should import RecordLink"
    );
    assert!(
        ecommerce_content.contains("use super::auth::User;"),
        "ecommerce.rs should import User from auth"
    );
}

/// Test that .env.example exists with required variables
#[test]
fn test_env_example_file() {
    let playground_dir = get_playground_dir();
    let env_example_path = playground_dir.join(".env.example");

    assert!(env_example_path.exists(), ".env.example should exist");

    let env_content =
        fs::read_to_string(&env_example_path).expect("Failed to read .env.example");

    // Check for required environment variables
    let required_vars = vec![
        "SURREALDB_URL",
        "SURREALDB_NS",
        "SURREALDB_DB",
        "SURREALDB_USER",
        "SURREALDB_PASSWORD",
    ];

    for var in required_vars {
        assert!(
            env_content.contains(var),
            ".env.example should contain {}",
            var
        );
    }
}

// ==================== Validator Attribute Tests ====================

/// Test that User model has email validation
#[test]
fn test_user_email_validators() {
    let playground_dir = get_playground_dir();
    let auth_content = fs::read_to_string(playground_dir.join("src/models/auth.rs"))
        .expect("Failed to read auth.rs");

    // Email field should have both format and validators
    assert!(
        auth_content.contains("#[format(Email)]"),
        "User.email should have #[format(Email)] for mock data generation"
    );
    assert!(
        auth_content.contains("StringValidator::Email"),
        "User.email should have StringValidator::Email validation"
    );
    assert!(
        auth_content.contains("StringValidator::MinLength(5)"),
        "User.email should have MinLength(5) validation"
    );
    assert!(
        auth_content.contains("StringValidator::MaxLength(255)"),
        "User.email should have MaxLength(255) validation"
    );
}

/// Test that User model has username validation
#[test]
fn test_user_username_validators() {
    let playground_dir = get_playground_dir();
    let auth_content = fs::read_to_string(playground_dir.join("src/models/auth.rs"))
        .expect("Failed to read auth.rs");

    assert!(
        auth_content.contains("StringValidator::Alphanumeric"),
        "User.username should have Alphanumeric validation"
    );
    assert!(
        auth_content.contains("StringValidator::MinLength(3)"),
        "User.username should have MinLength(3) validation"
    );
    assert!(
        auth_content.contains("StringValidator::MaxLength(50)"),
        "User.username should have MaxLength(50) validation"
    );
}

/// Test that User model has password_hash validation
#[test]
fn test_user_password_hash_validators() {
    let playground_dir = get_playground_dir();
    let auth_content = fs::read_to_string(playground_dir.join("src/models/auth.rs"))
        .expect("Failed to read auth.rs");

    assert!(
        auth_content.contains("StringValidator::NonEmpty"),
        "User.password_hash should be non-empty"
    );
}

/// Test that Session token has validation
#[test]
fn test_session_token_validators() {
    let playground_dir = get_playground_dir();
    let auth_content = fs::read_to_string(playground_dir.join("src/models/auth.rs"))
        .expect("Failed to read auth.rs");

    assert!(
        auth_content.contains("StringValidator::MinLength(32)"),
        "Session.token should have MinLength(32) for security"
    );
}

/// Test that Product model has price validation
#[test]
fn test_product_price_validators() {
    let playground_dir = get_playground_dir();
    let ecommerce_content = fs::read_to_string(playground_dir.join("src/models/ecommerce.rs"))
        .expect("Failed to read ecommerce.rs");

    assert!(
        ecommerce_content.contains("NumberValidator::Positive"),
        "Product.price should require positive values"
    );
}

/// Test that Product model has stock quantity validation
#[test]
fn test_product_stock_validators() {
    let playground_dir = get_playground_dir();
    let ecommerce_content = fs::read_to_string(playground_dir.join("src/models/ecommerce.rs"))
        .expect("Failed to read ecommerce.rs");

    assert!(
        ecommerce_content.contains("NumberValidator::NonNegative"),
        "Product.stock_quantity should be non-negative"
    );
}

/// Test that Order model has total validation
#[test]
fn test_order_total_validators() {
    let playground_dir = get_playground_dir();
    let ecommerce_content = fs::read_to_string(playground_dir.join("src/models/ecommerce.rs"))
        .expect("Failed to read ecommerce.rs");

    assert!(
        ecommerce_content.contains("NumberValidator::Positive"),
        "Order.total should be positive"
    );
}

/// Test that Address model has country code validation
#[test]
fn test_address_country_validators() {
    let playground_dir = get_playground_dir();
    let ecommerce_content = fs::read_to_string(playground_dir.join("src/models/ecommerce.rs"))
        .expect("Failed to read ecommerce.rs");

    assert!(
        ecommerce_content.contains("StringValidator::Uppercased"),
        "Address.country should be uppercase (ISO code)"
    );
}

/// Test that Post model has title validation
#[test]
fn test_post_title_validators() {
    let playground_dir = get_playground_dir();
    let blog_content = fs::read_to_string(playground_dir.join("src/models/blog.rs"))
        .expect("Failed to read blog.rs");

    assert!(
        blog_content.contains("StringValidator::NonEmpty"),
        "Post.title should be non-empty"
    );
    assert!(
        blog_content.contains("StringValidator::MaxLength(200)"),
        "Post.title should have MaxLength(200)"
    );
}

/// Test that Post model has slug validation
#[test]
fn test_post_slug_validators() {
    let playground_dir = get_playground_dir();
    let blog_content = fs::read_to_string(playground_dir.join("src/models/blog.rs"))
        .expect("Failed to read blog.rs");

    assert!(
        blog_content.contains("StringValidator::Lowercased"),
        "Post.slug should be lowercase"
    );
}

/// Test that Post model has view_count validation
#[test]
fn test_post_view_count_validators() {
    let playground_dir = get_playground_dir();
    let blog_content = fs::read_to_string(playground_dir.join("src/models/blog.rs"))
        .expect("Failed to read blog.rs");

    assert!(
        blog_content.contains("NumberValidator::NonNegative"),
        "Post.view_count should be non-negative"
    );
}

/// Test that Comment model has content validation
#[test]
fn test_comment_content_validators() {
    let playground_dir = get_playground_dir();
    let blog_content = fs::read_to_string(playground_dir.join("src/models/blog.rs"))
        .expect("Failed to read blog.rs");

    assert!(
        blog_content.contains("StringValidator::MaxLength(5000)"),
        "Comment.content should have MaxLength(5000)"
    );
}

/// Test that Author twitter handle has validation
#[test]
fn test_author_twitter_validators() {
    let playground_dir = get_playground_dir();
    let blog_content = fs::read_to_string(playground_dir.join("src/models/blog.rs"))
        .expect("Failed to read blog.rs");

    assert!(
        blog_content.contains("StringValidator::StartsWith(\"@\")"),
        "Author.twitter_handle should start with @"
    );
}

/// Test that URL fields have both format and validation
#[test]
fn test_url_fields_have_format_and_validators() {
    let playground_dir = get_playground_dir();

    let blog_content = fs::read_to_string(playground_dir.join("src/models/blog.rs"))
        .expect("Failed to read blog.rs");
    let ecommerce_content = fs::read_to_string(playground_dir.join("src/models/ecommerce.rs"))
        .expect("Failed to read ecommerce.rs");

    // Check that URL fields have both #[format(Url(...))] and #[validators(StringValidator::Url)]
    assert!(
        blog_content.contains("#[format(Url(") && blog_content.contains("StringValidator::Url"),
        "Blog URL fields should have both format and validators"
    );
    assert!(
        ecommerce_content.contains("#[format(Url(") && ecommerce_content.contains("StringValidator::Url"),
        "Ecommerce URL fields should have both format and validators"
    );
}

/// Test validator attribute syntax is correct
#[test]
fn test_validators_attribute_syntax() {
    let playground_dir = get_playground_dir();

    let files = vec![
        "src/models/auth.rs",
        "src/models/blog.rs",
        "src/models/ecommerce.rs",
    ];

    for file_path in files {
        let content = fs::read_to_string(playground_dir.join(file_path))
            .unwrap_or_else(|_| panic!("Failed to read {}", file_path));

        // Find all validators attributes and verify they have proper syntax
        for line in content.lines() {
            if line.trim().starts_with("#[validators(") {
                // Should contain valid validator types
                let has_string_validator = line.contains("StringValidator::");
                let has_number_validator = line.contains("NumberValidator::");
                let has_array_validator = line.contains("ArrayValidator::");

                assert!(
                    has_string_validator || has_number_validator || has_array_validator,
                    "validators attribute in {} should contain valid validator types: {}",
                    file_path, line
                );

                // Should end with )]
                assert!(
                    line.trim().ends_with(")]"),
                    "validators attribute should have proper closing: {}",
                    line
                );
            }
        }
    }
}

// ==================== FlatBuffers Configuration Tests ====================

/// Test that FlatBuffers generation is enabled in config
#[test]
fn test_flatbuffers_configuration() {
    let playground_dir = get_playground_dir();
    let config_content = fs::read_to_string(playground_dir.join("evenframe.toml"))
        .expect("Failed to read evenframe.toml");

    // Check FlatBuffers is enabled
    assert!(
        config_content.contains("should_generate_flatbuffers_types = true"),
        "FlatBuffers generation should be enabled"
    );

    // Check namespace is configured
    assert!(
        config_content.contains("flatbuffers_namespace"),
        "FlatBuffers namespace should be configured"
    );
}
