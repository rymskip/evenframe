use crate::default::field_type_to_default_value;
use crate::error::{EvenframeError, Result};
use crate::types::{
    EnumRepresentation, FieldType, StructConfig, StructField, TaggedUnion, Variant, VariantData,
};
use crate::typesync::doc_comment::format_jsdoc;
use crate::typesync::foreign_ts::{RECORD_LINK, fill};
use crate::typesync::js_checks::{self, JsCheck, LengthCheck, ONE_CHARACTER, string_literal};
use crate::typesync::map_key::{BOOL_KEYS, MapKey};
use crate::validator::string_rules::StringRule;
use crate::validator::{
    ArrayValidator, BigDecimalValidator, BigIntValidator, DateValidator, DurationValidator,
    NumberValidator, StringValidator, Validator, bounds,
};
use convert_case::{Case, Casing};
use std::collections::BTreeMap;
use tracing;

/// Converts a single enum variant into its ArkType representation,
/// respecting the serde enum representation strategy. A struct variant's
/// fields are written inline, as serde writes them.
fn variant_to_arktype(
    variant: &Variant,
    representation: &EnumRepresentation,
    enums: &BTreeMap<String, TaggedUnion>,
    registry: &crate::types::ForeignTypeRegistry,
    helpers: &mut Helpers,
) -> Result<String> {
    let tag_entry = |tag: &str| format!("{tag}: ['===', '{}']", variant.name);
    let Some(variant_data) = &variant.data else {
        return Ok(match representation {
            EnumRepresentation::InternallyTagged { tag }
            | EnumRepresentation::AdjacentlyTagged { tag, .. } => {
                format!("{{ {} }}", tag_entry(tag))
            }
            EnumRepresentation::ExternallyTagged | EnumRepresentation::Untagged => {
                format!("['===', '{}']", variant.name)
            }
        });
    };
    let payload = match variant_data {
        VariantData::InlineStruct(inline) => {
            let entries = struct_field_entries(&inline.fields, enums, registry, helpers)?;
            if let EnumRepresentation::InternallyTagged { tag } = representation {
                // The tag is a field of the variant's own object.
                let mut merged = vec![tag_entry(tag)];
                merged.extend(entries);
                return Ok(format!("{{ {} }}", merged.join(", ")));
            }
            format!("{{ {} }}", entries.join(", "))
        }
        VariantData::DataStructureRef(field_type) => {
            field_type_to_arktype(field_type, enums, registry)?
        }
    };
    Ok(match representation {
        EnumRepresentation::ExternallyTagged => format!("{{ {}: {payload} }}", variant.name),
        // serde writes the tag into the struct or map the variant holds.
        EnumRepresentation::InternallyTagged { tag } => {
            format!("[{{ {} }}, '&', {payload}]", tag_entry(tag))
        }
        EnumRepresentation::AdjacentlyTagged { tag, content } => {
            format!("{{ {}, {content}: {payload} }}", tag_entry(tag))
        }
        EnumRepresentation::Untagged => payload,
    })
}

/// A struct's fields as ArkType object entries, validators applied.
fn struct_field_entries(
    fields: &[StructField],
    enums: &BTreeMap<String, TaggedUnion>,
    registry: &crate::types::ForeignTypeRegistry,
    helpers: &mut Helpers,
) -> Result<Vec<String>> {
    fields
        .iter()
        .map(|field| {
            Ok(format!(
                "{}: {}",
                field.field_name.to_case(Case::Camel),
                field_arktype(field, enums, registry, helpers)?
            ))
        })
        .collect()
}

