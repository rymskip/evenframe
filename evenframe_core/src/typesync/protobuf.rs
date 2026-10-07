//! Protocol Buffers schema generation with validator annotations.
//!
//! This module generates Protocol Buffers `.proto` files (proto3 syntax) with
//! `[(validate.rules).type = {...}]` options at the field level for validators.
//!
//! The validation options follow the protoc-gen-validate style for compatibility
//! with common protobuf validation tooling.

use crate::error::{EvenframeError, Result};
use crate::types::{FieldType, StructConfig, StructField, TaggedUnion, VariantData};
use crate::typesync::doc_comment::format_double_slash;
use crate::typesync::map_key::MapKey;
use crate::typesync::protobuf_rules;
use crate::validator::Validator;
use crate::validator::morph::Morph;
use convert_case::{Case, Casing};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

/// Main entry point for generating Protocol Buffers schema.
///
/// # Arguments
/// * `structs` - Map of struct configurations to generate as messages
/// * `enums` - Map of enum configurations to generate
/// * `package` - Optional package name (e.g., "com.example.app")
/// * `import_validate` - Whether to write protoc-gen-validate rules, importing
///   validate.proto when any field has one
pub fn generate_protobuf_schema_string(
    structs: &BTreeMap<String, StructConfig>,
    enums: &BTreeMap<String, TaggedUnion>,
    package: Option<&str>,
    import_validate: bool,
    registry: &crate::types::ForeignTypeRegistry,
) -> Result<String> {
    tracing::info!(
        struct_count = structs.len(),
        enum_count = enums.len(),
        "Generating Protocol Buffers schema"
    );

    let mut output = String::new();

    // Proto3 syntax declaration
    output.push_str("syntax = \"proto3\";\n\n");

    // Add package if provided
    if let Some(pkg) = package {
        output.push_str(&format!("package {};\n\n", pkg));
    }

    // Deduplicate structs by PascalCase name
    let mut seen_structs = BTreeSet::new();
    let unique_structs: Vec<&StructConfig> = structs
        .values()
        .filter(|s| {
            // `resolve_only` types are registered for resolution but not emitted.
            if s.resolve_only {
                return false;
            }
            let name = s.struct_name.to_case(Case::Pascal);
            if seen_structs.contains(&name) {
                false
            } else {
                seen_structs.insert(name);
                true
            }
        })
        .collect();

    // Deduplicate enums by PascalCase name
    let mut seen_enums = BTreeSet::new();
    let unique_enums: Vec<&TaggedUnion> = enums
        .values()
        .filter(|e| {
            if e.resolve_only {
                return false;
            }
            let name = e.enum_name.to_case(Case::Pascal);
            if seen_enums.contains(&name) {
                false
            } else {
                seen_enums.insert(name);
                true
            }
        })
        .collect();

    let proto = Proto {
        package,
        import_validate,
        registry,
        counts_length: Cell::new(false),
        writes_rules: Cell::new(false),
    };
    let mut body = String::new();

    // Generate enums first (they may be referenced by messages)
    for enum_def in &unique_enums {
        body.push_str(&proto.enum_definition(enum_def.effective())?);
        body.push('\n');
    }

    // Generate messages
    for struct_config in &unique_structs {
        let struct_config = struct_config.effective();
        let mut message = String::new();
        if let Some(ref doc) = struct_config.doccom {
            message.push_str(&format_double_slash(doc, ""));
        }
        message.push_str(&proto.message(
            &struct_config.struct_name.to_case(Case::Pascal),
            &struct_config.fields,
            "",
        )?);
        body.push_str(&message);
        body.push('\n');
    }

    // protoc warns about an import no rule uses.
    if proto.writes_rules.get() {
        output.push_str("import \"validate/validate.proto\";\n\n");
    }
    if proto.counts_length.get() {
        output.push_str(
            "// protoc-gen-validate counts string lengths in code points; the validators count\n\
             // UTF-16 units, so the two differ for characters outside the Basic Multilingual Plane.\n\n",
        );
    }
    output.push_str(&body);

    tracing::info!(
        output_length = output.len(),
        "Protocol Buffers schema generation complete"
    );
    Ok(output)
}

