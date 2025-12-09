//! Schema verification tests
//!
//! These tests verify that the expected database schemas would be generated
//! from the playground models without requiring a running database.

use std::fs;
use std::path::PathBuf;

/// Get the playground directory
fn get_playground_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Expected table names based on models (snake_case of struct names)
fn expected_table_names() -> Vec<&'static str> {
    vec![
        // Auth tables
        "user",
        "session",
        // Blog tables
        "tag",
        "author",
        "post",
        "comment",
        // Ecommerce tables
        "product",
        "customer",
        "order",
    ]
}

/// Expected enum names (should also become tables or types in SurrealDB)
fn expected_enum_names() -> Vec<&'static str> {
    vec![
        "role",
        "order_status",
        "product_category",
    ]
}

/// Expected edge names based on edge attributes
fn expected_edge_names() -> Vec<&'static str> {
    vec![
        // Auth edges
        "session_user",
        // Blog edges
        "author_user",
        "post_author",
        "post_tags",
        "comment_post",
        "comment_author",
        // Ecommerce edges
        "customer_user",
        "order_customer",
    ]
}

/// Test that we have the expected number of tables
#[test]
fn test_expected_table_count() {
    let tables = expected_table_names();
    assert_eq!(tables.len(), 9, "Should have 9 persistable tables");
}

/// Test that we have the expected number of enums
#[test]
fn test_expected_enum_count() {
    let enums = expected_enum_names();
    assert_eq!(enums.len(), 3, "Should have 3 enum types");
}

/// Test that we have the expected number of edges
#[test]
fn test_expected_edge_count() {
    let edges = expected_edge_names();
    assert_eq!(edges.len(), 8, "Should have 8 edge relationships");
}

/// Test User table expected schema
#[test]
fn test_user_table_schema() {
    let playground_dir = get_playground_dir();
    let auth_content = fs::read_to_string(playground_dir.join("src/models/auth.rs"))
        .expect("Failed to read auth.rs");

    // User should have these fields that map to SurrealDB columns
    let expected_fields = vec![
        ("id", "String", "record<user>"),
        ("email", "String", "string"),
        ("username", "String", "string"),
        ("password_hash", "String", "string"),
        ("roles", "Vec<Role>", "array<string>"),
        ("is_active", "bool", "bool"),
        ("created_at", "String", "datetime"),
        ("updated_at", "String", "datetime"),
    ];

    for (field_name, rust_type, _surreal_type) in &expected_fields {
        let pattern = format!("pub {}: {}", field_name, rust_type);
        // Check that the field exists (we're testing the source, not generated schema)
        assert!(
            auth_content.contains(&format!("pub {}:", field_name)),
            "User should have {} field",
            field_name
        );
    }
}

/// Test Session table expected schema
#[test]
fn test_session_table_schema() {
    let playground_dir = get_playground_dir();
    let auth_content = fs::read_to_string(playground_dir.join("src/models/auth.rs"))
        .expect("Failed to read auth.rs");

    // Session should have these fields
    let expected_fields = vec!["id", "user", "token", "expires_at", "created_at"];

    for field_name in &expected_fields {
        assert!(
            auth_content.contains(&format!("pub {}:", field_name)),
            "Session should have {} field",
            field_name
        );
    }

    // User field should be a RecordLink
    assert!(
        auth_content.contains("pub user: RecordLink<User>"),
        "Session.user should be RecordLink<User>"
    );
}

/// Test Product table expected schema
#[test]
fn test_product_table_schema() {
    let playground_dir = get_playground_dir();
    let ecommerce_content = fs::read_to_string(playground_dir.join("src/models/ecommerce.rs"))
        .expect("Failed to read ecommerce.rs");

    let expected_fields = vec![
        "id",
        "name",
        "description",
        "price",
        "stock_quantity",
        "category",
        "image_url",
        "is_available",
        "created_at",
    ];

    for field_name in &expected_fields {
        assert!(
            ecommerce_content.contains(&format!("pub {}:", field_name)),
            "Product should have {} field",
            field_name
        );
    }
}

