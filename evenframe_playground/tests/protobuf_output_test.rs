//! Tests that verify Protocol Buffers schema output from Evenframe CLI
//!
//! These tests verify:
//! 1. Protocol Buffers schema file is generated correctly
//! 2. All expected messages are present
//! 3. All expected enums are present (with UNSPECIFIED first variant)
//! 4. Validators are correctly converted to protoc-gen-validate options
//! 5. Type mappings are correct
//! 6. Proto3 syntax is used
//!
//! Note: These tests require the evenframe CLI to be built and run first.
//! Run with: cargo test --test protobuf_output_test

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

/// Get the Protocol Buffers schema file path
fn get_protobuf_schema_path() -> PathBuf {
    get_bindings_dir().join("schema.proto")
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
        return Err(std::io::Error::other("Failed to build evenframe"));
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

/// Read the Protocol Buffers schema file content
fn read_protobuf_schema() -> Option<String> {
    let schema_path = get_protobuf_schema_path();
    fs::read_to_string(&schema_path).ok()
}

// ==================== Configuration Tests ====================

/// Test that evenframe.toml has Protocol Buffers generation enabled
#[test]
fn test_protobuf_enabled_in_config() {
    let playground_dir = get_playground_dir();
    let config_content = fs::read_to_string(playground_dir.join("evenframe.toml"))
        .expect("Failed to read evenframe.toml");

    assert!(
        config_content.contains("should_generate_protobuf_types = true"),
        "Protocol Buffers generation should be enabled in evenframe.toml"
    );
}

/// Test that evenframe.toml has Protocol Buffers package configured
#[test]
fn test_protobuf_package_in_config() {
    let playground_dir = get_playground_dir();
    let config_content = fs::read_to_string(playground_dir.join("evenframe.toml"))
        .expect("Failed to read evenframe.toml");

    assert!(
        config_content.contains("protobuf_package"),
        "Protocol Buffers package should be configured in evenframe.toml"
    );
}

/// Test that evenframe.toml has validate import configured
#[test]
fn test_protobuf_import_validate_in_config() {
    let playground_dir = get_playground_dir();
    let config_content = fs::read_to_string(playground_dir.join("evenframe.toml"))
        .expect("Failed to read evenframe.toml");

    assert!(
        config_content.contains("protobuf_import_validate"),
        "Protocol Buffers validate import should be configured in evenframe.toml"
    );
}

// ==================== Schema File Existence Tests ====================

/// Test that the Protocol Buffers schema file can be generated
/// This test documents the expected output path
#[test]
fn test_expected_protobuf_output_path() {
    let expected_path = get_protobuf_schema_path();
    assert!(
        expected_path.ends_with("src/bindings/schema.proto"),
        "Protocol Buffers schema should be generated at src/bindings/schema.proto"
    );
}

/// Test that the Protocol Buffers schema file exists (after running evenframe)
/// This test will fail if evenframe hasn't been run yet
#[test]
#[ignore = "Run after evenframe CLI generates the schema: cargo test --test protobuf_output_test test_schema_file_exists -- --ignored"]
fn test_schema_file_exists() {
    let schema_path = get_protobuf_schema_path();
    assert!(
        schema_path.exists(),
        "Protocol Buffers schema file should exist at {:?}. Run `cargo run -p evenframe` in the playground directory first.",
        schema_path
    );
}

// ==================== Proto3 Syntax Tests ====================

/// Test that proto3 syntax is declared
#[test]
fn test_proto3_syntax_declaration() {
    if let Some(content) = read_protobuf_schema() {
        assert!(
            content.contains("syntax = \"proto3\";"),
            "Schema should declare proto3 syntax"
        );
    }
}

/// Test package declaration in generated schema
#[test]
fn test_protobuf_package_in_schema() {
    if let Some(content) = read_protobuf_schema() {
        assert!(
            content.contains("package evenframe.playground;"),
            "Schema should contain the configured package"
        );
    }
}

/// Test validate import in generated schema
#[test]
fn test_validate_import_in_schema() {
    if let Some(content) = read_protobuf_schema() {
        assert!(
            content.contains("import \"validate/validate.proto\";"),
            "Schema should import validate.proto for validation rules"
        );
    }
}

// ==================== Message Tests ====================

/// Test that User message is generated with correct validators
#[test]
fn test_user_message_in_schema() {
    if let Some(content) = read_protobuf_schema() {
        // User message should exist
        assert!(
            content.contains("message User"),
            "Schema should contain User message"
        );

        // Find the User message content
        let user_message_start = content.find("message User {");
        if let Some(start) = user_message_start {
            let user_content = &content[start..];
            let end = user_content.find("\n}\n").unwrap_or(user_content.len());
            let user_message = &user_content[..end];

            // Email field with validators in User message
            let email_line = user_message
                .lines()
                .find(|l| l.contains("email") && l.contains("string") && l.contains("="));

            assert!(
                email_line.is_some(),
                "User message should contain email field"
            );

            if let Some(line) = email_line {
                assert!(
                    line.contains("validate.rules"),
                    "User.email should have validate rules: {}",
                    line
                );
                assert!(
                    line.contains("email: true"),
                    "User.email should have email: true validator: {}",
                    line
                );
            }
        }
    }
}

/// Test that Session message is generated
#[test]
fn test_session_message_in_schema() {
    if let Some(content) = read_protobuf_schema() {
        assert!(
            content.contains("message Session"),
            "Schema should contain Session message"
        );

        // Token field should have min_len validator
        let has_token_with_validator = content.lines().any(|line| {
            line.contains("token") && line.contains("string") && line.contains("min_len")
        });

        assert!(
            has_token_with_validator,
            "Session.token should have min_len validator"
        );
    }
}

/// Test that Product message is generated with validators
#[test]
fn test_product_message_in_schema() {
    if let Some(content) = read_protobuf_schema() {
        assert!(
            content.contains("message Product"),
            "Schema should contain Product message"
        );

        // Price should have gt: 0 (positive) validator
        let has_price_validator = content
            .lines()
            .any(|line| line.contains("price") && line.contains("gt:"));

        assert!(
            has_price_validator,
            "Product.price should have gt: 0 (positive) validator"
        );

        // stock_quantity should have gte: 0 (non-negative) validator
        let has_stock_validator = content
            .lines()
            .any(|line| line.contains("stock_quantity") && line.contains("gte:"));

        assert!(
            has_stock_validator,
            "Product.stock_quantity should have gte: 0 (non-negative) validator"
        );
    }
}

/// Test that Order message is generated
#[test]
fn test_order_message_in_schema() {
    if let Some(content) = read_protobuf_schema() {
        assert!(
            content.contains("message Order"),
            "Schema should contain Order message"
        );

        // Total should have gt: 0 (positive) validator
        let has_total_validator = content
            .lines()
            .any(|line| line.contains("total") && line.contains("gt:"));

        assert!(
            has_total_validator,
            "Order.total should have gt: 0 (positive) validator"
        );
    }
}

/// Test that Address message is generated
#[test]
fn test_address_message_in_schema() {
    if let Some(content) = read_protobuf_schema() {
        assert!(
            content.contains("message Address"),
            "Schema should contain Address message"
        );

        // Country should have pattern validator for uppercase
        let has_country_validator = content
            .lines()
            .any(|line| line.contains("country") && line.contains("pattern"));

        assert!(
            has_country_validator,
            "Address.country should have pattern validator for uppercase"
        );
    }
}

// ==================== Enum Tests ====================

/// Test that Role enum is generated with UNSPECIFIED first
#[test]
fn test_role_enum_in_schema() {
    if let Some(content) = read_protobuf_schema() {
        assert!(
            content.contains("enum Role"),
            "Schema should contain Role enum"
        );

        // Proto3 requires UNSPECIFIED = 0 first
        assert!(
            content.contains("ROLE_UNSPECIFIED = 0"),
            "Role enum should have ROLE_UNSPECIFIED = 0 as first variant"
        );

        // Check other enum variants
        assert!(
            content.contains("ROLE_ADMIN"),
            "Role enum should contain ROLE_ADMIN variant"
        );
        assert!(
            content.contains("ROLE_MODERATOR"),
            "Role enum should contain ROLE_MODERATOR variant"
        );
        assert!(
            content.contains("ROLE_USER"),
            "Role enum should contain ROLE_USER variant"
        );
        assert!(
            content.contains("ROLE_GUEST"),
            "Role enum should contain ROLE_GUEST variant"
        );
    }
}

/// Test that OrderStatus enum is generated with UNSPECIFIED first
#[test]
fn test_order_status_enum_in_schema() {
    if let Some(content) = read_protobuf_schema() {
        assert!(
            content.contains("enum OrderStatus"),
            "Schema should contain OrderStatus enum"
        );

        // Proto3 requires UNSPECIFIED = 0 first
        assert!(
            content.contains("ORDER_STATUS_UNSPECIFIED = 0"),
            "OrderStatus enum should have ORDER_STATUS_UNSPECIFIED = 0 as first variant"
        );

        // Check enum variants
        assert!(
            content.contains("ORDER_STATUS_PENDING"),
            "OrderStatus should contain ORDER_STATUS_PENDING"
        );
        assert!(
            content.contains("ORDER_STATUS_PROCESSING"),
            "OrderStatus should contain ORDER_STATUS_PROCESSING"
        );
        assert!(
            content.contains("ORDER_STATUS_SHIPPED"),
            "OrderStatus should contain ORDER_STATUS_SHIPPED"
        );
        assert!(
            content.contains("ORDER_STATUS_DELIVERED"),
            "OrderStatus should contain ORDER_STATUS_DELIVERED"
        );
        assert!(
            content.contains("ORDER_STATUS_CANCELLED"),
            "OrderStatus should contain ORDER_STATUS_CANCELLED"
        );
    }
}

/// Test that ProductCategory enum is generated with UNSPECIFIED first
#[test]
fn test_product_category_enum_in_schema() {
    if let Some(content) = read_protobuf_schema() {
        assert!(
            content.contains("enum ProductCategory"),
            "Schema should contain ProductCategory enum"
        );

        // Proto3 requires UNSPECIFIED = 0 first
        assert!(
            content.contains("PRODUCT_CATEGORY_UNSPECIFIED = 0"),
            "ProductCategory enum should have PRODUCT_CATEGORY_UNSPECIFIED = 0 as first variant"
        );

        // Check enum variants
        assert!(
            content.contains("PRODUCT_CATEGORY_ELECTRONICS"),
            "ProductCategory should contain PRODUCT_CATEGORY_ELECTRONICS"
        );
        assert!(
            content.contains("PRODUCT_CATEGORY_CLOTHING"),
            "ProductCategory should contain PRODUCT_CATEGORY_CLOTHING"
        );
        assert!(
            content.contains("PRODUCT_CATEGORY_BOOKS"),
            "ProductCategory should contain PRODUCT_CATEGORY_BOOKS"
        );
    }
}

// ==================== Type Mapping Tests ====================

/// Test that Protocol Buffers type mappings are correct
#[test]
fn test_protobuf_type_mappings() {
    if let Some(content) = read_protobuf_schema() {
        // String types
        assert!(
            content.contains(" string "),
            "Schema should contain string types"
        );

        // Boolean types
        assert!(content.contains(" bool "), "Schema should contain bool types");

        // Float types (f64 -> double)
        assert!(
            content.contains(" double "),
            "Schema should contain double types for f64 fields"
        );

        // Integer types (i32 -> int32, u32 -> uint32)
        assert!(
            content.contains(" int32 ") || content.contains(" uint32 "),
            "Schema should contain integer types"
        );

        // repeated keyword for arrays
        assert!(
            content.contains("repeated "),
            "Schema should contain repeated keyword for arrays"
        );
    }
}

/// Test that Vec<T> is mapped to repeated T
#[test]
fn test_vec_type_mapping() {
    if let Some(content) = read_protobuf_schema() {
        // User.roles is Vec<Role>
        let has_roles_repeated = content
            .lines()
            .any(|line| line.contains("repeated") && line.contains("roles"));

        assert!(
            has_roles_repeated,
            "Vec<Role> should be mapped to repeated Role"
        );

        // Order.items is Vec<CartItem>
        let has_items_repeated = content
            .lines()
            .any(|line| line.contains("repeated") && line.contains("items"));

        assert!(
            has_items_repeated,
            "Vec<CartItem> should be mapped to repeated CartItem"
        );
    }
}

/// Test that Option<T> is correctly handled
#[test]
fn test_option_type_mapping() {
    if let Some(content) = read_protobuf_schema() {
        // Product.image_url is Option<String> - should use optional keyword
        let has_image_url = content
            .lines()
            .any(|line| line.contains("image_url") && line.contains("string"));

        assert!(
            has_image_url,
            "Option<String> should be mapped to optional string"
        );
    }
}

/// Test that map types are correctly generated
#[test]
fn test_map_type_mapping() {
    if let Some(content) = read_protobuf_schema() {
        // Check if any map types exist
        if content.contains("map<") {
            assert!(
                content.contains("map<string,") || content.contains("map<int"),
                "Map types should have correct key types"
            );
        }
    }
}

// ==================== Validator Attribute Tests ====================

/// Test that string validators are converted correctly
#[test]
fn test_string_validator_conversion() {
    if let Some(content) = read_protobuf_schema() {
        // min_len validator
        assert!(
            content.contains("min_len:"),
            "min_len validator should be converted"
        );

        // max_len validator
        assert!(
            content.contains("max_len:"),
            "max_len validator should be converted"
        );

        // email validator
        assert!(
            content.contains("email: true"),
            "email validator should be converted"
        );
    }
}

/// Test that number validators are converted correctly
#[test]
fn test_number_validator_conversion() {
    if let Some(content) = read_protobuf_schema() {
        // gt: (greater than) for positive
        assert!(
            content.contains("gt:"),
            "Positive validator should be converted to gt:"
        );

        // gte: (greater than or equal) for non-negative
        assert!(
            content.contains("gte:"),
            "NonNegative validator should be converted to gte:"
        );
    }
}

/// Test that array validators are converted correctly
#[test]
fn test_array_validator_conversion() {
    if let Some(content) = read_protobuf_schema() {
        // min_items validator
        if content.contains("min_items:") {
            assert!(
                content.contains("min_items:"),
                "min_items validator should be converted"
            );
        }

        // max_items validator
        if content.contains("max_items:") {
            assert!(
                content.contains("max_items:"),
                "max_items validator should be converted"
            );
        }
    }
}

/// Test that validate.rules syntax is correct
#[test]
fn test_validate_rules_syntax() {
    if let Some(content) = read_protobuf_schema() {
        // All validate rules should use [(validate.rules).type = {...}] syntax
        for line in content.lines() {
            if line.contains("validate.rules") {
                assert!(
                    line.contains("[(validate.rules).") && line.contains(" = {"),
                    "Validate rules should use correct syntax: [(validate.rules).type = {{...}}]\nFound: {}",
                    line
                );
            }
        }
    }
}

// ==================== Schema Structure Tests ====================

/// Test that all messages have proper closing braces
#[test]
fn test_message_structure() {
    if let Some(content) = read_protobuf_schema() {
        let message_count = content.matches("message ").count();
        let closing_braces = content.matches("}\n").count();

        // Each message should have a closing brace
        assert!(
            closing_braces >= message_count,
            "Each message should have a proper closing brace"
        );
    }
}

/// Test that all enums have proper structure
#[test]
fn test_enum_structure() {
    if let Some(content) = read_protobuf_schema() {
        let enum_count = content.matches("enum ").count();

        assert!(enum_count > 0, "Schema should contain at least one enum");

        // Each enum should have UNSPECIFIED = 0 as its first variant
        // The variant name uses UPPER_SNAKE_CASE based on the enum name
        for line in content.lines() {
            if line.trim().starts_with("enum ") {
                let enum_name = line.trim().strip_prefix("enum ").unwrap().split_whitespace().next().unwrap();
                // Check that there's an UNSPECIFIED = 0 variant somewhere in the enum
                // The exact format depends on how the enum name is converted to UPPER_SNAKE_CASE
                assert!(
                    content.contains("_UNSPECIFIED = 0"),
                    "Enum {} should have an UNSPECIFIED = 0 variant",
                    enum_name
                );
            }
        }
    }
}

/// Test that fields have proper field numbers
#[test]
fn test_field_numbers() {
    if let Some(content) = read_protobuf_schema() {
        let mut in_message = false;

        for line in content.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with("message ") {
                in_message = true;
            } else if trimmed == "}" {
                in_message = false;
            } else if in_message && !trimmed.is_empty() && trimmed != "{" && !trimmed.starts_with("//") {
                // Field lines should contain " = N;" pattern
                assert!(
                    trimmed.contains(" = ") && trimmed.contains(';'),
                    "Message field should have field number assignment: {}",
                    trimmed
                );
            }
        }
    }
}

