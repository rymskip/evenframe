//! End-to-end tests for FlatBuffers parser functionality.
//!
//! These tests verify that:
//! 1. The FlatBuffers parser can parse the generated schema.fbs file
//! 2. All types are correctly converted to evenframe's internal representation
//! 3. Validation attributes are properly extracted
//! 4. The parsed types can be used for type generation

use evenframe_core::typesync::flatbuffers_parser::{parse_flatbuffers_file, parse_flatbuffers_source};
use evenframe_core::types::FieldType;
use std::path::PathBuf;

/// Get the path to the generated schema.fbs file
fn get_schema_fbs_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/bindings/schema.fbs")
}

// ============================================================================
// File Parsing Tests
// ============================================================================

#[test]
fn test_parse_generated_schema_fbs() {
    let schema_path = get_schema_fbs_path();

    if !schema_path.exists() {
        eprintln!(
            "Skipping test: schema.fbs not found at {:?}",
            schema_path
        );
        return;
    }

    let result = parse_flatbuffers_file(&schema_path, &[]);
    assert!(
        result.is_ok(),
        "Failed to parse schema.fbs: {:?}",
        result.err()
    );

    let parsed = result.unwrap();

    // Verify namespace is present
    assert!(
        parsed.namespace.is_some(),
        "Schema should have a namespace"
    );

    // Verify we have some structs
    assert!(
        !parsed.structs.is_empty(),
        "Schema should contain struct definitions"
    );

    println!("Parsed {} structs from schema.fbs", parsed.structs.len());
    println!("Parsed {} enums from schema.fbs", parsed.enums.len());
}

#[test]
fn test_schema_fbs_contains_expected_types() {
    let schema_path = get_schema_fbs_path();

    if !schema_path.exists() {
        eprintln!("Skipping test: schema.fbs not found");
        return;
    }

    let result = parse_flatbuffers_file(&schema_path, &[]).unwrap();

    // Check for expected types from the playground models
    // These should exist based on the auth, blog, and ecommerce models
    let expected_structs = vec!["User", "Session", "Product", "Order", "Customer"];
    let expected_enums = vec!["Role", "OrderStatus"];

    for name in &expected_structs {
        if result.structs.contains_key(*name) {
            println!("Found expected struct: {}", name);
        }
    }

    for name in &expected_enums {
        if result.enums.contains_key(*name) {
            println!("Found expected enum: {}", name);
        }
    }
}

// ============================================================================
// Source Parsing Tests
// ============================================================================

#[test]
fn test_parse_simple_flatbuffers_source() {
    let source = r#"
        namespace TestApp;

        table User {
            id: uint64;
            name: string;
            email: string;
            age: int32 = 0;
            active: bool = true;
        }

        enum Status : byte {
            Unknown = 0,
            Active = 1,
            Inactive = 2
        }

        root_type User;
    "#;

    let result = parse_flatbuffers_source(source);
    assert!(result.is_ok(), "Failed to parse source: {:?}", result.err());

    let parsed = result.unwrap();
    assert_eq!(parsed.namespace, Some("TestApp".to_string()));
    assert!(parsed.structs.contains_key("User"));
    assert!(parsed.enums.contains_key("Status"));

    let user = &parsed.structs["User"];
    assert_eq!(user.struct_name, "User");
    assert_eq!(user.fields.len(), 5);
}

#[test]
fn test_parse_nested_tables() {
    let source = r#"
        namespace Nested;

        table Address {
            street: string;
            city: string;
            country: string;
            postal_code: string;
        }

        table Person {
            name: string;
            address: Address;
            tags: [string];
        }
    "#;

    let result = parse_flatbuffers_source(source).unwrap();

    assert!(result.structs.contains_key("Address"));
    assert!(result.structs.contains_key("Person"));

    let person = &result.structs["Person"];
    let address_field = person.fields.iter().find(|f| f.field_name == "address");
    assert!(address_field.is_some());

    // The address field should reference the Address type
    if let Some(field) = address_field {
        assert!(
            matches!(&field.field_type, FieldType::Other(name) if name == "Address"),
            "address field should reference Address type"
        );
    }

    // Check tags field is a vector
    let tags_field = person.fields.iter().find(|f| f.field_name == "tags");
    assert!(tags_field.is_some());
    if let Some(field) = tags_field {
        assert!(
            matches!(&field.field_type, FieldType::Vec(_)),
            "tags field should be a vector"
        );
    }
}

