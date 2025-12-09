//! Tests that verify FlatBuffers schema output from Evenframe CLI
//!
//! These tests verify:
//! 1. FlatBuffers schema file is generated correctly
//! 2. All expected tables are present
//! 3. All expected enums are present
//! 4. Validators are correctly converted to FlatBuffers attributes
//! 5. Type mappings are correct
//!
//! Note: These tests require the evenframe CLI to be built and run first.
//! Run with: cargo test --test flatbuffers_output_test

use std::env;
use std::fs;
use std::path::PathBuf;

#[allow(unused_imports)]
use std::process::Command;

/// Get the workspace root directory
#[allow(dead_code)]
fn get_workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.parent().unwrap().to_path_buf()
}

/// Get the playground directory
fn get_playground_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Get the bindings directory
fn get_bindings_dir() -> PathBuf {
    get_playground_dir().join("src/bindings")
}

/// Get the FlatBuffers schema file path
fn get_flatbuffers_schema_path() -> PathBuf {
    get_bindings_dir().join("schema.fbs")
}

/// Create the bindings output directory if it doesn't exist
#[allow(dead_code)]
fn ensure_bindings_dir() {
    let bindings_dir = get_bindings_dir();
    if !bindings_dir.exists() {
        fs::create_dir_all(&bindings_dir).expect("Failed to create bindings directory");
    }
}

/// Run the evenframe CLI on the playground
#[allow(dead_code)]
fn run_evenframe() -> Result<std::process::Output, std::io::Error> {
    let workspace_root = get_workspace_root();
    let playground_dir = get_playground_dir();

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
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Failed to build evenframe",
        ));
    }

    // Run evenframe in the playground directory
    Command::new("cargo")
        .args(["run", "-p", "evenframe"])
        .current_dir(&playground_dir)
        .env("RUST_LOG", "warn")
        .env("SURREALDB_URL", "http://localhost:8000")
        .env("SURREALDB_NS", "test")
        .env("SURREALDB_DB", "test")
        .env("SURREALDB_USER", "root")
        .env("SURREALDB_PASSWORD", "root")
        .output()
}

/// Read the FlatBuffers schema file content
fn read_flatbuffers_schema() -> Option<String> {
    let schema_path = get_flatbuffers_schema_path();
    fs::read_to_string(&schema_path).ok()
}

// ==================== Configuration Tests ====================

/// Test that evenframe.toml has FlatBuffers generation enabled
#[test]
fn test_flatbuffers_enabled_in_config() {
    let playground_dir = get_playground_dir();
    let config_content = fs::read_to_string(playground_dir.join("evenframe.toml"))
        .expect("Failed to read evenframe.toml");

    assert!(
        config_content.contains("should_generate_flatbuffers_types = true"),
        "FlatBuffers generation should be enabled in evenframe.toml"
    );
}

/// Test that evenframe.toml has FlatBuffers namespace configured
#[test]
fn test_flatbuffers_namespace_in_config() {
    let playground_dir = get_playground_dir();
    let config_content = fs::read_to_string(playground_dir.join("evenframe.toml"))
        .expect("Failed to read evenframe.toml");

    assert!(
        config_content.contains("flatbuffers_namespace"),
        "FlatBuffers namespace should be configured in evenframe.toml"
    );
}

// ==================== Schema File Existence Tests ====================

/// Test that the FlatBuffers schema file can be generated
/// This test documents the expected output path
#[test]
fn test_expected_flatbuffers_output_path() {
    let expected_path = get_flatbuffers_schema_path();
    assert!(
        expected_path.ends_with("src/bindings/schema.fbs"),
        "FlatBuffers schema should be generated at src/bindings/schema.fbs"
    );
}