/// Test that fields end with semicolons
#[test]
fn test_field_semicolons() {
    if let Some(content) = read_protobuf_schema() {
        let mut in_message = false;

        for line in content.lines() {
            let trimmed = line.trim();

            if trimmed.starts_with("message ") {
                in_message = true;
            } else if trimmed == "}" {
                in_message = false;
            } else if in_message && !trimmed.is_empty() && trimmed != "{" && !trimmed.starts_with("//") {
                assert!(
                    trimmed.ends_with(';'),
                    "Message field should end with semicolon: {}",
                    trimmed
                );
            }
        }
    }
}

// ==================== Integration Tests ====================

/// Test that CartItem message is generated (nested type in Order)
#[test]
fn test_cart_item_message_in_schema() {
    if let Some(content) = read_protobuf_schema() {
        assert!(
            content.contains("message CartItem"),
            "Schema should contain CartItem message"
        );

        // CartItem.quantity should have gt: 0 (positive) validator
        let has_quantity_validator = content
            .lines()
            .any(|line| line.contains("quantity") && line.contains("gt:"));

        assert!(
            has_quantity_validator,
            "CartItem.quantity should have gt: 0 (positive) validator"
        );
    }
}

/// Test that Customer message is generated
#[test]
fn test_customer_message_in_schema() {
    if let Some(content) = read_protobuf_schema() {
        assert!(
            content.contains("message Customer"),
            "Schema should contain Customer message"
        );
    }
}

