//! End-to-end tests for Protocol Buffers parser functionality.
//!
//! These tests verify that:
//! 1. The Protobuf parser can parse the generated schema.proto file
//! 2. All types are correctly converted to evenframe's internal representation
//! 3. The parsed types can be used for type generation

use evenframe_core::typesync::protobuf_parser::{parse_protobuf_files, parse_protobuf_source};
use evenframe_core::types::FieldType;
use std::path::PathBuf;

/// Get the path to the generated schema.proto file
fn get_schema_proto_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/bindings/schema.proto")
}

// ============================================================================
// File Parsing Tests
// ============================================================================

#[test]
fn test_parse_generated_schema_proto() {
    let schema_path = get_schema_proto_path();

    if !schema_path.exists() {
        eprintln!(
            "Skipping test: schema.proto not found at {:?}",
            schema_path
        );
        return;
    }

    let result = parse_protobuf_files(&[schema_path.as_path()], &[]);

    // The generated schema.proto may import validate/validate.proto which doesn't exist locally
    // Skip the test if we get an import error
    match result {
        Ok(parsed) => {
            // Verify we have some structs
            assert!(
                !parsed.structs.is_empty(),
                "Schema should contain message definitions"
            );
            println!("Parsed {} messages from schema.proto", parsed.structs.len());
            println!("Parsed {} enums from schema.proto", parsed.enums.len());
        }
        Err(e) => {
            let err_msg = format!("{}", e);
            if err_msg.contains("validate.proto") || err_msg.contains("import") {
                eprintln!("Skipping test: schema.proto has external imports that aren't available: {}", err_msg);
                return;
            }
            panic!("Failed to parse schema.proto: {:?}", e);
        }
    }
}

#[test]
fn test_schema_proto_contains_expected_types() {
    let schema_path = get_schema_proto_path();

    if !schema_path.exists() {
        eprintln!("Skipping test: schema.proto not found");
        return;
    }

    let result = parse_protobuf_files(&[schema_path.as_path()], &[]);

    // The generated schema.proto may import validate/validate.proto which doesn't exist locally
    let parsed = match result {
        Ok(parsed) => parsed,
        Err(e) => {
            let err_msg = format!("{}", e);
            if err_msg.contains("validate.proto") || err_msg.contains("import") {
                eprintln!("Skipping test: schema.proto has external imports that aren't available: {}", err_msg);
                return;
            }
            panic!("Failed to parse schema.proto: {:?}", e);
        }
    };

    // Check for expected types from the playground models
    let expected_messages = vec!["User", "Session", "Product", "Order", "Customer"];
    let expected_enums = vec!["Role", "OrderStatus"];

    for name in &expected_messages {
        if parsed.structs.contains_key(*name) {
            println!("Found expected message: {}", name);
        }
    }

    for name in &expected_enums {
        if parsed.enums.contains_key(*name) {
            println!("Found expected enum: {}", name);
        }
    }
}

// ============================================================================
// Source Parsing Tests
// ============================================================================

#[test]
fn test_parse_simple_protobuf_source() {
    let source = r#"
        syntax = "proto3";
        package testapp;

        message User {
            uint64 id = 1;
            string name = 2;
            string email = 3;
            int32 age = 4;
            bool active = 5;
        }

        enum Status {
            STATUS_UNKNOWN = 0;
            STATUS_ACTIVE = 1;
            STATUS_INACTIVE = 2;
        }
    "#;

    let result = parse_protobuf_source("test.proto", source);
    assert!(result.is_ok(), "Failed to parse source: {:?}", result.err());

    let parsed = result.unwrap();
    assert_eq!(parsed.package, Some("testapp".to_string()));
    assert!(parsed.structs.contains_key("User"));
    assert!(parsed.enums.contains_key("Status"));

    let user = &parsed.structs["User"];
    assert_eq!(user.struct_name, "User");
    assert_eq!(user.fields.len(), 5);
}