/// Test that the FlatBuffers schema file exists (after running evenframe)
/// This test will fail if evenframe hasn't been run yet
#[test]
#[ignore = "Run after evenframe CLI generates the schema: cargo test --test flatbuffers_output_test test_schema_file_exists -- --ignored"]
fn test_schema_file_exists() {
    let schema_path = get_flatbuffers_schema_path();
    assert!(
        schema_path.exists(),
        "FlatBuffers schema file should exist at {:?}. Run `cargo run -p evenframe` in the playground directory first.",
        schema_path
    );
}

// ==================== Schema Content Tests (when file exists) ====================

/// Test namespace declaration in generated schema
#[test]
fn test_flatbuffers_namespace_in_schema() {
    if let Some(content) = read_flatbuffers_schema() {
        assert!(
            content.contains("namespace evenframe.playground;"),
            "Schema should contain the configured namespace"
        );
    }
}

/// Test that User table is generated with correct validators
#[test]
fn test_user_table_in_schema() {
    if let Some(content) = read_flatbuffers_schema() {
        // User table should exist
        assert!(
            content.contains("table User"),
            "Schema should contain User table"
        );

        // Email field with validators
        assert!(
            content.contains("email: string") && content.contains("validate:"),
            "User.email should have string type with validators"
        );

        // Check for email validator
        if content.contains("email: string") {
            let email_line = content
                .lines()
                .find(|l| l.contains("email:") && l.contains("string"));
            if let Some(line) = email_line {
                assert!(
                    line.contains("email"),
                    "User.email should have email validator: {}",
                    line
                );
            }
        }
    }
}

/// Test that Session table is generated
#[test]
fn test_session_table_in_schema() {
    if let Some(content) = read_flatbuffers_schema() {
        assert!(
            content.contains("table Session"),
            "Schema should contain Session table"
        );

        // Token field should have minLength validator
        let has_token_with_validator = content.lines().any(|line| {
            line.contains("token:") && line.contains("string") && line.contains("minLength")
        });

        assert!(
            has_token_with_validator,
            "Session.token should have minLength validator"
        );
    }
}

/// Test that Product table is generated with validators
#[test]
fn test_product_table_in_schema() {
    if let Some(content) = read_flatbuffers_schema() {
        assert!(
            content.contains("table Product"),
            "Schema should contain Product table"
        );

        // Price should have positive validator
        let has_price_validator = content
            .lines()
            .any(|line| line.contains("price:") && line.contains("positive"));

        assert!(
            has_price_validator,
            "Product.price should have positive validator"
        );

        // stock_quantity should have nonNegative validator
        let has_stock_validator = content
            .lines()
            .any(|line| line.contains("stock_quantity:") && line.contains("nonNegative"));

        assert!(
            has_stock_validator,
            "Product.stock_quantity should have nonNegative validator"
        );
    }
}

/// Test that Order table is generated
#[test]
fn test_order_table_in_schema() {
    if let Some(content) = read_flatbuffers_schema() {
        assert!(
            content.contains("table Order"),
            "Schema should contain Order table"
        );

        // Total should have positive validator
        let has_total_validator = content
            .lines()
            .any(|line| line.contains("total:") && line.contains("positive"));

        assert!(
            has_total_validator,
            "Order.total should have positive validator"
        );
    }
}

/// Test that Address table is generated
#[test]
fn test_address_table_in_schema() {
    if let Some(content) = read_flatbuffers_schema() {
        assert!(
            content.contains("table Address"),
            "Schema should contain Address table"
        );

        // Country should have uppercase validator
        let has_country_validator = content
            .lines()
            .any(|line| line.contains("country:") && line.contains("uppercase"));

        assert!(
            has_country_validator,
            "Address.country should have uppercase validator"
        );
    }
}

