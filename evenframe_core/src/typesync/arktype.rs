use crate::config::fill;
use crate::error::{EvenframeError, Result};
use crate::types::{EnumRepresentation, FieldType, StructField, Variant, VariantData};
use crate::typesync::config::OutputKind;
use crate::typesync::default_value::field_type_to_default_value;
use crate::typesync::doc_comment::format_jsdoc;
use crate::typesync::foreign_ts::{RecordLinkMapping, record_link_mapping};
use crate::typesync::js_checks::{
    self, JsCheck, LengthCheck, ONE_CHARACTER, object_key, string_literal,
};
use crate::typesync::map_key::{BOOL_KEYS, MapKey};
use crate::typesync::type_index::TypeIndex;
use crate::validator::string_rules::StringRule;
use crate::validator::{
    ArrayValidator, BigIntValidator, DateValidator, NumberValidator, StringValidator, Validator,
    bounds,
};
use convert_case::{Case, Casing};
use tracing;

/// Converts a single enum variant into its ArkType representation,
/// respecting the serde enum representation strategy. A struct variant's
/// fields are written inline, as serde writes them.
fn variant_to_arktype(
    variant: &Variant,
    representation: &EnumRepresentation,
    index: &TypeIndex,
    registry: &crate::types::ForeignTypeRegistry,
    helpers: &mut Helpers,
) -> Result<String> {
    let name = string_literal(variant.serde_name())?;
    let tag_entry =
        |tag: &str| -> Result<String> { Ok(format!("{}: ['===', {name}]", object_key(tag)?)) };
    let Some(variant_data) = &variant.data else {
        return Ok(match representation {
            EnumRepresentation::InternallyTagged { tag }
            | EnumRepresentation::AdjacentlyTagged { tag, .. } => {
                format!("{{ {} }}", tag_entry(tag)?)
            }
            EnumRepresentation::ExternallyTagged => format!("['===', {name}]"),
            // serde writes an untagged unit variant as null.
            EnumRepresentation::Untagged => "'null'".to_owned(),
        });
    };
    let payload = match variant_data {
        VariantData::InlineStruct(inline) => {
            let entries = struct_field_entries(&inline.fields, index, registry, helpers)?;
            if let EnumRepresentation::InternallyTagged { tag } = representation {
                // The tag is a field of the variant's own object.
                let mut merged = vec![tag_entry(tag)?];
                merged.extend(entries);
                return Ok(format!("{{ {} }}", merged.join(", ")));
            }
            format!("{{ {} }}", entries.join(", "))
        }
        VariantData::DataStructureRef(field_type) => payload_arktype(
            field_type,
            &variant.element_validators,
            variant.serde_name(),
            index,
            registry,
            helpers,
        )?,
    };
    Ok(match representation {
        EnumRepresentation::ExternallyTagged => {
            format!("{{ {}: {payload} }}", object_key(variant.serde_name())?)
        }
        // serde writes the tag into the struct or map the variant holds.
        EnumRepresentation::InternallyTagged { tag } => {
            format!("[{{ {} }}, '&', {payload}]", tag_entry(tag)?)
        }
        EnumRepresentation::AdjacentlyTagged { tag, content } => {
            format!(
                "{{ {}, {}: {payload} }}",
                tag_entry(tag)?,
                object_key(content)?
            )
        }
        EnumRepresentation::Untagged => payload,
    })
}

