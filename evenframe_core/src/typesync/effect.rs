use crate::config::fill;
use crate::config::{EffectMapping, ForeignTypeConfig};
use crate::error::{EvenframeError, Result};
use crate::types::{
    EnumRepresentation, FieldType, NewtypeKind, StructConfig, StructField, TaggedUnion, VariantData,
};
use crate::typesync::config::OutputKind;
use crate::typesync::doc_comment::format_jsdoc;
use crate::typesync::foreign_ts::{RecordLinkMapping, record_link_mapping};
use crate::typesync::js_checks::{
    self, JsCheck, LengthCheck, ONE_CHARACTER, object_key, string_literal, template_literal,
};
use crate::typesync::map_key::{BOOL_KEYS, MapKey};
use crate::typesync::type_index::TypeIndex;
use crate::validator::keywords;
use crate::validator::string_rules::{StringParse, StringRule, StringTransform};
use crate::validator::{
    ArrayValidator, BigDecimalValidator, BigIntValidator, DateValidator, DurationValidator,
    NumberValidator, StringValidator, Validator, bounds,
};
use convert_case::{Case, Casing};
use std::collections::BTreeSet;
use std::fmt::Write;
use tracing;

/// The Effect schemas of every type, in one file. With `print_types`, each
/// schema's `…Type` alias follows the schemas.
pub fn generate_effect_schema_string(
    index: &TypeIndex,
    print_types: bool,
    registry: &crate::types::ForeignTypeRegistry,
) -> Result<String> {
    tracing::info!(
        struct_count = index.structs().len(),
        enum_count = index.enums().len(),
        print_types = print_types,
        "Generating Effect Schema string"
    );
    let mut emitter = EffectEmitter::new(index, registry, Defined::emitted_only());
    for name in index.ordered() {
        if emitter.defined.contains(name) {
            continue;
        }
        // `resolve_only` types stay in the maps for reference resolution but
        // are not emitted as their own schema.
        let resolve_only = index
            .enums_named(name)
            .iter()
            .any(|tagged_union| tagged_union.resolve_only)
            || index
                .structs_named(name)
                .iter()
                .any(|struct_config| struct_config.resolve_only);
        if resolve_only {
            emitter.defined.emitted.insert(name.clone());
            continue;
        }
        emitter.emit(name, false)?;
    }

    let EffectEmitter {
        classes,
        types,
        encoded,
        ..
    } = emitter;
    let result = if print_types {
        format!("{classes}\n{encoded}\n{types}")
    } else {
        format!("{classes}\n{encoded}")
    };
    tracing::info!(
        output_length = result.len(),
        "Effect Schema generation complete"
    );
    Ok(result)
}

/// The Effect schemas of one per-file output's file, `type_names`, each
/// followed by its `…Type` alias. Every type outside the file counts as
/// already defined, since the file imports it, so no reference to one is
/// suspended.
pub fn generate_effect_schema_for_types(
    type_names: &[String],
    index: &TypeIndex,
    registry: &crate::types::ForeignTypeRegistry,
) -> Result<String> {
    let in_file: BTreeSet<String> = type_names.iter().cloned().collect();
    let mut emitter = EffectEmitter::new(
        index,
        registry,
        Defined {
            emitted: BTreeSet::new(),
            outside: Some((index.effective_names(), &in_file)),
        },
    );
    for name in index.in_definition_order(&in_file) {
        if !emitter.defined.contains(name) {
            emitter.emit(name, true)?;
        }
    }
    Ok(format!("{}\n{}", emitter.classes, emitter.encoded))
}

/// The types a reference can name directly rather than through
/// `Schema.suspend`: those defined earlier in the file and, for one file of a
/// per-file output, every type outside it.
struct Defined<'s> {
    emitted: BTreeSet<String>,
    /// Every type's effective name, and the names in this file.
    outside: Option<(&'s BTreeSet<String>, &'s BTreeSet<String>)>,
}

impl Defined<'_> {
    fn emitted_only() -> Self {
        Self {
            emitted: BTreeSet::new(),
            outside: None,
        }
    }

    fn contains(&self, name: &str) -> bool {
        self.emitted.contains(name)
            || self
                .outside
                .is_some_and(|(all, in_file)| all.contains(name) && !in_file.contains(name))
    }
}

/// Writes Effect schemas one type at a time, in definition order.
struct EffectEmitter<'i, 'a, 's> {
    index: &'i TypeIndex<'a>,
    registry: &'i crate::types::ForeignTypeRegistry,
    defined: Defined<'s>,
    classes: String,
    types: String,
    encoded: String,
}

impl<'i, 'a, 's> EffectEmitter<'i, 'a, 's> {
    fn new(
        index: &'i TypeIndex<'a>,
        registry: &'i crate::types::ForeignTypeRegistry,
        defined: Defined<'s>,
    ) -> Self {
        Self {
            index,
            registry,
            defined,
            classes: String::new(),
            types: String::new(),
            encoded: String::new(),
        }
    }