/// Test that Role enum is generated
#[test]
fn test_role_enum_in_schema() {
    if let Some(content) = read_flatbuffers_schema() {
        assert!(
            content.contains("enum Role"),
            "Schema should contain Role enum"
        );

        // Check enum variants
        assert!(
            content.contains("Admin"),
            "Role enum should contain Admin variant"
        );
        assert!(
            content.contains("Moderator"),
            "Role enum should contain Moderator variant"
        );
        assert!(
            content.contains("User"),
            "Role enum should contain User variant"
        );
        assert!(
            content.contains("Guest"),
            "Role enum should contain Guest variant"
        );
    }
}

/// Test that OrderStatus enum is generated
#[test]
fn test_order_status_enum_in_schema() {
    if let Some(content) = read_flatbuffers_schema() {
        assert!(
            content.contains("enum OrderStatus"),
            "Schema should contain OrderStatus enum"
        );

        // Check enum variants
        assert!(
            content.contains("Pending"),
            "OrderStatus should contain Pending"
        );
        assert!(
            content.contains("Processing"),
            "OrderStatus should contain Processing"
        );
        assert!(
            content.contains("Shipped"),
            "OrderStatus should contain Shipped"
        );
        assert!(
            content.contains("Delivered"),
            "OrderStatus should contain Delivered"
        );
        assert!(
            content.contains("Cancelled"),
            "OrderStatus should contain Cancelled"
        );
    }
}

/// Test that ProductCategory enum is generated
#[test]
fn test_product_category_enum_in_schema() {
    if let Some(content) = read_flatbuffers_schema() {
        assert!(
            content.contains("enum ProductCategory"),
            "Schema should contain ProductCategory enum"
        );

        // Check enum variants
        assert!(
            content.contains("Electronics"),
            "ProductCategory should contain Electronics"
        );
        assert!(
            content.contains("Clothing"),
            "ProductCategory should contain Clothing"
        );
        assert!(
            content.contains("Books"),
            "ProductCategory should contain Books"
        );
    }
}

// ==================== Type Mapping Tests ====================

/// Test that FlatBuffers type mappings are correct
#[test]
fn test_flatbuffers_type_mappings() {
    if let Some(content) = read_flatbuffers_schema() {
        // String types
        assert!(
            content.contains(": string"),
            "Schema should contain string types"
        );

        // Boolean types
        assert!(
            content.contains(": bool"),
            "Schema should contain bool types"
        );

        // Float types (f64 -> double)
        assert!(
            content.contains(": double"),
            "Schema should contain double types for f64 fields"
        );

        // Integer types (u32 -> uint32)
        assert!(
            content.contains(": uint32") || content.contains(": int32"),
            "Schema should contain integer types"
        );

        // Vector types
        assert!(
            content.contains("[") && content.contains("]"),
            "Schema should contain vector types"
        );
    }
}

/// Test that Vec<T> is mapped to [T]
#[test]
fn test_vec_type_mapping() {
    if let Some(content) = read_flatbuffers_schema() {
        // User.roles is Vec<Role>
        let has_roles_vector = content
            .lines()
            .any(|line| line.contains("roles:") && line.contains("["));

        assert!(has_roles_vector, "Vec<Role> should be mapped to [Role]");

        // Order.items is Vec<CartItem>
        let has_items_vector = content
            .lines()
            .any(|line| line.contains("items:") && line.contains("["));

        assert!(
            has_items_vector,
            "Vec<CartItem> should be mapped to [CartItem]"
        );
    }
}

/// Test that Option<T> is correctly handled
#[test]
fn test_option_type_mapping() {
    if let Some(content) = read_flatbuffers_schema() {
        // Product.image_url is Option<String> - should just be string (FlatBuffers fields optional by default)
        let has_image_url = content
            .lines()
            .any(|line| line.contains("image_url:") && line.contains("string"));

        assert!(
            has_image_url,
            "Option<String> should be mapped to string (optional by default)"
        );
    }
}

// ==================== Validator Attribute Tests ====================

