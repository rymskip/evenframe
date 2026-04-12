//! Testing utilities for output rule plugins.
//!
//! Runs plugin output through the actual macroforge generator to verify
//! the generated TypeScript is correct.

use crate::types::{ForeignTypeRegistry, StructConfig, StructField, TaggedUnion};
use crate::typesync::macroforge::generate_macroforge_type_string;
use std::collections::HashMap;

/// Result of running a plugin through the generator pipeline.
#[derive(Debug)]
pub struct GeneratedOutput {
    pub full_output: String,
}

impl GeneratedOutput {
    /// Assert the output contains a string.
    pub fn assert_contains(&self, expected: &str) {
        assert!(
            self.full_output.contains(expected),
            "Expected output to contain:\n  {}\n\nActual output:\n{}",
            expected,
            self.full_output
        );
    }

    /// Assert the output does NOT contain a string.
    pub fn assert_not_contains(&self, unexpected: &str) {
        assert!(
            !self.full_output.contains(unexpected),
            "Expected output NOT to contain:\n  {}\n\nActual output:\n{}",
            unexpected,
            self.full_output
        );
    }
}

/// Run a StructConfig through the actual macroforge generator.
/// The `override_config` is set as the struct's `output_override`.
pub fn generate_struct_with_override(
    name: &str,
    override_config: Option<Box<StructConfig>>,
    field_overrides: HashMap<String, Box<StructField>>,
    fields: Vec<StructField>,
) -> GeneratedOutput {
    let mut struct_config = StructConfig {
        struct_name: name.to_string(),
        fields,
        validators: vec![],
        doccom: None,
        macroforge_derives: vec![],
        annotations: vec![],
        pipeline: crate::types::Pipeline::Both,
        rust_derives: vec![],
        output_override: override_config,
        raw_attributes: HashMap::new(),
    };

    for (field_name, ov) in field_overrides {
        if let Some(field) = struct_config
            .fields
            .iter_mut()
            .find(|f| f.field_name == field_name)
        {
            field.output_override = Some(ov);
        }
    }

    let mut structs = HashMap::new();
    structs.insert(name.to_string(), struct_config);
    let enums: HashMap<String, TaggedUnion> = HashMap::new();
    let registry = ForeignTypeRegistry::default();

    let output = generate_macroforge_type_string(
        &structs,
        &enums,
        false,
        crate::typesync::config::ArrayStyle::default(),
        &registry,
    );
    GeneratedOutput {
        full_output: output,
    }
}

/// Run a TaggedUnion through the actual macroforge generator.
pub fn generate_enum_with_override(
    name: &str,
    override_config: Option<Box<TaggedUnion>>,
) -> GeneratedOutput {
    let enum_config = TaggedUnion {
        enum_name: name.to_string(),
        variants: vec![],
        representation: crate::types::EnumRepresentation::default(),
        doccom: None,
        macroforge_derives: vec![],
        annotations: vec![],
        pipeline: crate::types::Pipeline::Both,
        rust_derives: vec![],
        output_override: override_config,
        raw_attributes: HashMap::new(),
    };

    let structs: HashMap<String, StructConfig> = HashMap::new();
    let mut enums = HashMap::new();
    enums.insert(name.to_string(), enum_config);
    let registry = ForeignTypeRegistry::default();

    let output = generate_macroforge_type_string(
        &structs,
        &enums,
        false,
        crate::typesync::config::ArrayStyle::default(),
        &registry,
    );
    GeneratedOutput {
        full_output: output,
    }
}

/// Create a simple StructField for testing.
pub fn test_field(name: &str, field_type: crate::types::FieldType) -> StructField {
    StructField {
        field_name: name.to_string(),
        field_type,
        edge_config: None,
        define_config: None,
        format: None,
        validators: vec![],
        always_regenerate: false,
        doccom: None,
        annotations: vec![],
        unique: false,
        mock_plugin: None,
        output_override: None,
        raw_attributes: HashMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn override_preserves_struct_body() {
        let override_config = StructConfig {
            struct_name: "Site".to_string(),
            fields: vec![],
            validators: vec![],
            doccom: None,
            macroforge_derives: vec![
                "Default".into(),
                "Serialize".into(),
                "Deserialize".into(),
                "Gigaform".into(),
                "Overview".into(),
            ],
            annotations: vec!["@overview({ dataName: \"site\" })".into()],
            pipeline: crate::types::Pipeline::Both,
            rust_derives: vec![],
            output_override: None,
            raw_attributes: HashMap::new(),
        };

        let output = generate_struct_with_override(
            "Site",
            Some(Box::new(override_config)),
            HashMap::new(),
            vec![
                test_field("id", crate::types::FieldType::String),
                test_field("name", crate::types::FieldType::String),
            ],
        );

        output.assert_contains("@derive(Default, Serialize, Deserialize, Gigaform, Overview)");
        output.assert_contains("@overview");
        output.assert_contains("export interface Site {");
        output.assert_contains("id: string");
        output.assert_contains("name: string");
    }

    #[test]
    fn field_override_preserves_declaration() {
        let mut field_overrides: HashMap<String, Box<StructField>> = HashMap::new();
        let mut email_override = test_field("email", crate::types::FieldType::String);
        email_override.annotations = vec!["@textController({ label: \"Email\" })".into()];
        field_overrides.insert("email".to_string(), Box::new(email_override));

        let output = generate_struct_with_override(
            "User",
            None,
            field_overrides,
            vec![test_field("email", crate::types::FieldType::String)],
        );

        output.assert_contains("@textController");
        output.assert_contains("email: string");
    }

    #[test]
    fn no_override_falls_back_to_default_derive() {
        let output = generate_struct_with_override(
            "Foo",
            None,
            HashMap::new(),
            vec![test_field("bar", crate::types::FieldType::String)],
        );

        output.assert_contains("@derive(Deserialize)");
        output.assert_contains("export interface Foo {");
    }
}