/// A scalar field type's proto3 type.
fn scalar(field_type: &FieldType) -> Result<&'static str> {
    Ok(match field_type {
        FieldType::String | FieldType::Char => "string",
        FieldType::Bool => "bool",
        FieldType::F32 => "float",
        FieldType::F64 => "double",
        FieldType::I8 | FieldType::I16 | FieldType::I32 => "int32",
        FieldType::I64 | FieldType::Isize => "int64",
        FieldType::U8 | FieldType::U16 | FieldType::U32 => "uint32",
        FieldType::U64 | FieldType::Usize => "uint64",
        // proto3 has no 128-bit integers; the decimal text is exact.
        FieldType::I128 | FieldType::U128 => "string",
        other => {
            return Err(EvenframeError::type_sync(format!(
                "{other:?} is not a protobuf scalar"
            )));
        }
    })
}

/// Renders proto3 definitions. Every reference to a scanned type is fully
/// qualified, so the nested messages a definition declares never shadow one.
struct Proto<'a> {
    package: Option<&'a str>,
    import_validate: bool,
    registry: &'a crate::types::ForeignTypeRegistry,
    /// Whether any string length rule was written.
    counts_length: Cell<bool>,
    /// Whether any field has a protoc-gen-validate rule.
    writes_rules: Cell<bool>,
}

/// How a field holds its value.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Label {
    Single,
    Optional,
    Repeated,
    Map,
}

/// A field's label and type, before its name and number.
struct ProtoField {
    label: Label,
    type_name: String,
    /// False when the field's validators moved into the message wrapping it.
    holds_validators: bool,
}

impl ProtoField {
    fn single(type_name: String) -> Self {
        Self {
            label: Label::Single,
            type_name,
            holds_validators: true,
        }
    }

    fn declaration(&self, name: &str, number: usize) -> String {
        let label = match self.label {
            Label::Single | Label::Map => "",
            Label::Optional => "optional ",
            Label::Repeated => "repeated ",
        };
        format!("{label}{} {name} = {number}", self.type_name)
    }
}

/// The nested messages one message declares, by name.
#[derive(Default)]
struct Nested {
    definitions: Vec<String>,
    names: BTreeSet<String>,
}

impl Nested {
    fn declare(&mut self, owner: &str, name: &str, definition: String) -> Result<()> {
        if !self.names.insert(name.to_string()) {
            return Err(EvenframeError::type_sync(format!(
                "protobuf message `{owner}` needs two nested messages named `{name}`; rename the \
                 field or variant one of them comes from"
            )));
        }
        self.definitions.push(definition);
        Ok(())
    }
}