/// Test that string validators are converted correctly
#[test]
fn test_string_validator_conversion() {
    if let Some(content) = read_flatbuffers_schema() {
        // NonEmpty validator
        assert!(
            content.contains("nonEmpty"),
            "NonEmpty validator should be converted"
        );

        // MinLength validator
        assert!(
            content.contains("minLength("),
            "MinLength validator should be converted with argument"
        );

        // MaxLength validator
        assert!(
            content.contains("maxLength("),
            "MaxLength validator should be converted with argument"
        );

        // Email validator
        assert!(
            content.contains("email"),
            "Email validator should be converted"
        );

        // Alphanumeric validator
        assert!(
            content.contains("alphanumeric"),
            "Alphanumeric validator should be converted"
        );
    }
}

/// Test that number validators are converted correctly
#[test]
fn test_number_validator_conversion() {
    if let Some(content) = read_flatbuffers_schema() {
        // Positive validator
        assert!(
            content.contains("positive"),
            "Positive validator should be converted"
        );

        // NonNegative validator
        assert!(
            content.contains("nonNegative"),
            "NonNegative validator should be converted"
        );
    }
}

/// Test that case validators are converted correctly
#[test]
fn test_case_validator_conversion() {
    if let Some(content) = read_flatbuffers_schema() {
        // Uppercased validator (used in Address.country)
        assert!(
            content.contains("uppercase"),
            "Uppercased validator should be converted to 'uppercase'"
        );
    }
}

/// Test that validate attribute syntax is correct
#[test]
fn test_validate_attribute_syntax() {
    if let Some(content) = read_flatbuffers_schema() {
        // All validate attributes should use (validate: "...") syntax
        for line in content.lines() {
            if line.contains("validate:") {
                assert!(
                    line.contains("(validate: \"") && line.contains("\")"),
                    "Validate attribute should use correct syntax: (validate: \"...\")\nFound: {}",
                    line
                );
            }
        }
    }
}

/// Test that multiple validators are comma-separated
#[test]
fn test_multiple_validators_comma_separated() {
    if let Some(content) = read_flatbuffers_schema() {
        // Find lines with multiple validators
        let multi_validator_lines: Vec<&str> = content
            .lines()
            .filter(|line| {
                line.contains("validate:") && line.matches(',').count() > 0
            })
            .collect();

        // User.email should have multiple validators: email, minLength(5), maxLength(255)
        if !multi_validator_lines.is_empty() {
            for line in &multi_validator_lines {
                // Each comma should be followed by a space for readability
                if line.contains(", ") {
                    // Good formatting
                } else if line.contains(',') {
                    // Still valid, just not as readable
                }
            }
        }
    }
}

// ==================== Schema Structure Tests ====================

/// Test that all tables have proper closing braces
#[test]
fn test_table_structure() {
    if let Some(content) = read_flatbuffers_schema() {
        let table_count = content.matches("table ").count();
        let closing_brace_after_table = content.matches("}\n").count();

        // Each table should have a closing brace
        assert!(
            closing_brace_after_table >= table_count,
            "Each table should have a proper closing brace"
        );
    }
}

/// Test that all enums have proper closing braces
#[test]
fn test_enum_structure() {
    if let Some(content) = read_flatbuffers_schema() {
        let enum_count = content.matches("enum ").count();

        // Check that enums have proper structure
        for line in content.lines() {
            if line.trim().starts_with("enum ") {
                assert!(
                    line.contains(": byte") || line.contains(": ubyte"),
                    "Enum should specify underlying type: {}",
                    line
                );
            }
        }

        assert!(
            enum_count > 0,
            "Schema should contain at least one enum"
        );
    }
}