    /// Writes the schema, `…Type` alias and `…Encoded` type of `name`,
    /// putting the alias after the schema when `alias_inline`, else with the
    /// other aliases.
    fn emit(&mut self, name: &str, alias_inline: bool) -> Result<()> {
        let index = self.index;
        let registry = self.registry;
        let to_schema = |field_type: &FieldType, current: &str, defined: &Defined| {
            field_type_to_effect_schema(field_type, index, current, defined, registry)
        };
        let alias = format!("export type {name}Type = typeof {name}.Type;\n");
        if let Some(tagged_union) = index.enum_named(name) {
            if let Some(doc) = &tagged_union.doccom {
                self.classes.push_str(&format_jsdoc(doc, ""));
            }
            let variants = tagged_union
                .variants
                .iter()
                .map(|variant| {
                    enum_variant_to_schema(
                        variant,
                        variant.serde_representation(&tagged_union.representation),
                        name,
                        &to_schema,
                        &self.defined,
                        index,
                    )
                })
                .collect::<Result<Vec<_>>>()?
                .join(", ");
            writeln!(
                self.classes,
                "export const {name} = Schema.Union({variants}).annotations({{ identifier: `{name}` }});"
            )
            .map_err(write_failed)?;
            self.encoded
                .push_str(&encoded_alias_for_enum(tagged_union, index, registry)?);
        } else if let Some(struct_config) = index.struct_named(name)
            && struct_config
                .fields
                .iter()
                .any(|field| field.effective().wire.serde_flatten)
        {
            // A class keeps only its own fields, so a struct whose keys are
            // partly known only from a value is a plain schema.
            if let Some(doc) = &struct_config.doccom {
                self.classes.push_str(&format_jsdoc(doc, ""));
            }
            let mut entries = Vec::new();
            let mut held_values = Vec::new();
            let mut record_values = Vec::new();
            let mut extensions = Vec::new();
            for field in struct_config.fields.iter().map(StructField::effective) {
                let schema = to_schema(&field.field_type, name, &self.defined)?;
                if field.wire.serde_flatten {
                    match field.field_type.flattened_map_value() {
                        Some(value) => record_values.push(to_schema(value, name, &self.defined)?),
                        None => extensions.push(schema),
                    }
                    continue;
                }
                entries.push(field_schema_entry(field, index, |field_type| {
                    to_schema(field_type, name, &self.defined)
                })?);
                held_values.push(schema);
            }
            let record = if record_values.is_empty() {
                String::new()
            } else {
                // A key of the map may share an object with every named field.
                let values: Vec<String> = record_values.into_iter().chain(held_values).collect();
                format!(
                    ", Schema.Record({{ key: Schema.String, value: Schema.Union({}) }})",
                    values.join(", ")
                )
            };
            let schema = extensions.into_iter().fold(
                format!("Schema.Struct({{ {} }}{record})", entries.join(", ")),
                |schema, held| format!("Schema.extend({schema}, {held})"),
            );
            writeln!(
                self.classes,
                "export const {name} = {schema}.annotations({{ identifier: `{name}` }});\nexport type {name} = typeof {name}.Type;\n"
            )
            .map_err(write_failed)?;
            self.encoded.push_str(&encoded_interface_for_struct(
                struct_config,
                index,
                registry,
            )?);
        } else if let Some(struct_config) = index.struct_named(name) {
            if let Some(doc) = &struct_config.doccom {
                self.classes.push_str(&format_jsdoc(doc, ""));
            }
            writeln!(
                self.classes,
                "export class {name} extends Schema.Class<{name}>(\"{name}\")( {{ "
            )
            .map_err(write_failed)?;
            for (position, field) in struct_config
                .fields
                .iter()
                .map(StructField::effective)
                .enumerate()
            {
                if let Some(doc) = &field.doccom {
                    self.classes.push_str(&format_jsdoc(doc, "  "));
                }
                let entry = field_schema_entry(field, index, |field_type| {
                    to_schema(field_type, name, &self.defined)
                })?;
                let separator = if position + 1 == struct_config.fields.len() {
                    ""
                } else {
                    ","
                };
                writeln!(self.classes, "  {entry}{separator}").map_err(write_failed)?;
            }
            self.classes.push_str("}) {[key: string]: unknown}\n\n");
            self.encoded.push_str(&encoded_interface_for_struct(
                struct_config,
                index,
                registry,
            )?);
        } else if let Some(newtype) = index.newtype_named(name) {
            if let Some(doc) = &newtype.doccom {
                self.classes.push_str(&format_jsdoc(doc, ""));
            }
            let schema = renewed(
                apply_validators_to_schema(
                    payload_schema(&newtype.inner, &newtype.element_validators, name, |held| {
                        to_schema(held, name, &self.defined)
                    })?,
                    index.underlying(&newtype.inner),
                    &newtype.validators,
                    name,
                )?,
                &newtype.inner,
                &newtype.validators,
                index,
                |field_type| to_schema(field_type, name, &self.defined),
            )?;
            let brand = match newtype.kind {
                NewtypeKind::Branded => format!(".pipe(Schema.brand({}))", string_literal(name)?),
                NewtypeKind::Alias => String::new(),
            };
            writeln!(
                self.classes,
                "export const {name} = {schema}{brand}.annotations({{ identifier: `{name}` }});"
            )
            .map_err(write_failed)?;
            let encoded = if parses_text(&newtype.validators) {
                "string".to_owned()
            } else {
                encoded_payload(&newtype.inner, &newtype.element_validators, index, registry)?
            };
            writeln!(self.encoded, "export type {name}Encoded = {encoded};\n")
                .map_err(write_failed)?;
        } else {
            self.defined.emitted.insert(name.to_string());
            return Ok(());
        }
        if alias_inline {
            self.classes.push_str(&alias);
        } else {
            self.types.push_str(&alias);
        }
        self.defined.emitted.insert(name.to_string());
        Ok(())
    }
}

fn write_failed(error: std::fmt::Error) -> EvenframeError {
    EvenframeError::type_sync(format!("writing the Effect schema failed: {error}"))
}

// ----- Encoded Type Generation Helpers -------------------------------------

/// A struct field as an `...Encoded` entry, `readonly name: type;`.
fn encoded_field_entry(
    field: &StructField,
    index: &TypeIndex,
    registry: &crate::types::ForeignTypeRegistry,
) -> Result<String> {
    let field = field.effective();
    let optional = if field.wire.serde_optional { "?" } else { "" };
    Ok(format!(
        "readonly {}{optional}: {};",
        object_key(&field.ts_name())?,
        encoded_field_type(field, index, registry)?
    ))
}

/// The encoded TypeScript type of a field's value.
fn encoded_field_type(
    field: &StructField,
    index: &TypeIndex,
    registry: &crate::types::ForeignTypeRegistry,
) -> Result<String> {
    Ok(match (&field.field_type, parses_string_input(field)) {
        (FieldType::Option(_), true) => "string | null | undefined".to_owned(),
        (_, true) => "string".to_owned(),
        (field_type, false) => field_type_to_ts_encoded(field_type, index, registry)?,
    })
}

/// Generates an `...Encoded` TypeScript type for a given struct: an interface,
/// with an index signature for a flattened map, or an intersection with what
/// any other flattened field holds.
fn encoded_interface_for_struct(
    struct_config: &StructConfig,
    index: &TypeIndex,
    registry: &crate::types::ForeignTypeRegistry,
) -> Result<String> {
    let name = struct_config.struct_name.to_case(Case::Pascal);
    let mut entries = Vec::new();
    let mut held_values = Vec::new();
    let mut index_values = Vec::new();
    let mut intersections = Vec::new();
    for field in struct_config.fields.iter().map(StructField::effective) {
        if field.wire.serde_flatten {
            match field.field_type.flattened_map_value() {
                Some(value) => index_values.push(field_type_to_ts_encoded(value, index, registry)?),
                None => intersections.push(field_type_to_ts_encoded(
                    &field.field_type,
                    index,
                    registry,
                )?),
            }
            continue;
        }
        entries.push(format!(
            "  {}",
            encoded_field_entry(field, index, registry)?
        ));
        held_values.push(encoded_field_type(field, index, registry)?);
    }
    if !index_values.is_empty() {
        let values: Vec<String> = index_values.into_iter().chain(held_values).collect();
        entries.push(format!("  readonly [key: string]: {};", values.join(" | ")));
    }
    let body = entries.join("\n");
    Ok(if intersections.is_empty() {
        format!("export interface {name}Encoded {{\n{body}\n}}\n\n")
    } else {
        format!(
            "export type {name}Encoded = {{\n{body}\n}} & {};\n\n",
            intersections.join(" & ")
        )
    })
}

/// Generates an `...Encoded` TypeScript type alias for a given enum/union.
fn encoded_alias_for_enum(
    en: &TaggedUnion,
    index: &TypeIndex,
    registry: &crate::types::ForeignTypeRegistry,
) -> Result<String> {
    tracing::trace!(enum_name = %en.enum_name, "Creating encoded alias for enum");
    let name = en.enum_name.to_case(Case::Pascal);
    let body = en
        .variants
        .iter()
        .map(|variant| {
            enum_variant_to_encoded(
                variant,
                variant.serde_representation(&en.representation),
                index,
                registry,
            )
        })
        .collect::<Result<Vec<_>>>()?
        .join(" | ");
    Ok(format!("export type {}Encoded = {};\n\n", name, body))
}

// ----- Representation-Aware Variant Helpers --------------------------------