fn field_type_to_arktype(
    field_type: &FieldType,
    enums: &BTreeMap<String, TaggedUnion>,
    registry: &crate::types::ForeignTypeRegistry,
) -> Result<String> {
    tracing::trace!(field_type = ?field_type, "Converting field type to Arktype");
    Ok(match field_type {
        FieldType::String => "'string'".to_string(),
        FieldType::Char => format!("new RegExp({})", string_literal(ONE_CHARACTER)?),
        FieldType::Bool => "'boolean'".to_string(),
        FieldType::Unit => "'null'".to_string(),
        FieldType::F32 | FieldType::F64 => "'number'".to_string(),
        FieldType::I8
        | FieldType::I16
        | FieldType::I32
        | FieldType::I64
        | FieldType::I128
        | FieldType::Isize => "'number'".to_string(),
        FieldType::U8
        | FieldType::U16
        | FieldType::U32
        | FieldType::U64
        | FieldType::U128
        | FieldType::Usize => "'number'".to_string(),

        FieldType::Tuple(types) => format!(
            "[{}]",
            types
                .iter()
                .map(|item| field_type_to_arktype(item, enums, registry))
                .collect::<Result<Vec<String>>>()?
                .join(", ")
        ),

        FieldType::Struct(fields) => format!(
            "{{ {} }}",
            fields
                .iter()
                .map(|(name, field_type)| {
                    Ok(format!(
                        "{}: {}",
                        name,
                        field_type_to_arktype(field_type, enums, registry)?
                    ))
                })
                .collect::<Result<Vec<String>>>()?
                .join(", ")
        ),

        FieldType::Option(inner) => format!(
            "[[{}, '|', 'undefined'], '|', 'null']",
            field_type_to_arktype(inner, enums, registry)?
        ),

        FieldType::Vec(inner) => {
            format!("[{}, '[]']", field_type_to_arktype(inner, enums, registry)?)
        }

        FieldType::HashMap(key, value) | FieldType::BTreeMap(key, value) => map_to_arktype(
            key,
            &field_type_to_arktype(value, enums, registry)?,
            enums,
            registry,
        )?,

        FieldType::RecordLink(inner) => {
            let linked = field_type_to_arktype(inner, enums, registry)?;
            match registry
                .lookup(RECORD_LINK)
                .and_then(|record_link| record_link.arktype.as_ref())
            {
                Some(mapping) => fill(&mapping.type_expr, &[linked]),
                None => format!(r#"[{linked}, "|",  "string"]"#),
            }
        }

        FieldType::Other(type_name) => match registry.lookup(type_name) {
            Some(foreign) => {
                let mapping = foreign.arktype.as_ref().ok_or_else(|| {
                    EvenframeError::type_sync(format!(
                        "the foreign type `{type_name}` has no arktype mapping"
                    ))
                })?;
                mapping.type_expr.clone()
            }
            // Every struct and enum is a scope alias under its PascalCase name.
            None => format!("'{}'", type_name.to_case(Case::Pascal)),
        },
    })
}

/// A map as an ArkType object that rejects every key serde would not write.
/// A map keyed by a bool or an enum holds any subset of its values, so each
/// value is an optional key; any other key is an index signature.
fn map_to_arktype(
    key: &FieldType,
    value: &str,
    enums: &BTreeMap<String, TaggedUnion>,
    registry: &crate::types::ForeignTypeRegistry,
) -> Result<String> {
    let index = |definition: &str| -> Result<Vec<String>> {
        Ok(vec![format!(
            "{}: {value}",
            string_literal(&format!("[{definition}]"))?
        )])
    };
    let optional = |names: &mut dyn Iterator<Item = &str>| -> Result<Vec<String>> {
        names
            .map(|name| Ok(format!("{}: {value}", string_literal(&format!("{name}?"))?)))
            .collect()
    };
    let entries = match MapKey::require(key)? {
        MapKey::Text => index("string")?,
        MapKey::Bool => optional(&mut BOOL_KEYS.into_iter())?,
        MapKey::Char => index(&format!("/{ONE_CHARACTER}/"))?,
        MapKey::Integer => index("string.integer")?,
        MapKey::Named(type_name) => match registry.lookup(type_name) {
            Some(foreign) => {
                let definition = foreign
                    .arktype
                    .as_ref()
                    .map(|mapping| mapping.type_expr.as_str())
                    .unwrap_or_default();
                index(
                    definition
                        .strip_prefix('\'')
                        .and_then(|inner| inner.strip_suffix('\''))
                        .ok_or_else(|| {
                            EvenframeError::type_sync(format!(
                                "map key `{type_name}` maps to the ArkType definition \
                                 {definition:?}, which is not a quoted string definition an \
                                 index signature can hold"
                            ))
                        })?,
                )?
            }
            None => {
                let pascal = type_name.to_case(Case::Pascal);
                let tagged_union = enums
                    .values()
                    .map(TaggedUnion::effective)
                    .find(|tagged_union| tagged_union.enum_name.to_case(Case::Pascal) == pascal)
                    .ok_or_else(|| {
                        EvenframeError::type_sync(format!(
                            "map key `{type_name}` is neither a foreign type nor a scanned enum"
                        ))
                    })?;
                optional(
                    &mut tagged_union
                        .variants
                        .iter()
                        .map(|variant| variant.effective().name.as_str()),
                )?
            }
        },
    };
    Ok(format!(
        "{{ '+': 'reject'{} }}",
        entries
            .iter()
            .map(|entry| format!(", {entry}"))
            .collect::<String>()
    ))
}

pub fn generate_arktype_type_string(
    structs: &BTreeMap<String, StructConfig>,
    enums: &BTreeMap<String, TaggedUnion>,
    registry: &crate::types::ForeignTypeRegistry,
) -> Result<String> {
    tracing::info!(
        struct_count = structs.len(),
        enum_count = enums.len(),
        "Generating Arktype type string"
    );
    let mut output = String::new();
    let mut scope_output = String::new();
    let mut types_output = String::new();
    let mut defaults_output = String::new();
    let mut helpers = Helpers::default();

    scope_output.push_str("export const bindings = scope({\n\n");

    // First, process all enums. Use `effective()` so overrides replace
    // the scanned type.
    for schema_enum in enums.values() {
        // `resolve_only` types are kept in the maps for reference resolution
        // (below) but are not emitted as their own interface.
        if schema_enum.resolve_only {
            continue;
        }
        let schema_enum = schema_enum.effective();
        // Write doc comment if present
        if let Some(ref doc) = schema_enum.doccom {
            scope_output.push_str(&format_jsdoc(doc, ""));
        }

        // Write the Arktype binding name
        scope_output.push_str(&format!(
            "{}: ",
            schema_enum.enum_name.to_case(Case::Pascal)
        ));

        // We'll accumulate the "nesting" into this string.
        let mut union_ast = String::new();

        for (i, variant) in schema_enum.variants.iter().enumerate() {
            // Convert the variant into either a data type or a literal union piece,
            // respecting the serde enum representation.
            let item_str = variant_to_arktype(
                variant,
                &schema_enum.representation,
                enums,
                registry,
                &mut helpers,
            )?;

            // If this is our first variant, it becomes the entire union so far,
            // otherwise we nest the "union so far" together with the new item.
            if i == 0 {
                union_ast = item_str;
            } else {
                union_ast = format!("[{}, '|', {}]", union_ast, item_str);
            }
        }

        // Now write out the final folded union string in your scope output
        scope_output.push_str(&format!("{},\n", union_ast));

        // And write the corresponding TypeScript type
        types_output.push_str(&format!(
            "export type {} = typeof exported.{}.infer;\n",
            schema_enum.enum_name.to_case(Case::Pascal),
            schema_enum.enum_name.to_case(Case::Pascal)
        ));
    }

    // Then, process all structs. Use `effective()` so overrides replace
    // the scanned type.
    tracing::debug!("Processing structs for Arktype");
    for struct_config in structs.values() {
        // `resolve_only` types are kept in the maps for reference resolution
        // but are not emitted as their own interface.
        if struct_config.resolve_only {
            continue;
        }
        let struct_config = struct_config.effective();
        tracing::trace!(struct_name = %struct_config.struct_name, "Processing struct");
        let type_name = struct_config.struct_name.to_case(Case::Pascal);

        // Write doc comment if present
        if let Some(ref doc) = struct_config.doccom {
            scope_output.push_str(&format_jsdoc(doc, ""));
        }

        scope_output.push_str(&format!("{}: {{\n", type_name));
        defaults_output.push_str(&format!(
            "export const default{}: {} = {{\n",
            &type_name, &type_name
        ));

        for (index, field) in struct_config.fields.iter().enumerate() {
            let field_name = field.field_name.to_case(Case::Camel);

            // Write field doc comment if present
            if let Some(ref doc) = field.doccom {
                scope_output.push_str(&format_jsdoc(doc, "  "));
            }

            scope_output.push_str(&format!(
                "  {}: {}",
                field_name,
                field_arktype(field, enums, registry, &mut helpers)?
            ));
            defaults_output.push_str(&format!(
                "{}: {}",
                field_name,
                field_type_to_default_value(&field.field_type, structs, enums, registry)?
            ));
            // Add a comma if it's not the last field
            if index + 1 < struct_config.fields.len() {
                scope_output.push_str(",\n");
                defaults_output.push_str(",\n");
            } else {
                scope_output.push('\n');
            }
        }

        scope_output.push_str("},\n");
        defaults_output.push_str("\n};\n");
        types_output.push_str(&format!(
            "export type {} = typeof exported.{}.infer;\n",
            type_name, type_name
        ));
    }
    scope_output.push_str("\n});\n\n");

    if helpers.compare_decimal {
        output.push_str(js_checks::COMPARE_DECIMAL);
    }
    if helpers.duration_nanos {
        output.push_str(js_checks::DURATION_NANOS);
    }
    // The defaults are annotated with these types, so they are always emitted.
    output.push_str(&format!(
        "{scope_output}const exported = bindings.export();\n\n{defaults_output}\n{types_output}"
    ));

    tracing::info!(
        output_length = output.len(),
        "Arktype type string generation complete"
    );
    Ok(output)
}

// ----- Validators ------------------------------------------------------------

/// Helper functions the generated file needs.
#[derive(Debug, Default)]
struct Helpers {
    compare_decimal: bool,
    duration_nanos: bool,
}

/// One validator as ArkType: a definition to intersect or pipe into, or a
/// predicate over `v` to narrow with.
enum Step {
    Type(String),
    Narrow(String),
}

/// A field's ArkType definition: its type, narrowed and piped through its
/// validators. An optional field's validators apply to the present value.
fn field_arktype(
    field: &StructField,
    enums: &BTreeMap<String, TaggedUnion>,
    registry: &crate::types::ForeignTypeRegistry,
    helpers: &mut Helpers,
) -> Result<String> {
    let validated = |field_type: &FieldType, helpers: &mut Helpers| {
        validated_arktype(
            field_type_to_arktype(field_type, enums, registry)?,
            &field.validators,
            &field.field_name,
            helpers,
        )
    };
    match &field.field_type {
        FieldType::Option(inner) if !field.validators.is_empty() => Ok(format!(
            "[[{}, '|', 'undefined'], '|', 'null']",
            validated(inner, helpers)?
        )),
        field_type => validated(field_type, helpers),
    }
}

/// `base` constrained by `validators` in order. A parse morph replaces the
/// input type with its keyword; after any morph, later steps pipe.
fn validated_arktype(
    base: String,
    validators: &[Validator],
    field_name: &str,
    helpers: &mut Helpers,
) -> Result<String> {
    bounds::check_validators(validators)
        .map_err(|problem| EvenframeError::config(format!("field '{field_name}': {problem}")))?;
    let mut definition = base;
    let mut piped = false;
    for validator in validators {
        let steps = match validator {
            Validator::StringValidator(string_validator) => match string_validator.rule() {
                StringRule::Carrier => continue,
                StringRule::Parse(_) => {
                    let keyword = keyword(string_validator)?;
                    definition = if matches!(string_validator, StringValidator::JsonParse) {
                        format!("['{keyword}', '|>', {definition}]")
                    } else {
                        format!("'{keyword}'")
                    };
                    piped = true;
                    continue;
                }
                StringRule::Transform(_) => {
                    definition = format!("[{definition}, '|>', '{}']", keyword(string_validator)?);
                    piped = true;
                    continue;
                }
                StringRule::Check => vec![string_step(string_validator)?],
            },
            Validator::NumberValidator(number_validator) => number_steps(number_validator),
            Validator::ArrayValidator(array_validator) => vec![Step::Type(match array_validator {
                ArrayValidator::MinItems(count) => format!("'unknown[] >= {count}'"),
                ArrayValidator::MaxItems(count) => format!("'unknown[] <= {count}'"),
                ArrayValidator::ItemsCount(count) => format!("'unknown[] == {count}'"),
            })],
            Validator::DateValidator(date_validator) => vec![date_step(date_validator)?],
            Validator::BigIntValidator(big_int_validator) => {
                vec![big_int_step(big_int_validator)?]
            }
            Validator::BigDecimalValidator(decimal_validator) => {
                helpers.compare_decimal = true;
                vec![decimal_step(decimal_validator)?]
            }
            Validator::DurationValidator(duration_validator) => {
                helpers.duration_nanos = true;
                vec![duration_step(duration_validator)?]
            }
        };
        for step in steps {
            definition = match step {
                Step::Type(constraint) => {
                    let operator = if piped { "|>" } else { "&" };
                    format!("[{definition}, '{operator}', {constraint}]")
                }
                Step::Narrow(predicate) => format!("[{definition}, ':', (v) => {predicate}]"),
            };
        }
    }
    Ok(definition)
}

fn keyword(validator: &StringValidator) -> Result<&'static str> {
    validator
        .arktype_keyword()
        .ok_or_else(|| EvenframeError::config(format!("{validator:?} has no ArkType keyword")))
}

