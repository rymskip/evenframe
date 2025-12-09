//! Macroforge TypeScript interface generation with JSDoc validator annotations.
//!
//! This module generates TypeScript interfaces with `@derive(Deserialize)` at the type level
//! and `@serde({ validate: [...] })` annotations at the field level for validators.

use crate::types::{FieldType, StructConfig, TaggedUnion, VariantData};
use crate::validator::{
    ArrayValidator, BigDecimalValidator, BigIntValidator, DateValidator, DurationValidator,
    NumberValidator, StringValidator, Validator,
};
use convert_case::{Case, Casing};
use macroforge_ts::macros::ts_template;
use std::collections::{HashMap, HashSet};

/// Main entry point for generating Macroforge TypeScript interfaces.
pub fn generate_macroforge_type_string(
    structs: &HashMap<String, StructConfig>,
    enums: &HashMap<String, TaggedUnion>,
    _print_types: bool,
) -> String {
    tracing::info!(
        struct_count = structs.len(),
        enum_count = enums.len(),
        "Generating Macroforge TypeScript interfaces"
    );

    // Deduplicate structs by PascalCase name to avoid generating the same interface twice
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

    let result = ts_template! {
        {#for struct_config in &unique_structs}
            {$let name = struct_config.struct_name.to_case(Case::Pascal)}
            {$let field_count = struct_config.fields.len()}
            @{"/** @derive(Deserialize) */"}
            export interface @{&name} {
                {#for (index, field) in struct_config.fields.iter().enumerate()}
                    {$let validators_str = collect_validators_for_field(&field.validators, &field.field_type)}
                    {#if !validators_str.is_empty()}
                        @{format!("/** @serde({{ validate: [{}] }}) */", &validators_str)}
                    {/if}
                    @{field.field_name.to_case(Case::Camel)}: @{field_type_to_typescript(&field.field_type)}{#if index + 1 != field_count};{/if}
                {/for}
            }

        {/for}
        {#for enum_def in &unique_enums}
            {$let name = enum_def.enum_name.to_case(Case::Pascal)}
            {$let variant_count = enum_def.variants.len()}
            @{"/** @derive(Deserialize) */"}
            export type @{&name} =
                {#for (index, variant) in enum_def.variants.iter().enumerate()}
                    {#if let Some(data) = &variant.data}
                        {#match data}
                            {:case VariantData::InlineStruct(s)}
                                @{s.struct_name.to_case(Case::Pascal)}
                            {:case VariantData::DataStructureRef(ft)}
                                @{field_type_to_typescript(ft)}
                        {/match}
                    {:else}
                        "@{variant.name}"
                    {/if}
                    {#if index + 1 != variant_count} | {/if}
                {/for}
            ;

        {/for}
    }
    .source()
    .to_string();

    tracing::info!(
        output_length = result.len(),
        "Macroforge interface generation complete"
    );
    result
}

/// Convert a FieldType to its TypeScript representation.
fn field_type_to_typescript(field_type: &FieldType) -> String {
    ts_template! {
        {#match field_type}
            {:case FieldType::String | FieldType::Char}
                string
            {:case FieldType::Bool}
                boolean
            {:case FieldType::Unit}
                null
            {:case FieldType::Decimal | FieldType::OrderedFloat(_) | FieldType::F32 | FieldType::F64}
                number
            {:case FieldType::I8 | FieldType::I16 | FieldType::I32 | FieldType::I64 | FieldType::I128 | FieldType::Isize}
                number
            {:case FieldType::U8 | FieldType::U16 | FieldType::U32 | FieldType::U64 | FieldType::U128 | FieldType::Usize}
                number
            {:case FieldType::EvenframeRecordId}
                string
            {:case FieldType::DateTime}
                string
            {:case FieldType::EvenframeDuration}
                number
            {:case FieldType::Timezone}
                string
            {:case FieldType::Option(inner)}
                @{field_type_to_typescript(inner)} | null
            {:case FieldType::Vec(inner)}
                @{field_type_to_typescript(inner)}[]
            {:case FieldType::Tuple(items)}
                [@{items.iter().map(field_type_to_typescript).collect::<Vec<_>>().join(", ")}]
            {:case FieldType::Struct(fields)}
                { @{fields.iter().map(|(name, ft)| format!("{}: {}", name, field_type_to_typescript(ft))).collect::<Vec<_>>().join("; ")} }
            {:case FieldType::RecordLink(inner)}
                string | @{field_type_to_typescript(inner)}
            {:case FieldType::HashMap(key, value) | FieldType::BTreeMap(key, value)}
                Record<@{field_type_to_typescript(key)}, @{field_type_to_typescript(value)}>
            {:case FieldType::Other(type_name)}
                @{type_name.to_case(Case::Pascal)}
        {/match}
    }
    .source()
    .to_string()
}

/// Collect validators and format them as a comma-separated string for JSDoc.
/// For String fields, automatically adds "nonEmpty" unless already present.
fn collect_validators_for_field(validators: &[Validator], field_type: &FieldType) -> String {
    let mut result: Vec<String> = validators
        .iter()
        .filter_map(validator_to_macroforge_string)
        .collect();

    // Add nonEmpty for String fields by default (unless already present or field is optional)
    if matches!(field_type, FieldType::String | FieldType::Char)
        && !result.iter().any(|v| v == "nonEmpty")
    {
        result.insert(0, "nonEmpty".to_string());
    }

    result
        .iter()
        .map(|v| format!("\"{}\"", v))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Convert a Validator to its Macroforge string representation.
/// Returns None for transformation validators that should be skipped.
fn validator_to_macroforge_string(validator: &Validator) -> Option<String> {
    match validator {
        Validator::StringValidator(sv) => string_validator_to_macroforge(sv),
        Validator::NumberValidator(nv) => number_validator_to_macroforge(nv),
        Validator::ArrayValidator(av) => array_validator_to_macroforge(av),
        Validator::DateValidator(dv) => date_validator_to_macroforge(dv),
        Validator::BigIntValidator(biv) => bigint_validator_to_macroforge(biv),
        Validator::BigDecimalValidator(bdv) => bigdecimal_validator_to_macroforge(bdv),
        Validator::DurationValidator(dv) => duration_validator_to_macroforge(dv),
    }
}

fn string_validator_to_macroforge(sv: &StringValidator) -> Option<String> {
    match sv {
        // Length validators
        StringValidator::MinLength(n) => Some(format!("minLength({})", n)),
        StringValidator::MaxLength(n) => Some(format!("maxLength({})", n)),
        StringValidator::Length(n) => Some(format!("length({})", n)),
        StringValidator::NonEmpty => Some("nonEmpty".to_string()),

        // Format validators
        StringValidator::Email => Some("email".to_string()),
        StringValidator::Url => Some("url".to_string()),
        StringValidator::Uuid
        | StringValidator::UuidV1
        | StringValidator::UuidV2
        | StringValidator::UuidV3
        | StringValidator::UuidV4
        | StringValidator::UuidV5
        | StringValidator::UuidV6
        | StringValidator::UuidV7
        | StringValidator::UuidV8 => Some("uuid".to_string()),
        StringValidator::Ip => Some("ip".to_string()),
        StringValidator::IpV4 => Some("ipv4".to_string()),
        StringValidator::IpV6 => Some("ipv6".to_string()),
        StringValidator::CreditCard => Some("creditCard".to_string()),
        StringValidator::Semver => Some("semver".to_string()),
        StringValidator::Json => Some("json".to_string()),
        StringValidator::Base64 => Some("base64".to_string()),
        StringValidator::Base64Url => Some("base64Url".to_string()),

        // Character type validators
        StringValidator::Alpha => Some("alpha".to_string()),
        StringValidator::Alphanumeric => Some("alphanumeric".to_string()),
        StringValidator::Digits => Some("digits".to_string()),
        StringValidator::Hex => Some("hex".to_string()),
        StringValidator::Integer => Some("integer".to_string()),
        StringValidator::Numeric => Some("numeric".to_string()),

        // Case/state validators (validation-only, not transformations)
        StringValidator::Lowercased | StringValidator::LowerPreformatted => {
            Some("lowercase".to_string())
        }
        StringValidator::Uppercased | StringValidator::UpperPreformatted => {
            Some("uppercase".to_string())
        }
        StringValidator::Trimmed | StringValidator::TrimPreformatted => Some("trimmed".to_string()),
        StringValidator::Capitalized | StringValidator::CapitalizePreformatted => {
            Some("capitalized".to_string())
        }
        StringValidator::Uncapitalized => Some("uncapitalized".to_string()),

        // Substring validators
        StringValidator::StartsWith(s) => Some(format!("startsWith(\"{}\")", escape_for_jsdoc(s))),
        StringValidator::EndsWith(s) => Some(format!("endsWith(\"{}\")", escape_for_jsdoc(s))),
        StringValidator::Includes(s) => Some(format!("includes(\"{}\")", escape_for_jsdoc(s))),

        // Pattern validators
        StringValidator::RegexLiteral(format) => {
            let regex = format.clone().into_regex();
            Some(format!("pattern(\"{}\")", escape_for_jsdoc(regex.as_str())))
        }
        StringValidator::Literal(s) => Some(format!("literal(\"{}\")", escape_for_jsdoc(s))),

        // Date validators
        StringValidator::Date => Some("date".to_string()),
        StringValidator::DateIso => Some("dateIso".to_string()),
        StringValidator::DateEpoch => Some("dateEpoch".to_string()),

        // Skip transformation validators - these modify data rather than validate
        StringValidator::String
        | StringValidator::Capitalize
        | StringValidator::Lower
        | StringValidator::Upper
        | StringValidator::Trim
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
        | StringValidator::StringEmbedded(_) => None,
    }
}

fn number_validator_to_macroforge(nv: &NumberValidator) -> Option<String> {
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

fn array_validator_to_macroforge(av: &ArrayValidator) -> Option<String> {
    match av {
        ArrayValidator::MinItems(n) => Some(format!("minItems({})", n)),
        ArrayValidator::MaxItems(n) => Some(format!("maxItems({})", n)),
        ArrayValidator::ItemsCount(n) => Some(format!("itemsCount({})", n)),
    }
}

fn date_validator_to_macroforge(dv: &DateValidator) -> Option<String> {
    match dv {
        DateValidator::ValidDate => Some("validDate".to_string()),
        DateValidator::GreaterThanDate(d) => {
            Some(format!("greaterThanDate(\"{}\")", escape_for_jsdoc(d)))
        }
        DateValidator::GreaterThanOrEqualToDate(d) => Some(format!(
            "greaterThanOrEqualToDate(\"{}\")",
            escape_for_jsdoc(d)
        )),
        DateValidator::LessThanDate(d) => {
            Some(format!("lessThanDate(\"{}\")", escape_for_jsdoc(d)))
        }
        DateValidator::LessThanOrEqualToDate(d) => Some(format!(
            "lessThanOrEqualToDate(\"{}\")",
            escape_for_jsdoc(d)
        )),
        DateValidator::BetweenDate(start, end) => Some(format!(
            "betweenDate(\"{}\", \"{}\")",
            escape_for_jsdoc(start),
            escape_for_jsdoc(end)
        )),
    }
}

fn bigint_validator_to_macroforge(biv: &BigIntValidator) -> Option<String> {
    match biv {
        BigIntValidator::PositiveBigInt => Some("positiveBigInt".to_string()),
        BigIntValidator::NegativeBigInt => Some("negativeBigInt".to_string()),
        BigIntValidator::NonPositiveBigInt => Some("nonPositiveBigInt".to_string()),
        BigIntValidator::NonNegativeBigInt => Some("nonNegativeBigInt".to_string()),
        BigIntValidator::GreaterThanBigInt(n) => {
            Some(format!("greaterThanBigInt(\"{}\")", escape_for_jsdoc(n)))
        }
        BigIntValidator::GreaterThanOrEqualToBigInt(n) => Some(format!(
            "greaterThanOrEqualToBigInt(\"{}\")",
            escape_for_jsdoc(n)
        )),
        BigIntValidator::LessThanBigInt(n) => {
            Some(format!("lessThanBigInt(\"{}\")", escape_for_jsdoc(n)))
        }
        BigIntValidator::LessThanOrEqualToBigInt(n) => Some(format!(
            "lessThanOrEqualToBigInt(\"{}\")",
            escape_for_jsdoc(n)
        )),
        BigIntValidator::BetweenBigInt(start, end) => Some(format!(
            "betweenBigInt(\"{}\", \"{}\")",
            escape_for_jsdoc(start),
            escape_for_jsdoc(end)
        )),
    }
}

fn bigdecimal_validator_to_macroforge(bdv: &BigDecimalValidator) -> Option<String> {
    match bdv {
        BigDecimalValidator::PositiveBigDecimal => Some("positiveBigDecimal".to_string()),
        BigDecimalValidator::NegativeBigDecimal => Some("negativeBigDecimal".to_string()),
        BigDecimalValidator::NonPositiveBigDecimal => Some("nonPositiveBigDecimal".to_string()),
        BigDecimalValidator::NonNegativeBigDecimal => Some("nonNegativeBigDecimal".to_string()),
        BigDecimalValidator::GreaterThanBigDecimal(n) => Some(format!(
            "greaterThanBigDecimal(\"{}\")",
            escape_for_jsdoc(n)
        )),
        BigDecimalValidator::GreaterThanOrEqualToBigDecimal(n) => Some(format!(
            "greaterThanOrEqualToBigDecimal(\"{}\")",
            escape_for_jsdoc(n)
        )),
        BigDecimalValidator::LessThanBigDecimal(n) => {
            Some(format!("lessThanBigDecimal(\"{}\")", escape_for_jsdoc(n)))
        }
        BigDecimalValidator::LessThanOrEqualToBigDecimal(n) => Some(format!(
            "lessThanOrEqualToBigDecimal(\"{}\")",
            escape_for_jsdoc(n)
        )),
        BigDecimalValidator::BetweenBigDecimal(start, end) => Some(format!(
            "betweenBigDecimal(\"{}\", \"{}\")",
            escape_for_jsdoc(start),
            escape_for_jsdoc(end)
        )),
    }
}

fn duration_validator_to_macroforge(dv: &DurationValidator) -> Option<String> {
    match dv {
        DurationValidator::GreaterThanDuration(d) => {
            Some(format!("greaterThanDuration(\"{}\")", escape_for_jsdoc(d)))
        }
        DurationValidator::GreaterThanOrEqualToDuration(d) => Some(format!(
            "greaterThanOrEqualToDuration(\"{}\")",
            escape_for_jsdoc(d)
        )),
        DurationValidator::LessThanDuration(d) => {
            Some(format!("lessThanDuration(\"{}\")", escape_for_jsdoc(d)))
        }
        DurationValidator::LessThanOrEqualToDuration(d) => Some(format!(
            "lessThanOrEqualToDuration(\"{}\")",
            escape_for_jsdoc(d)
        )),
        DurationValidator::BetweenDuration(start, end) => Some(format!(
            "betweenDuration(\"{}\", \"{}\")",
            escape_for_jsdoc(start),
            escape_for_jsdoc(end)
        )),
    }
}

/// Escape special characters for JSDoc strings.
fn escape_for_jsdoc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::StructField;
    use ordered_float::OrderedFloat;

    #[test]
    fn test_string_validators_to_macroforge() {
        assert_eq!(
            validator_to_macroforge_string(&Validator::StringValidator(StringValidator::Email)),
            Some("email".to_string())
        );
        assert_eq!(
            validator_to_macroforge_string(&Validator::StringValidator(
                StringValidator::MinLength(8)
            )),
            Some("minLength(8)".to_string())
        );
        assert_eq!(
            validator_to_macroforge_string(&Validator::StringValidator(
                StringValidator::MaxLength(50)
            )),
            Some("maxLength(50)".to_string())
        );
        assert_eq!(
            validator_to_macroforge_string(&Validator::StringValidator(StringValidator::Uuid)),
            Some("uuid".to_string())
        );
        assert_eq!(
            validator_to_macroforge_string(&Validator::StringValidator(
                StringValidator::Lowercased
            )),
            Some("lowercase".to_string())
        );
    }

    #[test]
    fn test_number_validators_to_macroforge() {
        assert_eq!(
            validator_to_macroforge_string(&Validator::NumberValidator(NumberValidator::Int)),
            Some("int".to_string())
        );
        assert_eq!(
            validator_to_macroforge_string(&Validator::NumberValidator(NumberValidator::Between(
                OrderedFloat(18.0),
                OrderedFloat(120.0)
            ))),
            Some("between(18, 120)".to_string())
        );
        assert_eq!(
            validator_to_macroforge_string(&Validator::NumberValidator(NumberValidator::Positive)),
            Some("positive".to_string())
        );
    }

    #[test]
    fn test_array_validators_to_macroforge() {
        assert_eq!(
            validator_to_macroforge_string(&Validator::ArrayValidator(ArrayValidator::MinItems(1))),
            Some("minItems(1)".to_string())
        );
        assert_eq!(
            validator_to_macroforge_string(&Validator::ArrayValidator(ArrayValidator::MaxItems(5))),
            Some("maxItems(5)".to_string())
        );
    }

    #[test]
    fn test_transformation_validators_skipped() {
        // These should return None as they're transformations, not validations
        assert_eq!(
            validator_to_macroforge_string(&Validator::StringValidator(StringValidator::Lower)),
            None
        );
        assert_eq!(
            validator_to_macroforge_string(&Validator::StringValidator(StringValidator::Upper)),
            None
        );
        assert_eq!(
            validator_to_macroforge_string(&Validator::StringValidator(StringValidator::Trim)),
            None
        );
        assert_eq!(
            validator_to_macroforge_string(&Validator::StringValidator(
                StringValidator::IntegerParse
            )),
            None
        );
    }

    #[test]
    fn test_field_type_to_typescript() {
        // ts_template! adds whitespace, so we trim for comparison
        assert!(field_type_to_typescript(&FieldType::String).trim() == "string");
        assert!(field_type_to_typescript(&FieldType::Bool).trim() == "boolean");
        assert!(field_type_to_typescript(&FieldType::I32).trim() == "number");
        assert!(field_type_to_typescript(&FieldType::F64).trim() == "number");
        assert!(
            field_type_to_typescript(&FieldType::Option(Box::new(FieldType::String)))
                .contains("string")
                && field_type_to_typescript(&FieldType::Option(Box::new(FieldType::String)))
                    .contains("null")
        );
        let vec_output = field_type_to_typescript(&FieldType::Vec(Box::new(FieldType::I32)));
        assert!(vec_output.contains("number") && vec_output.contains("[]"));
        assert!(
            field_type_to_typescript(&FieldType::Other("UserProfile".to_string()))
                .contains("UserProfile")
        );
    }

    #[test]
    fn test_collect_validators_for_string_adds_nonempty() {
        let validators = vec![
            Validator::StringValidator(StringValidator::Email),
            Validator::StringValidator(StringValidator::MinLength(5)),
        ];
        // String fields get nonEmpty added by default
        assert_eq!(
            collect_validators_for_field(&validators, &FieldType::String),
            "\"nonEmpty\", \"email\", \"minLength(5)\""
        );
    }

    #[test]
    fn test_collect_validators_for_number_no_nonempty() {
        let validators = vec![Validator::NumberValidator(NumberValidator::Int)];
        // Number fields don't get nonEmpty
        assert_eq!(
            collect_validators_for_field(&validators, &FieldType::I32),
            "\"int\""
        );
    }

    #[test]
    fn test_collect_validators_skips_transformations() {
        let validators = vec![
            Validator::StringValidator(StringValidator::Email),
            Validator::StringValidator(StringValidator::Lower), // Should be skipped
            Validator::StringValidator(StringValidator::MinLength(5)),
        ];
        // nonEmpty is added first for strings
        assert_eq!(
            collect_validators_for_field(&validators, &FieldType::String),
            "\"nonEmpty\", \"email\", \"minLength(5)\""
        );
    }

    #[test]
    fn test_collect_validators_doesnt_duplicate_nonempty() {
        let validators = vec![
            Validator::StringValidator(StringValidator::NonEmpty),
            Validator::StringValidator(StringValidator::Email),
        ];
        // NonEmpty already present, shouldn't be duplicated
        assert_eq!(
            collect_validators_for_field(&validators, &FieldType::String),
            "\"nonEmpty\", \"email\""
        );
    }

    #[test]
    fn test_generate_complete_interface() {
        let mut structs = HashMap::new();
        structs.insert(
            "user_registration_form".to_string(),
            StructConfig {
                struct_name: "user_registration_form".to_string(),
                fields: vec![
                    StructField {
                        field_name: "email".to_string(),
                        field_type: FieldType::String,
                        validators: vec![Validator::StringValidator(StringValidator::Email)],
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
                ],
                validators: vec![],
            },
        );

        let output = generate_macroforge_type_string(&structs, &HashMap::new(), true);

        assert!(output.contains("/** @derive(Deserialize) */"));
        assert!(output.contains("export interface UserRegistrationForm"));
        // String fields now get nonEmpty by default
        assert!(output.contains("@serde({ validate: [\"nonEmpty\", \"email\"] })"));
        assert!(output.contains("@serde({ validate: [\"nonEmpty\", \"minLength(8)\", \"maxLength(50)\"] })"));
        // Number fields don't get nonEmpty
        assert!(output.contains("@serde({ validate: [\"int\", \"between(18, 120)\"] })"));
        assert!(output.contains("email: string"));
        assert!(output.contains("password: string"));
        assert!(output.contains("age: number"));
    }

    #[test]
    fn test_comment_syntax_comparison() {
        // This test demonstrates the difference between raw comments and interpolated comments
        // in ts_template!

        // Raw comment syntax - gets converted to #[doc = "..."] by the macro
        let raw_comment_output = ts_template! {
            /** @derive(Deserialize) */
            export interface Test {}
        }
        .source()
        .to_string();

        // Interpolated comment syntax - preserved as literal string
        let interpolated_comment_output = ts_template! {
            @{"/** @derive(Deserialize) */"}
            export interface Test {}
        }
        .source()
        .to_string();

        println!("Raw comment output: {:?}", raw_comment_output);
        println!("Interpolated comment output: {:?}", interpolated_comment_output);

        // Raw comments are converted to Rust doc syntax (with spaces: "# [doc = ...")
        assert!(
            raw_comment_output.contains("doc") && raw_comment_output.contains("@derive"),
            "Raw comments should be converted to doc attribute syntax"
        );

        // Interpolated comments preserve the JSDoc format
        assert!(
            interpolated_comment_output.contains("/** @derive(Deserialize) */"),
            "Interpolated comments should preserve JSDoc syntax"
        );
    }
}