/// A struct field as a schema entry, `name: schema`, with its validators. A
/// required field reports a missing value by its title.
fn field_schema_entry(
    field: &StructField,
    index: &TypeIndex,
    schema_of: impl Fn(&FieldType) -> Result<String>,
) -> Result<String> {
    let field = field.effective();
    let schema = validated_field_schema(field, index, schema_of)?;
    // An absent key decodes to `None`, so only a non-Option key needs marking.
    let entry = if matches!(field.field_type, FieldType::Option(_)) {
        schema
    } else if field.wire.serde_optional {
        format!("Schema.optional({schema})")
    } else {
        format!(
            "Schema.propertySignature({schema}).annotations({{ missingMessage: () => {} }})",
            template_literal(&format!(
                "'{}' is required",
                field.field_name.to_case(Case::Title)
            ))
        )
    };
    Ok(format!("{}: {entry}", object_key(&field.ts_name())?))
}

/// Converts a single enum variant into its Effect Schema representation,
/// taking the serde `EnumRepresentation` into account. An internally tagged
/// variant's tag is merged with the fields of the struct it holds, as serde
/// writes them.
fn enum_variant_to_schema<F>(
    v: &crate::types::Variant,
    repr: &EnumRepresentation,
    enum_name: &str,
    to_schema: &F,
    defined: &Defined,
    index: &TypeIndex,
) -> Result<String>
where
    F: Fn(&FieldType, &str, &Defined) -> Result<String>,
{
    let name = string_literal(v.serde_name())?;
    let tag_entry = |tag: &str| -> Result<String> {
        Ok(format!("{}: Schema.Literal({name})", object_key(tag)?))
    };
    let Some(data) = &v.data else {
        return Ok(match repr {
            EnumRepresentation::InternallyTagged { tag }
            | EnumRepresentation::AdjacentlyTagged { tag, .. } => {
                format!("Schema.Struct({{ {} }})", tag_entry(tag)?)
            }
            EnumRepresentation::ExternallyTagged => format!("Schema.Literal({name})"),
            // serde writes an untagged unit variant as null.
            EnumRepresentation::Untagged => "Schema.Null".to_owned(),
        });
    };
    let fields_schema = |fields: &[StructField], tag: Option<&str>| -> Result<String> {
        let mut entries: Vec<String> = tag.map(tag_entry).transpose()?.into_iter().collect();
        for field in fields {
            entries.push(field_schema_entry(field, index, |field_type| {
                to_schema(field_type, enum_name, defined)
            })?);
        }
        Ok(format!("Schema.Struct({{ {} }})", entries.join(", ")))
    };
    let tag = match repr {
        EnumRepresentation::InternallyTagged { tag } => Some(tag.as_str()),
        _ => None,
    };
    let payload = match data {
        VariantData::InlineStruct(inline) => match tag {
            Some(tag) => return fields_schema(&inline.fields, Some(tag)),
            None => fields_schema(&inline.fields, None)?,
        },
        VariantData::DataStructureRef(field_type) => match tag {
            Some(tag) => return fields_schema(held_struct_fields(field_type, index)?, Some(tag)),
            None => payload_schema(field_type, &v.element_validators, v.serde_name(), |held| {
                to_schema(held, enum_name, defined)
            })?,
        },
    };
    Ok(match repr {
        EnumRepresentation::ExternallyTagged | EnumRepresentation::InternallyTagged { .. } => {
            format!(
                "Schema.Struct({{ {}: {payload} }})",
                object_key(v.serde_name())?
            )
        }
        EnumRepresentation::AdjacentlyTagged { tag, content } => {
            format!(
                "Schema.Struct({{ {}, {}: {payload} }})",
                tag_entry(tag)?,
                object_key(content)?
            )
        }
        EnumRepresentation::Untagged => payload,
    })
}

/// Converts a single enum variant into its TypeScript Encoded type representation,
/// taking the serde `EnumRepresentation` into account.
fn enum_variant_to_encoded(
    v: &crate::types::Variant,
    repr: &EnumRepresentation,
    index: &TypeIndex,
    registry: &crate::types::ForeignTypeRegistry,
) -> Result<String> {
    let name = string_literal(v.serde_name())?;
    let tag_entry =
        |tag: &str| -> Result<String> { Ok(format!("readonly {}: {name};", object_key(tag)?)) };
    let Some(data) = &v.data else {
        return Ok(match repr {
            EnumRepresentation::InternallyTagged { tag }
            | EnumRepresentation::AdjacentlyTagged { tag, .. } => {
                format!("{{ {} }}", tag_entry(tag)?)
            }
            EnumRepresentation::ExternallyTagged => name,
            EnumRepresentation::Untagged => "null".to_owned(),
        });
    };
    let payload = match data {
        VariantData::InlineStruct(inline) => {
            let entries = inline
                .fields
                .iter()
                .map(|field| encoded_field_entry(field, index, registry))
                .collect::<Result<Vec<String>>>()?;
            if let EnumRepresentation::InternallyTagged { tag } = repr {
                return Ok(format!("{{ {} {} }}", tag_entry(tag)?, entries.join(" ")));
            }
            format!("{{ {} }}", entries.join(" "))
        }
        VariantData::DataStructureRef(field_type) => {
            let payload = encoded_payload(field_type, &v.element_validators, index, registry)?;
            // serde writes the tag into the struct the variant holds.
            if let EnumRepresentation::InternallyTagged { tag } = repr {
                return Ok(format!("({{ {} }} & {payload})", tag_entry(tag)?));
            }
            payload
        }
    };
    Ok(match repr {
        EnumRepresentation::ExternallyTagged | EnumRepresentation::InternallyTagged { .. } => {
            format!("{{ readonly {}: {payload} }}", object_key(v.serde_name())?)
        }
        EnumRepresentation::AdjacentlyTagged { tag, content } => {
            format!(
                "{{ {} readonly {}: {payload}; }}",
                tag_entry(tag)?,
                object_key(content)?
            )
        }
        EnumRepresentation::Untagged => payload,
    })
}

/// The fields of the struct an internally tagged newtype variant holds. serde
/// writes the tag into that struct's object, which only a struct can take.
fn held_struct_fields<'a>(
    field_type: &FieldType,
    index: &TypeIndex<'a>,
) -> Result<&'a [StructField]> {
    let held = match field_type {
        FieldType::Other(name) => index.struct_named(name),
        _ => None,
    };
    held.map(|struct_config| struct_config.effective().fields.as_slice())
        .ok_or_else(|| {
            EvenframeError::type_sync(format!(
                "an internally tagged newtype variant holds `{}`, but serde can only write the \
                 tag into a struct",
                field_type.canonical_name()
            ))
        })
}

// ----- Schema and Type Conversion Logic ------------------------------------

/// A foreign type's Effect mapping, which a foreign type the Effect output
/// names must have.
fn effect_mapping<'a>(name: &str, foreign: &'a ForeignTypeConfig) -> Result<&'a EffectMapping> {
    foreign.effect.as_ref().ok_or_else(|| {
        EvenframeError::type_sync(format!("the foreign type `{name}` has no effect mapping"))
    })
}

/// A `char`: a string of exactly one character.
fn char_schema() -> Result<String> {
    Ok(format!(
        "Schema.String.pipe(Schema.pattern(new RegExp({})))",
        string_literal(ONE_CHARACTER)?
    ))
}

