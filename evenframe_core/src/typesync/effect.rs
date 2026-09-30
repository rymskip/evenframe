use crate::config::{EffectMapping, ForeignTypeConfig};
use crate::dependency::{RecursionInfo, analyse_recursion, deps_of};
use crate::error::{EvenframeError, Result};
use crate::types::{
    EnumRepresentation, FieldType, StructConfig, StructField, TaggedUnion, VariantData,
};
use crate::typesync::doc_comment::format_jsdoc;
use crate::typesync::foreign_ts::{RECORD_LINK, fill};
use crate::typesync::js_checks::{
    self, JsCheck, LengthCheck, ONE_CHARACTER, string_literal, template_literal,
};
use crate::typesync::map_key::{BOOL_KEYS, MapKey};
use crate::validator::keywords;
use crate::validator::string_rules::{StringParse, StringRule, StringTransform};
use crate::validator::{
    ArrayValidator, BigDecimalValidator, BigIntValidator, DateValidator, DurationValidator,
    NumberValidator, StringValidator, Validator, bounds,
};
use convert_case::{Case, Casing};
use petgraph::{algo::toposort, graphmap::DiGraphMap};
use std::collections::{BTreeMap, BTreeSet};
use tracing;

pub fn generate_effect_schema_string(
    structs: &BTreeMap<String, StructConfig>,
    enums: &BTreeMap<String, TaggedUnion>,
    print_types: bool,
    registry: &crate::types::ForeignTypeRegistry,
) -> Result<String> {
    tracing::info!(
        struct_count = structs.len(),
        enum_count = enums.len(),
        print_types = print_types,
        "Generating Effect Schema string"
    );

    // 1.  Analyse recursion once at the beginning.
    tracing::debug!("Analyzing recursion in types");
    let rec = analyse_recursion(structs, enums);

    // 2.  Topologically sort components so all **non-recursive**
    //     dependencies appear first. This removes the need for
    //     `Schema.suspend` outside of recursive strongly connected components (SCCs).
    tracing::debug!("Performing topological sort of components");
    let mut condensation = DiGraphMap::<usize, ()>::new();
    // Every component is a node, so a type with no dependency edges is still emitted.
    for &comp_id in rec.meta.keys() {
        condensation.add_node(comp_id);
    }
    for (t1, _tos) in rec
        .meta
        .values()
        .flat_map(|(_, mem)| mem.iter())
        .filter_map(|n| rec.comp_of.get(n).map(|&c| (n, c)))
    {
        let from_comp = rec.comp_of[t1];
        for t2 in &deps_of(t1, structs, enums) {
            let to_comp = rec.comp_of[t2];
            if from_comp != to_comp {
                // An edge A -> B means "A depends on B".
                condensation.add_edge(from_comp, to_comp, ());
            }
        }
    }
    // `toposort` gives an order where dependencies come first. We reverse it
    // to process dependencies before the types that use them.
    let mut ordered_comps = toposort(&condensation, None).unwrap_or_default();
    ordered_comps.reverse();

    // 3.  Generate all TypeScript code in a single, unified loop.
    tracing::debug!("Generating schema classes, types, and encoded interfaces");
    let mut out_classes = String::new();
    let mut out_types = String::new();
    let mut out_encoded = String::new(); // All '...Encoded' interfaces/types go here.
    let mut processed = BTreeSet::<String>::new();

    // Helper closure for field conversion that has access to `rec`.
    let to_schema = |ft: &FieldType, cur: &str, proc: &BTreeSet<String>| -> Result<String> {
        field_type_to_effect_schema(ft, structs, cur, &rec, proc, registry)
    };

    for comp_id in ordered_comps {
        // Order inside the SCC is arbitrary; preserve original order for deterministic output.
        let mut members = rec.meta[&comp_id].1.clone();
        members.sort();

        for name in members {
            if processed.contains(&name) {
                continue; // Skip if already processed
            }

            // `resolve_only` types stay in the maps for reference resolution
            // (field conversion below) but are not emitted as their own schema
            // class/interface.
            let resolve_only = enums
                .values()
                .any(|e| e.resolve_only && e.enum_name.to_case(Case::Pascal) == name)
                || structs
                    .values()
                    .any(|s| s.resolve_only && s.struct_name.to_case(Case::Pascal) == name);
            if resolve_only {
                processed.insert(name);
                continue;
            }

            if let Some(e) = enums
                .values()
                .find(|e| e.enum_name.to_case(Case::Pascal) == name)
            {
                // ---- ENUM ---------------------------------------------------
                // Write doc comment if present
                if let Some(ref doc) = e.doccom {
                    out_classes.push_str(&format_jsdoc(doc, ""));
                }

                // Generate the schema class for the enum.
                out_classes.push_str(&format!("export const {} = Schema.Union(", name));
                let variants = e
                    .variants
                    .iter()
                    .map(|v| {
                        enum_variant_to_schema(
                            v,
                            &e.representation,
                            &name,
                            &to_schema,
                            &processed,
                            structs,
                        )
                    })
                    .collect::<Result<Vec<_>>>()?
                    .join(", ");
                out_classes.push_str(&variants);
                out_classes.push_str(&format!(").annotations({{ identifier: `{}` }});\n", name));

                // Generate the `.Type` alias.
                out_types.push_str(&format!(
                    "export type {}Type = typeof {}.Type;\n",
                    name, name
                ));

                // Generate the `...Encoded` type alias for the enum.
                out_encoded.push_str(&encoded_alias_for_enum(e, registry)?);
            } else if let Some(struct_config) = structs
                .values()
                .find(|sc| sc.struct_name.to_case(Case::Pascal) == name)
            {
                // ---- STRUCT -------------------------------------------------
                // Write doc comment if present
                if let Some(ref doc) = struct_config.doccom {
                    out_classes.push_str(&format_jsdoc(doc, ""));
                }

                // Generate the schema class for the struct.
                out_classes.push_str(&format!(
                    "export class {} extends Schema.Class<{}>(\"{}\")( {{ \n",
                    name, name, name
                ));
                for (idx, f) in struct_config.fields.iter().enumerate() {
                    if let Some(ref doc) = f.doccom {
                        out_classes.push_str(&format_jsdoc(doc, "  "));
                    }
                    let entry = field_schema_entry(f, |field_type| {
                        to_schema(field_type, &name, &processed)
                    })?;
                    let separator = if idx + 1 == struct_config.fields.len() {
                        ""
                    } else {
                        ","
                    };
                    out_classes.push_str(&format!("  {entry}{separator}\n"));
                }
                out_classes.push_str("}) {[key: string]: unknown}\n\n");

                // Generate the `.Type` alias.
                out_types.push_str(&format!(
                    "export type {}Type = typeof {}.Type;\n",
                    name, name
                ));

                // Generate the `...Encoded` interface for the struct.
                out_encoded.push_str(&encoded_interface_for_struct(struct_config, registry)?);
            }
            processed.insert(name);
        }
    }

    let result = if print_types {
        format!("{out_classes}\n{out_encoded}\n{out_types}")
    } else {
        format!("{out_classes}\n{out_encoded}")
    };

    tracing::info!(
        output_length = result.len(),
        "Effect Schema generation complete"
    );
    Ok(result)
}