fn string_step(validator: &StringValidator) -> Result<Step> {
    if let Some(keyword) = validator.arktype_keyword() {
        return Ok(Step::Type(format!("'{keyword}'")));
    }
    Ok(match js_checks::string_check(validator)? {
        Some(JsCheck::Pattern(source)) => {
            Step::Type(format!("new RegExp({})", string_literal(&source)?))
        }
        Some(JsCheck::Predicate(predicate)) => Step::Narrow(predicate),
        Some(JsCheck::Length(LengthCheck::Exactly(length))) => {
            Step::Type(format!("'string == {length}'"))
        }
        Some(JsCheck::Length(LengthCheck::AtLeast(length))) => {
            Step::Type(format!("'string >= {length}'"))
        }
        Some(JsCheck::Length(LengthCheck::AtMost(length))) => {
            Step::Type(format!("'string <= {length}'"))
        }
        None => {
            return Err(EvenframeError::config(format!(
                "{validator:?} is not a string check"
            )));
        }
    })
}

fn number_steps(validator: &NumberValidator) -> Vec<Step> {
    let range = |constraint: String| vec![Step::Type(format!("'{constraint}'"))];
    match validator {
        NumberValidator::GreaterThan(bound) => range(format!("number > {}", bound.0)),
        NumberValidator::GreaterThanOrEqualTo(bound) => range(format!("number >= {}", bound.0)),
        NumberValidator::LessThan(bound) => range(format!("number < {}", bound.0)),
        NumberValidator::LessThanOrEqualTo(bound) => range(format!("number <= {}", bound.0)),
        NumberValidator::Between(start, end) => {
            range(format!("{} <= number <= {}", start.0, end.0))
        }
        NumberValidator::Int => range("number.integer".to_owned()),
        NumberValidator::NonNaN => vec![Step::Narrow("!Number.isNaN(v)".to_owned())],
        NumberValidator::Finite => vec![Step::Narrow("Number.isFinite(v)".to_owned())],
        NumberValidator::Positive => range("number > 0".to_owned()),
        NumberValidator::NonNegative => range("number >= 0".to_owned()),
        NumberValidator::Negative => range("number < 0".to_owned()),
        NumberValidator::NonPositive => range("number <= 0".to_owned()),
        NumberValidator::MultipleOf(divisor) => vec![Step::Narrow(format!(
            "((value: number, divisor: number) => {{ const places = Math.max(...[value, divisor].map((n) => (String(n).split(\".\")[1] ?? \"\").length)); const scale = 10 ** places; return Math.round(value * scale) % Math.round(divisor * scale) === 0; }})(v, {})",
            divisor.0
        ))],
        NumberValidator::Uint8 => vec![
            Step::Type("'number.integer'".to_owned()),
            Step::Type("'0 <= number <= 255'".to_owned()),
        ],
    }
}