impl Proto<'_> {
    /// A scanned type by its fully qualified name.
    fn user_type(&self, name: &str) -> String {
        match self.package {
            Some(package) => format!(".{package}.{}", name.to_case(Case::Pascal)),
            None => format!(".{}", name.to_case(Case::Pascal)),
        }
    }

    /// A message named `name` with `fields`, and the nested messages they need.
    fn message(&self, name: &str, fields: &[StructField], indent: &str) -> Result<String> {
        let mut nested = Nested::default();
        let mut body = String::new();
        for (index, field) in fields.iter().enumerate() {
            let field = field.effective();
            if let Some(ref doc) = field.doccom {
                body.push_str(&format_double_slash(doc, &format!("{indent}    ")));
            }
            let field_name = field.field_name.to_case(Case::Snake);
            let proto_field = self.field(
                &field.field_type,
                &field.field_name.to_case(Case::Pascal),
                name,
                &mut nested,
                &format!("{indent}    "),
                &field.validators,
            )?;
            let mut declaration = proto_field.declaration(&field_name, index + 1);
            if self.import_validate && !field.morphs.is_empty() {
                let steps = field
                    .morphs
                    .iter()
                    .map(Morph::endec_step)
                    .collect::<std::result::Result<Vec<_>, String>>()
                    .map_err(|problem| {
                        EvenframeError::type_sync(format!(
                            "`{name}.{}`: {problem}",
                            field.field_name
                        ))
                    })?;
                body.push_str(&format!(
                    "{indent}    // protoc-gen-validate cannot normalize: {}\n",
                    steps.join("; ")
                ));
            }
            if self.import_validate && proto_field.holds_validators {
                let rules = protobuf_rules::field_rules(
                    &format!("{name}.{}", field.field_name),
                    &field.validators,
                    &field.field_type,
                    self.registry,
                )?;
                if !rules.unenforced.is_empty() {
                    body.push_str(&format!(
                        "{indent}    // protoc-gen-validate cannot check: {}\n",
                        rules.unenforced.join("; ")
                    ));
                }
                if let Some(option) = rules.option {
                    declaration.push_str(&format!(" [{option}]"));
                    self.writes_rules.set(true);
                }
                if rules.counts_length {
                    self.counts_length.set(true);
                }
            }
            body.push_str(&format!("{indent}    {declaration};\n"));
        }
        Ok(format!(
            "{indent}message {name} {{\n{}{body}{indent}}}\n",
            nested.definitions.concat()
        ))
    }

    /// A field of `field_type`. `hint` names any nested message it needs, and
    /// a wrapper takes `validators` along to the value it holds.
    fn field(
        &self,
        field_type: &FieldType,
        hint: &str,
        owner: &str,
        nested: &mut Nested,
        indent: &str,
        validators: &[Validator],
    ) -> Result<ProtoField> {
        Ok(match field_type {
            // `optional` cannot hold a repeated, map or optional field, so the
            // value is wrapped to keep `None` distinct.
            FieldType::Option(inner)
                if matches!(
                    **inner,
                    FieldType::Option(_)
                        | FieldType::Vec(_)
                        | FieldType::HashMap(..)
                        | FieldType::BTreeMap(..)
                ) =>
            {
                ProtoField {
                    label: Label::Optional,
                    type_name: self.wrapper(inner, hint, owner, nested, indent, validators)?,
                    holds_validators: false,
                }
            }
            FieldType::Option(inner) => ProtoField {
                label: Label::Optional,
                type_name: self.single(inner, hint, owner, nested, indent)?,
                holds_validators: true,
            },
            FieldType::Vec(inner) => ProtoField {
                label: Label::Repeated,
                type_name: self.single(inner, &format!("{hint}Item"), owner, nested, indent)?,
                holds_validators: true,
            },
            FieldType::HashMap(key, value) | FieldType::BTreeMap(key, value) => ProtoField {
                label: Label::Map,
                type_name: format!(
                    "map<{}, {}>",
                    self.map_key(key)?,
                    self.single(value, &format!("{hint}Value"), owner, nested, indent)?
                ),
                holds_validators: true,
            },
            _ => ProtoField::single(self.single(field_type, hint, owner, nested, indent)?),
        })
    }

    /// `field_type` where proto3 allows exactly one value: a map value, a
    /// repeated element or a oneof member.
    fn single(
        &self,
        field_type: &FieldType,
        hint: &str,
        owner: &str,
        nested: &mut Nested,
        indent: &str,
    ) -> Result<String> {
        Ok(match field_type {
            FieldType::Option(_)
            | FieldType::Vec(_)
            | FieldType::HashMap(..)
            | FieldType::BTreeMap(..) => {
                self.wrapper(field_type, hint, owner, nested, indent, &[])?
            }
            FieldType::Unit => {
                nested.declare(
                    owner,
                    hint,
                    format!("{indent}message {hint} {{\n{indent}}}\n"),
                )?;
                hint.to_string()
            }
            FieldType::Tuple(items) => {
                let fields: Vec<StructField> = items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| StructField {
                        field_name: format!("item{index}"),
                        field_type: item.clone(),
                        ..Default::default()
                    })
                    .collect();
                nested.declare(owner, hint, self.message(hint, &fields, indent)?)?;
                hint.to_string()
            }
            FieldType::Struct(members) => {
                let fields: Vec<StructField> = members
                    .iter()
                    .map(|(name, member)| StructField {
                        field_name: name.clone(),
                        field_type: member.clone(),
                        ..Default::default()
                    })
                    .collect();
                nested.declare(owner, hint, self.message(hint, &fields, indent)?)?;
                hint.to_string()
            }
            FieldType::Duration => {
                self.single(&FieldType::serde_duration(), hint, owner, nested, indent)?
            }
            FieldType::RecordLink(inner) => self.single(inner, hint, owner, nested, indent)?,
            // A text form is held as the value its text writes, and an
            // instant as its epoch milliseconds.
            FieldType::FromText(kind) => {
                self.single(kind.value_type(), hint, owner, nested, indent)?
            }
            FieldType::JsonText(inner) => self.single(inner, hint, owner, nested, indent)?,
            FieldType::IsoDate | FieldType::EpochMillis => "int64".to_string(),
            FieldType::Other(name) => match self.registry.lookup(name) {
                Some(foreign) if !foreign.protobuf.is_empty() => foreign.protobuf.clone(),
                _ => self.user_type(name),
            },
            scalar_type => scalar(scalar_type)?.to_string(),
        })
    }

    /// A nested message holding `field_type` as its one `value` field, with
    /// `validators` on it.
    fn wrapper(
        &self,
        field_type: &FieldType,
        hint: &str,
        owner: &str,
        nested: &mut Nested,
        indent: &str,
        validators: &[Validator],
    ) -> Result<String> {
        let value = StructField {
            field_name: "value".to_string(),
            field_type: field_type.clone(),
            validators: validators.to_vec(),
            ..Default::default()
        };
        nested.declare(
            owner,
            hint,
            self.message(hint, std::slice::from_ref(&value), indent)?,
        )?;
        Ok(hint.to_string())
    }

    /// A map key as a proto3 map key type: an integer, a bool or a string.
    fn map_key(&self, key: &FieldType) -> Result<String> {
        const KEY_TYPES: [&str; 12] = [
            "string", "bool", "int32", "int64", "uint32", "uint64", "sint32", "sint64", "fixed32",
            "fixed64", "sfixed32", "sfixed64",
        ];
        match MapKey::require(key)? {
            MapKey::Named(name) => match self.registry.lookup(name) {
                Some(foreign) if KEY_TYPES.contains(&foreign.protobuf.as_str()) => {
                    Ok(foreign.protobuf.clone())
                }
                Some(foreign) => Err(EvenframeError::type_sync(format!(
                    "map key `{name}` maps to the protobuf type {:?}, but a proto3 map key must \
                     be an integer, bool or string type",
                    foreign.protobuf
                ))),
                // proto3 forbids enum keys; the key is the variant name serde writes.
                None => Ok("string".to_string()),
            },
            MapKey::Text | MapKey::Char | MapKey::Integer | MapKey::Bool => {
                Ok(scalar(key)?.to_string())
            }
        }
    }

    /// An enum: a proto3 enum when every variant is a unit variant, else a
    /// message whose oneof holds one member per variant.
    fn enum_definition(&self, enum_def: &TaggedUnion) -> Result<String> {
        let name = enum_def.enum_name.to_case(Case::Pascal);
        let mut output = String::new();
        if let Some(ref doc) = enum_def.doccom {
            output.push_str(&format_double_slash(doc, ""));
        }
        let variants: Vec<_> = enum_def.variants.iter().map(|v| v.effective()).collect();

        if variants.iter().all(|variant| variant.data.is_none()) {
            // Proto3 requires first value to be 0 (UNSPECIFIED)
            let enum_prefix = name.to_case(Case::UpperSnake);
            output.push_str(&format!("enum {} {{\n", name));
            output.push_str(&format!("    {}_UNSPECIFIED = 0;\n", enum_prefix));
            for (i, variant) in variants.iter().enumerate() {
                output.push_str(&format!(
                    "    {}_{} = {};\n",
                    enum_prefix,
                    variant.name.to_case(Case::UpperSnake),
                    i + 1
                ));
            }
            output.push_str("}\n");
            return Ok(output);
        }

        let mut nested = Nested::default();
        let mut members = String::new();
        for (i, variant) in variants.iter().enumerate() {
            let variant_name = variant.name.to_case(Case::Pascal);
            let member_type = match &variant.data {
                None => {
                    nested.declare(
                        &name,
                        &variant_name,
                        format!("    message {variant_name} {{\n    }}\n"),
                    )?;
                    variant_name.clone()
                }
                Some(VariantData::InlineStruct(inline)) => {
                    nested.declare(
                        &name,
                        &variant_name,
                        self.message(&variant_name, &inline.fields, "    ")?,
                    )?;
                    variant_name.clone()
                }
                Some(VariantData::DataStructureRef(field_type)) => {
                    self.single(field_type, &variant_name, &name, &mut nested, "    ")?
                }
            };
            members.push_str(&format!(
                "        {member_type} {} = {};\n",
                variant.name.to_case(Case::Snake),
                i + 1
            ));
        }
        output.push_str(&format!(
            "message {name} {{\n{}    oneof variant {{\n{members}    }}\n}}\n",
            nested.definitions.concat()
        ));
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BTreeMap, Cell, FieldType, Nested, Proto, StructConfig, TaggedUnion, Validator,
        generate_protobuf_schema_string, scalar,
    };
    use crate::types::{EnumRepresentation, StructField};
    use crate::validator::{NumberValidator, StringValidator};
    use ordered_float::OrderedFloat;

    fn proto(registry: &crate::types::ForeignTypeRegistry) -> Proto<'_> {
        Proto {
            package: None,
            import_validate: false,
            registry,
            counts_length: Cell::new(false),
            writes_rules: Cell::new(false),
        }
    }

    fn render(field_type: FieldType) -> (String, String) {
        let registry = crate::types::ForeignTypeRegistry::default();
        let proto = proto(&registry);
        let mut nested = Nested::default();
        let field = proto
            .field(&field_type, "Field", "Owner", &mut nested, "", &[])
            .unwrap();
        (field.declaration("field", 1), nested.definitions.concat())
    }

    #[test]
    fn scalars_map_to_proto3_types() {
        for (field_type, expected) in [
            (FieldType::String, "string"),
            (FieldType::Char, "string"),
            (FieldType::Bool, "bool"),
            (FieldType::I8, "int32"),
            (FieldType::I16, "int32"),
            (FieldType::I32, "int32"),
            (FieldType::I64, "int64"),
            (FieldType::Isize, "int64"),
            (FieldType::I128, "string"),
            (FieldType::U8, "uint32"),
            (FieldType::U16, "uint32"),
            (FieldType::U32, "uint32"),
            (FieldType::U64, "uint64"),
            (FieldType::Usize, "uint64"),
            (FieldType::U128, "string"),
            (FieldType::F32, "float"),
            (FieldType::F64, "double"),
        ] {
            assert_eq!(scalar(&field_type).unwrap(), expected, "{field_type:?}");
        }
    }

    #[test]
    fn labels_follow_the_field_shape() {
        assert_eq!(
            render(FieldType::Vec(Box::new(FieldType::String))).0,
            "repeated string field = 1"
        );
        assert_eq!(
            render(FieldType::Option(Box::new(FieldType::String))).0,
            "optional string field = 1"
        );
        assert_eq!(
            render(FieldType::HashMap(
                Box::new(FieldType::String),
                Box::new(FieldType::I32)
            ))
            .0,
            "map<string, int32> field = 1"
        );
    }

    #[test]
    fn scanned_types_are_fully_qualified() {
        assert_eq!(
            render(FieldType::Other("user_profile".to_string())).0,
            ".UserProfile field = 1"
        );
        let registry = crate::types::ForeignTypeRegistry::default();
        let proto = Proto {
            package: Some("com.example"),
            ..proto(&registry)
        };
        assert_eq!(proto.user_type("UserProfile"), ".com.example.UserProfile");
    }

    #[test]
    fn shapes_proto3_cannot_write_directly_are_wrapped() {
        let (declaration, nested) = render(FieldType::Option(Box::new(FieldType::Vec(Box::new(
            FieldType::String,
        )))));
        assert_eq!(declaration, "optional Field field = 1");
        assert!(nested.contains("message Field {"), "{nested}");
        assert!(nested.contains("repeated string value = 1;"), "{nested}");

        let (declaration, nested) = render(FieldType::Vec(Box::new(FieldType::Vec(Box::new(
            FieldType::I32,
        )))));
        assert_eq!(declaration, "repeated FieldItem field = 1");
        assert!(nested.contains("repeated int32 value = 1;"), "{nested}");

        let (declaration, nested) = render(FieldType::BTreeMap(
            Box::new(FieldType::U8),
            Box::new(FieldType::BTreeMap(
                Box::new(FieldType::U8),
                Box::new(FieldType::Bool),
            )),
        ));
        assert_eq!(declaration, "map<uint32, FieldValue> field = 1");
        assert!(nested.contains("map<uint32, bool> value = 1;"), "{nested}");

        let (declaration, nested) =
            render(FieldType::Tuple(vec![FieldType::String, FieldType::I64]));
        assert_eq!(declaration, "Field field = 1");
        assert!(nested.contains("string item_0 = 1;"), "{nested}");
        assert!(nested.contains("int64 item_1 = 2;"), "{nested}");
    }

    #[test]
    fn enum_keys_become_string_keys() {
        assert_eq!(
            render(FieldType::HashMap(
                Box::new(FieldType::Other("Role".to_string())),
                Box::new(FieldType::String)
            ))
            .0,
            "map<string, string> field = 1"
        );
    }

    #[test]
    fn a_float_map_key_is_rejected() {
        let registry = crate::types::ForeignTypeRegistry::default();
        let proto = proto(&registry);
        let error = proto
            .field(
                &FieldType::HashMap(Box::new(FieldType::F64), Box::new(FieldType::String)),
                "Field",
                "Owner",
                &mut Nested::default(),
                "",
                &[],
            )
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(
            error.contains("a map keyed by `f64` cannot exist: f32 and f64 implement neither Hash"),
            "{error}"
        );
    }

    #[test]
    fn nested_names_that_collide_are_rejected() {
        let registry = crate::types::ForeignTypeRegistry::default();
        let proto = proto(&registry);
        let fields = vec![
            StructField {
                field_name: "pair".to_string(),
                field_type: FieldType::Tuple(vec![FieldType::I32]),
                ..Default::default()
            },
            StructField {
                field_name: "Pair".to_string(),
                field_type: FieldType::Tuple(vec![FieldType::I64]),
                ..Default::default()
            },
        ];
        let error = proto
            .message("Owner", &fields, "")
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(
            error.contains("two nested messages named `Pair`"),
            "{error}"
        );
    }

    #[test]
    fn test_generate_simple_message() {
        let mut structs = BTreeMap::new();
        structs.insert(
            "user".to_string(),
            StructConfig {
                resolve_only: false,
                struct_name: "user".to_string(),
                fields: vec![
                    StructField {
                        field_name: "email".to_string(),
                        field_type: FieldType::String,
                        validators: vec![Validator::StringValidator(StringValidator::Email)],
                        ..Default::default()
                    },
                    StructField {
                        field_name: "age".to_string(),
                        field_type: FieldType::I32,
                        validators: vec![Validator::NumberValidator(NumberValidator::Between(
                            OrderedFloat(18.0),
                            OrderedFloat(120.0),
                        ))],
                        ..Default::default()
                    },
                ],
                doccom: None,
                macroforge_derives: vec![],
                annotations: vec![],
                pipeline: crate::types::Pipeline::default(),
                rust_derives: vec![],
                output_override: None,
                raw_attributes: std::collections::BTreeMap::new(),
            },
        );

        let output = generate_protobuf_schema_string(
            &structs,
            &BTreeMap::new(),
            None,
            true,
            &crate::types::ForeignTypeRegistry::default(),
        )
        .unwrap();

        assert!(output.contains("syntax = \"proto3\";"));
        assert!(output.contains("message User"));
        assert!(output.contains("string email = 1"));
        assert!(output.contains("(validate.rules).string = {pattern: \"^[0-9A-Za-z_%+.-]+@"));
        assert!(output.contains("int32 age = 2"));
        assert!(output.contains("gte: 18, lte: 120"));
    }

    #[test]
    fn test_generate_message_with_package() {
        let output = generate_protobuf_schema_string(
            &BTreeMap::new(),
            &BTreeMap::new(),
            Some("com.example.app"),
            false,
            &crate::types::ForeignTypeRegistry::default(),
        )
        .unwrap();
        assert!(output.contains("package com.example.app;"));
    }

    #[test]
    fn validate_is_imported_only_when_a_rule_uses_it() {
        let output = generate_protobuf_schema_string(
            &BTreeMap::new(),
            &BTreeMap::new(),
            None,
            true,
            &crate::types::ForeignTypeRegistry::default(),
        )
        .unwrap();
        assert!(!output.contains("validate/validate.proto"), "{output}");
    }

    #[test]
    fn test_generate_simple_enum() {
        use crate::types::Variant;

        let mut enums = BTreeMap::new();
        enums.insert(
            "Status".to_string(),
            TaggedUnion {
                resolve_only: false,
                enum_name: "Status".to_string(),
                variants: vec![
                    Variant {
                        name: "Active".to_string(),
                        wire: Default::default(),
                        data: None,
                        doccom: None,
                        annotations: vec![],
                        output_override: None,
                        raw_attributes: std::collections::BTreeMap::new(),
                        is_default: false,
                        element_morphs: Vec::new(),
                        element_validators: Vec::new(),
                        element_validator_overrides: Vec::new(),
                    },
                    Variant {
                        name: "Inactive".to_string(),
                        wire: Default::default(),
                        data: None,
                        doccom: None,
                        annotations: vec![],
                        output_override: None,
                        raw_attributes: std::collections::BTreeMap::new(),
                        is_default: false,
                        element_morphs: Vec::new(),
                        element_validators: Vec::new(),
                        element_validator_overrides: Vec::new(),
                    },
                    Variant {
                        name: "Pending".to_string(),
                        wire: Default::default(),
                        data: None,
                        doccom: None,
                        annotations: vec![],
                        output_override: None,
                        raw_attributes: std::collections::BTreeMap::new(),
                        is_default: false,
                        element_morphs: Vec::new(),
                        element_validators: Vec::new(),
                        element_validator_overrides: Vec::new(),
                    },
                ],
                representation: EnumRepresentation::default(),
                doccom: None,
                macroforge_derives: vec![],
                annotations: vec![],
                pipeline: crate::types::Pipeline::default(),
                rust_derives: vec![],
                output_override: None,
                raw_attributes: std::collections::BTreeMap::new(),
            },
        );

        let output = generate_protobuf_schema_string(
            &BTreeMap::new(),
            &enums,
            None,
            false,
            &crate::types::ForeignTypeRegistry::default(),
        )
        .unwrap();

        assert!(output.contains("enum Status"));
        assert!(output.contains("STATUS_UNSPECIFIED = 0;"));
        assert!(output.contains("STATUS_ACTIVE = 1"));
        assert!(output.contains("STATUS_INACTIVE = 2"));
        assert!(output.contains("STATUS_PENDING = 3"));
    }

    #[test]
    fn test_generate_complete_schema() {
        use crate::types::Variant;

        let mut structs = BTreeMap::new();
        structs.insert(
            "user_registration_form".to_string(),
            StructConfig {
                resolve_only: false,
                struct_name: "user_registration_form".to_string(),
                fields: vec![
                    StructField {
                        field_name: "email".to_string(),
                        field_type: FieldType::String,
                        validators: vec![
                            Validator::StringValidator(StringValidator::Email),
                            Validator::StringValidator(StringValidator::NonEmpty),
                        ],
                        ..Default::default()
                    },
                    StructField {
                        field_name: "password".to_string(),
                        field_type: FieldType::String,
                        validators: vec![
                            Validator::StringValidator(StringValidator::MinLength(8)),
                            Validator::StringValidator(StringValidator::MaxLength(50)),
                        ],
                        ..Default::default()
                    },
                    StructField {
                        field_name: "age".to_string(),
                        field_type: FieldType::I32,
                        validators: vec![Validator::NumberValidator(NumberValidator::Between(
                            OrderedFloat(18.0),
                            OrderedFloat(120.0),
                        ))],
                        ..Default::default()
                    },
                    StructField {
                        field_name: "tags".to_string(),
                        field_type: FieldType::Vec(Box::new(FieldType::String)),
                        validators: vec![],
                        ..Default::default()
                    },
                ],
                doccom: None,
                macroforge_derives: vec![],
                annotations: vec![],
                pipeline: crate::types::Pipeline::default(),
                rust_derives: vec![],
                output_override: None,
                raw_attributes: std::collections::BTreeMap::new(),
            },
        );

        let mut enums = BTreeMap::new();
        enums.insert(
            "Role".to_string(),
            TaggedUnion {
                resolve_only: false,
                enum_name: "Role".to_string(),
                variants: vec![
                    Variant {
                        name: "Admin".to_string(),
                        wire: Default::default(),
                        data: None,
                        doccom: None,
                        annotations: vec![],
                        output_override: None,
                        raw_attributes: std::collections::BTreeMap::new(),
                        is_default: false,
                        element_morphs: Vec::new(),
                        element_validators: Vec::new(),
                        element_validator_overrides: Vec::new(),
                    },
                    Variant {
                        name: "User".to_string(),
                        wire: Default::default(),
                        data: None,
                        doccom: None,
                        annotations: vec![],
                        output_override: None,
                        raw_attributes: std::collections::BTreeMap::new(),
                        is_default: false,
                        element_morphs: Vec::new(),
                        element_validators: Vec::new(),
                        element_validator_overrides: Vec::new(),
                    },
                ],
                representation: EnumRepresentation::default(),
                doccom: None,
                macroforge_derives: vec![],
                annotations: vec![],
                pipeline: crate::types::Pipeline::default(),
                rust_derives: vec![],
                output_override: None,
                raw_attributes: std::collections::BTreeMap::new(),
            },
        );

        let output = generate_protobuf_schema_string(
            &structs,
            &enums,
            Some("com.example.users"),
            true,
            &crate::types::ForeignTypeRegistry::default(),
        )
        .unwrap();

        // Check syntax and package
        assert!(output.contains("syntax = \"proto3\";"));
        assert!(output.contains("package com.example.users;"));
        assert!(output.contains("import \"validate/validate.proto\";"));

        // Check enum
        assert!(output.contains("enum Role"));
        assert!(output.contains("ROLE_UNSPECIFIED = 0;"));
        assert!(output.contains("ROLE_ADMIN = 1"));
        assert!(output.contains("ROLE_USER = 2"));

        // Check message
        assert!(output.contains("message UserRegistrationForm"));
        assert!(output.contains("string email = 1"));
        assert!(output.contains("string password = 2"));
        assert!(output.contains("int32 age = 3"));
        assert!(output.contains("repeated string tags = 4"));
    }
}