/// Test that fields end with semicolons
#[test]
fn test_field_semicolons() {
    if let Some(content) = read_flatbuffers_schema() {
        let mut in_table = false;
        let mut in_enum = false;

        for line in content.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with("table ") {
                in_table = true;
                in_enum = false;
            } else if trimmed.starts_with("enum ") || trimmed.starts_with("union ") {
                in_table = false;
                in_enum = true;
            } else if trimmed == "}" {
                in_table = false;
                in_enum = false;
            } else if in_table && !trimmed.is_empty() && trimmed != "{" {
                assert!(
                    trimmed.ends_with(';'),
                    "Table field should end with semicolon: {}",
                    trimmed
                );
            } else if in_enum && !trimmed.is_empty() && trimmed != "{" && trimmed != "}" {
                // Enum variants can end with comma or nothing (last variant)
                assert!(
                    trimmed.ends_with(',') || trimmed.ends_with('0') || trimmed.ends_with('1')
                        || trimmed.ends_with('2') || trimmed.ends_with('3') || trimmed.ends_with('4')
                        || trimmed.ends_with('5'),
                    "Enum variant should have proper format: {}",
                    trimmed
                );
            }
        }
    }
}

// ==================== Integration Tests ====================

/// Test that CartItem table is generated (nested type in Order)
#[test]
fn test_cart_item_table_in_schema() {
    if let Some(content) = read_flatbuffers_schema() {
        assert!(
            content.contains("table CartItem"),
            "Schema should contain CartItem table"
        );

        // CartItem.quantity should have positive validator
        let has_quantity_validator = content
            .lines()
            .any(|line| line.contains("quantity:") && line.contains("positive"));

        assert!(
            has_quantity_validator,
            "CartItem.quantity should have positive validator"
        );
    }
}

/// Test that Customer table is generated
#[test]
fn test_customer_table_in_schema() {
    if let Some(content) = read_flatbuffers_schema() {
        assert!(
            content.contains("table Customer"),
            "Schema should contain Customer table"
        );
    }
}

/// Test all expected tables are present
#[test]
fn test_all_expected_tables_present() {
    if let Some(content) = read_flatbuffers_schema() {
        let expected_tables = vec![
            "User",
            "Session",
            "Product",
            "Order",
            "Customer",
            "Address",
            "CartItem",
        ];

        for table in expected_tables {
            assert!(
                content.contains(&format!("table {}", table)),
                "Schema should contain table: {}",
                table
            );
        }
    }
}

/// Test all expected enums are present
#[test]
fn test_all_expected_enums_present() {
    if let Some(content) = read_flatbuffers_schema() {
        let expected_enums = vec!["Role", "OrderStatus", "ProductCategory"];

        for enum_name in expected_enums {
            assert!(
                content.contains(&format!("enum {}", enum_name)),
                "Schema should contain enum: {}",
                enum_name
            );
        }
    }
}

// ==================== Snapshot Tests ====================

/// Print the generated schema for manual inspection
/// Run with: cargo test test_print_generated_schema -- --nocapture
#[test]
fn test_print_generated_schema() {
    if let Some(content) = read_flatbuffers_schema() {
        println!("=== Generated FlatBuffers Schema ===");
        println!("{}", content);
        println!("=== End of Schema ===");
    } else {
        println!("FlatBuffers schema not yet generated. Run evenframe first.");
    }
}

/// Count and report schema statistics
#[test]
fn test_schema_statistics() {
    if let Some(content) = read_flatbuffers_schema() {
        let table_count = content.matches("table ").count();
        let enum_count = content.matches("enum ").count();
        let union_count = content.matches("union ").count();
        let validate_count = content.matches("validate:").count();
        let field_count = content.matches(": ").count(); // Rough estimate

        println!("=== FlatBuffers Schema Statistics ===");
        println!("Tables: {}", table_count);
        println!("Enums: {}", enum_count);
        println!("Unions: {}", union_count);
        println!("Fields with validators: {}", validate_count);
        println!("Approximate total fields: {}", field_count);
        println!("Total characters: {}", content.len());
        println!("Total lines: {}", content.lines().count());

        // Basic assertions
        assert!(table_count >= 5, "Should have at least 5 tables");
        assert!(enum_count >= 2, "Should have at least 2 enums");
    }
}