/// A map key as the string schema a `Schema.Record` key must be. JSON keys are
/// strings, so an integer key is a string of decimal digits.
fn map_key_schema(key: &FieldType, registry: &crate::types::ForeignTypeRegistry) -> Result<String> {
    Ok(match MapKey::require(key)? {
        MapKey::Text => "Schema.String".to_string(),
        MapKey::Char => char_schema()?,
        MapKey::Bool => format!(
            "Schema.Literal({})",
            BOOL_KEYS
                .into_iter()
                .map(string_literal)
                .collect::<Result<Vec<_>>>()?
                .join(", ")
        ),
        MapKey::Integer => format!(
            "Schema.String.pipe(Schema.pattern(new RegExp({})))",
            string_literal(keywords::INTEGER)?
        ),
        MapKey::Named(name) => match registry.lookup(name) {
            Some(foreign) => effect_mapping(name, foreign)?.type_expr.clone(),
            None => name.to_case(Case::Pascal),
        },
    })
}

/// A map key's type in an `...Encoded` interface.
fn map_key_encoded(
    key: &FieldType,
    registry: &crate::types::ForeignTypeRegistry,
) -> Result<String> {
    Ok(match MapKey::require(key)? {
        MapKey::Text | MapKey::Char | MapKey::Integer => "string".to_string(),
        MapKey::Bool => BOOL_KEYS
            .into_iter()
            .map(string_literal)
            .collect::<Result<Vec<_>>>()?
            .join(" | "),
        MapKey::Named(name) => match registry.lookup(name) {
            Some(foreign) => effect_mapping(name, foreign)?.encoded.clone(),
            None => format!("{}Encoded", name.to_case(Case::Pascal)),
        },
    })
}

/// Converts a `FieldType` into its corresponding Effect `Schema` representation.
fn field_type_to_effect_schema(
    field_type: &FieldType,
    index: &TypeIndex,
    current: &str,
    defined: &Defined,
    registry: &crate::types::ForeignTypeRegistry,
) -> Result<String> {
    enum WorkItem<'a> {
        Generate(&'a FieldType),
        AssembleOption,
        AssembleVec,
        AssembleTuple { count: usize },
        AssembleStruct { field_names: Vec<String> },
        AssembleRecordLink,
        AssembleMap { key: String, finite_keys: bool },
    }

    let mut work_stack: Vec<WorkItem> = Vec::new();
    let mut value_stack: Vec<String> = Vec::new();

    work_stack.push(WorkItem::Generate(field_type));

    while let Some(work_item) = work_stack.pop() {
        match work_item {
            WorkItem::Generate(field_type) => match field_type {
                FieldType::String => value_stack.push("Schema.String".to_string()),
                FieldType::Char => value_stack.push(char_schema()?),
                FieldType::Bool => value_stack.push("Schema.Boolean".to_string()),
                FieldType::Unit => value_stack.push("Schema.Null".to_string()),
                // serde writes whole seconds and the nanoseconds past them,
                // and rejects any other key.
                FieldType::Duration => value_stack.push(
                    "Schema.Struct({ secs: Schema.Number.pipe(Schema.int(), Schema.nonNegative()), nanos: Schema.Number.pipe(Schema.int(), Schema.between(0, 999999999)) }).annotations({ parseOptions: { onExcessProperty: \"error\" } })"
                        .to_string(),
                ),
                FieldType::F32 | FieldType::F64 => value_stack.push("Schema.Number".to_string()),
                FieldType::I8
                | FieldType::I16
                | FieldType::I32
                | FieldType::I64
                | FieldType::I128
                | FieldType::Isize => value_stack.push("Schema.Number".to_string()),
                FieldType::U8
                | FieldType::U16
                | FieldType::U32
                | FieldType::U64
                | FieldType::U128
                | FieldType::Usize => value_stack.push("Schema.Number".to_string()),
                FieldType::Option(i) => {
                    work_stack.push(WorkItem::AssembleOption);
                    work_stack.push(WorkItem::Generate(i));
                }
                FieldType::Vec(i) => {
                    work_stack.push(WorkItem::AssembleVec);
                    work_stack.push(WorkItem::Generate(i));
                }
                FieldType::Tuple(v) => {
                    work_stack.push(WorkItem::AssembleTuple { count: v.len() });
                    for inner_type in v.iter().rev() {
                        work_stack.push(WorkItem::Generate(inner_type));
                    }
                }
                FieldType::Struct(fs) => {
                    let field_names: Vec<String> =
                        fs.iter().map(|(name, _)| name.clone()).collect();
                    work_stack.push(WorkItem::AssembleStruct { field_names });
                    for (_, ftype) in fs.iter().rev() {
                        work_stack.push(WorkItem::Generate(ftype));
                    }
                }
                FieldType::RecordLink(i) => {
                    work_stack.push(WorkItem::AssembleRecordLink);
                    work_stack.push(WorkItem::Generate(i));
                }
                FieldType::HashMap(k, v) | FieldType::BTreeMap(k, v) => {
                    work_stack.push(WorkItem::AssembleMap {
                        key: map_key_schema(index.underlying(k), registry)?,
                        finite_keys: MapKey::require(index.underlying(k))?.is_finite(registry),
                    });
                    work_stack.push(WorkItem::Generate(v));
                }
                FieldType::Other(name) => {
                    if let Some(foreign) = registry.lookup(name) {
                        let mapping = effect_mapping(name, foreign)?;
                        value_stack.push(mapping.type_expr.clone());
                        continue;
                    }

                    let pascal = name.to_case(Case::Pascal);
                    let wrap_id = format!("{}Ref", pascal);
                    // Decide whether we need Schema.suspend for recursion.
                    if index.recursion().is_recursive_pair(current, &pascal)
                        && !defined.contains(&pascal)
                    {
                        // Forward edge *inside* a recursive SCC requires suspension.
                        if index.struct_named(&pascal).is_some() {
                            value_stack.push(format!(
                                "Schema.suspend((): Schema.Schema<{}, {}Encoded> => {}).annotations({{ identifier: `{}` }})",
                                pascal, pascal, pascal, wrap_id
                            ));
                        } else {
                            value_stack.push(format!(
                                "Schema.suspend((): Schema.Schema<typeof {}.Type, {}Encoded> => {}).annotations({{ identifier: `{}` }})",
                                pascal, pascal, pascal, wrap_id
                            ));
                        }
                    } else {
                        // Direct reference for non-recursive or already processed types.
                        value_stack.push(pascal);
                    }
                }
            },
            WorkItem::AssembleOption => {
                let inner = value_stack.pop().unwrap();
                value_stack.push(format!("Schema.OptionFromNullishOr({}, null)", inner));
            }
            WorkItem::AssembleVec => {
                let inner = value_stack.pop().unwrap();
                value_stack.push(format!("Schema.Array({})", inner));
            }
            WorkItem::AssembleTuple { count } => {
                let items: Vec<_> = value_stack.drain(value_stack.len() - count..).collect();
                value_stack.push(format!("Schema.Tuple({})", items.join(", ")));
            }
            WorkItem::AssembleStruct { field_names } => {
                let count = field_names.len();
                let values: Vec<_> = value_stack.drain(value_stack.len() - count..).collect();
                let assignments: Vec<String> = field_names
                    .into_iter()
                    .zip(values)
                    .map(|(name, value)| format!("{}: {}", name, value))
                    .collect();
                value_stack.push(format!("Schema.Struct({{ {} }})", assignments.join(", ")));
            }
            WorkItem::AssembleRecordLink => {
                let inner = value_stack.pop().unwrap();
                value_stack.push(
                    match record_link_mapping(registry, OutputKind::Effect, |foreign| foreign.effect.as_ref())? {
                        RecordLinkMapping::Configured(mapping) => fill(&mapping.type_expr, &[inner]),
                        RecordLinkMapping::Own { record_id } => {
                            format!("Schema.Union({}, {inner})", record_id.type_expr)
                        }
                    },
                );
            }
            WorkItem::AssembleMap { key, finite_keys } => {
                let v = value_stack.pop().unwrap();
                // A map keyed by a bool or an enum holds any subset of its values.
                let partial = if finite_keys {
                    ".pipe(Schema.partialWith({ exact: true }))"
                } else {
                    ""
                };
                // serde rejects a key outside the key type, so the record does too.
                value_stack.push(format!(
                    "Schema.Record({{ key: {}, value: {} }}){partial}.annotations({{ parseOptions: {{ onExcessProperty: \"error\" }} }})",
                    key, v
                ));
            }
        }
    }

    assert_eq!(
        value_stack.len(),
        1,
        "Generation ended with not exactly one value on the stack."
    );
    Ok(value_stack.pop().unwrap())
}