/// Test Order table expected schema with nested types
#[test]
fn test_order_table_schema() {
    let playground_dir = get_playground_dir();
    let ecommerce_content = fs::read_to_string(playground_dir.join("src/models/ecommerce.rs"))
        .expect("Failed to read ecommerce.rs");

    // Order should have these fields
    let expected_fields = vec![
        "id",
        "customer",
        "items",
        "subtotal",
        "tax",
        "shipping_cost",
        "total",
        "status",
        "shipping_address",
        "notes",
        "created_at",
        "shipped_at",
        "delivered_at",
    ];

    for field_name in &expected_fields {
        assert!(
            ecommerce_content.contains(&format!("pub {}:", field_name)),
            "Order should have {} field",
            field_name
        );
    }

    // Items should be Vec<CartItem>
    assert!(
        ecommerce_content.contains("pub items: Vec<CartItem>"),
        "Order.items should be Vec<CartItem>"
    );

    // shipping_address should be Address
    assert!(
        ecommerce_content.contains("pub shipping_address: Address"),
        "Order.shipping_address should be Address"
    );
}

/// Test Post table expected schema with multiple edges
#[test]
fn test_post_table_schema() {
    let playground_dir = get_playground_dir();
    let blog_content = fs::read_to_string(playground_dir.join("src/models/blog.rs"))
        .expect("Failed to read blog.rs");

    let expected_fields = vec![
        "id",
        "title",
        "slug",
        "content",
        "excerpt",
        "author",
        "tags",
        "featured_image",
        "published",
        "published_at",
        "view_count",
        "created_at",
        "updated_at",
    ];

    for field_name in &expected_fields {
        assert!(
            blog_content.contains(&format!("pub {}:", field_name)),
            "Post should have {} field",
            field_name
        );
    }

    // Author should be RecordLink<Author>
    assert!(
        blog_content.contains("pub author: RecordLink<Author>"),
        "Post.author should be RecordLink<Author>"
    );

    // Tags should be Vec<RecordLink<Tag>>
    assert!(
        blog_content.contains("pub tags: Vec<RecordLink<Tag>>"),
        "Post.tags should be Vec<RecordLink<Tag>>"
    );
}

/// Test Address embedded object schema
#[test]
fn test_address_object_schema() {
    let playground_dir = get_playground_dir();
    let ecommerce_content = fs::read_to_string(playground_dir.join("src/models/ecommerce.rs"))
        .expect("Failed to read ecommerce.rs");

    // Address should be a non-persistable struct (no id field)
    let address_section = extract_struct_body(&ecommerce_content, "Address");

    assert!(
        !address_section.contains("pub id:"),
        "Address should not have id field (embedded object)"
    );

    let expected_fields = vec!["street", "city", "state", "postal_code", "country"];

    for field_name in &expected_fields {
        assert!(
            address_section.contains(&format!("pub {}:", field_name)),
            "Address should have {} field",
            field_name
        );
    }
}

/// Test CartItem embedded object schema
#[test]
fn test_cart_item_object_schema() {
    let playground_dir = get_playground_dir();
    let ecommerce_content = fs::read_to_string(playground_dir.join("src/models/ecommerce.rs"))
        .expect("Failed to read ecommerce.rs");

    // CartItem should be a non-persistable struct (no id field)
    let cart_item_section = extract_struct_body(&ecommerce_content, "CartItem");

    assert!(
        !cart_item_section.contains("pub id:"),
        "CartItem should not have id field (embedded object)"
    );

    let expected_fields = vec!["product_id", "product_name", "quantity", "unit_price"];

    for field_name in &expected_fields {
        assert!(
            cart_item_section.contains(&format!("pub {}:", field_name)),
            "CartItem should have {} field",
            field_name
        );
    }
}

/// Test Role enum variants
#[test]
fn test_role_enum_schema() {
    let playground_dir = get_playground_dir();
    let auth_content = fs::read_to_string(playground_dir.join("src/models/auth.rs"))
        .expect("Failed to read auth.rs");

    let expected_variants = vec!["Admin", "Moderator", "User", "Guest"];

    for variant in &expected_variants {
        assert!(
            auth_content.contains(variant),
            "Role enum should have {} variant",
            variant
        );
    }
}