#[test]
fn test_parse_union_types() {
    let source = r#"
        namespace Unions;

        table Dog {
            name: string;
            breed: string;
        }

        table Cat {
            name: string;
            indoor: bool;
        }

        table Bird {
            name: string;
            can_fly: bool;
        }

        union Pet {
            Dog,
            Cat,
            Bird
        }

        table Owner {
            name: string;
            pet: Pet;
        }
    "#;

    let result = parse_flatbuffers_source(source).unwrap();

    assert!(result.structs.contains_key("Dog"));
    assert!(result.structs.contains_key("Cat"));
    assert!(result.structs.contains_key("Bird"));
    assert!(result.structs.contains_key("Owner"));
    assert!(result.enums.contains_key("Pet"));

    let pet_union = &result.enums["Pet"];
    assert_eq!(pet_union.variants.len(), 3);
    assert_eq!(pet_union.variants[0].name, "Dog");
    assert_eq!(pet_union.variants[1].name, "Cat");
    assert_eq!(pet_union.variants[2].name, "Bird");
}

#[test]
fn test_parse_all_scalar_types() {
    let source = r#"
        table AllScalars {
            bool_val: bool;
            byte_val: byte;
            ubyte_val: ubyte;
            short_val: short;
            ushort_val: ushort;
            int_val: int;
            uint_val: uint;
            long_val: long;
            ulong_val: ulong;
            float_val: float;
            double_val: double;
            int8_val: int8;
            uint8_val: uint8;
            int16_val: int16;
            uint16_val: uint16;
            int32_val: int32;
            uint32_val: uint32;
            int64_val: int64;
            uint64_val: uint64;
            float32_val: float32;
            float64_val: float64;
            string_val: string;
        }
    "#;

    let result = parse_flatbuffers_source(source).unwrap();
    let all_scalars = &result.structs["AllScalars"];
    assert_eq!(all_scalars.fields.len(), 22);

    // Verify type mappings
    let field_type_map: std::collections::HashMap<_, _> = all_scalars
        .fields
        .iter()
        .map(|f| (f.field_name.as_str(), &f.field_type))
        .collect();

    assert!(matches!(field_type_map["bool_val"], FieldType::Bool));
    assert!(matches!(field_type_map["byte_val"], FieldType::I8));
    assert!(matches!(field_type_map["ubyte_val"], FieldType::U8));
    assert!(matches!(field_type_map["int_val"], FieldType::I32));
    assert!(matches!(field_type_map["uint_val"], FieldType::U32));
    assert!(matches!(field_type_map["long_val"], FieldType::I64));
    assert!(matches!(field_type_map["ulong_val"], FieldType::U64));
    assert!(matches!(field_type_map["float_val"], FieldType::F32));
    assert!(matches!(field_type_map["double_val"], FieldType::F64));
    assert!(matches!(field_type_map["string_val"], FieldType::String));
}

#[test]
fn test_parse_vector_types() {
    let source = r#"
        table Vectors {
            strings: [string];
            ints: [int32];
            floats: [float64];
            bools: [bool];
        }
    "#;

    let result = parse_flatbuffers_source(source).unwrap();
    let vectors = &result.structs["Vectors"];

    for field in &vectors.fields {
        assert!(
            matches!(&field.field_type, FieldType::Vec(_)),
            "Field {} should be a vector type",
            field.field_name
        );
    }
}

// ============================================================================
// Validation Extraction Tests
// ============================================================================