/// Converts a `FieldType` into its corresponding raw TypeScript type for the `...Encoded` interface.
fn field_type_to_ts_encoded(
    ft: &FieldType,
    index: &TypeIndex,
    registry: &crate::types::ForeignTypeRegistry,
) -> Result<String> {
    enum WorkItem<'a> {
        Generate(&'a FieldType),
        AssembleOption,
        AssembleVec,
        AssembleTuple { count: usize },
        AssembleStruct { field_names: Vec<String> },
        AssembleRecordLink,
        AssembleMap { key: String, finite_keys: bool },
    }

    let mut work_stack: Vec<WorkItem> = Vec::new();
    let mut value_stack: Vec<String> = Vec::new();

    work_stack.push(WorkItem::Generate(ft));

    while let Some(work_item) = work_stack.pop() {
        match work_item {
            WorkItem::Generate(ft) => {
                match ft {
                    // Primitives
                    FieldType::String | FieldType::Char => value_stack.push("string".to_string()),
                    FieldType::Bool => value_stack.push("boolean".to_string()),
                    FieldType::Unit => value_stack.push("null".to_string()),
                    FieldType::Duration => value_stack.push(field_type_to_ts_encoded(
                        &FieldType::serde_duration(),
                        index,
                        registry,
                    )?),
                    FieldType::F32
                    | FieldType::F64
                    | FieldType::I8
                    | FieldType::I16
                    | FieldType::I32
                    | FieldType::I64
                    | FieldType::I128
                    | FieldType::Isize
                    | FieldType::U8
                    | FieldType::U16
                    | FieldType::U32
                    | FieldType::U64
                    | FieldType::U128
                    | FieldType::Usize => value_stack.push("number".to_string()),

                    // Containers
                    FieldType::Option(inner) => {
                        work_stack.push(WorkItem::AssembleOption);
                        work_stack.push(WorkItem::Generate(inner));
                    }
                    FieldType::Vec(inner) => {
                        work_stack.push(WorkItem::AssembleVec);
                        work_stack.push(WorkItem::Generate(inner));
                    }
                    FieldType::Tuple(items) => {
                        work_stack.push(WorkItem::AssembleTuple { count: items.len() });
                        for item in items.iter().rev() {
                            work_stack.push(WorkItem::Generate(item));
                        }
                    }
                    FieldType::Struct(fs) => {
                        let field_names: Vec<String> =
                            fs.iter().map(|(name, _)| name.clone()).collect();
                        work_stack.push(WorkItem::AssembleStruct { field_names });
                        for (_, ftype) in fs.iter().rev() {
                            work_stack.push(WorkItem::Generate(ftype));
                        }
                    }
                    FieldType::HashMap(k, v) | FieldType::BTreeMap(k, v) => {
                        work_stack.push(WorkItem::AssembleMap {
                            key: map_key_encoded(index.underlying(k), registry)?,
                            finite_keys: MapKey::require(index.underlying(k))?.is_finite(registry),
                        });
                        work_stack.push(WorkItem::Generate(v));
                    }
                    FieldType::RecordLink(inner) => {
                        work_stack.push(WorkItem::AssembleRecordLink);
                        work_stack.push(WorkItem::Generate(inner));
                    }

                    // User-defined types
                    FieldType::Other(name) => {
                        if let Some(foreign) = registry.lookup(name) {
                            let mapping = effect_mapping(name, foreign)?;
                            value_stack.push(mapping.encoded.clone());
                            continue;
                        }
                        value_stack.push(format!("{}Encoded", name.to_case(Case::Pascal)))
                    }
                }
            }
            WorkItem::AssembleOption => {
                let inner = value_stack.pop().unwrap();
                value_stack.push(format!("{} | null | undefined", inner));
            }
            WorkItem::AssembleVec => {
                let inner = value_stack.pop().unwrap();
                value_stack.push(format!("ReadonlyArray<{}>", inner));
            }
            WorkItem::AssembleTuple { count } => {
                let items: Vec<_> = value_stack.drain(value_stack.len() - count..).collect();
                value_stack.push(format!("readonly [{}]", items.join(", ")));
            }
            WorkItem::AssembleStruct { field_names } => {
                let count = field_names.len();
                let values: Vec<_> = value_stack.drain(value_stack.len() - count..).collect();
                let assignments: Vec<String> = field_names
                    .into_iter()
                    .zip(values)
                    .map(|(name, value)| format!("  readonly {}: {};", name, value))
                    .collect();
                value_stack.push(format!("{{\n{}\n}}", assignments.join("\n")));
            }
            WorkItem::AssembleMap { key, finite_keys } => {
                let v = value_stack.pop().unwrap();
                let record = format!("Record<{}, {}>", key, v);
                value_stack.push(if finite_keys {
                    format!("Partial<{record}>")
                } else {
                    record
                });
            }
            WorkItem::AssembleRecordLink => {
                let inner = value_stack.pop().unwrap();
                value_stack.push(
                    match record_link_mapping(registry, OutputKind::Effect, |foreign| {
                        foreign.effect.as_ref()
                    })? {
                        RecordLinkMapping::Configured(mapping) => fill(&mapping.encoded, &[inner]),
                        RecordLinkMapping::Own { record_id } => {
                            format!("{} | {inner}", record_id.encoded)
                        }
                    },
                );
            }
        }
    }

    assert_eq!(
        value_stack.len(),
        1,
        "Generation ended with not exactly one value on the stack."
    );
    Ok(value_stack.pop().unwrap())
}

// ----- Validator Application Logic -----------------------------------------

/// A field's schema with its validators applied. An optional field's
/// validators constrain the present value, as the Rust deserializer does, so
/// they go on the inner schema before it is wrapped.
fn validated_field_schema(
    field: &StructField,
    index: &TypeIndex,
    schema_of: impl Fn(&FieldType) -> Result<String>,
) -> Result<String> {
    let validated = |field_type: &FieldType| {
        renewed(
            apply_validators_to_schema(
                schema_of(field_type)?,
                field_type,
                &field.validators,
                &field.field_name,
            )?,
            field_type,
            &field.validators,
            index,
            &schema_of,
        )
    };
    match &field.field_type {
        FieldType::Option(inner) if !field.validators.is_empty() => Ok(format!(
            "Schema.OptionFromNullishOr({}, null)",
            validated(inner)?
        )),
        field_type => validated(field_type),
    }
}

