//! Protocol Buffers schema generation with validator annotations.
//!
//! This module generates Protocol Buffers `.proto` files (proto3 syntax) with
//! `[(validate.rules).type = {...}]` options at the field level for validators.
//!
//! The validation options follow the protoc-gen-validate style for compatibility
//! with common protobuf validation tooling.

use crate::types::{FieldType, StructConfig, TaggedUnion, VariantData};
use crate::validator::{
    ArrayValidator, BigDecimalValidator, BigIntValidator, DateValidator, DurationValidator,
    NumberValidator, StringValidator, Validator,
};
use convert_case::{Case, Casing};
use std::collections::{HashMap, HashSet};

/// Main entry point for generating Protocol Buffers schema.
///
/// # Arguments
/// * `structs` - Map of struct configurations to generate as messages
/// * `enums` - Map of enum configurations to generate
/// * `package` - Optional package name (e.g., "com.example.app")
/// * `import_validate` - Whether to import the validate.proto file for validation rules
pub fn generate_protobuf_schema_string(
    structs: &HashMap<String, StructConfig>,
    enums: &HashMap<String, TaggedUnion>,
    package: Option<&str>,
    import_validate: bool,
) -> String {
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

    // Import validate.proto if validation rules are being used
    if import_validate {
        output.push_str("import \"validate/validate.proto\";\n\n");
    }

    // Deduplicate structs by PascalCase name
    let mut seen_structs = HashSet::new();
    let unique_structs: Vec<&StructConfig> = structs
        .values()
        .filter(|s| {
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
    let mut seen_enums = HashSet::new();
    let unique_enums: Vec<&TaggedUnion> = enums
        .values()
        .filter(|e| {
            let name = e.enum_name.to_case(Case::Pascal);
            if seen_enums.contains(&name) {
                false
            } else {
                seen_enums.insert(name);
                true
            }
        })
        .collect();

    // Generate enums first (they may be referenced by messages)
    for enum_def in &unique_enums {
        output.push_str(&generate_enum(enum_def));
        output.push('\n');
    }

    // Generate messages
    for struct_config in &unique_structs {
        output.push_str(&generate_message(struct_config, import_validate));
        output.push('\n');
    }

    tracing::info!(
        output_length = output.len(),
        "Protocol Buffers schema generation complete"
    );
    output
}

/// Generate a Protocol Buffers enum from a TaggedUnion.
/// Proto3 enums require the first value to be 0 (UNSPECIFIED).
fn generate_enum(enum_def: &TaggedUnion) -> String {
    let name = enum_def.enum_name.to_case(Case::Pascal);

    // Check if this is a simple enum (no data variants) or needs to be a oneof
    let has_data_variants = enum_def.variants.iter().any(|v| v.data.is_some());

    if has_data_variants {
        // Generate as a message with oneof for variants with data
        let mut output = format!("message {} {{\n", name);
        output.push_str("    oneof variant {\n");
        for (i, variant) in enum_def.variants.iter().enumerate() {
            let variant_name = variant.name.to_case(Case::Snake);
            if let Some(data) = &variant.data {
                let type_name = match data {
                    VariantData::InlineStruct(s) => s.struct_name.to_case(Case::Pascal),
                    VariantData::DataStructureRef(ft) => field_type_to_protobuf(ft),
                };
                output.push_str(&format!(
                    "        {} {} = {};\n",
                    type_name,
                    variant_name,
                    i + 1
                ));
            } else {
                // Simple variant becomes a bool marker
                output.push_str(&format!("        bool {} = {};\n", variant_name, i + 1));
            }
        }
        output.push_str("    }\n");
        output.push_str("}\n");
        output
    } else {
        // Generate as a simple proto3 enum
        let enum_prefix = name.to_case(Case::UpperSnake);
        let mut output = format!("enum {} {{\n", name);

        // Proto3 requires first value to be 0 (UNSPECIFIED)
        output.push_str(&format!("    {}_UNSPECIFIED = 0;\n", enum_prefix));

        for (i, variant) in enum_def.variants.iter().enumerate() {
            let variant_name = format!(
                "{}_{}",
                enum_prefix,
                variant.name.to_case(Case::UpperSnake)
            );
            output.push_str(&format!("    {} = {};\n", variant_name, i + 1));
        }
        output.push_str("}\n");
        output
    }
}

/// Generate a Protocol Buffers message from a StructConfig.
fn generate_message(struct_config: &StructConfig, include_validators: bool) -> String {
    let name = struct_config.struct_name.to_case(Case::Pascal);
    let mut output = format!("message {} {{\n", name);

    for (index, field) in struct_config.fields.iter().enumerate() {
        let field_number = index + 1;
        let field_name = field.field_name.to_case(Case::Snake);
        let (field_prefix, field_type) = field_type_to_protobuf_with_prefix(&field.field_type);

        output.push_str(&format!("    {}{} {} = {}", field_prefix, field_type, field_name, field_number));

        // Add validation options if there are validators and we're including them
        if include_validators {
            let validators_str = collect_validators_for_field(&field.validators, &field.field_type);
            if !validators_str.is_empty() {
                output.push_str(&format!(" [{}]", validators_str));
            }
        }

        output.push_str(";\n");
    }

    output.push_str("}\n");
    output
}

/// Convert a FieldType to its Protocol Buffers type representation with optional prefix.
/// Returns (prefix, type) where prefix is "repeated " for arrays or "optional " for options.
fn field_type_to_protobuf_with_prefix(field_type: &FieldType) -> (String, String) {
    match field_type {
        FieldType::Option(inner) => {
            let (_, inner_type) = field_type_to_protobuf_with_prefix(inner);
            ("optional ".to_string(), inner_type)
        }
        FieldType::Vec(inner) => {
            let (_, inner_type) = field_type_to_protobuf_with_prefix(inner);
            ("repeated ".to_string(), inner_type)
        }
        _ => ("".to_string(), field_type_to_protobuf(field_type)),
    }
}

/// Convert a FieldType to its Protocol Buffers type representation.
fn field_type_to_protobuf(field_type: &FieldType) -> String {
    match field_type {
        FieldType::String | FieldType::Char => "string".to_string(),
        FieldType::Bool => "bool".to_string(),
        FieldType::Unit => "bool".to_string(), // Placeholder for unit type
        FieldType::F32 => "float".to_string(),
        FieldType::F64 => "double".to_string(),
        FieldType::I8 | FieldType::I16 | FieldType::I32 => "int32".to_string(),
        FieldType::I64 => "int64".to_string(),
        FieldType::I128 => "string".to_string(), // No native 128-bit support
        FieldType::Isize => "int64".to_string(),
        FieldType::U8 | FieldType::U16 | FieldType::U32 => "uint32".to_string(),
        FieldType::U64 => "uint64".to_string(),
        FieldType::U128 => "string".to_string(), // No native 128-bit support
        FieldType::Usize => "uint64".to_string(),
        FieldType::EvenframeRecordId => "string".to_string(),
        FieldType::DateTime => "string".to_string(), // ISO 8601 string or google.protobuf.Timestamp
        FieldType::EvenframeDuration => "int64".to_string(), // Nanoseconds or google.protobuf.Duration
        FieldType::Timezone => "string".to_string(),
        FieldType::Decimal => "string".to_string(), // Arbitrary precision as string
        FieldType::OrderedFloat(inner) => field_type_to_protobuf(inner),

        FieldType::Option(inner) => {
            // For nested options, just return the inner type
            field_type_to_protobuf(inner)
        }

        FieldType::Vec(inner) => {
            // For nested vecs, just return the inner type
            field_type_to_protobuf(inner)
        }

        FieldType::Tuple(types) => {
            // Tuples become a generated message type
            format!("Tuple{}", types.len())
        }

        FieldType::Struct(fields) => {
            // Inline struct - would need separate message definition
            // For now, generate a placeholder
            let field_strs: Vec<String> = fields
                .iter()
                .map(|(name, _)| name.clone())
                .collect();
            format!("InlineStruct_{}", field_strs.join("_"))
        }

        FieldType::HashMap(key, value) | FieldType::BTreeMap(key, value) => {
            // Proto3 supports maps natively
            format!(
                "map<{}, {}>",
                field_type_to_protobuf(key),
                field_type_to_protobuf(value)
            )
        }

        FieldType::RecordLink(inner) => {
            // For record links, use the inner type name
            if let FieldType::Other(type_name) = inner.as_ref() {
                type_name.to_case(Case::Pascal)
            } else {
                field_type_to_protobuf(inner)
            }
        }

        FieldType::Other(type_name) => type_name.to_case(Case::Pascal),
    }
}

/// Collect validators and format them as protoc-gen-validate style options.
fn collect_validators_for_field(validators: &[Validator], field_type: &FieldType) -> String {
    let rules: Vec<String> = validators
        .iter()
        .filter_map(|v| validator_to_protobuf_rule(v, field_type))
        .collect();

    if rules.is_empty() {
        return String::new();
    }

    // Determine the rule type based on the field type
    let rule_type = get_validate_rule_type(field_type);

    // Combine all rules into a single validate option
    format!("(validate.rules).{} = {{{}}}", rule_type, rules.join(", "))
}

/// Get the validate rule type name for a given field type.
fn get_validate_rule_type(field_type: &FieldType) -> &'static str {
    match field_type {
        FieldType::String | FieldType::Char => "string",
        FieldType::Bool => "bool",
        FieldType::F32 => "float",
        FieldType::F64 => "double",
        FieldType::I8 | FieldType::I16 | FieldType::I32 => "int32",
        FieldType::I64 | FieldType::Isize => "int64",
        FieldType::U8 | FieldType::U16 | FieldType::U32 => "uint32",
        FieldType::U64 | FieldType::Usize => "uint64",
        FieldType::Vec(_) => "repeated",
        FieldType::Option(inner) => get_validate_rule_type(inner),
        FieldType::HashMap(_, _) | FieldType::BTreeMap(_, _) => "map",
        FieldType::DateTime | FieldType::EvenframeRecordId | FieldType::Timezone => "string",
        FieldType::EvenframeDuration => "int64",
        FieldType::Decimal | FieldType::I128 | FieldType::U128 => "string",
        _ => "message",
    }
}

/// Convert a Validator to its protoc-gen-validate rule representation.
fn validator_to_protobuf_rule(validator: &Validator, field_type: &FieldType) -> Option<String> {
    match validator {
        Validator::StringValidator(sv) => string_validator_to_protobuf(sv),
        Validator::NumberValidator(nv) => number_validator_to_protobuf(nv, field_type),
        Validator::ArrayValidator(av) => array_validator_to_protobuf(av),
        Validator::DateValidator(dv) => date_validator_to_protobuf(dv),
        Validator::BigIntValidator(biv) => bigint_validator_to_protobuf(biv),
        Validator::BigDecimalValidator(bdv) => bigdecimal_validator_to_protobuf(bdv),
        Validator::DurationValidator(dv) => duration_validator_to_protobuf(dv),
    }
}

fn string_validator_to_protobuf(sv: &StringValidator) -> Option<String> {
    match sv {
        // Length validators
        StringValidator::MinLength(n) => Some(format!("min_len: {}", n)),
        StringValidator::MaxLength(n) => Some(format!("max_len: {}", n)),
        StringValidator::Length(n) => Some(format!("len: {}", n)),
        StringValidator::NonEmpty => Some("min_len: 1".to_string()),

        // Format validators (well-known types in protoc-gen-validate)
        StringValidator::Email => Some("email: true".to_string()),
        StringValidator::Url => Some("uri: true".to_string()),
        StringValidator::Uuid
        | StringValidator::UuidV1
        | StringValidator::UuidV2
        | StringValidator::UuidV3
        | StringValidator::UuidV4
        | StringValidator::UuidV5
        | StringValidator::UuidV6
        | StringValidator::UuidV7
        | StringValidator::UuidV8 => Some("uuid: true".to_string()),
        StringValidator::Ip => Some("ip: true".to_string()),
        StringValidator::IpV4 => Some("ipv4: true".to_string()),
        StringValidator::IpV6 => Some("ipv6: true".to_string()),

        // Pattern validators
        StringValidator::RegexLiteral(format) => {
            let regex = format.clone().into_regex();
            Some(format!("pattern: \"{}\"", escape_for_protobuf(regex.as_str())))
        }

        // Prefix/Suffix validators
        StringValidator::StartsWith(s) => Some(format!("prefix: \"{}\"", escape_for_protobuf(s))),
        StringValidator::EndsWith(s) => Some(format!("suffix: \"{}\"", escape_for_protobuf(s))),
        StringValidator::Includes(s) => Some(format!("contains: \"{}\"", escape_for_protobuf(s))),

        // Literal/const value
        StringValidator::Literal(s) => Some(format!("const: \"{}\"", escape_for_protobuf(s))),

        // Character type validators - map to patterns
        StringValidator::Alpha => Some("pattern: \"^[a-zA-Z]*$\"".to_string()),
        StringValidator::Alphanumeric => Some("pattern: \"^[a-zA-Z0-9]*$\"".to_string()),
        StringValidator::Digits => Some("pattern: \"^[0-9]*$\"".to_string()),
        StringValidator::Hex => Some("pattern: \"^[a-fA-F0-9]*$\"".to_string()),

        // Case validators - not directly supported, use patterns
        StringValidator::Lowercased | StringValidator::LowerPreformatted => {
            Some("pattern: \"^[^A-Z]*$\"".to_string())
        }
        StringValidator::Uppercased | StringValidator::UpperPreformatted => {
            Some("pattern: \"^[^a-z]*$\"".to_string())
        }

        // Skip transformation validators and others that don't map to validation
        StringValidator::String
        | StringValidator::Capitalize
        | StringValidator::CapitalizePreformatted
        | StringValidator::Lower
        | StringValidator::Upper
        | StringValidator::Trim
        | StringValidator::TrimPreformatted
        | StringValidator::Trimmed
        | StringValidator::Capitalized
        | StringValidator::Uncapitalized
        | StringValidator::Normalize
        | StringValidator::NormalizeNFC
        | StringValidator::NormalizeNFD
        | StringValidator::NormalizeNFKC
        | StringValidator::NormalizeNFKD
        | StringValidator::NormalizeNFCPreformatted
        | StringValidator::NormalizeNFDPreformatted
        | StringValidator::NormalizeNFKCPreformatted
        | StringValidator::NormalizeNFKDPreformatted
        | StringValidator::DateParse
        | StringValidator::DateEpochParse
        | StringValidator::DateIsoParse
        | StringValidator::IntegerParse
        | StringValidator::NumericParse
        | StringValidator::JsonParse
        | StringValidator::UrlParse
        | StringValidator::Regex
        | StringValidator::StringEmbedded(_)
        | StringValidator::Base64
        | StringValidator::Base64Url
        | StringValidator::CreditCard
        | StringValidator::Date
        | StringValidator::DateEpoch
        | StringValidator::DateIso
        | StringValidator::Integer
        | StringValidator::Json
        | StringValidator::Numeric
        | StringValidator::Semver => None,
    }
}

fn number_validator_to_protobuf(nv: &NumberValidator, _field_type: &FieldType) -> Option<String> {
    match nv {
        NumberValidator::GreaterThan(n) => Some(format!("gt: {}", n.0)),
        NumberValidator::GreaterThanOrEqualTo(n) => Some(format!("gte: {}", n.0)),
        NumberValidator::LessThan(n) => Some(format!("lt: {}", n.0)),
        NumberValidator::LessThanOrEqualTo(n) => Some(format!("lte: {}", n.0)),
        NumberValidator::Between(start, end) => Some(format!("gte: {}, lte: {}", start.0, end.0)),
        NumberValidator::Positive => Some("gt: 0".to_string()),
        NumberValidator::NonNegative => Some("gte: 0".to_string()),
        NumberValidator::Negative => Some("lt: 0".to_string()),
        NumberValidator::NonPositive => Some("lte: 0".to_string()),

        // Int, Finite, NonNaN don't have direct protobuf equivalents
        NumberValidator::Int
        | NumberValidator::Finite
        | NumberValidator::NonNaN
        | NumberValidator::MultipleOf(_)
        | NumberValidator::Uint8 => None,
    }
}

fn array_validator_to_protobuf(av: &ArrayValidator) -> Option<String> {
    match av {
        ArrayValidator::MinItems(n) => Some(format!("min_items: {}", n)),
        ArrayValidator::MaxItems(n) => Some(format!("max_items: {}", n)),
        ArrayValidator::ItemsCount(n) => Some(format!("min_items: {}, max_items: {}", n, n)),
    }
}

fn date_validator_to_protobuf(dv: &DateValidator) -> Option<String> {
    // Date validators would typically be applied to Timestamp fields
    // protoc-gen-validate supports timestamp rules
    match dv {
        DateValidator::ValidDate => None, // Implicit in protobuf Timestamp
        DateValidator::GreaterThanDate(d) => {
            Some(format!("gt: {{ seconds: {} }}", parse_date_to_seconds(d)))
        }
        DateValidator::GreaterThanOrEqualToDate(d) => {
            Some(format!("gte: {{ seconds: {} }}", parse_date_to_seconds(d)))
        }
        DateValidator::LessThanDate(d) => {
            Some(format!("lt: {{ seconds: {} }}", parse_date_to_seconds(d)))
        }
        DateValidator::LessThanOrEqualToDate(d) => {
            Some(format!("lte: {{ seconds: {} }}", parse_date_to_seconds(d)))
        }
        DateValidator::BetweenDate(start, end) => Some(format!(
            "gte: {{ seconds: {} }}, lte: {{ seconds: {} }}",
            parse_date_to_seconds(start),
            parse_date_to_seconds(end)
        )),
    }
}

fn bigint_validator_to_protobuf(biv: &BigIntValidator) -> Option<String> {
    // BigInt validators - since we represent as string, use string rules
    match biv {
        BigIntValidator::PositiveBigInt => Some("pattern: \"^[1-9][0-9]*$\"".to_string()),
        BigIntValidator::NegativeBigInt => Some("pattern: \"^-[1-9][0-9]*$\"".to_string()),
        BigIntValidator::NonNegativeBigInt => Some("pattern: \"^(0|[1-9][0-9]*)$\"".to_string()),
        BigIntValidator::NonPositiveBigInt => Some("pattern: \"^(0|-[1-9][0-9]*)$\"".to_string()),
        BigIntValidator::GreaterThanBigInt(_)
        | BigIntValidator::GreaterThanOrEqualToBigInt(_)
        | BigIntValidator::LessThanBigInt(_)
        | BigIntValidator::LessThanOrEqualToBigInt(_)
        | BigIntValidator::BetweenBigInt(_, _) => None, // Complex comparisons not expressible in protobuf
    }
}

fn bigdecimal_validator_to_protobuf(bdv: &BigDecimalValidator) -> Option<String> {
    // BigDecimal validators - since we represent as string, use patterns
    match bdv {
        BigDecimalValidator::PositiveBigDecimal => {
            Some("pattern: \"^[0-9]*\\.?[0-9]+$\"".to_string())
        }
        BigDecimalValidator::NegativeBigDecimal => {
            Some("pattern: \"^-[0-9]*\\.?[0-9]+$\"".to_string())
        }
        BigDecimalValidator::NonNegativeBigDecimal => {
            Some("pattern: \"^[0-9]*\\.?[0-9]+$\"".to_string())
        }
        BigDecimalValidator::NonPositiveBigDecimal => {
            Some("pattern: \"^(0|-[0-9]*\\.?[0-9]+)$\"".to_string())
        }
        BigDecimalValidator::GreaterThanBigDecimal(_)
        | BigDecimalValidator::GreaterThanOrEqualToBigDecimal(_)
        | BigDecimalValidator::LessThanBigDecimal(_)
        | BigDecimalValidator::LessThanOrEqualToBigDecimal(_)
        | BigDecimalValidator::BetweenBigDecimal(_, _) => None,
    }
}

fn duration_validator_to_protobuf(dv: &DurationValidator) -> Option<String> {
    // Duration validators - would apply to Duration fields
    // For now, skip as we represent durations as int64 nanoseconds
    match dv {
        DurationValidator::GreaterThanDuration(_)
        | DurationValidator::GreaterThanOrEqualToDuration(_)
        | DurationValidator::LessThanDuration(_)
        | DurationValidator::LessThanOrEqualToDuration(_)
        | DurationValidator::BetweenDuration(_, _) => None,
    }
}

/// Parse a date string to Unix seconds (placeholder - returns 0 for now)
fn parse_date_to_seconds(date_str: &str) -> i64 {
    // In a real implementation, this would parse the date string
    // For now, we just return the string representation
    // The caller should use a proper date parsing library
    date_str.parse().unwrap_or(0)
}

/// Escape special characters for Protocol Buffers strings.
fn escape_for_protobuf(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::StructField;
    use ordered_float::OrderedFloat;

    #[test]
    fn test_string_validators_to_protobuf() {
        assert_eq!(
            string_validator_to_protobuf(&StringValidator::Email),
            Some("email: true".to_string())
        );
        assert_eq!(
            string_validator_to_protobuf(&StringValidator::MinLength(8)),
            Some("min_len: 8".to_string())
        );
        assert_eq!(
            string_validator_to_protobuf(&StringValidator::MaxLength(50)),
            Some("max_len: 50".to_string())
        );
        assert_eq!(
            string_validator_to_protobuf(&StringValidator::Uuid),
            Some("uuid: true".to_string())
        );
        assert_eq!(
            string_validator_to_protobuf(&StringValidator::NonEmpty),
            Some("min_len: 1".to_string())
        );
    }

    #[test]
    fn test_transformation_validators_skipped() {
        // Transformation validators should return None
        assert_eq!(string_validator_to_protobuf(&StringValidator::Lower), None);
        assert_eq!(string_validator_to_protobuf(&StringValidator::Upper), None);
        assert_eq!(string_validator_to_protobuf(&StringValidator::Trim), None);
        assert_eq!(
            string_validator_to_protobuf(&StringValidator::IntegerParse),
            None
        );
    }

    #[test]
    fn test_number_validators_to_protobuf() {
        assert_eq!(
            number_validator_to_protobuf(
                &NumberValidator::GreaterThan(OrderedFloat(5.0)),
                &FieldType::I32
            ),
            Some("gt: 5".to_string())
        );
        assert_eq!(
            number_validator_to_protobuf(
                &NumberValidator::Between(OrderedFloat(18.0), OrderedFloat(120.0)),
                &FieldType::I32
            ),
            Some("gte: 18, lte: 120".to_string())
        );
        assert_eq!(
            number_validator_to_protobuf(&NumberValidator::Positive, &FieldType::I32),
            Some("gt: 0".to_string())
        );
        assert_eq!(
            number_validator_to_protobuf(&NumberValidator::NonNegative, &FieldType::I32),
            Some("gte: 0".to_string())
        );
    }

    #[test]
    fn test_array_validators_to_protobuf() {
        assert_eq!(
            array_validator_to_protobuf(&ArrayValidator::MinItems(1)),
            Some("min_items: 1".to_string())
        );
        assert_eq!(
            array_validator_to_protobuf(&ArrayValidator::MaxItems(5)),
            Some("max_items: 5".to_string())
        );
        assert_eq!(
            array_validator_to_protobuf(&ArrayValidator::ItemsCount(3)),
            Some("min_items: 3, max_items: 3".to_string())
        );
    }

    #[test]
    fn test_field_type_to_protobuf() {
        assert_eq!(field_type_to_protobuf(&FieldType::String), "string");
        assert_eq!(field_type_to_protobuf(&FieldType::Bool), "bool");
        assert_eq!(field_type_to_protobuf(&FieldType::I8), "int32");
        assert_eq!(field_type_to_protobuf(&FieldType::I16), "int32");
        assert_eq!(field_type_to_protobuf(&FieldType::I32), "int32");
        assert_eq!(field_type_to_protobuf(&FieldType::I64), "int64");
        assert_eq!(field_type_to_protobuf(&FieldType::U8), "uint32");
        assert_eq!(field_type_to_protobuf(&FieldType::U16), "uint32");
        assert_eq!(field_type_to_protobuf(&FieldType::U32), "uint32");
        assert_eq!(field_type_to_protobuf(&FieldType::U64), "uint64");
        assert_eq!(field_type_to_protobuf(&FieldType::F32), "float");
        assert_eq!(field_type_to_protobuf(&FieldType::F64), "double");
    }

    #[test]
    fn test_field_type_vec_to_protobuf() {
        let (prefix, type_name) =
            field_type_to_protobuf_with_prefix(&FieldType::Vec(Box::new(FieldType::String)));
        assert_eq!(prefix, "repeated ");
        assert_eq!(type_name, "string");
    }

    #[test]
    fn test_field_type_option_to_protobuf() {
        let (prefix, type_name) =
            field_type_to_protobuf_with_prefix(&FieldType::Option(Box::new(FieldType::String)));
        assert_eq!(prefix, "optional ");
        assert_eq!(type_name, "string");
    }

    #[test]
    fn test_field_type_map_to_protobuf() {
        assert_eq!(
            field_type_to_protobuf(&FieldType::HashMap(
                Box::new(FieldType::String),
                Box::new(FieldType::I32)
            )),
            "map<string, int32>"
        );
    }

    #[test]
    fn test_field_type_other_to_protobuf() {
        assert_eq!(
            field_type_to_protobuf(&FieldType::Other("UserProfile".to_string())),
            "UserProfile"
        );
        assert_eq!(
            field_type_to_protobuf(&FieldType::Other("user_profile".to_string())),
            "UserProfile"
        );
    }

    #[test]
    fn test_collect_validators_for_field() {
        let validators = vec![
            Validator::StringValidator(StringValidator::Email),
            Validator::StringValidator(StringValidator::MinLength(5)),
        ];
        let result = collect_validators_for_field(&validators, &FieldType::String);
        assert!(result.contains("email: true"));
        assert!(result.contains("min_len: 5"));
        assert!(result.starts_with("(validate.rules).string = {"));
    }

    #[test]
    fn test_collect_validators_empty() {
        let validators: Vec<Validator> = vec![];
        let result = collect_validators_for_field(&validators, &FieldType::String);
        assert_eq!(result, "");
    }

    #[test]
    fn test_generate_simple_message() {
        let mut structs = HashMap::new();
        structs.insert(
            "user".to_string(),
            StructConfig {
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
                validators: vec![],
            },
        );

        let output = generate_protobuf_schema_string(&structs, &HashMap::new(), None, true);

        assert!(output.contains("syntax = \"proto3\";"));
        assert!(output.contains("message User"));
        assert!(output.contains("string email = 1"));
        assert!(output.contains("email: true"));
        assert!(output.contains("int32 age = 2"));
        assert!(output.contains("gte: 18, lte: 120"));
    }

    #[test]
    fn test_generate_message_with_package() {
        let output = generate_protobuf_schema_string(
            &HashMap::new(),
            &HashMap::new(),
            Some("com.example.app"),
            false,
        );
        assert!(output.contains("package com.example.app;"));
    }

    #[test]
    fn test_generate_message_with_import() {
        let output =
            generate_protobuf_schema_string(&HashMap::new(), &HashMap::new(), None, true);
        assert!(output.contains("import \"validate/validate.proto\";"));
    }

    #[test]
    fn test_generate_simple_enum() {
        use crate::types::Variant;

        let mut enums = HashMap::new();
        enums.insert(
            "Status".to_string(),
            TaggedUnion {
                enum_name: "Status".to_string(),
                variants: vec![
                    Variant {
                        name: "Active".to_string(),
                        data: None,
                    },
                    Variant {
                        name: "Inactive".to_string(),
                        data: None,
                    },
                    Variant {
                        name: "Pending".to_string(),
                        data: None,
                    },
                ],
            },
        );

        let output = generate_protobuf_schema_string(&HashMap::new(), &enums, None, false);

        assert!(output.contains("enum Status"));
        assert!(output.contains("STATUS_UNSPECIFIED = 0;"));
        assert!(output.contains("STATUS_ACTIVE = 1"));
        assert!(output.contains("STATUS_INACTIVE = 2"));
        assert!(output.contains("STATUS_PENDING = 3"));
    }

    #[test]
    fn test_generate_complete_schema() {
        use crate::types::Variant;

        let mut structs = HashMap::new();
        structs.insert(
            "user_registration_form".to_string(),
            StructConfig {
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
                validators: vec![],
            },
        );

        let mut enums = HashMap::new();
        enums.insert(
            "Role".to_string(),
            TaggedUnion {
                enum_name: "Role".to_string(),
                variants: vec![
                    Variant {
                        name: "Admin".to_string(),
                        data: None,
                    },
                    Variant {
                        name: "User".to_string(),
                        data: None,
                    },
                ],
            },
        );

        let output =
            generate_protobuf_schema_string(&structs, &enums, Some("com.example.users"), true);

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

    #[test]
    fn test_escape_for_protobuf() {
        assert_eq!(escape_for_protobuf("hello"), "hello");
        assert_eq!(escape_for_protobuf("hello\"world"), "hello\\\"world");
        assert_eq!(escape_for_protobuf("path\\to\\file"), "path\\\\to\\\\file");
        assert_eq!(escape_for_protobuf("line1\nline2"), "line1\\nline2");
    }

    #[test]
    fn test_get_validate_rule_type() {
        assert_eq!(get_validate_rule_type(&FieldType::String), "string");
        assert_eq!(get_validate_rule_type(&FieldType::I32), "int32");
        assert_eq!(get_validate_rule_type(&FieldType::I64), "int64");
        assert_eq!(get_validate_rule_type(&FieldType::U32), "uint32");
        assert_eq!(get_validate_rule_type(&FieldType::F64), "double");
        assert_eq!(
            get_validate_rule_type(&FieldType::Vec(Box::new(FieldType::String))),
            "repeated"
        );
    }

    #[test]
    fn test_prefix_pattern_validators() {
        assert_eq!(
            string_validator_to_protobuf(&StringValidator::StartsWith("test".to_string())),
            Some("prefix: \"test\"".to_string())
        );
        assert_eq!(
            string_validator_to_protobuf(&StringValidator::EndsWith("test".to_string())),
            Some("suffix: \"test\"".to_string())
        );
        assert_eq!(
            string_validator_to_protobuf(&StringValidator::Includes("test".to_string())),
            Some("contains: \"test\"".to_string())
        );
    }

    #[test]
    fn test_ip_validators() {
        assert_eq!(
            string_validator_to_protobuf(&StringValidator::Ip),
            Some("ip: true".to_string())
        );
        assert_eq!(
            string_validator_to_protobuf(&StringValidator::IpV4),
            Some("ipv4: true".to_string())
        );
        assert_eq!(
            string_validator_to_protobuf(&StringValidator::IpV6),
            Some("ipv6: true".to_string())
        );
    }
}
