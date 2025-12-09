//! FlatBuffers schema generation with validator attributes.
//!
//! This module generates FlatBuffers schema files (.fbs) with
//! `(validate: "...")` attributes at the field level for validators.

use crate::types::{FieldType, StructConfig, TaggedUnion, VariantData};
use crate::validator::{
    ArrayValidator, BigDecimalValidator, BigIntValidator, DateValidator, DurationValidator,
    NumberValidator, StringValidator, Validator,
};
use convert_case::{Case, Casing};
use std::collections::{HashMap, HashSet};

/// Main entry point for generating FlatBuffers schema.
pub fn generate_flatbuffers_schema_string(
    structs: &HashMap<String, StructConfig>,
    enums: &HashMap<String, TaggedUnion>,
    namespace: Option<&str>,
) -> String {
    tracing::info!(
        struct_count = structs.len(),
        enum_count = enums.len(),
        "Generating FlatBuffers schema"
    );

    let mut output = String::new();

    // Add namespace if provided
    if let Some(ns) = namespace {
        output.push_str(&format!("namespace {};\n\n", ns));
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

    // Generate enums first (they may be referenced by tables)
    for enum_def in &unique_enums {
        output.push_str(&generate_enum(enum_def));
        output.push('\n');
    }

    // Generate tables
    for struct_config in &unique_structs {
        output.push_str(&generate_table(struct_config));
        output.push('\n');
    }

    tracing::info!(
        output_length = output.len(),
        "FlatBuffers schema generation complete"
    );
    output
}

/// Generate a FlatBuffers enum or union from a TaggedUnion.
fn generate_enum(enum_def: &TaggedUnion) -> String {
    let name = enum_def.enum_name.to_case(Case::Pascal);

    // Check if this is a simple enum (no data variants) or a union
    let has_data_variants = enum_def.variants.iter().any(|v| v.data.is_some());

    if has_data_variants {
        // Generate as FlatBuffers union
        let mut output = format!("union {} {{\n", name);
        for variant in &enum_def.variants {
            if let Some(data) = &variant.data {
                let type_name = match data {
                    VariantData::InlineStruct(s) => s.struct_name.to_case(Case::Pascal),
                    VariantData::DataStructureRef(ft) => field_type_to_flatbuffers(ft),
                };
                output.push_str(&format!("    {},\n", type_name));
            }
            // Simple variants in unions are skipped (FlatBuffers unions only contain tables)
        }
        output.push_str("}\n");
        output
    } else {
        // Generate as FlatBuffers enum
        let mut output = format!("enum {} : byte {{\n", name);
        for (i, variant) in enum_def.variants.iter().enumerate() {
            output.push_str(&format!("    {} = {}", variant.name, i));
            if i < enum_def.variants.len() - 1 {
                output.push(',');
            }
            output.push('\n');
        }
        output.push_str("}\n");
        output
    }
}

/// Generate a FlatBuffers table from a StructConfig.
fn generate_table(struct_config: &StructConfig) -> String {
    let name = struct_config.struct_name.to_case(Case::Pascal);
    let mut output = format!("table {} {{\n", name);

    for field in &struct_config.fields {
        let field_name = field.field_name.to_case(Case::Snake);
        let field_type = field_type_to_flatbuffers(&field.field_type);
        let validators_str = collect_validators_for_field(&field.validators);

        output.push_str(&format!("    {}: {}", field_name, field_type));

        // Add attributes if there are validators
        if !validators_str.is_empty() {
            output.push_str(&format!(" (validate: \"{}\")", validators_str));
        }

        output.push_str(";\n");
    }

    output.push_str("}\n");
    output
}

/// Convert a FieldType to its FlatBuffers type representation.
fn field_type_to_flatbuffers(field_type: &FieldType) -> String {
    match field_type {
        FieldType::String | FieldType::Char => "string".to_string(),
        FieldType::Bool => "bool".to_string(),
        FieldType::Unit => "bool".to_string(), // Placeholder for unit type
        FieldType::F32 => "float".to_string(),
        FieldType::F64 => "double".to_string(),
        FieldType::I8 => "int8".to_string(),
        FieldType::I16 => "int16".to_string(),
        FieldType::I32 => "int32".to_string(),
        FieldType::I64 => "int64".to_string(),
        FieldType::I128 => "string".to_string(), // No native 128-bit support
        FieldType::Isize => "int64".to_string(),
        FieldType::U8 => "uint8".to_string(),
        FieldType::U16 => "uint16".to_string(),
        FieldType::U32 => "uint32".to_string(),
        FieldType::U64 => "uint64".to_string(),
        FieldType::U128 => "string".to_string(), // No native 128-bit support
        FieldType::Usize => "uint64".to_string(),
        FieldType::EvenframeRecordId => "string".to_string(),
        FieldType::DateTime => "string".to_string(), // ISO 8601
        FieldType::EvenframeDuration => "int64".to_string(), // Nanoseconds
        FieldType::Timezone => "string".to_string(),
        FieldType::Decimal => "string".to_string(), // Arbitrary precision
        FieldType::OrderedFloat(inner) => field_type_to_flatbuffers(inner),

        FieldType::Option(inner) => {
            // FlatBuffers fields are optional by default, just use inner type
            field_type_to_flatbuffers(inner)
        }

        FieldType::Vec(inner) => {
            format!("[{}]", field_type_to_flatbuffers(inner))
        }

        FieldType::Tuple(types) => {
            // Tuples become inline structs - FlatBuffers doesn't have native tuple support
            // For now, reference a generated tuple table
            format!("Tuple{}", types.len())
        }

        FieldType::Struct(fields) => {
            // Inline struct - would need separate table definition
            // For now, generate inline struct syntax (not valid FlatBuffers, but informative)
            let field_strs: Vec<String> = fields
                .iter()
                .map(|(name, ft)| format!("{}: {}", name, field_type_to_flatbuffers(ft)))
                .collect();
            format!("{{ {} }}", field_strs.join(", "))
        }

        FieldType::HashMap(key, value) | FieldType::BTreeMap(key, value) => {
            // Maps become vectors of key-value pairs
            format!(
                "[{}{}Entry]",
                capitalize_fbs_type(&field_type_to_flatbuffers(key)),
                capitalize_fbs_type(&field_type_to_flatbuffers(value))
            )
        }

        FieldType::RecordLink(inner) => {
            // For record links, we just use the inner type name
            if let FieldType::Other(type_name) = inner.as_ref() {
                type_name.to_case(Case::Pascal)
            } else {
                field_type_to_flatbuffers(inner)
            }
        }

        FieldType::Other(type_name) => type_name.to_case(Case::Pascal),
    }
}

/// Capitalize a FlatBuffers type name for use in compound type names.
fn capitalize_fbs_type(fbs_type: &str) -> String {
    match fbs_type {
        "string" => "String".to_string(),
        "bool" => "Bool".to_string(),
        "float" => "Float".to_string(),
        "double" => "Double".to_string(),
        "int8" => "Int8".to_string(),
        "int16" => "Int16".to_string(),
        "int32" => "Int32".to_string(),
        "int64" => "Int64".to_string(),
        "uint8" => "Uint8".to_string(),
        "uint16" => "Uint16".to_string(),
        "uint32" => "Uint32".to_string(),
        "uint64" => "Uint64".to_string(),
        other => other.to_case(Case::Pascal),
    }
}

/// Collect validators and format them as a comma-separated string for FlatBuffers attributes.
fn collect_validators_for_field(validators: &[Validator]) -> String {
    validators
        .iter()
        .filter_map(validator_to_flatbuffers_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Convert a Validator to its FlatBuffers attribute string representation.
/// Unlike macroforge, this includes ALL validators (including transformations).
fn validator_to_flatbuffers_string(validator: &Validator) -> Option<String> {
    match validator {
        Validator::StringValidator(sv) => string_validator_to_flatbuffers(sv),
        Validator::NumberValidator(nv) => number_validator_to_flatbuffers(nv),
        Validator::ArrayValidator(av) => array_validator_to_flatbuffers(av),
        Validator::DateValidator(dv) => date_validator_to_flatbuffers(dv),
        Validator::BigIntValidator(biv) => bigint_validator_to_flatbuffers(biv),
        Validator::BigDecimalValidator(bdv) => bigdecimal_validator_to_flatbuffers(bdv),
        Validator::DurationValidator(dv) => duration_validator_to_flatbuffers(dv),
    }
}

fn string_validator_to_flatbuffers(sv: &StringValidator) -> Option<String> {
    match sv {
        // Basic type validators
        StringValidator::String => Some("string".to_string()),
        StringValidator::Alpha => Some("alpha".to_string()),
        StringValidator::Alphanumeric => Some("alphanumeric".to_string()),
        StringValidator::Base64 => Some("base64".to_string()),
        StringValidator::Base64Url => Some("base64Url".to_string()),
        StringValidator::CreditCard => Some("creditCard".to_string()),
        StringValidator::Digits => Some("digits".to_string()),
        StringValidator::Email => Some("email".to_string()),
        StringValidator::Hex => Some("hex".to_string()),
        StringValidator::Integer => Some("integer".to_string()),
        StringValidator::Ip => Some("ip".to_string()),
        StringValidator::IpV4 => Some("ipv4".to_string()),
        StringValidator::IpV6 => Some("ipv6".to_string()),
        StringValidator::Json => Some("json".to_string()),
        StringValidator::Numeric => Some("numeric".to_string()),
        StringValidator::Regex => Some("regex".to_string()),
        StringValidator::Semver => Some("semver".to_string()),
        StringValidator::Url => Some("url".to_string()),

        // Date validators
        StringValidator::Date => Some("date".to_string()),
        StringValidator::DateEpoch => Some("dateEpoch".to_string()),
        StringValidator::DateIso => Some("dateIso".to_string()),

        // UUID validators
        StringValidator::Uuid => Some("uuid".to_string()),
        StringValidator::UuidV1 => Some("uuidV1".to_string()),
        StringValidator::UuidV2 => Some("uuidV2".to_string()),
        StringValidator::UuidV3 => Some("uuidV3".to_string()),
        StringValidator::UuidV4 => Some("uuidV4".to_string()),
        StringValidator::UuidV5 => Some("uuidV5".to_string()),
        StringValidator::UuidV6 => Some("uuidV6".to_string()),
        StringValidator::UuidV7 => Some("uuidV7".to_string()),
        StringValidator::UuidV8 => Some("uuidV8".to_string()),

        // Length validators
        StringValidator::MinLength(n) => Some(format!("minLength({})", n)),
        StringValidator::MaxLength(n) => Some(format!("maxLength({})", n)),
        StringValidator::Length(n) => Some(format!("length({})", n)),
        StringValidator::NonEmpty => Some("nonEmpty".to_string()),

        // Case/state validators
        StringValidator::Lowercased | StringValidator::LowerPreformatted => {
            Some("lowercase".to_string())
        }
        StringValidator::Uppercased | StringValidator::UpperPreformatted => {
            Some("uppercase".to_string())
        }
        StringValidator::Trimmed | StringValidator::TrimPreformatted => {
            Some("trimmed".to_string())
        }
        StringValidator::Capitalized | StringValidator::CapitalizePreformatted => {
            Some("capitalized".to_string())
        }
        StringValidator::Uncapitalized => Some("uncapitalized".to_string()),

        // Transformation validators (INCLUDED per requirements)
        StringValidator::Capitalize => Some("capitalize".to_string()),
        StringValidator::Lower => Some("lower".to_string()),
        StringValidator::Upper => Some("upper".to_string()),
        StringValidator::Trim => Some("trim".to_string()),
        StringValidator::Normalize => Some("normalize".to_string()),
        StringValidator::NormalizeNFC => Some("normalizeNFC".to_string()),
        StringValidator::NormalizeNFD => Some("normalizeNFD".to_string()),
        StringValidator::NormalizeNFKC => Some("normalizeNFKC".to_string()),
        StringValidator::NormalizeNFKD => Some("normalizeNFKD".to_string()),
        StringValidator::NormalizeNFCPreformatted => Some("normalizedNFC".to_string()),
        StringValidator::NormalizeNFDPreformatted => Some("normalizedNFD".to_string()),
        StringValidator::NormalizeNFKCPreformatted => Some("normalizedNFKC".to_string()),
        StringValidator::NormalizeNFKDPreformatted => Some("normalizedNFKD".to_string()),

        // Parse validators (INCLUDED per requirements)
        StringValidator::DateParse => Some("dateParse".to_string()),
        StringValidator::DateEpochParse => Some("dateEpochParse".to_string()),
        StringValidator::DateIsoParse => Some("dateIsoParse".to_string()),
        StringValidator::IntegerParse => Some("integerParse".to_string()),
        StringValidator::NumericParse => Some("numericParse".to_string()),
        StringValidator::JsonParse => Some("jsonParse".to_string()),
        StringValidator::UrlParse => Some("urlParse".to_string()),

        // Substring validators
        StringValidator::StartsWith(s) => Some(format!("startsWith(\\\"{}\\\")", escape_for_fbs(s))),
        StringValidator::EndsWith(s) => Some(format!("endsWith(\\\"{}\\\")", escape_for_fbs(s))),
        StringValidator::Includes(s) => Some(format!("includes(\\\"{}\\\")", escape_for_fbs(s))),

        // Pattern validators
        StringValidator::RegexLiteral(format) => {
            let regex = format.clone().into_regex();
            Some(format!("pattern(\\\"{}\\\")", escape_for_fbs(regex.as_str())))
        }
        StringValidator::Literal(s) => Some(format!("literal(\\\"{}\\\")", escape_for_fbs(s))),

        // Special cases - skip internal validators
        StringValidator::StringEmbedded(_) => None,
    }
}

fn number_validator_to_flatbuffers(nv: &NumberValidator) -> Option<String> {
    match nv {
        NumberValidator::Int => Some("int".to_string()),
        NumberValidator::Finite => Some("finite".to_string()),
        NumberValidator::NonNaN => Some("nonNaN".to_string()),
        NumberValidator::Positive => Some("positive".to_string()),
        NumberValidator::Negative => Some("negative".to_string()),
        NumberValidator::NonPositive => Some("nonPositive".to_string()),
        NumberValidator::NonNegative => Some("nonNegative".to_string()),
        NumberValidator::GreaterThan(n) => Some(format!("greaterThan({})", n.0)),
        NumberValidator::GreaterThanOrEqualTo(n) => Some(format!("greaterThanOrEqualTo({})", n.0)),
        NumberValidator::LessThan(n) => Some(format!("lessThan({})", n.0)),
        NumberValidator::LessThanOrEqualTo(n) => Some(format!("lessThanOrEqualTo({})", n.0)),
        NumberValidator::Between(start, end) => Some(format!("between({}, {})", start.0, end.0)),
        NumberValidator::MultipleOf(n) => Some(format!("multipleOf({})", n.0)),
        NumberValidator::Uint8 => Some("uint8".to_string()),
    }
}

fn array_validator_to_flatbuffers(av: &ArrayValidator) -> Option<String> {
    match av {
        ArrayValidator::MinItems(n) => Some(format!("minItems({})", n)),
        ArrayValidator::MaxItems(n) => Some(format!("maxItems({})", n)),
        ArrayValidator::ItemsCount(n) => Some(format!("itemsCount({})", n)),
    }
}

fn date_validator_to_flatbuffers(dv: &DateValidator) -> Option<String> {
    match dv {
        DateValidator::ValidDate => Some("validDate".to_string()),
        DateValidator::GreaterThanDate(d) => {
            Some(format!("greaterThanDate(\\\"{}\\\")", escape_for_fbs(d)))
        }
        DateValidator::GreaterThanOrEqualToDate(d) => Some(format!(
            "greaterThanOrEqualToDate(\\\"{}\\\")",
            escape_for_fbs(d)
        )),
        DateValidator::LessThanDate(d) => {
            Some(format!("lessThanDate(\\\"{}\\\")", escape_for_fbs(d)))
        }
        DateValidator::LessThanOrEqualToDate(d) => Some(format!(
            "lessThanOrEqualToDate(\\\"{}\\\")",
            escape_for_fbs(d)
        )),
        DateValidator::BetweenDate(start, end) => Some(format!(
            "betweenDate(\\\"{}\\\", \\\"{}\\\")",
            escape_for_fbs(start),
            escape_for_fbs(end)
        )),
    }
}

fn bigint_validator_to_flatbuffers(biv: &BigIntValidator) -> Option<String> {
    match biv {
        BigIntValidator::PositiveBigInt => Some("positiveBigInt".to_string()),
        BigIntValidator::NegativeBigInt => Some("negativeBigInt".to_string()),
        BigIntValidator::NonPositiveBigInt => Some("nonPositiveBigInt".to_string()),
        BigIntValidator::NonNegativeBigInt => Some("nonNegativeBigInt".to_string()),
        BigIntValidator::GreaterThanBigInt(n) => {
            Some(format!("greaterThanBigInt(\\\"{}\\\")", escape_for_fbs(n)))
        }
        BigIntValidator::GreaterThanOrEqualToBigInt(n) => Some(format!(
            "greaterThanOrEqualToBigInt(\\\"{}\\\")",
            escape_for_fbs(n)
        )),
        BigIntValidator::LessThanBigInt(n) => {
            Some(format!("lessThanBigInt(\\\"{}\\\")", escape_for_fbs(n)))
        }
        BigIntValidator::LessThanOrEqualToBigInt(n) => Some(format!(
            "lessThanOrEqualToBigInt(\\\"{}\\\")",
            escape_for_fbs(n)
        )),
        BigIntValidator::BetweenBigInt(start, end) => Some(format!(
            "betweenBigInt(\\\"{}\\\", \\\"{}\\\")",
            escape_for_fbs(start),
            escape_for_fbs(end)
        )),
    }
}

fn bigdecimal_validator_to_flatbuffers(bdv: &BigDecimalValidator) -> Option<String> {
    match bdv {
        BigDecimalValidator::PositiveBigDecimal => Some("positiveBigDecimal".to_string()),
        BigDecimalValidator::NegativeBigDecimal => Some("negativeBigDecimal".to_string()),
        BigDecimalValidator::NonPositiveBigDecimal => Some("nonPositiveBigDecimal".to_string()),
        BigDecimalValidator::NonNegativeBigDecimal => Some("nonNegativeBigDecimal".to_string()),
        BigDecimalValidator::GreaterThanBigDecimal(n) => Some(format!(
            "greaterThanBigDecimal(\\\"{}\\\")",
            escape_for_fbs(n)
        )),
        BigDecimalValidator::GreaterThanOrEqualToBigDecimal(n) => Some(format!(
            "greaterThanOrEqualToBigDecimal(\\\"{}\\\")",
            escape_for_fbs(n)
        )),
        BigDecimalValidator::LessThanBigDecimal(n) => {
            Some(format!("lessThanBigDecimal(\\\"{}\\\")", escape_for_fbs(n)))
        }
        BigDecimalValidator::LessThanOrEqualToBigDecimal(n) => Some(format!(
            "lessThanOrEqualToBigDecimal(\\\"{}\\\")",
            escape_for_fbs(n)
        )),
        BigDecimalValidator::BetweenBigDecimal(start, end) => Some(format!(
            "betweenBigDecimal(\\\"{}\\\", \\\"{}\\\")",
            escape_for_fbs(start),
            escape_for_fbs(end)
        )),
    }
}

fn duration_validator_to_flatbuffers(dv: &DurationValidator) -> Option<String> {
    match dv {
        DurationValidator::GreaterThanDuration(d) => {
            Some(format!("greaterThanDuration(\\\"{}\\\")", escape_for_fbs(d)))
        }
        DurationValidator::GreaterThanOrEqualToDuration(d) => Some(format!(
            "greaterThanOrEqualToDuration(\\\"{}\\\")",
            escape_for_fbs(d)
        )),
        DurationValidator::LessThanDuration(d) => {
            Some(format!("lessThanDuration(\\\"{}\\\")", escape_for_fbs(d)))
        }
        DurationValidator::LessThanOrEqualToDuration(d) => Some(format!(
            "lessThanOrEqualToDuration(\\\"{}\\\")",
            escape_for_fbs(d)
        )),
        DurationValidator::BetweenDuration(start, end) => Some(format!(
            "betweenDuration(\\\"{}\\\", \\\"{}\\\")",
            escape_for_fbs(start),
            escape_for_fbs(end)
        )),
    }
}

/// Escape special characters for FlatBuffers attribute strings.
fn escape_for_fbs(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::StructField;
    use ordered_float::OrderedFloat;

    #[test]
    fn test_string_validators_to_flatbuffers() {
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::StringValidator(StringValidator::Email)),
            Some("email".to_string())
        );
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::StringValidator(
                StringValidator::MinLength(8)
            )),
            Some("minLength(8)".to_string())
        );
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::StringValidator(
                StringValidator::MaxLength(50)
            )),
            Some("maxLength(50)".to_string())
        );
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::StringValidator(StringValidator::Uuid)),
            Some("uuid".to_string())
        );
    }

    #[test]
    fn test_transformation_validators_included() {
        // Unlike macroforge, transformations ARE included in FlatBuffers
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::StringValidator(StringValidator::Lower)),
            Some("lower".to_string())
        );
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::StringValidator(StringValidator::Upper)),
            Some("upper".to_string())
        );
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::StringValidator(StringValidator::Trim)),
            Some("trim".to_string())
        );
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::StringValidator(
                StringValidator::Capitalize
            )),
            Some("capitalize".to_string())
        );
    }

    #[test]
    fn test_number_validators_to_flatbuffers() {
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::NumberValidator(NumberValidator::Int)),
            Some("int".to_string())
        );
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::NumberValidator(NumberValidator::Between(
                OrderedFloat(18.0),
                OrderedFloat(120.0)
            ))),
            Some("between(18, 120)".to_string())
        );
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::NumberValidator(NumberValidator::Positive)),
            Some("positive".to_string())
        );
    }

    #[test]
    fn test_array_validators_to_flatbuffers() {
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::ArrayValidator(ArrayValidator::MinItems(1))),
            Some("minItems(1)".to_string())
        );
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::ArrayValidator(ArrayValidator::MaxItems(5))),
            Some("maxItems(5)".to_string())
        );
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::ArrayValidator(ArrayValidator::ItemsCount(
                3
            ))),
            Some("itemsCount(3)".to_string())
        );
    }

    #[test]
    fn test_field_type_to_flatbuffers() {
        assert_eq!(field_type_to_flatbuffers(&FieldType::String), "string");
        assert_eq!(field_type_to_flatbuffers(&FieldType::Bool), "bool");
        assert_eq!(field_type_to_flatbuffers(&FieldType::I8), "int8");
        assert_eq!(field_type_to_flatbuffers(&FieldType::I16), "int16");
        assert_eq!(field_type_to_flatbuffers(&FieldType::I32), "int32");
        assert_eq!(field_type_to_flatbuffers(&FieldType::I64), "int64");
        assert_eq!(field_type_to_flatbuffers(&FieldType::U8), "uint8");
        assert_eq!(field_type_to_flatbuffers(&FieldType::U16), "uint16");
        assert_eq!(field_type_to_flatbuffers(&FieldType::U32), "uint32");
        assert_eq!(field_type_to_flatbuffers(&FieldType::U64), "uint64");
        assert_eq!(field_type_to_flatbuffers(&FieldType::F32), "float");
        assert_eq!(field_type_to_flatbuffers(&FieldType::F64), "double");
    }

    #[test]
    fn test_field_type_vec_to_flatbuffers() {
        assert_eq!(
            field_type_to_flatbuffers(&FieldType::Vec(Box::new(FieldType::String))),
            "[string]"
        );
        assert_eq!(
            field_type_to_flatbuffers(&FieldType::Vec(Box::new(FieldType::I32))),
            "[int32]"
        );
    }

    #[test]
    fn test_field_type_option_to_flatbuffers() {
        // Option just unwraps to inner type since FlatBuffers fields are optional by default
        assert_eq!(
            field_type_to_flatbuffers(&FieldType::Option(Box::new(FieldType::String))),
            "string"
        );
    }

    #[test]
    fn test_field_type_other_to_flatbuffers() {
        assert_eq!(
            field_type_to_flatbuffers(&FieldType::Other("UserProfile".to_string())),
            "UserProfile"
        );
        assert_eq!(
            field_type_to_flatbuffers(&FieldType::Other("user_profile".to_string())),
            "UserProfile"
        );
    }

    #[test]
    fn test_collect_validators_for_field() {
        let validators = vec![
            Validator::StringValidator(StringValidator::Email),
            Validator::StringValidator(StringValidator::MinLength(5)),
        ];
        assert_eq!(
            collect_validators_for_field(&validators),
            "email, minLength(5)"
        );
    }

    #[test]
    fn test_collect_validators_empty() {
        let validators: Vec<Validator> = vec![];
        assert_eq!(collect_validators_for_field(&validators), "");
    }

    #[test]
    fn test_generate_simple_table() {
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

        let output = generate_flatbuffers_schema_string(&structs, &HashMap::new(), None);

        assert!(output.contains("table User"));
        assert!(output.contains("email: string (validate: \"email\")"));
        assert!(output.contains("age: int32 (validate: \"between(18, 120)\")"));
    }

    #[test]
    fn test_generate_table_with_namespace() {
        let output = generate_flatbuffers_schema_string(
            &HashMap::new(),
            &HashMap::new(),
            Some("com.example.app"),
        );
        assert!(output.starts_with("namespace com.example.app;"));
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

        let output = generate_flatbuffers_schema_string(&HashMap::new(), &enums, None);

        assert!(output.contains("enum Status : byte"));
        assert!(output.contains("Active = 0"));
        assert!(output.contains("Inactive = 1"));
        assert!(output.contains("Pending = 2"));
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
                        validators: vec![
                            Validator::NumberValidator(NumberValidator::Int),
                            Validator::NumberValidator(NumberValidator::Between(
                                OrderedFloat(18.0),
                                OrderedFloat(120.0),
                            )),
                        ],
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
            generate_flatbuffers_schema_string(&structs, &enums, Some("com.example.users"));

        // Check namespace
        assert!(output.contains("namespace com.example.users;"));

        // Check enum
        assert!(output.contains("enum Role : byte"));
        assert!(output.contains("Admin = 0"));
        assert!(output.contains("User = 1"));

        // Check table
        assert!(output.contains("table UserRegistrationForm"));
        assert!(output.contains("email: string (validate: \"email, nonEmpty\")"));
        assert!(output.contains("password: string (validate: \"minLength(8), maxLength(50)\")"));
        assert!(output.contains("age: int32 (validate: \"int, between(18, 120)\")"));
        assert!(output.contains("tags: [string];"));
    }

    #[test]
    fn test_date_validator_to_flatbuffers() {
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::DateValidator(DateValidator::ValidDate)),
            Some("validDate".to_string())
        );
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::DateValidator(
                DateValidator::GreaterThanDate("2024-01-01".to_string())
            )),
            Some("greaterThanDate(\\\"2024-01-01\\\")".to_string())
        );
    }

    #[test]
    fn test_bigint_validator_to_flatbuffers() {
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::BigIntValidator(
                BigIntValidator::PositiveBigInt
            )),
            Some("positiveBigInt".to_string())
        );
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::BigIntValidator(
                BigIntValidator::GreaterThanBigInt("1000000".to_string())
            )),
            Some("greaterThanBigInt(\\\"1000000\\\")".to_string())
        );
    }

    #[test]
    fn test_duration_validator_to_flatbuffers() {
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::DurationValidator(
                DurationValidator::GreaterThanDuration("1h".to_string())
            )),
            Some("greaterThanDuration(\\\"1h\\\")".to_string())
        );
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::DurationValidator(
                DurationValidator::BetweenDuration("1m".to_string(), "1h".to_string())
            )),
            Some("betweenDuration(\\\"1m\\\", \\\"1h\\\")".to_string())
        );
    }

    #[test]
    fn test_escape_for_fbs() {
        assert_eq!(escape_for_fbs("hello"), "hello");
        assert_eq!(escape_for_fbs("hello\"world"), "hello\\\"world");
        assert_eq!(escape_for_fbs("path\\to\\file"), "path\\\\to\\\\file");
    }

    #[test]
    fn test_capitalize_fbs_type() {
        assert_eq!(capitalize_fbs_type("string"), "String");
        assert_eq!(capitalize_fbs_type("int32"), "Int32");
        assert_eq!(capitalize_fbs_type("uint64"), "Uint64");
        assert_eq!(capitalize_fbs_type("MyCustomType"), "MyCustomType");
    }
}