// ----- Encoded Type Generation Helpers -------------------------------------

/// A struct field as an `...Encoded` entry, `readonly name: type;`.
fn encoded_field_entry(
    field: &StructField,
    registry: &crate::types::ForeignTypeRegistry,
) -> Result<String> {
    let encoded = match (&field.field_type, parses_string_input(field)) {
        (FieldType::Option(_), true) => "string | null | undefined".to_owned(),
        (_, true) => "string".to_owned(),
        (field_type, false) => field_type_to_ts_encoded(field_type, registry)?,
    };
    Ok(format!(
        "readonly {}: {encoded};",
        field.field_name.to_case(Case::Camel)
    ))
}

/// Generates an `...Encoded` TypeScript interface for a given struct.
fn encoded_interface_for_struct(
    struct_config: &StructConfig,
    registry: &crate::types::ForeignTypeRegistry,
) -> Result<String> {
    let name = struct_config.struct_name.to_case(Case::Pascal);
    let body = struct_config
        .fields
        .iter()
        .map(|field| Ok(format!("  {}", encoded_field_entry(field, registry)?)))
        .collect::<Result<Vec<_>>>()?
        .join("\n");

    Ok(format!(
        "export interface {}Encoded {{\n{}\n}}\n\n",
        name, body
    ))
}

/// Generates an `...Encoded` TypeScript type alias for a given enum/union.
fn encoded_alias_for_enum(
    en: &TaggedUnion,
    registry: &crate::types::ForeignTypeRegistry,
) -> Result<String> {
    tracing::trace!(enum_name = %en.enum_name, "Creating encoded alias for enum");
    let name = en.enum_name.to_case(Case::Pascal);
    let body = en
        .variants
        .iter()
        .map(|v| enum_variant_to_encoded(v, &en.representation, registry))
        .collect::<Result<Vec<_>>>()?
        .join(" | ");
    Ok(format!("export type {}Encoded = {};\n\n", name, body))
}