/// Milliseconds since the epoch for a date bound.
fn date_millis(bound: &str) -> Result<i64> {
    bounds::date(bound)
        .map(|instant| instant.timestamp_millis())
        .map_err(EvenframeError::config)
}

fn date_step(validator: &DateValidator) -> Result<Step> {
    let instant = "new Date(v).valueOf()";
    Ok(Step::Narrow(match validator {
        DateValidator::ValidDate => format!("!Number.isNaN({instant})"),
        DateValidator::GreaterThanDate(bound) => format!("{instant} > {}", date_millis(bound)?),
        DateValidator::GreaterThanOrEqualToDate(bound) => {
            format!("{instant} >= {}", date_millis(bound)?)
        }
        DateValidator::LessThanDate(bound) => format!("{instant} < {}", date_millis(bound)?),
        DateValidator::LessThanOrEqualToDate(bound) => {
            format!("{instant} <= {}", date_millis(bound)?)
        }
        DateValidator::BetweenDate(start, end) => format!(
            "((millis) => millis >= {} && millis <= {})({instant})",
            date_millis(start)?,
            date_millis(end)?
        ),
    }))
}

fn big_int_literal(bound: &str) -> Result<String> {
    bounds::big_int(bound)
        .map(|number| format!("{number}n"))
        .map_err(EvenframeError::config)
}