/// A tuple payload with each element's validators applied, or a newtype
/// payload with its one element's.
fn payload_arktype(
    field_type: &FieldType,
    element_validators: &[Vec<Validator>],
    owner: &str,
    index: &TypeIndex,
    registry: &crate::types::ForeignTypeRegistry,
    helpers: &mut Helpers,
) -> Result<String> {
    match (field_type, element_validators) {
        (_, []) => field_type_to_arktype(field_type, index, registry),
        (FieldType::Tuple(items), validators) if items.len() == validators.len() => {
            let elements = items
                .iter()
                .zip(validators)
                .enumerate()
                .map(|(position, (item, validators))| {
                    validated_arktype(
                        field_type_to_arktype(item, index, registry)?,
                        validators,
                        &format!("{owner}.{position}"),
                        helpers,
                    )
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(format!("[{}]", elements.join(", ")))
        }
        (single, [validators]) => validated_arktype(
            field_type_to_arktype(single, index, registry)?,
            validators,
            owner,
            helpers,
        ),
        (_, validators) => Err(EvenframeError::config(format!(
            "`{owner}` has validators for {} elements but holds {field_type:?}",
            validators.len()
        ))),
    }
}

/// A struct's fields as ArkType object entries, validators applied.
fn struct_field_entries(
    fields: &[StructField],
    index: &TypeIndex,
    registry: &crate::types::ForeignTypeRegistry,
    helpers: &mut Helpers,
) -> Result<Vec<String>> {
    fields
        .iter()
        .map(StructField::effective)
        .map(|field| {
            Ok(format!(
                "{}: {}",
                field_key(field)?,
                field_arktype(field, index, registry, helpers)?
            ))
        })
        .collect()
}

/// A TypeScript field key; ArkType marks an optional key with `?`.
fn field_key(field: &StructField) -> Result<String> {
    if field.wire.serde_optional {
        string_literal(&format!("{}?", field.ts_name()))
    } else {
        object_key(&field.ts_name())
    }
}

fn field_type_to_arktype(
    field_type: &FieldType,
    index: &TypeIndex,
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
        // serde writes whole seconds and the nanoseconds past them, and
        // rejects any other key.
        FieldType::Duration => "{ '+': 'reject', secs: 'number.integer >= 0', nanos: '0 <= number.integer < 1000000000' }".to_string(),

        FieldType::Tuple(types) => format!(
            "[{}]",
            types
                .iter()
                .map(|item| field_type_to_arktype(item, index, registry))
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
                        field_type_to_arktype(field_type, index, registry)?
                    ))
                })
                .collect::<Result<Vec<String>>>()?
                .join(", ")
        ),

        FieldType::Option(inner) => format!(
            "[[{}, '|', 'undefined'], '|', 'null']",
            field_type_to_arktype(inner, index, registry)?
        ),

        FieldType::Vec(inner) => {
            format!("[{}, '[]']", field_type_to_arktype(inner, index, registry)?)
        }

        FieldType::HashMap(key, value) | FieldType::BTreeMap(key, value) => map_to_arktype(
            key,
            &field_type_to_arktype(value, index, registry)?,
            index,
            registry,
        )?,

        FieldType::RecordLink(inner) => {
            let linked = field_type_to_arktype(inner, index, registry)?;
            match record_link_mapping(registry, OutputKind::Arktype, |foreign| foreign.arktype.as_ref())? {
                RecordLinkMapping::Configured(mapping) => fill(&mapping.type_expr, &[linked]),
                RecordLinkMapping::Own { record_id } => {
                    format!(r#"[{linked}, "|", {}]"#, record_id.type_expr)
                }
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
    index: &TypeIndex,
    registry: &crate::types::ForeignTypeRegistry,
) -> Result<String> {
    let index_signature = |definition: &str| -> Result<Vec<String>> {
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
    let entries = match MapKey::require(index.underlying(key))? {
        MapKey::Text => index_signature("string")?,
        MapKey::Bool => optional(&mut BOOL_KEYS.into_iter())?,
        MapKey::Char => index_signature(&format!("/{ONE_CHARACTER}/"))?,
        MapKey::Integer => index_signature("string.integer")?,
        MapKey::Named(type_name) => match registry.lookup(type_name) {
            Some(foreign) => {
                let definition = foreign
                    .arktype
                    .as_ref()
                    .map(|mapping| mapping.type_expr.as_str())
                    .unwrap_or_default();
                index_signature(
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
                let tagged_union = index.effective_enum_named(type_name).ok_or_else(|| {
                    EvenframeError::type_sync(format!(
                        "map key `{type_name}` is neither a foreign type nor a scanned enum"
                    ))
                })?;
                optional(
                    &mut tagged_union
                        .variants
                        .iter()
                        .map(|variant| variant.effective().serde_name()),
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
    index: &TypeIndex,
    registry: &crate::types::ForeignTypeRegistry,
) -> Result<String> {
    tracing::info!(
        struct_count = index.structs().len(),
        enum_count = index.enums().len(),
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
    for schema_enum in index.enums().values() {
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
                variant.serde_representation(&schema_enum.representation),
                index,
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
    for struct_config in index.structs().values() {
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

        let mut entries = Vec::new();
        let mut defaults = Vec::new();
        let mut held_values = Vec::new();
        let mut index_values = Vec::new();
        let mut intersections = Vec::new();
        for field in struct_config.fields.iter().map(StructField::effective) {
            if field.wire.serde_flatten {
                // serde writes what the field holds beside its siblings.
                match field.field_type.flattened_map_value() {
                    Some(value) => {
                        index_values.push(field_type_to_arktype(value, index, registry)?)
                    }
                    None => {
                        intersections.push(field_type_to_arktype(
                            &field.field_type,
                            index,
                            registry,
                        )?);
                        defaults.push(format!(
                            "...{}",
                            field_type_to_default_value(&field.field_type, index, registry)?
                        ));
                    }
                }
                continue;
            }
            let doc = field
                .doccom
                .as_ref()
                .map(|doc| format_jsdoc(doc, "  "))
                .unwrap_or_default();
            let definition = field_arktype(field, index, registry, &mut helpers)?;
            held_values.push(definition.clone());
            entries.push(format!("{doc}  {}: {definition}", field_key(field)?));
            defaults.push(format!(
                "{}: {}",
                object_key(&field.ts_name())?,
                field_type_to_default_value(&field.field_type, index, registry)?
            ));
        }
        if !index_values.is_empty() {
            // A key of the map may share an object with every named field, so
            // its value admits theirs too.
            let values = index_values
                .into_iter()
                .chain(held_values)
                .reduce(|union, value| format!("[{union}, '|', {value}]"))
                .unwrap_or_default();
            entries.push(format!("  \"[string]\": {values}"));
        }
        let object = if entries.is_empty() {
            "{\n}".to_owned()
        } else {
            format!("{{\n{}\n}}", entries.join(",\n"))
        };
        let definition = intersections.into_iter().fold(object, |definition, held| {
            format!("[{definition}, '&', {held}]")
        });
        scope_output.push_str(&format!("{type_name}: {definition},\n"));
        defaults_output.push_str(&format!(
            "export const default{type_name}: {type_name} = {{\n{}\n}};\n",
            defaults.join(",\n")
        ));
        types_output.push_str(&format!(
            "export type {} = typeof exported.{}.infer;\n",
            type_name, type_name
        ));
    }

    for (type_name, newtype) in index.named_newtypes() {
        if newtype.resolve_only {
            continue;
        }
        if let Some(ref doc) = newtype.doccom {
            scope_output.push_str(&format_jsdoc(doc, ""));
        }
        scope_output.push_str(&newtype_entries(
            type_name,
            newtype,
            index,
            registry,
            &mut helpers,
        )?);
        types_output.push_str(&format!(
            "export type {type_name} = typeof exported.{type_name}.infer;\n"
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

/// The suffix of the private scope alias that holds a newtype's value before
/// it is branded. Scope keys are PascalCase type names, which never hold `__`.
const NEWTYPE_VALUE_SUFFIX: &str = "__value";

/// A newtype's scope entries. ArkType brands only in string syntax, so a
/// newtype whose value is not a plain string definition is defined under a
/// private alias and branded by name.
fn newtype_entries(
    type_name: &str,
    newtype: &crate::types::NewtypeConfig,
    index: &TypeIndex,
    registry: &crate::types::ForeignTypeRegistry,
    helpers: &mut Helpers,
) -> Result<String> {
    let inner = payload_arktype(
        &newtype.inner,
        &newtype.element_validators,
        type_name,
        index,
        registry,
        helpers,
    )?;
    let value = renewed(
        validated_arktype(inner, &newtype.validators, type_name, helpers)?,
        &newtype.inner,
        &newtype.validators,
        index,
        registry,
    )?;
    if newtype.kind == crate::types::NewtypeKind::Alias {
        return Ok(format!("{type_name}: {value},\n"));
    }
    let plain = value
        .strip_prefix('\'')
        .and_then(|definition| definition.strip_suffix('\''))
        .filter(|definition| !definition.contains(['\'', '#']));
    Ok(match plain {
        Some(definition) => format!(
            "{type_name}: {},\n",
            string_literal(&format!("{definition}#{type_name}"))?
        ),
        None => {
            let private = format!("{type_name}{NEWTYPE_VALUE_SUFFIX}");
            format!(
                "{}: {value},\n{type_name}: {},\n",
                string_literal(&format!("#{private}"))?,
                string_literal(&format!("{private}#{type_name}"))?
            )
        }
    })
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
    index: &TypeIndex,
    registry: &crate::types::ForeignTypeRegistry,
    helpers: &mut Helpers,
) -> Result<String> {
    let validated = |field_type: &FieldType, helpers: &mut Helpers| {
        renewed(
            validated_arktype(
                field_type_to_arktype(field_type, index, registry)?,
                &field.validators,
                &field.field_name,
                helpers,
            )?,
            field_type,
            &field.validators,
            index,
            registry,
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

/// `definition`, piped back into the newtype `value_type` names once a
/// validator rewrote the value, which checks the newtype again and keeps its
/// brand, as the Rust deserializer does.
fn renewed(
    definition: String,
    value_type: &FieldType,
    validators: &[Validator],
    index: &TypeIndex,
    registry: &crate::types::ForeignTypeRegistry,
) -> Result<String> {
    match value_type {
        FieldType::Other(name)
            if validators.iter().any(Validator::rewrites)
                && index.newtype_named(name).is_some() =>
        {
            Ok(format!(
                "[{definition}, '|>', {}]",
                field_type_to_arktype(value_type, index, registry)?
            ))
        }
        _ => Ok(definition),
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
                vec![Step::Narrow(js_checks::decimal_check(decimal_validator)?)]
            }
            Validator::DurationValidator(duration_validator) => {
                helpers.duration_nanos = true;
                vec![Step::Narrow(js_checks::duration_check(duration_validator)?)]
            }
        };
        for step in steps {
            definition = match step {
                Step::Type(constraint) => {
                    let operator = if piped { "|>" } else { "&" };
                    format!("[{definition}, '{operator}', {constraint}]")
                }
                Step::Narrow(predicate) => format!("[{definition}, ':', (value) => {predicate}]"),
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
        Some(JsCheck::Pattern { source, flags }) => Step::Type(js_checks::regexp(&source, &flags)?),
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
        NumberValidator::NonNaN => vec![Step::Narrow("!Number.isNaN(value)".to_owned())],
        NumberValidator::Finite => vec![Step::Narrow("Number.isFinite(value)".to_owned())],
        NumberValidator::Positive => range("number > 0".to_owned()),
        NumberValidator::NonNegative => range("number >= 0".to_owned()),
        NumberValidator::Negative => range("number < 0".to_owned()),
        NumberValidator::NonPositive => range("number <= 0".to_owned()),
        NumberValidator::MultipleOf(divisor) => {
            vec![Step::Narrow(js_checks::multiple_of_check(divisor.0))]
        }
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
    let instant = "new Date(value).valueOf()";
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
            "(() => {{ try {{ const big = BigInt(value); return {condition}; }} catch {{ return false; }} }})()"
        )
    };
    Ok(Step::Narrow(match validator {
        BigIntValidator::GreaterThanBigInt(bound) => {
            compare(format!("big > {}", big_int_literal(bound)?))
        }
        BigIntValidator::GreaterThanOrEqualToBigInt(bound) => {
            compare(format!("big >= {}", big_int_literal(bound)?))
        }
        BigIntValidator::LessThanBigInt(bound) => {
            compare(format!("big < {}", big_int_literal(bound)?))
        }
        BigIntValidator::LessThanOrEqualToBigInt(bound) => {
            compare(format!("big <= {}", big_int_literal(bound)?))
        }
        BigIntValidator::BetweenBigInt(start, end) => compare(format!(
            "big >= {} && big <= {}",
            big_int_literal(start)?,
            big_int_literal(end)?
        )),
        BigIntValidator::PositiveBigInt => compare("big > 0n".to_owned()),
        BigIntValidator::NonNegativeBigInt => compare("big >= 0n".to_owned()),
        BigIntValidator::NegativeBigInt => compare("big < 0n".to_owned()),
        BigIntValidator::NonPositiveBigInt => compare("big <= 0n".to_owned()),
    }))
}

#[cfg(test)]
mod tests {
    use super::{FieldType, TypeIndex, field_type_to_arktype};
    use crate::types::ForeignTypeRegistry;
    use std::collections::BTreeMap;

    #[test]
    fn fields_follow_ts_policy_and_explicit_serde_names() {
        use crate::typesync::config::TsNames;
        use crate::typesync::naming::{apply_struct, wire_named_structs};
        for (policy, first_name) in [
            (TsNames::Default, "firstName"),
            (TsNames::RespectSerde, "first_name"),
        ] {
            let mut structs = wire_named_structs();
            for struct_config in structs.values_mut() {
                apply_struct(struct_config, policy).expect("apply TS naming policy");
            }
            let output = super::generate_arktype_type_string(
                &TypeIndex::new(&structs, &BTreeMap::new()).unwrap(),
                &ForeignTypeRegistry::default(),
            )
            .unwrap();
            assert!(
                output.contains(&format!("  {first_name}: 'string'")),
                "{output}"
            );
            assert!(output.contains("  lastName: 'string'"), "{output}");
            assert!(output.contains("  \"zip-code\": 'string'"), "{output}");
            assert!(output.contains("  \"nickname?\": 'string'"), "{output}");
        }
    }

    #[test]
    fn maps_become_index_signatures_keyed_as_json_writes_them() {
        let registry = ForeignTypeRegistry::default();
        let by_rank = FieldType::BTreeMap(
            Box::new(FieldType::U32),
            Box::new(FieldType::Option(Box::new(FieldType::String))),
        );
        assert_eq!(
            field_type_to_arktype(
                &by_rank,
                &TypeIndex::new(&BTreeMap::new(), &BTreeMap::new()).unwrap(),
                &registry
            )
            .unwrap(),
            r#"{ '+': 'reject', "[string.integer]": [['string', '|', 'undefined'], '|', 'null'] }"#
        );
    }

    #[test]
    fn a_bool_keyed_map_holds_either_key_or_both() {
        let registry = ForeignTypeRegistry::default();
        let by_flag = FieldType::HashMap(Box::new(FieldType::Bool), Box::new(FieldType::String));
        assert_eq!(
            field_type_to_arktype(
                &by_flag,
                &TypeIndex::new(&BTreeMap::new(), &BTreeMap::new()).unwrap(),
                &registry
            )
            .unwrap(),
            r#"{ '+': 'reject', "true?": 'string', "false?": 'string' }"#
        );
    }

    #[test]
    fn a_map_key_with_no_json_object_key_is_rejected() {
        let registry = ForeignTypeRegistry::default();
        let by_nothing = FieldType::HashMap(Box::new(FieldType::Unit), Box::new(FieldType::String));
        let error = field_type_to_arktype(
            &by_nothing,
            &TypeIndex::new(&BTreeMap::new(), &BTreeMap::new()).unwrap(),
            &registry,
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("a map keyed by `()` has no JSON object key"),
            "{error}"
        );
    }
}