/// `schema`, composed back into the newtype `value_type` names once a
/// validator rewrote the value, which checks the newtype again and keeps its
/// brand, as the Rust deserializer does.
fn renewed(
    schema: String,
    value_type: &FieldType,
    validators: &[Validator],
    index: &TypeIndex,
    schema_of: impl Fn(&FieldType) -> Result<String>,
) -> Result<String> {
    match value_type {
        FieldType::Other(name)
            if validators.iter().any(Validator::rewrites)
                && index.newtype_named(name).is_some() =>
        {
            Ok(format!(
                "{schema}.pipe(Schema.compose({}))",
                schema_of(value_type)?
            ))
        }
        _ => Ok(schema),
    }
}

/// Whether a field is read through a parse morph, so its encoded form is a
/// string whatever its Rust type.
fn parses_string_input(field: &StructField) -> bool {
    parses_text(&field.validators)
}

/// The encoded type of a payload, each element read through a parse morph
/// written as text.
fn encoded_payload(
    field_type: &FieldType,
    element_validators: &[Vec<Validator>],
    index: &TypeIndex,
    registry: &crate::types::ForeignTypeRegistry,
) -> Result<String> {
    let encoded = |held: &FieldType, validators: &[Validator]| {
        if parses_text(validators) {
            Ok("string".to_owned())
        } else {
            field_type_to_ts_encoded(held, index, registry)
        }
    };
    match (field_type, element_validators) {
        (FieldType::Tuple(items), validators)
            if !validators.is_empty() && items.len() == validators.len() =>
        {
            let elements = items
                .iter()
                .zip(validators)
                .map(|(item, validators)| encoded(item, validators))
                .collect::<Result<Vec<_>>>()?;
            Ok(format!("readonly [{}]", elements.join(", ")))
        }
        (single, [validators]) => encoded(single, validators),
        _ => field_type_to_ts_encoded(field_type, index, registry),
    }
}

/// A tuple payload with each element's validators applied, or a newtype
/// payload with its one element's.
fn payload_schema(
    field_type: &FieldType,
    element_validators: &[Vec<Validator>],
    owner: &str,
    schema_of: impl Fn(&FieldType) -> Result<String>,
) -> Result<String> {
    match (field_type, element_validators) {
        (_, []) => schema_of(field_type),
        (FieldType::Tuple(items), validators) if items.len() == validators.len() => {
            let elements = items
                .iter()
                .zip(validators)
                .enumerate()
                .map(|(position, (item, validators))| {
                    apply_validators_to_schema(
                        schema_of(item)?,
                        item,
                        validators,
                        &format!("{owner}.{position}"),
                    )
                })
                .collect::<Result<Vec<_>>>()?;
            Ok(format!("Schema.Tuple({})", elements.join(", ")))
        }
        (single, [validators]) => {
            apply_validators_to_schema(schema_of(single)?, single, validators, owner)
        }
        (_, validators) => Err(EvenframeError::config(format!(
            "`{owner}` has validators for {} elements but holds {field_type:?}",
            validators.len()
        ))),
    }
}

/// Whether `validators` start with a parse morph, which reads the value from text.
fn parses_text(validators: &[Validator]) -> bool {
    matches!(
        validators.first(),
        Some(Validator::StringValidator(validator))
            if matches!(validator.rule(), StringRule::Parse(_))
    )
}