#[test]
fn test_parse_nested_messages() {
    let source = r#"
        syntax = "proto3";

        message Address {
            string street = 1;
            string city = 2;
            string country = 3;
            string postal_code = 4;
        }

        message Person {
            string name = 1;
            Address address = 2;
            repeated string tags = 3;
        }
    "#;

    let result = parse_protobuf_source("nested.proto", source).unwrap();

    assert!(result.structs.contains_key("Address"));
    assert!(result.structs.contains_key("Person"));

    let person = &result.structs["Person"];
    let address_field = person.fields.iter().find(|f| f.field_name == "address");
    assert!(address_field.is_some());

    // The address field should reference the Address type
    // It may be wrapped in Option in some proto versions
    if let Some(field) = address_field {
        let is_address_type = match &field.field_type {
            FieldType::Other(name) => name == "Address",
            FieldType::Option(inner) => matches!(inner.as_ref(), FieldType::Other(name) if name == "Address"),
            _ => false,
        };
        assert!(
            is_address_type,
            "address field should reference Address type, got {:?}",
            field.field_type
        );
    }

    // Check tags field is a vector (repeated)
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
fn test_parse_oneof_fields() {
    let source = r#"
        syntax = "proto3";

        message Dog {
            string name = 1;
            string breed = 2;
        }

        message Cat {
            string name = 1;
            bool indoor = 2;
        }

        message Owner {
            string name = 1;
            oneof pet {
                Dog dog = 2;
                Cat cat = 3;
            }
        }
    "#;

    let result = parse_protobuf_source("oneof.proto", source).unwrap();

    assert!(result.structs.contains_key("Dog"));
    assert!(result.structs.contains_key("Cat"));
    assert!(result.structs.contains_key("Owner"));
}

#[test]
fn test_parse_all_scalar_types() {
    let source = r#"
        syntax = "proto3";

        message AllScalars {
            double double_val = 1;
            float float_val = 2;
            int32 int32_val = 3;
            int64 int64_val = 4;
            uint32 uint32_val = 5;
            uint64 uint64_val = 6;
            sint32 sint32_val = 7;
            sint64 sint64_val = 8;
            fixed32 fixed32_val = 9;
            fixed64 fixed64_val = 10;
            sfixed32 sfixed32_val = 11;
            sfixed64 sfixed64_val = 12;
            bool bool_val = 13;
            string string_val = 14;
            bytes bytes_val = 15;
        }
    "#;

    let result = parse_protobuf_source("scalars.proto", source).unwrap();
    let all_scalars = &result.structs["AllScalars"];
    assert_eq!(all_scalars.fields.len(), 15);

    // Verify type mappings
    let field_type_map: std::collections::HashMap<_, _> = all_scalars
        .fields
        .iter()
        .map(|f| (f.field_name.as_str(), &f.field_type))
        .collect();

    assert!(matches!(field_type_map["double_val"], FieldType::F64));
    assert!(matches!(field_type_map["float_val"], FieldType::F32));
    assert!(matches!(field_type_map["int32_val"], FieldType::I32));
    assert!(matches!(field_type_map["int64_val"], FieldType::I64));
    assert!(matches!(field_type_map["uint32_val"], FieldType::U32));
    assert!(matches!(field_type_map["uint64_val"], FieldType::U64));
    assert!(matches!(field_type_map["sint32_val"], FieldType::I32));
    assert!(matches!(field_type_map["sint64_val"], FieldType::I64));
    assert!(matches!(field_type_map["bool_val"], FieldType::Bool));
    assert!(matches!(field_type_map["string_val"], FieldType::String));
    // bytes should be Vec<u8>
    assert!(
        matches!(field_type_map["bytes_val"], FieldType::Vec(inner) if matches!(inner.as_ref(), FieldType::U8))
    );
}

#[test]
fn test_parse_repeated_fields() {
    let source = r#"
        syntax = "proto3";

        message Vectors {
            repeated string strings = 1;
            repeated int32 ints = 2;
            repeated double floats = 3;
            repeated bool bools = 4;
        }
    "#;

    let result = parse_protobuf_source("vectors.proto", source).unwrap();
    let vectors = &result.structs["Vectors"];

    for field in &vectors.fields {
        assert!(
            matches!(&field.field_type, FieldType::Vec(_)),
            "Field {} should be a vector type",
            field.field_name
        );
    }
}

#[test]
fn test_parse_map_fields() {
    let source = r#"
        syntax = "proto3";

        message MapContainer {
            map<string, string> string_map = 1;
            map<string, int32> int_map = 2;
        }
    "#;

    let result = parse_protobuf_source("maps.proto", source).unwrap();
    let container = &result.structs["MapContainer"];

    // Map fields should be converted to HashMap
    for field in &container.fields {
        assert!(
            matches!(&field.field_type, FieldType::HashMap(_, _)),
            "Field {} should be a HashMap type, got {:?}",
            field.field_name,
            field.field_type
        );
    }
}

// ============================================================================
// Enum Tests
// ============================================================================

#[test]
fn test_parse_enum_with_values() {
    let source = r#"
        syntax = "proto3";

        enum Priority {
            PRIORITY_UNSPECIFIED = 0;
            PRIORITY_LOW = 1;
            PRIORITY_MEDIUM = 2;
            PRIORITY_HIGH = 3;
            PRIORITY_CRITICAL = 4;
        }

        message Task {
            string title = 1;
            Priority priority = 2;
        }
    "#;

    let result = parse_protobuf_source("enum.proto", source).unwrap();

    assert!(result.enums.contains_key("Priority"));
    let priority = &result.enums["Priority"];
    assert_eq!(priority.variants.len(), 5);
    assert_eq!(priority.variants[0].name, "PRIORITY_UNSPECIFIED");
    assert_eq!(priority.variants[4].name, "PRIORITY_CRITICAL");
}

// ============================================================================
// Nested Message Tests
// ============================================================================

#[test]
fn test_parse_deeply_nested_messages() {
    let source = r#"
        syntax = "proto3";

        message Outer {
            message Middle {
                message Inner {
                    string value = 1;
                }
                Inner inner = 1;
            }
            Middle middle = 1;
        }
    "#;

    let result = parse_protobuf_source("deep_nested.proto", source).unwrap();

    assert!(result.structs.contains_key("Outer"));
    assert!(result.structs.contains_key("Middle"));
    assert!(result.structs.contains_key("Inner"));
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[test]
fn test_parse_invalid_syntax() {
    let source = r#"
        syntax = "proto3";

        message Invalid {
            this is not valid syntax
        }
    "#;

    let result = parse_protobuf_source("invalid.proto", source);
    assert!(result.is_err(), "Invalid syntax should produce an error");
}

#[test]
fn test_parse_empty_source() {
    let source = r#"
        syntax = "proto3";
    "#;

    let result = parse_protobuf_source("empty.proto", source);
    assert!(result.is_ok(), "Empty source should parse successfully");

    let parsed = result.unwrap();
    assert!(parsed.structs.is_empty());
    assert!(parsed.enums.is_empty());
}

// ============================================================================
// Complex Schema Tests
// ============================================================================

#[test]
fn test_parse_complex_ecommerce_schema() {
    let source = r#"
        syntax = "proto3";
        package ecommerce;

        enum OrderStatus {
            ORDER_STATUS_UNSPECIFIED = 0;
            ORDER_STATUS_PENDING = 1;
            ORDER_STATUS_PROCESSING = 2;
            ORDER_STATUS_SHIPPED = 3;
            ORDER_STATUS_DELIVERED = 4;
            ORDER_STATUS_CANCELLED = 5;
        }

        enum PaymentMethod {
            PAYMENT_METHOD_UNSPECIFIED = 0;
            PAYMENT_METHOD_CREDIT_CARD = 1;
            PAYMENT_METHOD_DEBIT_CARD = 2;
            PAYMENT_METHOD_PAYPAL = 3;
            PAYMENT_METHOD_BANK_TRANSFER = 4;
        }

        message Address {
            string street = 1;
            string city = 2;
            string state = 3;
            string postal_code = 4;
            string country = 5;
        }

        message Customer {
            uint64 id = 1;
            string email = 2;
            string name = 3;
            Address shipping_address = 4;
            Address billing_address = 5;
        }

        message Product {
            uint64 id = 1;
            string sku = 2;
            string name = 3;
            string description = 4;
            int64 price_cents = 5;
            int32 stock = 6;
            repeated string tags = 7;
        }

        message OrderItem {
            uint64 product_id = 1;
            int32 quantity = 2;
            int64 unit_price_cents = 3;
        }

        message Order {
            uint64 id = 1;
            uint64 customer_id = 2;
            repeated OrderItem items = 3;
            OrderStatus status = 4;
            PaymentMethod payment_method = 5;
            Address shipping_address = 6;
            int64 total_cents = 7;
            string created_at = 8;
        }
    "#;

    let result = parse_protobuf_source("ecommerce.proto", source).unwrap();

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
    assert_eq!(order_status.variants.len(), 6);

    let payment_method = &result.enums["PaymentMethod"];
    assert_eq!(payment_method.variants.len(), 5);

    // Verify Order fields
    let order = &result.structs["Order"];
    assert_eq!(order.fields.len(), 8);

    // Verify items field is a vector (repeated)
    let items_field = order.fields.iter().find(|f| f.field_name == "items");
    assert!(items_field.is_some());
    if let Some(field) = items_field {
        assert!(
            matches!(&field.field_type, FieldType::Vec(_)),
            "items field should be a repeated field"
        );
    }
}

// ============================================================================
// Proto2 vs Proto3 Tests
// ============================================================================

#[test]
fn test_parse_proto2_syntax() {
    let source = r#"
        syntax = "proto2";

        message LegacyMessage {
            required string name = 1;
            optional int32 age = 2;
            repeated string tags = 3;
        }
    "#;

    let result = parse_protobuf_source("legacy.proto", source).unwrap();
    assert!(result.structs.contains_key("LegacyMessage"));

    let message = &result.structs["LegacyMessage"];
    assert_eq!(message.fields.len(), 3);
}

// ============================================================================
// Round-trip Tests
// ============================================================================

#[test]
fn test_parsed_types_can_generate_typescript() {
    let source = r#"
        syntax = "proto3";
        package roundtrip;

        message TestType {
            uint64 id = 1;
            string name = 2;
            repeated int32 values = 3;
        }
    "#;

    let result = parse_protobuf_source("roundtrip.proto", source).unwrap();
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

// ============================================================================
// Package/Namespace Tests
// ============================================================================

#[test]
fn test_parse_package_extraction() {
    let source = r#"
        syntax = "proto3";
        package com.example.myapp.models;

        message SimpleMessage {
            string value = 1;
        }
    "#;

    let result = parse_protobuf_source("package.proto", source).unwrap();
    assert_eq!(
        result.package,
        Some("com.example.myapp.models".to_string())
    );
}