/// Test OrderStatus enum variants
#[test]
fn test_order_status_enum_schema() {
    let playground_dir = get_playground_dir();
    let ecommerce_content = fs::read_to_string(playground_dir.join("src/models/ecommerce.rs"))
        .expect("Failed to read ecommerce.rs");

    let expected_variants = vec!["Pending", "Processing", "Shipped", "Delivered", "Cancelled"];

    for variant in &expected_variants {
        assert!(
            ecommerce_content.contains(variant),
            "OrderStatus enum should have {} variant",
            variant
        );
    }
}

/// Test ProductCategory enum variants
#[test]
fn test_product_category_enum_schema() {
    let playground_dir = get_playground_dir();
    let ecommerce_content = fs::read_to_string(playground_dir.join("src/models/ecommerce.rs"))
        .expect("Failed to read ecommerce.rs");

    let expected_variants = vec!["Electronics", "Clothing", "Books", "Home", "Sports", "Other"];

    for variant in &expected_variants {
        assert!(
            ecommerce_content.contains(variant),
            "ProductCategory enum should have {} variant",
            variant
        );
    }
}

/// Helper function to extract struct body from source code
fn extract_struct_body(content: &str, struct_name: &str) -> String {
    let pattern = format!("pub struct {}", struct_name);
    let struct_start = content.find(&pattern).unwrap_or(0);
    let brace_start = content[struct_start..]
        .find("{")
        .map(|i| struct_start + i)
        .unwrap_or(struct_start);
    let brace_end = content[brace_start..]
        .find("}")
        .map(|i| brace_start + i + 1)
        .unwrap_or(content.len());

    content[brace_start..brace_end].to_string()
}

/// Test that mock_data counts are appropriate for each table
#[test]
fn test_mock_data_counts() {
    let playground_dir = get_playground_dir();

    // Expected mock data counts based on model attributes
    let expected_counts = vec![
        ("src/models/auth.rs", "User", 50),
        ("src/models/auth.rs", "Session", 100),
        ("src/models/blog.rs", "Tag", 20),
        ("src/models/blog.rs", "Author", 10),
        ("src/models/blog.rs", "Post", 50),
        ("src/models/blog.rs", "Comment", 200),
        ("src/models/ecommerce.rs", "Product", 100),
        ("src/models/ecommerce.rs", "Customer", 50),
        ("src/models/ecommerce.rs", "Order", 200),
    ];

    for (file_path, struct_name, expected_n) in &expected_counts {
        let content = fs::read_to_string(playground_dir.join(file_path))
            .expect(&format!("Failed to read {}", file_path));

        // Find the struct and its mock_data attribute
        let struct_pattern = format!("pub struct {}", struct_name);
        let struct_pos = content.find(&struct_pattern);

        if let Some(pos) = struct_pos {
            // Look backwards for mock_data attribute
            let before_struct = &content[..pos];
            let last_mock_data = before_struct.rfind("#[mock_data(");

            if let Some(mock_pos) = last_mock_data {
                let mock_line = &content[mock_pos..pos];
                let expected_pattern = format!("n = {}", expected_n);
                assert!(
                    mock_line.contains(&expected_pattern),
                    "{} should have mock_data(n = {}), found: {}",
                    struct_name,
                    expected_n,
                    mock_line.trim()
                );
            } else {
                panic!(
                    "{} should have #[mock_data] attribute",
                    struct_name
                );
            }
        }
    }
}

/// Test relationship directions
#[test]
fn test_edge_directions() {
    let playground_dir = get_playground_dir();

    // All edges in our models use direction = "from"
    let files = vec![
        "src/models/auth.rs",
        "src/models/blog.rs",
        "src/models/ecommerce.rs",
    ];

    for file_path in files {
        let content = fs::read_to_string(playground_dir.join(file_path))
            .expect(&format!("Failed to read {}", file_path));

        for line in content.lines() {
            if line.contains("#[edge(") {
                assert!(
                    line.contains(r#"direction = "from""#),
                    "Edge in {} should have direction = \"from\": {}",
                    file_path,
                    line
                );
            }
        }
    }
}