/// `schema` with `validators` applied in order. A parse morph replaces the
/// schema with one that decodes a string into the field's type.
fn apply_validators_to_schema(
    schema: String,
    field_type: &FieldType,
    validators: &[Validator],
    field_name: &str,
) -> Result<String> {
    let title = field_name.to_case(Case::Title);
    let message = |rule: &str| {
        format!(
            "{{ message: () => {} }}",
            template_literal(&format!("'{title}' {rule}"))
        )
    };
    let expected = |expectation: &str| message(&format!("must be {expectation}"));
    let pattern = |source: &str, expectation: &str| -> Result<String> {
        Ok(format!(
            "Schema.pattern(new RegExp({}), {})",
            string_literal(source)?,
            expected(expectation)
        ))
    };

    bounds::check_validators(validators)
        .map_err(|problem| EvenframeError::config(format!("field '{field_name}': {problem}")))?;
    let mut result = schema;
    for validator in validators {
        let filters: Vec<String> = match validator {
            Validator::StringValidator(sv) => match sv.rule() {
                StringRule::Carrier => continue,
                StringRule::Parse(parse) => {
                    let input = expected(sv.description());
                    result = match parse {
                        StringParse::Integer => format!(
                            "Schema.String.pipe({}).pipe(Schema.compose(Schema.NumberFromString)).pipe(Schema.between({}, {}, {input}))",
                            pattern(keywords::INTEGER, sv.description())?,
                            -keywords::MAX_SAFE_INTEGER,
                            keywords::MAX_SAFE_INTEGER
                        ),
                        StringParse::Numeric => format!(
                            "Schema.String.pipe({}).pipe(Schema.compose(Schema.NumberFromString))",
                            pattern(keywords::NUMERIC, sv.description())?
                        ),
                        StringParse::Date => "Schema.Date".to_owned(),
                        StringParse::DateIso => format!(
                            "Schema.String.pipe({}).pipe(Schema.compose(Schema.Date))",
                            pattern(keywords::ISO_8601, sv.description())?
                        ),
                        StringParse::DateEpoch => {
                            let check = js_checks::string_check(&StringValidator::DateEpoch)?;
                            let Some(JsCheck::Predicate(predicate)) = check else {
                                return Err(EvenframeError::config(
                                    "the epoch check is a predicate".to_owned(),
                                ));
                            };
                            format!(
                                "Schema.String.pipe(Schema.filter((value) => {predicate}, {input})).pipe(Schema.compose(Schema.NumberFromString)).pipe(Schema.compose(Schema.DateFromNumber))"
                            )
                        }
                        StringParse::Json => format!("Schema.parseJson({result})"),
                        StringParse::Url => "Schema.URL".to_owned(),
                    };
                    continue;
                }
                StringRule::Transform(transform) => vec![format!(
                    "Schema.compose({})",
                    match transform {
                        StringTransform::Lower => "Schema.Lowercase".to_owned(),
                        StringTransform::Upper => "Schema.Uppercase".to_owned(),
                        StringTransform::Trim => "Schema.Trim".to_owned(),
                        StringTransform::Capitalize => "Schema.Capitalize".to_owned(),
                        StringTransform::Normalize(form) => format!(
                            "Schema.transform(Schema.String, Schema.String, {{ strict: true, decode: (s) => s.normalize(\"{}\"), encode: (s) => s }})",
                            form.name()
                        ),
                    }
                )],
                StringRule::Check => match js_checks::string_check(sv)? {
                    Some(JsCheck::Pattern { source, flags }) => vec![format!(
                        "Schema.pattern({}, {})",
                        js_checks::regexp(&source, &flags)?,
                        expected(&sv.expectation())
                    )],
                    Some(JsCheck::Predicate(predicate)) => vec![format!(
                        "Schema.filter((value) => {predicate}, {})",
                        expected(&sv.expectation())
                    )],
                    Some(JsCheck::Length(LengthCheck::Exactly(length))) => vec![format!(
                        "Schema.length({length}, {})",
                        message(&format!("must be exactly {length} characters long"))
                    )],
                    Some(JsCheck::Length(LengthCheck::AtLeast(length))) => vec![format!(
                        "Schema.minLength({length}, {})",
                        message(&format!("must be at least {length} characters long"))
                    )],
                    Some(JsCheck::Length(LengthCheck::AtMost(length))) => vec![format!(
                        "Schema.maxLength({length}, {})",
                        message(&format!("must be at most {length} characters long"))
                    )],
                    None => continue,
                },
            },

            Validator::NumberValidator(nv) => match nv {
                NumberValidator::GreaterThan(value) => vec![format!(
                    "Schema.greaterThan({}, {})",
                    value.0,
                    message(&format!("must be greater than {}", value.0))
                )],
                NumberValidator::GreaterThanOrEqualTo(value) => vec![format!(
                    "Schema.greaterThanOrEqualTo({}, {})",
                    value.0,
                    message(&format!("must be greater than or equal to {}", value.0))
                )],
                NumberValidator::LessThan(value) => vec![format!(
                    "Schema.lessThan({}, {})",
                    value.0,
                    message(&format!("must be less than {}", value.0))
                )],
                NumberValidator::LessThanOrEqualTo(value) => vec![format!(
                    "Schema.lessThanOrEqualTo({}, {})",
                    value.0,
                    message(&format!("must be less than or equal to {}", value.0))
                )],
                NumberValidator::Between(start, end) => vec![format!(
                    "Schema.between({}, {}, {})",
                    start.0,
                    end.0,
                    message(&format!("must be between {} and {}", start.0, end.0))
                )],
                NumberValidator::Int => {
                    vec![format!("Schema.int({})", message("must be an integer"))]
                }
                NumberValidator::NonNaN => {
                    vec![format!("Schema.nonNaN({})", message("must not be NaN"))]
                }
                NumberValidator::Finite => {
                    vec![format!(
                        "Schema.finite({})",
                        message("must be a finite number")
                    )]
                }
                NumberValidator::Positive => {
                    vec![format!(
                        "Schema.positive({})",
                        message("must be a positive number")
                    )]
                }
                NumberValidator::NonNegative => vec![format!(
                    "Schema.nonNegative({})",
                    message("must be a non-negative number")
                )],
                NumberValidator::Negative => {
                    vec![format!(
                        "Schema.negative({})",
                        message("must be a negative number")
                    )]
                }
                NumberValidator::NonPositive => vec![format!(
                    "Schema.nonPositive({})",
                    message("must be a non-positive number")
                )],
                NumberValidator::MultipleOf(value) => vec![format!(
                    "Schema.multipleOf({}, {})",
                    value.0,
                    message(&format!("must be a multiple of {}", value.0))
                )],
                NumberValidator::Uint8 => vec![
                    format!("Schema.int({})", message("must be an integer")),
                    format!(
                        "Schema.between(0, 255, {})",
                        message("must be between 0 and 255")
                    ),
                ],
            },

            Validator::ArrayValidator(av) => vec![match av {
                ArrayValidator::MinItems(count) => format!(
                    "Schema.minItems({count}, {})",
                    message(&format!("must contain at least {count} items"))
                ),
                ArrayValidator::MaxItems(count) => format!(
                    "Schema.maxItems({count}, {})",
                    message(&format!("must contain at most {count} items"))
                ),
                ArrayValidator::ItemsCount(count) => format!(
                    "Schema.itemsCount({count}, {})",
                    message(&format!("must contain exactly {count} items"))
                ),
            }],

            Validator::DateValidator(dv) => vec![match dv {
                DateValidator::ValidDate => {
                    format!("Schema.validDate({})", message("must be a valid date"))
                }
                DateValidator::GreaterThanDate(date) => format!(
                    "Schema.greaterThanDate({}, {})",
                    date_literal(date)?,
                    message(&format!("must be after {date}"))
                ),
                DateValidator::GreaterThanOrEqualToDate(date) => format!(
                    "Schema.greaterThanOrEqualToDate({}, {})",
                    date_literal(date)?,
                    message(&format!("must be on or after {date}"))
                ),
                DateValidator::LessThanDate(date) => format!(
                    "Schema.lessThanDate({}, {})",
                    date_literal(date)?,
                    message(&format!("must be before {date}"))
                ),
                DateValidator::LessThanOrEqualToDate(date) => format!(
                    "Schema.lessThanOrEqualToDate({}, {})",
                    date_literal(date)?,
                    message(&format!("must be on or before {date}"))
                ),
                DateValidator::BetweenDate(start, end) => format!(
                    "Schema.betweenDate({}, {}, {})",
                    date_literal(start)?,
                    date_literal(end)?,
                    message(&format!("must be between {start} and {end}"))
                ),
            }],

            Validator::BigIntValidator(biv) => vec![match biv {
                BigIntValidator::GreaterThanBigInt(value) => format!(
                    "Schema.greaterThanBigInt({}, {})",
                    bigint_literal(value)?,
                    message(&format!("must be greater than {value}"))
                ),
                BigIntValidator::GreaterThanOrEqualToBigInt(value) => format!(
                    "Schema.greaterThanOrEqualToBigInt({}, {})",
                    bigint_literal(value)?,
                    message(&format!("must be greater than or equal to {value}"))
                ),
                BigIntValidator::LessThanBigInt(value) => format!(
                    "Schema.lessThanBigInt({}, {})",
                    bigint_literal(value)?,
                    message(&format!("must be less than {value}"))
                ),
                BigIntValidator::LessThanOrEqualToBigInt(value) => format!(
                    "Schema.lessThanOrEqualToBigInt({}, {})",
                    bigint_literal(value)?,
                    message(&format!("must be less than or equal to {value}"))
                ),
                BigIntValidator::BetweenBigInt(start, end) => format!(
                    "Schema.betweenBigInt({}, {}, {})",
                    bigint_literal(start)?,
                    bigint_literal(end)?,
                    message(&format!("must be between {start} and {end}"))
                ),
                BigIntValidator::PositiveBigInt => {
                    format!("Schema.positiveBigInt({})", message("must be positive"))
                }
                BigIntValidator::NonNegativeBigInt => {
                    format!(
                        "Schema.nonNegativeBigInt({})",
                        message("must be non-negative")
                    )
                }
                BigIntValidator::NegativeBigInt => {
                    format!("Schema.negativeBigInt({})", message("must be negative"))
                }
                BigIntValidator::NonPositiveBigInt => {
                    format!(
                        "Schema.nonPositiveBigInt({})",
                        message("must be non-positive")
                    )
                }
            }],

            Validator::BigDecimalValidator(bdv) => vec![match bdv {
                BigDecimalValidator::GreaterThanBigDecimal(value) => format!(
                    "Schema.greaterThanBigDecimal({}, {})",
                    big_decimal_literal(value)?,
                    message(&format!("must be greater than {value}"))
                ),
                BigDecimalValidator::GreaterThanOrEqualToBigDecimal(value) => format!(
                    "Schema.greaterThanOrEqualToBigDecimal({}, {})",
                    big_decimal_literal(value)?,
                    message(&format!("must be greater than or equal to {value}"))
                ),
                BigDecimalValidator::LessThanBigDecimal(value) => format!(
                    "Schema.lessThanBigDecimal({}, {})",
                    big_decimal_literal(value)?,
                    message(&format!("must be less than {value}"))
                ),
                BigDecimalValidator::LessThanOrEqualToBigDecimal(value) => format!(
                    "Schema.lessThanOrEqualToBigDecimal({}, {})",
                    big_decimal_literal(value)?,
                    message(&format!("must be less than or equal to {value}"))
                ),
                BigDecimalValidator::BetweenBigDecimal(start, end) => format!(
                    "Schema.betweenBigDecimal({}, {}, {})",
                    big_decimal_literal(start)?,
                    big_decimal_literal(end)?,
                    message(&format!("must be between {start} and {end}"))
                ),
                BigDecimalValidator::PositiveBigDecimal => {
                    format!("Schema.positiveBigDecimal({})", message("must be positive"))
                }
                BigDecimalValidator::NonNegativeBigDecimal => format!(
                    "Schema.nonNegativeBigDecimal({})",
                    message("must be non-negative")
                ),
                BigDecimalValidator::NegativeBigDecimal => {
                    format!("Schema.negativeBigDecimal({})", message("must be negative"))
                }
                BigDecimalValidator::NonPositiveBigDecimal => format!(
                    "Schema.nonPositiveBigDecimal({})",
                    message("must be non-positive")
                ),
            }],

            // Effect's duration filters read its own `Duration`; a Rust
            // duration is serde's `{ secs, nanos }`.
            Validator::DurationValidator(dv) if matches!(field_type, FieldType::Duration) => {
                vec![serde_duration_filter(dv, &message)?]
            }
            Validator::DurationValidator(dv) => vec![match dv {
                DurationValidator::GreaterThanDuration(value) => format!(
                    "Schema.greaterThanDuration({}, {})",
                    duration_literal(value)?,
                    message(&format!("must be longer than {value}"))
                ),
                DurationValidator::GreaterThanOrEqualToDuration(value) => format!(
                    "Schema.greaterThanOrEqualToDuration({}, {})",
                    duration_literal(value)?,
                    message(&format!("must be at least {value} long"))
                ),
                DurationValidator::LessThanDuration(value) => format!(
                    "Schema.lessThanDuration({}, {})",
                    duration_literal(value)?,
                    message(&format!("must be shorter than {value}"))
                ),
                DurationValidator::LessThanOrEqualToDuration(value) => format!(
                    "Schema.lessThanOrEqualToDuration({}, {})",
                    duration_literal(value)?,
                    message(&format!("must be at most {value} long"))
                ),
                DurationValidator::BetweenDuration(start, end) => format!(
                    "Schema.betweenDuration({}, {}, {})",
                    duration_literal(start)?,
                    duration_literal(end)?,
                    message(&format!("must be between {start} and {end} long"))
                ),
            }],
        };
        for filter in filters {
            result.push_str(&format!(".pipe({filter})"));
        }
    }

    Ok(result)
}

