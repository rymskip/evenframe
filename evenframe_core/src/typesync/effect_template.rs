//! Effect Schema generation using ts_template! macro from macroforge_ts_quote.
//!
//! This module maximizes the use of ts_template! for TypeScript code generation,
//! wrapping as much logic as possible into the template DSL.

use crate::dependency::{analyse_recursion, deps_of};
use crate::types::{FieldType, StructConfig, TaggedUnion, VariantData};
use crate::validator::{
    ArrayValidator, BigDecimalValidator, BigIntValidator, DateValidator, DurationValidator,
    NumberValidator, StringValidator, Validator,
};
use convert_case::{Case, Casing};
use macroforge_ts::macros::ts_template;
use petgraph::{algo::toposort, graphmap::DiGraphMap};
use std::collections::{HashMap, HashSet};
use tracing;

/// Main entry point for generating Effect Schema TypeScript code.
pub fn generate_effect_schema_string(
    structs: &HashMap<String, StructConfig>,
    enums: &HashMap<String, TaggedUnion>,
    _print_types: bool,
) -> String {
    tracing::info!(
        struct_count = structs.len(),
        enum_count = enums.len(),
        "Generating Effect Schema string (using ts_template!)"
    );

    let recursion_info = analyse_recursion(structs, enums);
    let mut condensation = DiGraphMap::<usize, ()>::new();

    for &comp_id in recursion_info.meta.keys() {
        condensation.add_node(comp_id);
    }

    for (type_name, _) in recursion_info
        .meta
        .values()
        .flat_map(|(_, members)| members.iter())
        .filter_map(|name| recursion_info.comp_of.get(name).map(|&comp| (name, comp)))
    {
        let from_comp = recursion_info.comp_of[type_name];
        for dep_name in &deps_of(type_name, structs, enums) {
            let to_comp = recursion_info.comp_of[dep_name];
            if from_comp != to_comp {
                condensation.add_edge(from_comp, to_comp, ());
            }
        }
    }

    let mut ordered_comps = toposort(&condensation, None).unwrap_or_default();
    ordered_comps.reverse();

    let mut processed = HashSet::<String>::new();

    // Collect ordered type names with their data
    let mut items: Vec<TypeItem> = Vec::new();

    for comp_id in ordered_comps {
        let mut members = recursion_info.meta[&comp_id].1.clone();
        members.sort();

        for name in members {
            if processed.contains(&name) {
                continue;
            }

            if let Some(enum_def) = enums
                .values()
                .find(|enum_def| enum_def.enum_name.to_case(Case::Pascal) == name)
            {
                items.push(TypeItem::Enum(name.clone(), enum_def.clone()));
            } else if let Some(struct_config) = structs
                .values()
                .find(|struct_config| struct_config.struct_name.to_case(Case::Pascal) == name)
            {
                items.push(TypeItem::Struct(name.clone(), struct_config.clone()));
            }
            processed.insert(name);
        }
    }

    // Reset processed for schema generation
    let mut processed = HashSet::<String>::new();

    let result = ts_template! {
        {#for item in &items}
            {#match item}
                {:case TypeItem::Enum(name, enum_def)}
                    {$let variant_count = enum_def.variants.len()}
                    export const @{name} = Schema.Union(
                        {#for (index, variant) in enum_def.variants.iter().enumerate()}
                            {#if let Some(variant_data) = &variant.data}
                                {#match variant_data}
                                    {:case VariantData::InlineStruct(_)}
                                        @{variant.name.to_case(Case::Pascal)}
                                    {:case VariantData::DataStructureRef(field_type)}
                                        @{field_type_to_effect_schema(field_type, structs, name, &recursion_info, &processed)}
                                {/match}
                            {:else}
                                Schema.Literal("@{variant.name}")
                            {/if}
                            {#if index + 1 != variant_count},{/if}
                        {/for}
                    ).annotations({ identifier: "@{name}" });
                    export type {|@{name}Encoded|} =
                        {#for (index, variant) in enum_def.variants.iter().enumerate()}
                            {#if let Some(variant_data) = &variant.data}
                                {#match variant_data}
                                    {:case VariantData::InlineStruct(_)}
                                        {|@{variant.name.to_case(Case::Pascal)}Encoded|}
                                    {:case VariantData::DataStructureRef(field_type)}
                                        @{field_type_to_ts_encoded(field_type)}
                                {/match}
                            {:else}
                                "@{variant.name}"
                            {/if}
                            {#if index + 1 != variant_count} | {/if}
                        {/for}
                    ;
                    export type {|@{name}Type|} = typeof @{name}.Type;
                    {$do processed.insert(name.clone())}

                {:case TypeItem::Struct(name, struct_config)}
                    {$let field_count = struct_config.fields.len()}
                    export class @{name} extends Schema.Class<@{name}>("@{name}")({
                        {#for (index, field) in struct_config.fields.iter().enumerate()}
                            {$let schema = field_type_to_effect_schema(&field.field_type, structs, name, &recursion_info, &processed)}
                            {$let schema_validated = apply_validators_to_schema(schema, &field.validators, &field.field_name)}
                            {#if matches!(field.field_type, FieldType::Option(_))}
                                @{field.field_name.to_case(Case::Camel)}: @{schema_validated}
                            {:else}
                                @{field.field_name.to_case(Case::Camel)}: Schema.propertySignature(@{schema_validated}).annotations({ missingMessage: () => "'^@{field.field_name.to_case(Case::Title)}' is required^'" })
                            {/if}
                            {#if index + 1 != field_count},{/if}
                        {/for}
                    }) {[key: string]: unknown}

                    export interface {|@{name}Encoded|} {
                        {#for field in &struct_config.fields}
                            readonly @{field.field_name.to_case(Case::Camel)}: @{field_type_to_ts_encoded(&field.field_type)};
                        {/for}
                    }
                    export type {|@{name}Type|} = typeof @{name}.Type;
                    {$do processed.insert(name.clone())}
            {/match}
        {/for}
    }.source().to_string();

    tracing::info!(
        output_length = result.len(),
        "Effect Schema generation complete"
    );
    result
}

#[derive(Clone)]
enum TypeItem {
    Enum(String, TaggedUnion),
    Struct(String, StructConfig),
}

fn field_type_to_effect_schema(
    field_type: &FieldType,
    structs: &HashMap<String, StructConfig>,
    current_type: &str,
    recursion_info: &crate::dependency::RecursionInfo,
    processed: &HashSet<String>,
) -> String {
    let recurse = |inner_type: &FieldType| {
        field_type_to_effect_schema(inner_type, structs, current_type, recursion_info, processed)
    };

    ts_template! {
        {#match field_type}
            {:case FieldType::String}
                Schema.String.pipe(Schema.nonEmptyString({ message: () => "'^Please enter a value^'" }))
            {:case FieldType::Char}
                Schema.String.pipe(Schema.maxLength(1))
            {:case FieldType::Bool}
                Schema.Boolean
            {:case FieldType::Unit}
                Schema.Null
            {:case FieldType::Decimal}
                Schema.NumberFromString
            {:case FieldType::OrderedFloat(_) | FieldType::F32 | FieldType::F64}
                Schema.Number
            {:case FieldType::I8 | FieldType::I16 | FieldType::I32 | FieldType::I64 | FieldType::I128 | FieldType::Isize}
                Schema.Number
            {:case FieldType::U8 | FieldType::U16 | FieldType::U32 | FieldType::U64 | FieldType::U128 | FieldType::Usize}
                Schema.Number
            {:case FieldType::EvenframeRecordId}
                Schema.String
            {:case FieldType::DateTime}
                Schema.DateTimeUtc
            {:case FieldType::EvenframeDuration}
                Schema.Duration
            {:case FieldType::Timezone}
                Schema.TimeZoneNamed
            {:case FieldType::Option(inner_type)}
                Schema.OptionFromNullishOr(@{recurse(inner_type)}, null)
            {:case FieldType::Vec(inner_type)}
                Schema.Array(@{recurse(inner_type)})
            {:case FieldType::Tuple(tuple_items)}
                Schema.Tuple(
                    {#for item in tuple_items}
                        @{recurse(item)},
                    {/for}
                )
            {:case FieldType::Struct(struct_fields)}
                Schema.Struct({
                    {#for (field_name, inner_type) in struct_fields}
                        @{field_name}: @{recurse(inner_type)},
                    {/for}
                })
            {:case FieldType::RecordLink(inner_type)}
                Schema.Union(Schema.String.pipe(Schema.nonEmptyString()), @{recurse(inner_type)}).annotations({ message: () => ({ message: "Please enter a valid value", override: true }) })
            {:case FieldType::HashMap(key_type, val_type) | FieldType::BTreeMap(key_type, val_type)}
                Schema.Record({ key: @{recurse(key_type)}, value: @{recurse(val_type)} })
            {:case FieldType::Other(type_name)}
                {$let pascal_name = type_name.to_case(Case::Pascal)}
                {#if recursion_info.is_recursive_pair(current_type, &pascal_name) && !processed.contains(&pascal_name)}
                    {#if structs.values().any(|struct_config| struct_config.struct_name.to_case(Case::Pascal) == pascal_name)}
                        Schema.suspend((): Schema.Schema<@{&pascal_name}, {|@{&pascal_name}Encoded|}> => @{&pascal_name}).annotations({ identifier: "{|@{&pascal_name}Ref|}" })
                    {:else}
                        Schema.suspend((): Schema.Schema<typeof @{&pascal_name}.Type, {|@{&pascal_name}Encoded|}> => @{&pascal_name}).annotations({ identifier: "{|@{&pascal_name}Ref|}" })
                    {/if}
                {:else}
                    @{pascal_name}
                {/if}
        {/match}
    }.source().to_string()
}

fn field_type_to_ts_encoded(field_type: &FieldType) -> String {
    ts_template! {
        {#match field_type}
            {:case FieldType::String | FieldType::Char | FieldType::EvenframeRecordId | FieldType::Timezone | FieldType::DateTime | FieldType::Decimal}
                string
            {:case FieldType::Bool}
                boolean
            {:case FieldType::EvenframeDuration}
                Schema.DurationEncoded | readonly [seconds: number, nanos: number]
            {:case FieldType::Unit}
                null
            {:case FieldType::OrderedFloat(_) | FieldType::F32 | FieldType::F64}
                number
            {:case FieldType::I8 | FieldType::I16 | FieldType::I32 | FieldType::I64 | FieldType::I128 | FieldType::Isize}
                number
            {:case FieldType::U8 | FieldType::U16 | FieldType::U32 | FieldType::U64 | FieldType::U128 | FieldType::Usize}
                number
            {:case FieldType::Option(inner_type)}
                @{field_type_to_ts_encoded(inner_type)} | null | undefined
            {:case FieldType::Vec(inner_type)}
                ReadonlyArray<@{field_type_to_ts_encoded(inner_type)}>
            {:case FieldType::Tuple(tuple_items)}
               readonly [
                    {#for item in tuple_items}
                        @{field_type_to_ts_encoded(item)},
                    {/for}
                    ]

            {:case FieldType::Struct(struct_fields)}
                {
                    {#for (field_name, inner_type) in struct_fields}
                        readonly @{field_name}: @{field_type_to_ts_encoded(inner_type)};
                    {/for}
                }
            {:case FieldType::HashMap(key_type, val_type) | FieldType::BTreeMap(key_type, val_type)}
                Record<@{field_type_to_ts_encoded(key_type)}, @{field_type_to_ts_encoded(val_type)}>
            {:case FieldType::RecordLink(inner_type)}
                string | @{field_type_to_ts_encoded(inner_type)}
            {:case FieldType::Other(type_name)}
                {|@{type_name.to_case(Case::Pascal)}Encoded|}
        {/match}
    }.source().to_string()
}

fn apply_validators_to_schema(
    schema: String,
    validators: &[Validator],
    field_name: &str,
) -> String {
    let field = field_name.to_case(Case::Title);
    let mut result = schema;

    ts_template! {
        {#for v in validators}
            {$let new_val = apply_single_validator(&result, v, &field)}
            {$do result = new_val}
        {/for}
    };

    result
}

fn apply_single_validator(schema: &str, validator: &Validator, field: &str) -> String {
    ts_template! {
        {#match validator}
            {:case Validator::StringValidator(sv)}
                {#match sv}
                    {:case StringValidator::MinLength(len)}
                        @{schema}.pipe(Schema.minLength(@{len}, { message: () => "'^@{field} must be at least @{len} characters long^'" }))
                    {:case StringValidator::MaxLength(len)}
                        @{schema}.pipe(Schema.maxLength(@{len}, { message: () => "'^@{field} must be at most @{len} characters long^'" }))
                    {:case StringValidator::Length(len)}
                        @{schema}.pipe(Schema.length(@{len}, { message: () => "'^@{field} must be exactly @{len} characters long^'" }))
                    {:case StringValidator::NonEmpty}
                        @{schema}.pipe(Schema.nonEmptyString({ message: () => "'^@{field} Please enter a value^'" }))
                    {:case StringValidator::StartsWith(prefix)}
                        @{schema}.pipe(Schema.startsWith("@{prefix}", { message: () => "'^@{field} must start with @{prefix}^'" }))
                    {:case StringValidator::EndsWith(suffix)}
                        @{schema}.pipe(Schema.endsWith("@{suffix}", { message: () => "'^@{field} must end with @{suffix}^'" }))
                    {:case StringValidator::Includes(substring)}
                        @{schema}.pipe(Schema.includes("@{substring}", { message: () => "'^@{field} must include @{substring}^'" }))
                    {:case StringValidator::Trimmed}
                        @{schema}.pipe(Schema.trimmed)
                    {:case StringValidator::Lowercased}
                        @{schema}.pipe(Schema.toLowerCase)
                    {:case StringValidator::Uppercased}
                        @{schema}.pipe(Schema.toUpperCase)
                    {:case StringValidator::Capitalized}
                        @{schema}.pipe(Schema.capitalize)
                    {:case StringValidator::Uncapitalized}
                        @{schema}.pipe(Schema.uncapitalize)
                    {:case StringValidator::RegexLiteral(format_variant)}
                        {$let regex = format_variant.to_owned().into_regex()}
                        @{schema}.pipe(Schema.pattern(/@{regex.as_str()}/, { message: () => "'^@{field} has an invalid format^'" }))
                    {:case _}
                        @{schema}
                {/match}
            {:case Validator::NumberValidator(nv)}
                {#match nv}
                    {:case NumberValidator::GreaterThan(value)}
                        @{schema}.pipe(Schema.greaterThan(@{value.0}, { message: () => "'^@{field} must be greater than @{value.0}^'" }))
                    {:case NumberValidator::GreaterThanOrEqualTo(value)}
                        @{schema}.pipe(Schema.greaterThanOrEqualTo(@{value.0}, { message: () => "'^@{field} must be greater than or equal to @{value.0}^'" }))
                    {:case NumberValidator::LessThan(value)}
                        @{schema}.pipe(Schema.lessThan(@{value.0}, { message: () => "'^@{field} must be less than @{value.0}^'" }))
                    {:case NumberValidator::LessThanOrEqualTo(value)}
                        @{schema}.pipe(Schema.lessThanOrEqualTo(@{value.0}, { message: () => "'^@{field} must be less than or equal to @{value.0}^'" }))
                    {:case NumberValidator::Between(start, end)}
                        @{schema}.pipe(Schema.between(@{start.0}, @{end.0}, { message: () => "'^@{field} must be between @{start.0} and @{end.0}^'" }))
                    {:case NumberValidator::Int}
                        @{schema}.pipe(Schema.int({ message: () => "'^@{field} must be an integer^'" }))
                    {:case NumberValidator::NonNaN}
                        @{schema}.pipe(Schema.nonNaN({ message: () => "'^@{field} must not be NaN^'" }))
                    {:case NumberValidator::Finite}
                        @{schema}.pipe(Schema.finite({ message: () => "'^@{field} must be a finite number^'" }))
                    {:case NumberValidator::Positive}
                        @{schema}.pipe(Schema.positive({ message: () => "'^@{field} must be a positive number^'" }))
                    {:case NumberValidator::NonNegative}
                        @{schema}.pipe(Schema.nonNegative({ message: () => "'^@{field} must be a non-negative number^'" }))
                    {:case NumberValidator::Negative}
                        @{schema}.pipe(Schema.negative({ message: () => "'^@{field} must be a negative number^'" }))
                    {:case NumberValidator::NonPositive}
                        @{schema}.pipe(Schema.nonPositive({ message: () => "'^@{field} must be a non-positive number^'" }))
                    {:case NumberValidator::MultipleOf(value)}
                        @{schema}.pipe(Schema.multipleOf(@{value.0}, { message: () => "'^@{field} must be a multiple of @{value.0}^'" }))
                    {:case NumberValidator::Uint8}
                        @{schema}
                {/match}
            {:case Validator::ArrayValidator(av)}
                {#match av}
                    {:case ArrayValidator::MinItems(count)}
                        @{schema}.pipe(Schema.minItems(@{count}, { message: () => "'^@{field} must contain at least @{count} items^'" }))
                    {:case ArrayValidator::MaxItems(count)}
                        @{schema}.pipe(Schema.maxItems(@{count}, { message: () => "'^@{field} must contain at most @{count} items^'" }))
                    {:case ArrayValidator::ItemsCount(count)}
                        @{schema}.pipe(Schema.itemsCount(@{count}, { message: () => "'^@{field} must contain exactly @{count} items^'" }))
                {/match}
            {:case Validator::DateValidator(dv)}
                {#match dv}
                    {:case DateValidator::ValidDate}
                        @{schema}.pipe(Schema.ValidDate)
                    {:case DateValidator::GreaterThanDate(date)}
                        @{schema}.pipe(Schema.greaterThan(new Date("@{date}"), { message: () => "'^@{field} must be after @{date}^'" }))
                    {:case DateValidator::GreaterThanOrEqualToDate(date)}
                        @{schema}.pipe(Schema.greaterThanOrEqualTo(new Date("@{date}"), { message: () => "'^@{field} must be on or after @{date}^'" }))
                    {:case DateValidator::LessThanDate(date)}
                        @{schema}.pipe(Schema.lessThan(new Date("@{date}"), { message: () => "'^@{field} must be before @{date}^'" }))
                    {:case DateValidator::LessThanOrEqualToDate(date)}
                        @{schema}.pipe(Schema.lessThanOrEqualTo(new Date("@{date}"), { message: () => "'^@{field} must be on or before @{date}^'" }))
                    {:case DateValidator::BetweenDate(start, end)}
                        @{schema}.pipe(Schema.between(new Date("@{start}"), new Date("@{end}"), { message: () => "'^@{field} must be between @{start} and @{end}^'" }))
                {/match}
            {:case Validator::BigIntValidator(biv)}
                {#match biv}
                    {:case BigIntValidator::GreaterThanBigInt(value)}
                        @{schema}.pipe(Schema.greaterThanBigInt(@{value}n, { message: () => "'^@{field} must be greater than @{value}^'" }))
                    {:case BigIntValidator::GreaterThanOrEqualToBigInt(value)}
                        @{schema}.pipe(Schema.greaterThanOrEqualToBigInt(@{value}n, { message: () => "'^@{field} must be greater than or equal to @{value}^'" }))
                    {:case BigIntValidator::LessThanBigInt(value)}
                        @{schema}.pipe(Schema.lessThanBigInt(@{value}n, { message: () => "'^@{field} must be less than @{value}^'" }))
                    {:case BigIntValidator::LessThanOrEqualToBigInt(value)}
                        @{schema}.pipe(Schema.lessThanOrEqualToBigInt(@{value}n, { message: () => "'^@{field} must be less than or equal to @{value}^'" }))
                    {:case BigIntValidator::BetweenBigInt(start, end)}
                        @{schema}.pipe(Schema.betweenBigInt(@{start}n, @{end}n, { message: () => "'^@{field} must be between @{start} and @{end}^'" }))
                    {:case BigIntValidator::PositiveBigInt}
                        @{schema}.pipe(Schema.positiveBigInt({ message: () => "'^@{field} must be a positive BigInt^'" }))
                    {:case BigIntValidator::NonNegativeBigInt}
                        @{schema}.pipe(Schema.nonNegativeBigInt({ message: () => "'^@{field} must be a non-negative BigInt^'" }))
                    {:case BigIntValidator::NegativeBigInt}
                        @{schema}.pipe(Schema.negativeBigInt({ message: () => "'^@{field} must be a negative BigInt^'" }))
                    {:case BigIntValidator::NonPositiveBigInt}
                        @{schema}.pipe(Schema.nonPositiveBigInt({ message: () => "'^@{field} must be a non-positive BigInt^'" }))
                {/match}
            {:case Validator::BigDecimalValidator(bdv)}
                {#match bdv}
                    {:case BigDecimalValidator::GreaterThanBigDecimal(value)}
                        @{schema}.pipe(Schema.greaterThanBigDecimal(BigDecimal.fromNumber(@{value}), { message: () => "'^@{field} must be greater than @{value}^'" }))
                    {:case BigDecimalValidator::GreaterThanOrEqualToBigDecimal(value)}
                        @{schema}.pipe(Schema.greaterThanOrEqualToBigDecimal(BigDecimal.fromNumber(@{value}), { message: () => "'^@{field} must be greater than or equal to @{value}^'" }))
                    {:case BigDecimalValidator::LessThanBigDecimal(value)}
                        @{schema}.pipe(Schema.lessThanBigDecimal(BigDecimal.fromNumber(@{value}), { message: () => "'^@{field} must be less than @{value}^'" }))
                    {:case BigDecimalValidator::LessThanOrEqualToBigDecimal(value)}
                        @{schema}.pipe(Schema.lessThanOrEqualToBigDecimal(BigDecimal.fromNumber(@{value}), { message: () => "'^@{field} must be less than or equal to @{value}^'" }))
                    {:case BigDecimalValidator::BetweenBigDecimal(start, end)}
                        @{schema}.pipe(Schema.betweenBigDecimal(BigDecimal.fromNumber(@{start}), BigDecimal.fromNumber(@{end}), { message: () => "'^@{field} must be between @{start} and @{end}^'" }))
                    {:case BigDecimalValidator::PositiveBigDecimal}
                        @{schema}.pipe(Schema.positiveBigDecimal({ message: () => "'^@{field} must be a positive BigDecimal^'" }))
                    {:case BigDecimalValidator::NonNegativeBigDecimal}
                        @{schema}.pipe(Schema.nonNegativeBigDecimal({ message: () => "'^@{field} must be a non-negative BigDecimal^'" }))
                    {:case BigDecimalValidator::NegativeBigDecimal}
                        @{schema}.pipe(Schema.negativeBigDecimal({ message: () => "'^@{field} must be a negative BigDecimal^'" }))
                    {:case BigDecimalValidator::NonPositiveBigDecimal}
                        @{schema}.pipe(Schema.nonPositiveBigDecimal({ message: () => "'^@{field} must be a non-positive BigDecimal^'" }))
                {/match}
            {:case Validator::DurationValidator(dv)}
                {#match dv}
                    {:case DurationValidator::GreaterThanDuration(value)}
                        @{schema}.pipe(Schema.greaterThanDuration("@{value}", { message: () => "'^@{field} must be longer than @{value}^'" }))
                    {:case DurationValidator::GreaterThanOrEqualToDuration(value)}
                        @{schema}.pipe(Schema.greaterThanOrEqualToDuration("@{value}", { message: () => "'^@{field} must be at least @{value} long^'" }))
                    {:case DurationValidator::LessThanDuration(value)}
                        @{schema}.pipe(Schema.lessThanDuration("@{value}", { message: () => "'^@{field} must be shorter than @{value}^'" }))
                    {:case DurationValidator::LessThanOrEqualToDuration(value)}
                        @{schema}.pipe(Schema.lessThanOrEqualToDuration("@{value}", { message: () => "'^@{field} must be at most @{value} long^'" }))
                    {:case DurationValidator::BetweenDuration(start, end)}
                        @{schema}.pipe(Schema.betweenDuration("@{start}", "@{end}", { message: () => "'^@{field} must be between @{start} and @{end} long^'" }))
                {/match}
        {/match}
    }.source().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::StructField;

    #[test]
    fn test_ts_template_basic() {
        let name = "User";
        let output = ts_template! {
            export class @{name} extends Schema.Class<@{name}>("@{name}")({}) {}
        };
        assert!(output.source().contains("User"));
        assert!(output.source().contains("Schema.Class"));
    }

    #[test]
    fn test_apply_single_validator() {
        let schema = "Schema.String";
        let validator = Validator::StringValidator(StringValidator::MinLength(5));
        let output = apply_single_validator(schema, &validator, "Username");
        assert!(output.contains("Schema.String"));
        assert!(output.contains("pipe"));
        assert!(output.contains("minLength"));
    }

    #[test]
    fn test_generate_effect_schema_string() {
        let mut structs = HashMap::new();
        structs.insert(
            "User".to_string(),
            StructConfig {
                struct_name: "User".to_string(),
                fields: vec![
                    StructField {
                        field_name: "username".to_string(),
                        field_type: FieldType::String,
                        validators: vec![
                            Validator::StringValidator(StringValidator::MinLength(3)),
                            Validator::StringValidator(StringValidator::MaxLength(50)),
                        ],
                        edge_config: None,
                        define_config: None,
                        format: None,
                        always_regenerate: false,
                    },
                    StructField {
                        field_name: "email".to_string(),
                        field_type: FieldType::String,
                        validators: vec![],
                        edge_config: None,
                        define_config: None,
                        format: None,
                        always_regenerate: false,
                    },
                    StructField {
                        field_name: "age".to_string(),
                        field_type: FieldType::Option(Box::new(FieldType::I32)),
                        validators: vec![],
                        edge_config: None,
                        define_config: None,
                        format: None,
                        always_regenerate: false,
                    },
                ],
                validators: vec![],
            },
        );

        let enums = HashMap::new();
        let output = generate_effect_schema_string(&structs, &enums, true);

        assert!(output.contains("export class User"));
        assert!(output.contains("Schema.Class"));
        assert!(output.contains("username"));
        assert!(output.contains("email"));
        assert!(output.contains("age"));
        assert!(output.contains("minLength(3"));
        assert!(output.contains("maxLength(50"));
        assert!(output.contains("UserEncoded"));
        assert!(output.contains("export type UserType"));
    }
}