fn big_int_step(validator: &BigIntValidator) -> Result<Step> {
    let compare = |condition: String| {
        format!(
            "(() => {{ try {{ const n = BigInt(v); return {condition}; }} catch {{ return false; }} }})()"
        )
    };
    Ok(Step::Narrow(match validator {
        BigIntValidator::GreaterThanBigInt(bound) => {
            compare(format!("n > {}", big_int_literal(bound)?))
        }
        BigIntValidator::GreaterThanOrEqualToBigInt(bound) => {
            compare(format!("n >= {}", big_int_literal(bound)?))
        }
        BigIntValidator::LessThanBigInt(bound) => {
            compare(format!("n < {}", big_int_literal(bound)?))
        }
        BigIntValidator::LessThanOrEqualToBigInt(bound) => {
            compare(format!("n <= {}", big_int_literal(bound)?))
        }
        BigIntValidator::BetweenBigInt(start, end) => compare(format!(
            "n >= {} && n <= {}",
            big_int_literal(start)?,
            big_int_literal(end)?
        )),
        BigIntValidator::PositiveBigInt => compare("n > 0n".to_owned()),
        BigIntValidator::NonNegativeBigInt => compare("n >= 0n".to_owned()),
        BigIntValidator::NegativeBigInt => compare("n < 0n".to_owned()),
        BigIntValidator::NonPositiveBigInt => compare("n <= 0n".to_owned()),
    }))
}