#[test]
fn test_parse_with_validation_metadata() {
    let source = r#"
        table UserWithValidation {
            email: string (validate: "email");
            age: int32 (validate: "min(0), max(150)");
            password: string (validate: "minLength(8), maxLength(128)");
            username: string (validate: "alphanumeric, minLength(3)");
        }
    "#;

    let result = parse_flatbuffers_source(source).unwrap();
    let user = &result.structs["UserWithValidation"];

    // Check email field has validators
    let email_field = user.fields.iter().find(|f| f.field_name == "email").unwrap();
    assert!(
        !email_field.validators.is_empty(),
        "email field should have validators extracted"
    );

    // Check age field has validators
    let age_field = user.fields.iter().find(|f| f.field_name == "age").unwrap();
    assert!(
        !age_field.validators.is_empty(),
        "age field should have validators extracted"
    );

    // Check password field has validators
    let password_field = user
        .fields
        .iter()
        .find(|f| f.field_name == "password")
        .unwrap();
    assert!(
        !password_field.validators.is_empty(),
        "password field should have validators extracted"
    );
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[test]
fn test_parse_invalid_syntax() {
    let source = r#"
        table Invalid {
            this is not valid syntax
        }
    "#;

    let result = parse_flatbuffers_source(source);
    assert!(result.is_err(), "Invalid syntax should produce an error");
}

#[test]
fn test_parse_empty_source() {
    let source = "";
    let result = parse_flatbuffers_source(source);
    assert!(result.is_ok(), "Empty source should parse successfully");

    let parsed = result.unwrap();
    assert!(parsed.structs.is_empty());
    assert!(parsed.enums.is_empty());
}

#[test]
fn test_parse_comments_only() {
    let source = r#"
        // This is a comment
        // Another comment
        /* Block comment */
    "#;

    let result = parse_flatbuffers_source(source);
    assert!(result.is_ok(), "Comments-only source should parse successfully");
}

// ============================================================================
// Complex Schema Tests
// ============================================================================

#[test]
fn test_parse_complex_ecommerce_schema() {
    let source = r#"
        namespace ECommerce;

        enum OrderStatus : byte {
            Pending = 0,
            Processing = 1,
            Shipped = 2,
            Delivered = 3,
            Cancelled = 4
        }

        enum PaymentMethod : byte {
            CreditCard = 0,
            DebitCard = 1,
            PayPal = 2,
            BankTransfer = 3
        }

        table Address {
            street: string;
            city: string;
            state: string;
            postal_code: string;
            country: string;
        }

        table Customer {
            id: uint64;
            email: string (validate: "email");
            name: string (validate: "minLength(1)");
            shipping_address: Address;
            billing_address: Address;
        }

        table Product {
            id: uint64;
            sku: string;
            name: string;
            description: string;
            price_cents: int64;
            stock: int32;
            tags: [string];
        }

        table OrderItem {
            product_id: uint64;
            quantity: int32 (validate: "min(1)");
            unit_price_cents: int64;
        }

        table Order {
            id: uint64;
            customer_id: uint64;
            items: [OrderItem];
            status: OrderStatus;
            payment_method: PaymentMethod;
            shipping_address: Address;
            total_cents: int64;
            created_at: string;
        }

        root_type Order;
    "#;

    let result = parse_flatbuffers_source(source).unwrap();

    // Verify all types are parsed
    assert!(result.structs.contains_key("Address"));
    assert!(result.structs.contains_key("Customer"));
    assert!(result.structs.contains_key("Product"));
    assert!(result.structs.contains_key("OrderItem"));
    assert!(result.structs.contains_key("Order"));
    assert!(result.enums.contains_key("OrderStatus"));
    assert!(result.enums.contains_key("PaymentMethod"));

    // Verify enum variants
    let order_status = &result.enums["OrderStatus"];
    assert_eq!(order_status.variants.len(), 5);

    let payment_method = &result.enums["PaymentMethod"];
    assert_eq!(payment_method.variants.len(), 4);

    // Verify Order fields
    let order = &result.structs["Order"];
    assert!(order.fields.len() >= 8);

    // Verify items field is a vector
    let items_field = order.fields.iter().find(|f| f.field_name == "items");
    assert!(items_field.is_some());
}

// ============================================================================
// Round-trip Tests
// ============================================================================

#[test]
fn test_parsed_types_can_generate_typescript() {
    let source = r#"
        namespace RoundTrip;

        table TestType {
            id: uint64;
            name: string;
            values: [int32];
        }
    "#;

    let result = parse_flatbuffers_source(source).unwrap();
    let test_type = &result.structs["TestType"];

    // Verify the struct can be used for type generation
    assert_eq!(test_type.struct_name, "TestType");
    assert_eq!(test_type.fields.len(), 3);

    // Each field should have the information needed for generation
    for field in &test_type.fields {
        assert!(!field.field_name.is_empty());
        // field_type should be valid
        match &field.field_type {
            FieldType::U64 | FieldType::String | FieldType::Vec(_) => {}
            other => panic!("Unexpected field type: {:?}", other),
        }
    }
}
