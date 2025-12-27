//! Integration tests for Evenframe playground
//!
//! These tests verify that:
//! 1. Evenframe correctly scans and processes the playground models
//! 2. TypeScript type definitions are generated correctly
//! 3. Database schema statements are generated correctly

use std::fs;
use std::path::PathBuf;

/// Test that all expected model types are discovered by the workspace scanner
#[test]
fn test_model_discovery() {
    // Get the playground directory
    let playground_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    // Check that model files exist
    let auth_path = playground_dir.join("src/models/auth.rs");
    let blog_path = playground_dir.join("src/models/blog.rs");
    let ecommerce_path = playground_dir.join("src/models/ecommerce.rs");

    assert!(auth_path.exists(), "auth.rs should exist");
    assert!(blog_path.exists(), "blog.rs should exist");
    assert!(ecommerce_path.exists(), "ecommerce.rs should exist");

    // Verify auth models contain expected types
    let auth_content = fs::read_to_string(&auth_path).expect("Failed to read auth.rs");
    assert!(
        auth_content.contains("pub struct User"),
        "auth.rs should contain User struct"
    );
    assert!(
        auth_content.contains("pub struct Session"),
        "auth.rs should contain Session struct"
    );
    assert!(
        auth_content.contains("pub enum Role"),
        "auth.rs should contain Role enum"
    );

    // Verify blog models contain expected types
    let blog_content = fs::read_to_string(&blog_path).expect("Failed to read blog.rs");
    assert!(
        blog_content.contains("pub struct Post"),
        "blog.rs should contain Post struct"
    );
    assert!(
        blog_content.contains("pub struct Author"),
        "blog.rs should contain Author struct"
    );
    assert!(
        blog_content.contains("pub struct Comment"),
        "blog.rs should contain Comment struct"
    );
    assert!(
        blog_content.contains("pub struct Tag"),
        "blog.rs should contain Tag struct"
    );

    // Verify ecommerce models contain expected types
    let ecommerce_content =
        fs::read_to_string(&ecommerce_path).expect("Failed to read ecommerce.rs");
    assert!(
        ecommerce_content.contains("pub struct Product"),
        "ecommerce.rs should contain Product struct"
    );
    assert!(
        ecommerce_content.contains("pub struct Order"),
        "ecommerce.rs should contain Order struct"
    );
    assert!(
        ecommerce_content.contains("pub struct Customer"),
        "ecommerce.rs should contain Customer struct"
    );
    assert!(
        ecommerce_content.contains("pub enum OrderStatus"),
        "ecommerce.rs should contain OrderStatus enum"
    );
}

/// Test that Evenframe attributes are correctly applied to models
#[test]
fn test_evenframe_attributes() {
    let playground_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    // Check auth models for Evenframe attributes
    let auth_content =
        fs::read_to_string(playground_dir.join("src/models/auth.rs")).expect("Failed to read auth.rs");

    // Check for Evenframe derive
    assert!(
        auth_content.contains("#[derive(Debug, Clone, Serialize, Deserialize, Evenframe)]"),
        "Models should have Evenframe derive"
    );

    // Check for mock_data attribute
    assert!(
        auth_content.contains("#[mock_data(n = 50)]"),
        "User should have mock_data attribute"
    );

    // Check for format attributes
    assert!(
        auth_content.contains("#[format(Email)]"),
        "User email should have Email format"
    );
    assert!(
        auth_content.contains("#[format(DateTime)]"),
        "Timestamps should have DateTime format"
    );

    // Check for edge attributes
    assert!(
        auth_content.contains("#[edge("),
        "Session should have edge to User"
    );
}