fn decimal_literal(bound: &str) -> Result<String> {
    bounds::decimal(bound).map_err(EvenframeError::config)?;
    js_checks::string_literal(bound)
}

fn decimal_step(validator: &BigDecimalValidator) -> Result<Step> {
    let compare = |bound: &str, condition: &str| -> Result<String> {
        Ok(format!(
            "compareDecimal(String(v), {}) {condition}",
            decimal_literal(bound)?
        ))
    };
    Ok(Step::Narrow(match validator {
        BigDecimalValidator::GreaterThanBigDecimal(bound) => compare(bound, "> 0")?,
        BigDecimalValidator::GreaterThanOrEqualToBigDecimal(bound) => compare(bound, ">= 0")?,
        BigDecimalValidator::LessThanBigDecimal(bound) => compare(bound, "< 0")?,
        BigDecimalValidator::LessThanOrEqualToBigDecimal(bound) => compare(bound, "<= 0")?,
        BigDecimalValidator::BetweenBigDecimal(start, end) => {
            format!("{} && {}", compare(start, ">= 0")?, compare(end, "<= 0")?)
        }
        BigDecimalValidator::PositiveBigDecimal => {
            "compareDecimal(String(v), \"0\") > 0".to_owned()
        }
        BigDecimalValidator::NonNegativeBigDecimal => {
            "compareDecimal(String(v), \"0\") >= 0".to_owned()
        }
        BigDecimalValidator::NegativeBigDecimal => {
            "compareDecimal(String(v), \"0\") < 0".to_owned()
        }
        BigDecimalValidator::NonPositiveBigDecimal => {
            "compareDecimal(String(v), \"0\") <= 0".to_owned()
        }
    }))
}