// ----- Representation-Aware Variant Helpers --------------------------------

/// A struct field as a schema entry, `name: schema`, with its validators. A
/// required field reports a missing value by its title.
fn field_schema_entry(
    field: &StructField,
    schema_of: impl Fn(&FieldType) -> Result<String>,
) -> Result<String> {
    let schema = validated_field_schema(field, schema_of)?;
    let entry = if matches!(field.field_type, FieldType::Option(_)) {
        schema
    } else {
        format!(
            "Schema.propertySignature({schema}).annotations({{ missingMessage: () => {} }})",
            template_literal(&format!(
                "'{}' is required",
                field.field_name.to_case(Case::Title)
            ))
        )
    };
    Ok(format!(
        "{}: {entry}",
        field.field_name.to_case(Case::Camel)
    ))
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
    processed: &BTreeSet<String>,
    structs: &BTreeMap<String, StructConfig>,
) -> Result<String>
where
    F: Fn(&FieldType, &str, &BTreeSet<String>) -> Result<String>,
{
    let tag_entry = |tag: &str| format!("{tag}: Schema.Literal(\"{}\")", v.name);
    let Some(data) = &v.data else {
        return Ok(match repr {
            EnumRepresentation::InternallyTagged { tag }
            | EnumRepresentation::AdjacentlyTagged { tag, .. } => {
                format!("Schema.Struct({{ {} }})", tag_entry(tag))
            }
            EnumRepresentation::ExternallyTagged | EnumRepresentation::Untagged => {
                format!("Schema.Literal(\"{}\")", v.name)
            }
        });
    };
    let fields_schema = |fields: &[StructField], tag: Option<&str>| -> Result<String> {
        let mut entries: Vec<String> = tag.map(tag_entry).into_iter().collect();
        for field in fields {
            entries.push(field_schema_entry(field, |field_type| {
                to_schema(field_type, enum_name, processed)
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
            Some(tag) => return fields_schema(held_struct_fields(field_type, structs)?, Some(tag)),
            None => to_schema(field_type, enum_name, processed)?,
        },
    };
    Ok(match repr {
        EnumRepresentation::ExternallyTagged | EnumRepresentation::InternallyTagged { .. } => {
            format!("Schema.Struct({{ {}: {payload} }})", v.name)
        }
        EnumRepresentation::AdjacentlyTagged { tag, content } => {
            format!(
                "Schema.Struct({{ {}, {content}: {payload} }})",
                tag_entry(tag)
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
    registry: &crate::types::ForeignTypeRegistry,
) -> Result<String> {
    let tag_entry = |tag: &str| format!("readonly {tag}: \"{}\";", v.name);
    let Some(data) = &v.data else {
        return Ok(match repr {
            EnumRepresentation::InternallyTagged { tag }
            | EnumRepresentation::AdjacentlyTagged { tag, .. } => {
                format!("{{ {} }}", tag_entry(tag))
            }
            EnumRepresentation::ExternallyTagged | EnumRepresentation::Untagged => {
                format!("\"{}\"", v.name)
            }
        });
    };
    let payload = match data {
        VariantData::InlineStruct(inline) => {
            let entries = inline
                .fields
                .iter()
                .map(|field| encoded_field_entry(field, registry))
                .collect::<Result<Vec<String>>>()?;
            if let EnumRepresentation::InternallyTagged { tag } = repr {
                return Ok(format!("{{ {} {} }}", tag_entry(tag), entries.join(" ")));
            }
            format!("{{ {} }}", entries.join(" "))
        }
        VariantData::DataStructureRef(field_type) => {
            let payload = field_type_to_ts_encoded(field_type, registry)?;
            // serde writes the tag into the struct the variant holds.
            if let EnumRepresentation::InternallyTagged { tag } = repr {
                return Ok(format!("({{ {} }} & {payload})", tag_entry(tag)));
            }
            payload
        }
    };
    Ok(match repr {
        EnumRepresentation::ExternallyTagged | EnumRepresentation::InternallyTagged { .. } => {
            format!("{{ readonly {}: {payload} }}", v.name)
        }
        EnumRepresentation::AdjacentlyTagged { tag, content } => {
            format!("{{ {} readonly {content}: {payload}; }}", tag_entry(tag))
        }
        EnumRepresentation::Untagged => payload,
    })
}

/// The fields of the struct an internally tagged newtype variant holds. serde
/// writes the tag into that struct's object, which only a struct can take.
fn held_struct_fields<'a>(
    field_type: &FieldType,
    structs: &'a BTreeMap<String, StructConfig>,
) -> Result<&'a [StructField]> {
    let held = match field_type {
        FieldType::Other(name) => {
            let pascal = name.to_case(Case::Pascal);
            structs
                .values()
                .find(|struct_config| struct_config.struct_name.to_case(Case::Pascal) == pascal)
        }
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

/// The Effect mapping a project configures for the record link, if any.
fn record_link_mapping(registry: &crate::types::ForeignTypeRegistry) -> Option<&EffectMapping> {
    registry
        .lookup(RECORD_LINK)
        .and_then(|record_link| record_link.effect.as_ref())
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
    structs: &BTreeMap<String, StructConfig>,
    current: &str,
    rec: &RecursionInfo,
    processed: &BTreeSet<String>,
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
                FieldType::String => value_stack.push(
                    "Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))"
                        .to_string(),
                ),
                FieldType::Char => value_stack.push(char_schema()?),
                FieldType::Bool => value_stack.push("Schema.Boolean".to_string()),
                FieldType::Unit => value_stack.push("Schema.Null".to_string()),
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
                        key: map_key_schema(k, registry)?,
                        finite_keys: MapKey::require(k)?.is_finite(registry),
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
                    if rec.is_recursive_pair(current, &pascal) && !processed.contains(&pascal) {
                        // Forward edge *inside* a recursive SCC requires suspension.
                        if structs
                            .values()
                            .any(|sc| sc.struct_name.to_case(Case::Pascal) == pascal)
                        {
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
                value_stack.push(match record_link_mapping(registry) {
                    Some(mapping) => fill(&mapping.type_expr, &[inner]),
                    None => format!(
                        "Schema.Union(Schema.String.pipe(Schema.nonEmptyString()), {}).annotations({{ message: () => ({{
                message: `Please enter a valid value`,
                override: true,
            }}), }})",
                        inner
                    ),
                });
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
                            key: map_key_encoded(k, registry)?,
                            finite_keys: MapKey::require(k)?.is_finite(registry),
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
                value_stack.push(match record_link_mapping(registry) {
                    Some(mapping) => fill(&mapping.encoded, &[inner]),
                    None => format!("string | {}", inner),
                });
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

/// Generates Effect Schema code for a specific subset of types (used in per-file mode).
///
/// Types NOT in `type_names` are treated as already-processed (imported from other files),
/// so `Schema.suspend()` will NOT be emitted for cross-file references.
pub fn generate_effect_schema_for_types(
    type_names: &[String],
    structs: &BTreeMap<String, StructConfig>,
    enums: &BTreeMap<String, TaggedUnion>,
    registry: &crate::types::ForeignTypeRegistry,
) -> Result<String> {
    let type_set: BTreeSet<String> = type_names.iter().cloned().collect();

    // Analyse recursion across ALL types (needed for correct SCC detection).
    let rec = analyse_recursion(structs, enums);

    // Build condensation graph for topological ordering.
    let mut condensation = DiGraphMap::<usize, ()>::new();
    // Every component is a node, so a type with no dependency edges is still emitted.
    for &comp_id in rec.meta.keys() {
        condensation.add_node(comp_id);
    }
    for (t1, _) in rec
        .meta
        .values()
        .flat_map(|(_, mem)| mem.iter())
        .filter_map(|n| rec.comp_of.get(n).map(|&c| (n, c)))
    {
        let from_comp = rec.comp_of[t1];
        for t2 in &deps_of(t1, structs, enums) {
            let to_comp = rec.comp_of[t2];
            if from_comp != to_comp {
                condensation.add_edge(from_comp, to_comp, ());
            }
        }
    }
    let mut ordered_comps = toposort(&condensation, None).unwrap_or_default();
    ordered_comps.reverse();

    // Pre-populate processed with all types NOT in this group.
    // This means cross-file references won't get Schema.suspend().
    // Use `effective()` so overrides replace the scanned type.
    let all_types: BTreeSet<String> = structs
        .values()
        .map(|s| s.effective().struct_name.to_case(Case::Pascal))
        .chain(
            enums
                .values()
                .map(|e| e.effective().enum_name.to_case(Case::Pascal)),
        )
        .collect();
    let mut processed: BTreeSet<String> = all_types.difference(&type_set).cloned().collect();

    let to_schema = |ft: &FieldType, cur: &str, proc: &BTreeSet<String>| -> Result<String> {
        field_type_to_effect_schema(ft, structs, cur, &rec, proc, registry)
    };

    let mut out_classes = String::new();
    let mut out_encoded = String::new();

    for comp_id in ordered_comps {
        let mut members = rec.meta[&comp_id].1.clone();
        members.sort();

        for name in members {
            if processed.contains(&name) || !type_set.contains(&name) {
                continue;
            }

            if let Some(e) = enums
                .values()
                .find(|e| e.enum_name.to_case(Case::Pascal) == name)
            {
                if let Some(ref doc) = e.doccom {
                    out_classes.push_str(&format_jsdoc(doc, ""));
                }
                out_classes.push_str(&format!("export const {} = Schema.Union(", name));
                let variants = e
                    .variants
                    .iter()
                    .map(|v| {
                        enum_variant_to_schema(
                            v,
                            &e.representation,
                            &name,
                            &to_schema,
                            &processed,
                            structs,
                        )
                    })
                    .collect::<Result<Vec<_>>>()?
                    .join(", ");
                out_classes.push_str(&variants);
                out_classes.push_str(&format!(").annotations({{ identifier: `{}` }});\n", name));
                out_classes.push_str(&format!(
                    "export type {}Type = typeof {}.Type;\n",
                    name, name
                ));
                out_encoded.push_str(&encoded_alias_for_enum(e, registry)?);
            } else if let Some(struct_config) = structs
                .values()
                .find(|sc| sc.struct_name.to_case(Case::Pascal) == name)
            {
                if let Some(ref doc) = struct_config.doccom {
                    out_classes.push_str(&format_jsdoc(doc, ""));
                }
                out_classes.push_str(&format!(
                    "export class {} extends Schema.Class<{}>(\"{}\")( {{ \n",
                    name, name, name
                ));
                for (idx, f) in struct_config.fields.iter().enumerate() {
                    if let Some(ref doc) = f.doccom {
                        out_classes.push_str(&format_jsdoc(doc, "  "));
                    }
                    let entry = field_schema_entry(f, |field_type| {
                        to_schema(field_type, &name, &processed)
                    })?;
                    let separator = if idx + 1 == struct_config.fields.len() {
                        ""
                    } else {
                        ","
                    };
                    out_classes.push_str(&format!("  {entry}{separator}\n"));
                }
                out_classes.push_str("}) {[key: string]: unknown}\n\n");
                out_classes.push_str(&format!(
                    "export type {}Type = typeof {}.Type;\n",
                    name, name
                ));
                out_encoded.push_str(&encoded_interface_for_struct(struct_config, registry)?);
            }
            processed.insert(name);
        }
    }

    Ok(format!("{out_classes}\n{out_encoded}"))
}

// ----- Validator Application Logic -----------------------------------------

/// A field's schema with its validators applied. An optional field's
/// validators constrain the present value, as the Rust deserializer does, so
/// they go on the inner schema before it is wrapped.
fn validated_field_schema(
    field: &StructField,
    schema_of: impl Fn(&FieldType) -> Result<String>,
) -> Result<String> {
    match &field.field_type {
        FieldType::Option(inner) if !field.validators.is_empty() => Ok(format!(
            "Schema.OptionFromNullishOr({}, null)",
            apply_validators_to_schema(schema_of(inner)?, &field.validators, &field.field_name)?
        )),
        field_type => {
            apply_validators_to_schema(schema_of(field_type)?, &field.validators, &field.field_name)
        }
    }
}

/// Whether a field is read through a parse morph, so its encoded form is a
/// string whatever its Rust type.
fn parses_string_input(field: &StructField) -> bool {
    matches!(
        field.validators.first(),
        Some(Validator::StringValidator(validator))
            if matches!(validator.rule(), StringRule::Parse(_))
    )
}

/// `schema` with `validators` applied in order. A parse morph replaces the
/// schema with one that decodes a string into the field's type.
fn apply_validators_to_schema(
    schema: String,
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
                                "Schema.String.pipe(Schema.filter((v) => {predicate}, {input})).pipe(Schema.compose(Schema.NumberFromString)).pipe(Schema.compose(Schema.DateFromNumber))"
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
                    Some(JsCheck::Pattern(source)) => vec![format!(
                        "Schema.pattern(new RegExp({}), {})",
                        string_literal(&source)?,
                        expected(&sv.expectation())
                    )],
                    Some(JsCheck::Predicate(predicate)) => vec![format!(
                        "Schema.filter((v) => {predicate}, {})",
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
fn duration_literal(value: &str) -> Result<String> {
    bounds::duration(value)
        .map(|nanos| format!("{nanos}n"))
        .map_err(EvenframeError::config)
}

#[cfg(test)]
mod tests {
    use super::*;

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
                std::slice::from_ref(&validator),
                "field",
            );
            assert!(result.is_err(), "{validator:?} was accepted");
        }
    }
}