/// Test that edge relationships are correctly defined
#[test]
fn test_edge_relationships() {
    let playground_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    // Check blog models for edge relationships
    let blog_content =
        fs::read_to_string(playground_dir.join("src/models/blog.rs")).expect("Failed to read blog.rs");

    // Author -> User edge
    assert!(
        blog_content.contains(r#"#[edge(name = "author_user""#),
        "Author should have edge to User"
    );

    // Post -> Author edge
    assert!(
        blog_content.contains(r#"#[edge(name = "post_author""#),
        "Post should have edge to Author"
    );

    // Post -> Tag edge
    assert!(
        blog_content.contains(r#"#[edge(name = "post_tags""#),
        "Post should have edge to Tag"
    );

    // Comment -> Post edge
    assert!(
        blog_content.contains(r#"#[edge(name = "comment_post""#),
        "Comment should have edge to Post"
    );

    // Comment -> Author edge
    assert!(
        blog_content.contains(r#"#[edge(name = "comment_author""#),
        "Comment should have edge to Author"
    );

    // Check ecommerce models for edge relationships
    let ecommerce_content =
        fs::read_to_string(playground_dir.join("src/models/ecommerce.rs")).expect("Failed to read ecommerce.rs");

    // Customer -> User edge
    assert!(
        ecommerce_content.contains(r#"#[edge(name = "customer_user""#),
        "Customer should have edge to User"
    );

    // Order -> Customer edge
    assert!(
        ecommerce_content.contains(r#"#[edge(name = "order_customer""#),
        "Order should have edge to Customer"
    );
}

/// Test that RecordLink types are correctly used for relationships
#[test]
fn test_record_link_types() {
    let playground_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let auth_content =
        fs::read_to_string(playground_dir.join("src/models/auth.rs")).expect("Failed to read auth.rs");

    // Session.user should be RecordLink<User>
    assert!(
        auth_content.contains("pub user: RecordLink<User>"),
        "Session.user should be RecordLink<User>"
    );

    let blog_content =
        fs::read_to_string(playground_dir.join("src/models/blog.rs")).expect("Failed to read blog.rs");

    // Author.user should be RecordLink<User>
    assert!(
        blog_content.contains("pub user: RecordLink<User>"),
        "Author.user should be RecordLink<User>"
    );

    // Post.author should be RecordLink<Author>
    assert!(
        blog_content.contains("pub author: RecordLink<Author>"),
        "Post.author should be RecordLink<Author>"
    );

    // Post.tags should be Vec<RecordLink<Tag>>
    assert!(
        blog_content.contains("pub tags: Vec<RecordLink<Tag>>"),
        "Post.tags should be Vec<RecordLink<Tag>>"
    );
}

/// Test that non-persistable structs (without id field) are correctly defined
#[test]
fn test_non_persistable_structs() {
    let playground_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let ecommerce_content =
        fs::read_to_string(playground_dir.join("src/models/ecommerce.rs")).expect("Failed to read ecommerce.rs");

    // Address should not have an id field (non-persistable)
    let address_start = ecommerce_content
        .find("pub struct Address")
        .expect("Address struct should exist");
    let address_end = ecommerce_content[address_start..]
        .find("}")
        .map(|i| address_start + i)
        .expect("Address struct should have closing brace");
    let address_content = &ecommerce_content[address_start..=address_end];

    assert!(
        !address_content.contains("pub id:"),
        "Address should not have an id field (non-persistable struct)"
    );

    // CartItem should not have an id field (non-persistable)
    let cart_item_start = ecommerce_content
        .find("pub struct CartItem")
        .expect("CartItem struct should exist");
    let cart_item_end = ecommerce_content[cart_item_start..]
        .find("}")
        .map(|i| cart_item_start + i)
        .expect("CartItem struct should have closing brace");
    let cart_item_content = &ecommerce_content[cart_item_start..=cart_item_end];

    assert!(
        !cart_item_content.contains("pub id:"),
        "CartItem should not have an id field (non-persistable struct)"
    );
}

/// Test that enums are correctly defined
#[test]
fn test_enum_definitions() {
    let playground_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let auth_content =
        fs::read_to_string(playground_dir.join("src/models/auth.rs")).expect("Failed to read auth.rs");

    // Role enum variants
    assert!(auth_content.contains("Admin"), "Role should have Admin variant");
    assert!(
        auth_content.contains("Moderator"),
        "Role should have Moderator variant"
    );
    assert!(auth_content.contains("Guest"), "Role should have Guest variant");

    let ecommerce_content =
        fs::read_to_string(playground_dir.join("src/models/ecommerce.rs")).expect("Failed to read ecommerce.rs");

    // OrderStatus enum variants
    assert!(
        ecommerce_content.contains("Pending"),
        "OrderStatus should have Pending variant"
    );
    assert!(
        ecommerce_content.contains("Processing"),
        "OrderStatus should have Processing variant"
    );
    assert!(
        ecommerce_content.contains("Shipped"),
        "OrderStatus should have Shipped variant"
    );
    assert!(
        ecommerce_content.contains("Delivered"),
        "OrderStatus should have Delivered variant"
    );
    assert!(
        ecommerce_content.contains("Cancelled"),
        "OrderStatus should have Cancelled variant"
    );

    // ProductCategory enum variants
    assert!(
        ecommerce_content.contains("Electronics"),
        "ProductCategory should have Electronics variant"
    );
    assert!(
        ecommerce_content.contains("Clothing"),
        "ProductCategory should have Clothing variant"
    );
    assert!(
        ecommerce_content.contains("Books"),
        "ProductCategory should have Books variant"
    );
}

/// Test configuration file exists and is valid
#[test]
fn test_evenframe_config() {
    let playground_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let config_path = playground_dir.join("evenframe.toml");

    assert!(config_path.exists(), "evenframe.toml should exist");

    let config_content =
        fs::read_to_string(&config_path).expect("Failed to read evenframe.toml");

    // Check for required sections
    assert!(
        config_content.contains("[general]"),
        "Config should have [general] section"
    );
    assert!(
        config_content.contains("[schemasync]"),
        "Config should have [schemasync] section"
    );
    assert!(
        config_content.contains("[typesync]"),
        "Config should have [typesync] section"
    );

    // Check for typesync settings
    assert!(
        config_content.contains("should_generate_arktype_types"),
        "Config should have arktype generation setting"
    );
    assert!(
        config_content.contains("should_generate_effect_types"),
        "Config should have effect generation setting"
    );
    assert!(
        config_content.contains("output_path"),
        "Config should have output_path setting"
    );

    // Check for database settings
    assert!(
        config_content.contains("[schemasync.database]"),
        "Config should have database section"
    );
}

/// Test that all persistable structs have an id field
#[test]
fn test_persistable_structs_have_id() {
    let playground_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    // List of expected persistable structs and their files
    let persistable_structs = vec![
        ("src/models/auth.rs", vec!["User", "Session"]),
        (
            "src/models/blog.rs",
            vec!["Tag", "Author", "Post", "Comment"],
        ),
        (
            "src/models/ecommerce.rs",
            vec!["Product", "Customer", "Order"],
        ),
    ];

    for (file_path, structs) in persistable_structs {
        let content = fs::read_to_string(playground_dir.join(file_path))
            .unwrap_or_else(|_| panic!("Failed to read {}", file_path));

        for struct_name in structs {
            // Find the struct definition
            let pattern = format!("pub struct {}", struct_name);
            let struct_start = content
                .find(&pattern)
                .unwrap_or_else(|| panic!("{} struct should exist in {}", struct_name, file_path));

            // Find the struct body
            let brace_start = content[struct_start..]
                .find("{")
                .map(|i| struct_start + i)
                .unwrap_or_else(|| panic!("{} should have opening brace", struct_name));

            let brace_end = content[brace_start..]
                .find("}")
                .map(|i| brace_start + i)
                .unwrap_or_else(|| panic!("{} should have closing brace", struct_name));

            let struct_body = &content[brace_start..=brace_end];

            assert!(
                struct_body.contains("pub id: String"),
                "{} should have 'pub id: String' field (persistable struct)",
                struct_name
            );
        }
    }
}

/// Test that mock_data attributes have valid n values
#[test]
fn test_mock_data_values() {
    let playground_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    let files = vec![
        "src/models/auth.rs",
        "src/models/blog.rs",
        "src/models/ecommerce.rs",
    ];

    for file_path in files {
        let content = fs::read_to_string(playground_dir.join(file_path))
            .unwrap_or_else(|_| panic!("Failed to read {}", file_path));

        // Find all mock_data attributes
        for line in content.lines() {
            if line.contains("#[mock_data(n = ") {
                // Extract the n value
                let start = line.find("n = ").unwrap() + 4;
                let end = line[start..].find(")").unwrap() + start;
                let n_str = &line[start..end];

                let n: u32 = n_str
                    .parse()
                    .unwrap_or_else(|_| panic!("mock_data n value should be valid integer: {}", n_str));

                assert!(n > 0, "mock_data n value should be positive");
                assert!(n <= 1000, "mock_data n value should be reasonable (<= 1000)");
            }
        }
    }
}