/// A date bound as a TypeScript `Date`.
fn date_literal(date: &str) -> Result<String> {
    bounds::date(date).map_err(EvenframeError::config)?;
    Ok(format!("new Date({})", string_literal(date)?))
}

/// An integer bound as a TypeScript bigint literal.
fn bigint_literal(value: &str) -> Result<String> {
    bounds::big_int(value)
        .map(|number| format!("{number}n"))
        .map_err(EvenframeError::config)
}

/// A decimal bound decoded exactly, never through a float.
fn big_decimal_literal(value: &str) -> Result<String> {
    bounds::decimal(value).map_err(EvenframeError::config)?;
    Ok(format!(
        "Schema.decodeSync(Schema.BigDecimal)({})",
        string_literal(value)?
    ))
}

/// A duration bound as bigint nanoseconds, which Effect accepts as a
/// `DurationInput`.
/// A duration validator as a filter over serde's `{ secs, nanos }`.
fn serde_duration_filter(
    validator: &DurationValidator,
    message: &dyn Fn(&str) -> String,
) -> Result<String> {
    let (condition, rule) = match validator {
        DurationValidator::GreaterThanDuration(bound) => (
            format!("nanos > {}", duration_literal(bound)?),
            format!("must be longer than {bound}"),
        ),
        DurationValidator::GreaterThanOrEqualToDuration(bound) => (
            format!("nanos >= {}", duration_literal(bound)?),
            format!("must be at least {bound} long"),
        ),
        DurationValidator::LessThanDuration(bound) => (
            format!("nanos < {}", duration_literal(bound)?),
            format!("must be shorter than {bound}"),
        ),
        DurationValidator::LessThanOrEqualToDuration(bound) => (
            format!("nanos <= {}", duration_literal(bound)?),
            format!("must be at most {bound} long"),
        ),
        DurationValidator::BetweenDuration(start, end) => (
            format!(
                "nanos >= {} && nanos <= {}",
                duration_literal(start)?,
                duration_literal(end)?
            ),
            format!("must be between {start} and {end} long"),
        ),
    };
    Ok(format!(
        "Schema.filter((value) => ((nanos: bigint) => {condition})(BigInt(value.secs) * 1000000000n + BigInt(value.nanos)), {})",
        message(&rule)
    ))
}

fn duration_literal(value: &str) -> Result<String> {
    bounds::duration(value)
        .map(|nanos| format!("{nanos}n"))
        .map_err(EvenframeError::config)
}

#[cfg(test)]
mod tests {
    use super::{
        BigDecimalValidator, BigIntValidator, DateValidator, DurationValidator, FieldType,
        Validator, apply_validators_to_schema,
    };

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
            let output = super::generate_effect_schema_string(
                &crate::typesync::type_index::TypeIndex::new(
                    &structs,
                    &std::collections::BTreeMap::new(),
                )
                .unwrap(),
                false,
                &crate::types::ForeignTypeRegistry::default(),
            )
            .unwrap();
            assert!(
                output.contains(&format!("{first_name}: Schema.propertySignature")),
                "{output}"
            );
            assert!(
                output.contains("lastName: Schema.propertySignature"),
                "{output}"
            );
            assert!(
                output.contains("\"zip-code\": Schema.propertySignature"),
                "{output}"
            );
            assert!(
                output.contains("nickname: Schema.optional(Schema.String)"),
                "{output}"
            );
            assert!(output.contains("readonly nickname?: string;"), "{output}");
        }
    }

    #[test]
    fn rejects_unparsable_validator_bounds() {
        let validators = [
            Validator::DurationValidator(DurationValidator::GreaterThanDuration("soon".into())),
            Validator::DateValidator(DateValidator::LessThanDate("tomorrow".into())),
            Validator::BigIntValidator(BigIntValidator::GreaterThanBigInt("1.5".into())),
            Validator::BigDecimalValidator(BigDecimalValidator::LessThanBigDecimal("1e5".into())),
        ];
        for validator in validators {
            let result = apply_validators_to_schema(
                "Schema.String".into(),
                &FieldType::String,
                std::slice::from_ref(&validator),
                "field",
            );
            assert!(result.is_err(), "{validator:?} was accepted");
        }
    }
}