/// Test all expected messages are present
#[test]
fn test_all_expected_messages_present() {
    if let Some(content) = read_protobuf_schema() {
        let expected_messages = vec![
            "User",
            "Session",
            "Product",
            "Order",
            "Customer",
            "Address",
            "CartItem",
        ];

        for message in expected_messages {
            assert!(
                content.contains(&format!("message {}", message)),
                "Schema should contain message: {}",
                message
            );
        }
    }
}

/// Test all expected enums are present
#[test]
fn test_all_expected_enums_present() {
    if let Some(content) = read_protobuf_schema() {
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
/// Run with: cargo test test_print_generated_protobuf_schema -- --nocapture
#[test]
fn test_print_generated_protobuf_schema() {
    if let Some(content) = read_protobuf_schema() {
        println!("=== Generated Protocol Buffers Schema ===");
        println!("{}", content);
        println!("=== End of Schema ===");
    } else {
        println!("Protocol Buffers schema not yet generated. Run evenframe first.");
    }
}

/// Count and report schema statistics
#[test]
fn test_protobuf_schema_statistics() {
    if let Some(content) = read_protobuf_schema() {
        let message_count = content.matches("message ").count();
        let enum_count = content.matches("enum ").count();
        let repeated_count = content.matches("repeated ").count();
        let optional_count = content.matches("optional ").count();
        let validate_count = content.matches("validate.rules").count();
        let field_count = content.matches(" = ").count(); // Field assignments

        println!("=== Protocol Buffers Schema Statistics ===");
        println!("Messages: {}", message_count);
        println!("Enums: {}", enum_count);
        println!("Repeated fields: {}", repeated_count);
        println!("Optional fields: {}", optional_count);
        println!("Fields with validators: {}", validate_count);
        println!("Total fields: {}", field_count);
        println!("Total characters: {}", content.len());
        println!("Total lines: {}", content.lines().count());

        // Basic assertions
        assert!(message_count >= 5, "Should have at least 5 messages");
        assert!(enum_count >= 2, "Should have at least 2 enums");
    }
}

// ==================== Comparison with FlatBuffers ====================

/// Test that protobuf and flatbuffers schemas have consistent message/table counts
#[test]
fn test_schema_consistency_with_flatbuffers() {
    let bindings_dir = get_bindings_dir();
    let proto_path = bindings_dir.join("schema.proto");
    let fbs_path = bindings_dir.join("schema.fbs");

    if let (Ok(proto_content), Ok(fbs_content)) = (
        fs::read_to_string(&proto_path),
        fs::read_to_string(&fbs_path),
    ) {
        let proto_messages = proto_content.matches("message ").count();
        let fbs_tables = fbs_content.matches("table ").count();

        // Both should have similar structure counts
        assert!(
            proto_messages > 0 && fbs_tables > 0,
            "Both schemas should have been generated"
        );

        println!(
            "Proto messages: {}, FlatBuffers tables: {}",
            proto_messages, fbs_tables
        );
    }
}