fn duration_literal(bound: &str) -> Result<String> {
    bounds::duration(bound)
        .map(|nanos| format!("{nanos}n"))
        .map_err(EvenframeError::config)
}

fn duration_step(validator: &DurationValidator) -> Result<Step> {
    let compare =
        |condition: String| format!("((nanos) => nanos !== null && {condition})(durationNanos(v))");
    Ok(Step::Narrow(match validator {
        DurationValidator::GreaterThanDuration(bound) => {
            compare(format!("nanos > {}", duration_literal(bound)?))
        }
        DurationValidator::GreaterThanOrEqualToDuration(bound) => {
            compare(format!("nanos >= {}", duration_literal(bound)?))
        }
        DurationValidator::LessThanDuration(bound) => {
            compare(format!("nanos < {}", duration_literal(bound)?))
        }
        DurationValidator::LessThanOrEqualToDuration(bound) => {
            compare(format!("nanos <= {}", duration_literal(bound)?))
        }
        DurationValidator::BetweenDuration(start, end) => compare(format!(
            "nanos >= {} && nanos <= {}",
            duration_literal(start)?,
            duration_literal(end)?
        )),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ForeignTypeRegistry;

    #[test]
    fn maps_become_index_signatures_keyed_as_json_writes_them() {
        let registry = ForeignTypeRegistry::default();
        let by_rank = FieldType::BTreeMap(
            Box::new(FieldType::U32),
            Box::new(FieldType::Option(Box::new(FieldType::String))),
        );
        assert_eq!(
            field_type_to_arktype(&by_rank, &BTreeMap::new(), &registry).unwrap(),
            r#"{ '+': 'reject', "[string.integer]": [['string', '|', 'undefined'], '|', 'null'] }"#
        );
    }

    #[test]
    fn a_bool_keyed_map_holds_either_key_or_both() {
        let registry = ForeignTypeRegistry::default();
        let by_flag = FieldType::HashMap(Box::new(FieldType::Bool), Box::new(FieldType::String));
        assert_eq!(
            field_type_to_arktype(&by_flag, &BTreeMap::new(), &registry).unwrap(),
            r#"{ '+': 'reject', "true?": 'string', "false?": 'string' }"#
        );
    }

    #[test]
    fn a_map_key_with_no_json_object_key_is_rejected() {
        let registry = ForeignTypeRegistry::default();
        let by_nothing = FieldType::HashMap(Box::new(FieldType::Unit), Box::new(FieldType::String));
        let error = field_type_to_arktype(&by_nothing, &BTreeMap::new(), &registry)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("a map keyed by `()` has no JSON object key"),
            "{error}"
        );
    }
}
