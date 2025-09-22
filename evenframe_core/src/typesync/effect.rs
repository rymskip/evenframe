use crate::dependency::{RecursionInfo, analyse_recursion, deps_of};
use crate::types::{FieldType, StructConfig, TaggedUnion, VariantData};
use crate::validator::{
    ArrayValidator, BigDecimalValidator, BigIntValidator, DateValidator, DurationValidator,
    NumberValidator, StringValidator, Validator,
};
use convert_case::{Case, Casing};
use petgraph::{algo::toposort, graphmap::DiGraphMap};
use std::collections::{HashMap, HashSet};
use tracing;

pub fn generate_effect_schema_string(
    structs: &HashMap<String, StructConfig>,
    enums: &HashMap<String, TaggedUnion>,
    print_types: bool,
) -> String {
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
    let mut processed = HashSet::<String>::new();

    // Helper closure for field conversion that has access to `rec`.
    let to_schema = |ft: &FieldType, cur: &str, proc: &HashSet<String>| -> String {
        field_type_to_effect_schema(ft, structs, cur, &rec, proc)
    };

    for comp_id in ordered_comps {
        // Order inside the SCC is arbitrary; preserve original order for deterministic output.
        let mut members = rec.meta[&comp_id].1.clone();
        members.sort();

        for name in members {
            if processed.contains(&name) {
                continue; // Skip if already processed
            }

            if let Some(e) = enums
                .values()
                .find(|e| e.enum_name.to_case(Case::Pascal) == name)
            {
                // ---- ENUM ---------------------------------------------------
                // Generate the schema class for the enum.
                out_classes.push_str(&format!("export const {} = Schema.Union(", name));
                let variants = e
                    .variants
                    .iter()
                    .map(|v| {
                        v.data
                            .as_ref()
                            .map(|variant_data| match variant_data {
                                VariantData::InlineStruct(_) => v.name.to_case(Case::Pascal),
                                VariantData::DataStructureRef(field_type) => {
                                    to_schema(field_type, &name, &processed)
                                }
                            })
                            .unwrap_or_else(|| format!("Schema.Literal(\"{}\")", v.name))
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                out_classes.push_str(&variants);
                out_classes.push_str(&format!(").annotations({{ identifier: `{}` }});\n", name));

                // Generate the `.Type` alias.
                out_types.push_str(&format!(
                    "export type {}Type = typeof {}.Type;\n",
                    name, name
                ));

                // Generate the `...Encoded` type alias for the enum.
                out_encoded.push_str(&encoded_alias_for_enum(e));
            } else if let Some(struct_config) = structs
                .values()
                .find(|sc| sc.struct_name.to_case(Case::Pascal) == name)
            {
                // ---- STRUCT -------------------------------------------------
                // Generate the schema class for the struct.
                out_classes.push_str(&format!(
                    "export class {} extends Schema.Class<{}>(\"{}\")( {{ \n",
                    name, name, name
                ));
                for (idx, f) in struct_config.fields.iter().enumerate() {
                    let schema = to_schema(&f.field_type, &name, &processed);
                    let schema_with_validators =
                        apply_validators_to_schema(schema, &f.validators, &f.field_name);
                    let field_name_camel = f.field_name.to_case(Case::Camel);
                    let field_name_title = f.field_name.to_case(Case::Title);

                    let is_optional = matches!(f.field_type, FieldType::Option(_));

                    let final_schema = if !is_optional {
                        format!(
                            "Schema.propertySignature({}).annotations({{ missingMessage: () => `'{}' is required` }})",
                            schema_with_validators, field_name_title
                        )
                    } else {
                        schema_with_validators
                    };

                    out_classes.push_str(&format!(
                        "  {}: {}{}",
                        field_name_camel,
                        final_schema,
                        if idx + 1 == struct_config.fields.len() {
                            ""
                        } else {
                            ","
                        }
                    ));
                    out_classes.push('\n');
                }
                out_classes.push_str("}) {[key: string]: unknown}\n\n");

                // Generate the `.Type` alias.
                out_types.push_str(&format!(
                    "export type {}Type = typeof {}.Type;\n",
                    name, name
                ));

                // Generate the `...Encoded` interface for the struct.
                out_encoded.push_str(&encoded_interface_for_struct(struct_config));
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
    result
}

// ----- Encoded Type Generation Helpers -------------------------------------

/// Generates an `...Encoded` TypeScript interface for a given struct.
fn encoded_interface_for_struct(struct_config: &StructConfig) -> String {
    let name = struct_config.struct_name.to_case(Case::Pascal);
    let body = struct_config
        .fields
        .iter()
        .map(|f| {
            format!(
                "  readonly {}: {};",
                f.field_name.to_case(Case::Camel),
                field_type_to_ts_encoded(&f.field_type)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!("export interface {}Encoded {{\n{}\n}}\n\n", name, body)
}

/// Generates an `...Encoded` TypeScript type alias for a given enum/union.
fn encoded_alias_for_enum(en: &TaggedUnion) -> String {
    tracing::trace!(enum_name = %en.enum_name, "Creating encoded alias for enum");
    let name = en.enum_name.to_case(Case::Pascal);
    let union = en
        .variants
        .iter()
        .map(|v| match &v.data {
            Some(variant_data) => match variant_data {
                VariantData::InlineStruct(_) => {
                    // For inline structs, use the variant name + "Encoded"
                    format!("{}Encoded", v.name.to_case(Case::Pascal))
                }
                VariantData::DataStructureRef(field_type) => field_type_to_ts_encoded(field_type),
            },
            None => format!("\"{}\"", v.name),
        })
        .collect::<Vec<_>>()
        .join(" | ");

    format!("export type {}Encoded = {};\n\n", name, union)
}

// ----- Schema and Type Conversion Logic ------------------------------------

/// Converts a `FieldType` into its corresponding Effect `Schema` representation.
fn field_type_to_effect_schema(
    field_type: &FieldType,
    structs: &HashMap<String, StructConfig>,
    current: &str,
    rec: &RecursionInfo,
    processed: &HashSet<String>,
) -> String {
    enum WorkItem<'a> {
        Generate(&'a FieldType),
        AssembleOption,
        AssembleVec,
        AssembleTuple { count: usize },
        AssembleStruct { field_names: Vec<String> },
        AssembleRecordLink,
        AssembleMap,
    }

    let mut work_stack: Vec<WorkItem> = Vec::new();
    let mut value_stack: Vec<String> = Vec::new();

    work_stack.push(WorkItem::Generate(field_type));

    while let Some(work_item) = work_stack.pop() {
        match work_item {
            WorkItem::Generate(field_type) => match field_type {
                FieldType::String => {
                    value_stack.push(
                        "Schema.String.pipe(Schema.nonEmptyString({ message: () => `Please enter a value` }))"
                            .to_string(),
                    )
                }
                FieldType::Char => {
                    value_stack.push("Schema.String.pipe(Schema.maxLength(1))".to_string())
                }
                FieldType::Bool => value_stack.push("Schema.Boolean".to_string()),
                FieldType::Unit => value_stack.push("Schema.Null".to_string()),
                FieldType::Decimal => value_stack.push("Schema.NumberFromString".to_string()),
                FieldType::OrderedFloat(_) => value_stack.push("Schema.Number".to_string()),
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
                FieldType::EvenframeRecordId => value_stack.push("Schema.String".to_string()),
                FieldType::DateTime => value_stack.push("Schema.DateTimeUtc".to_string()),
                FieldType::EvenframeDuration => value_stack.push("Schema.Duration".to_string()),
                FieldType::Timezone => value_stack.push("Schema.TimeZoneNamed".to_string()),
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
                    work_stack.push(WorkItem::AssembleMap);
                    work_stack.push(WorkItem::Generate(v));
                    work_stack.push(WorkItem::Generate(k));
                }
                FieldType::Other(name) => {
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
                    .zip(values.into_iter())
                    .map(|(name, value)| format!("{}: {}", name, value))
                    .collect();
                value_stack.push(format!("Schema.Struct({{ {} }})", assignments.join(", ")));
            }
            WorkItem::AssembleRecordLink => {
                let inner = value_stack.pop().unwrap();
                value_stack.push(format!(
                    "Schema.Union(Schema.String.pipe(Schema.nonEmptyString()), {}).annotations({{ message: () => ({{
                message: `Please enter a valid value`,
                override: true,
            }}), }})",
                    inner
                ));
            }
            WorkItem::AssembleMap => {
                let v = value_stack.pop().unwrap();
                let k = value_stack.pop().unwrap();
                value_stack.push(format!("Schema.Record({{ key: {}, value: {} }})", k, v));
            }
        }
    }

    assert_eq!(
        value_stack.len(),
        1,
        "Generation ended with not exactly one value on the stack."
    );
    value_stack.pop().unwrap()
}

/// Converts a `FieldType` into its corresponding raw TypeScript type for the `...Encoded` interface.
fn field_type_to_ts_encoded(ft: &FieldType) -> String {
    enum WorkItem<'a> {
        Generate(&'a FieldType),
        AssembleOption,
        AssembleVec,
        AssembleTuple { count: usize },
        AssembleStruct { field_names: Vec<String> },
        AssembleRecordLink,
        AssembleMap,
    }

    let mut work_stack: Vec<WorkItem> = Vec::new();
    let mut value_stack: Vec<String> = Vec::new();

    work_stack.push(WorkItem::Generate(ft));

    while let Some(work_item) = work_stack.pop() {
        match work_item {
            WorkItem::Generate(ft) => {
                match ft {
                    // Primitives
                    FieldType::String
                    | FieldType::Char
                    | FieldType::EvenframeRecordId
                    | FieldType::Timezone => value_stack.push("string".to_string()),
                    FieldType::Bool => value_stack.push("boolean".to_string()),
                    FieldType::DateTime => value_stack.push("string".to_string()), // ISO 8601 string
                    FieldType::EvenframeDuration => {
                        value_stack.push(
                            "| Schema.DurationEncoded |readonly [seconds: number, nanos: number]"
                                .to_string(),
                        );
                    }
                    FieldType::Unit => value_stack.push("null".to_string()),
                    FieldType::Decimal => value_stack.push("string".to_string()),
                    FieldType::OrderedFloat(_)
                    | FieldType::F32
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
                        work_stack.push(WorkItem::AssembleMap);
                        work_stack.push(WorkItem::Generate(v));
                        work_stack.push(WorkItem::Generate(k));
                    }
                    FieldType::RecordLink(inner) => {
                        work_stack.push(WorkItem::AssembleRecordLink);
                        work_stack.push(WorkItem::Generate(inner));
                    }

                    // User-defined types
                    FieldType::Other(name) => {
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
                    .zip(values.into_iter())
                    .map(|(name, value)| format!("  readonly {}: {};", name, value))
                    .collect();
                value_stack.push(format!("{{\n{}\n}}", assignments.join("\n")));
            }
            WorkItem::AssembleMap => {
                let v = value_stack.pop().unwrap();
                let k = value_stack.pop().unwrap();
                value_stack.push(format!("Record<{}, {}>", k, v));
            }
            WorkItem::AssembleRecordLink => {
                let inner = value_stack.pop().unwrap();
                value_stack.push(format!("string | {}", inner));
            }
        }
    }

    assert_eq!(
        value_stack.len(),
        1,
        "Generation ended with not exactly one value on the stack."
    );
    value_stack.pop().unwrap()
}

// ----- Validator Application Logic -----------------------------------------

/// Applies a series of validators to a schema string by chaining `.pipe()` calls.
fn apply_validators_to_schema(
    schema: String,
    validators: &[Validator],
    field_name: &str,
) -> String {
    if validators.is_empty() {
        return schema;
    }

    let mut result = schema;
    let field_name_title = field_name.to_case(Case::Title);

    for validator in validators {
        result = match validator {
            // String validators
            Validator::StringValidator(sv) => match sv {
                StringValidator::MinLength(len) => format!(
                    "{}.pipe(Schema.minLength({}, {{ message: () => `{}` must be at least {} characters long` }}))",
                    result, len, field_name_title, len
                ),
                StringValidator::MaxLength(len) => format!(
                    "{}.pipe(Schema.maxLength({}, {{ message: () => `{}` must be at most {} characters long` }}))",
                    result, len, field_name_title, len
                ),
                StringValidator::Length(len) => format!(
                    "{}.pipe(Schema.length({}, {{ message: () => `{}` must be exactly {} characters long` }}))",
                    result, len, field_name_title, len
                ),
                StringValidator::NonEmpty => format!(
                    "{}.pipe(Schema.nonEmptyString({{ message: () => `{}` Please enter a value` }}))",
                    result, field_name_title
                ),
                StringValidator::StartsWith(prefix) => format!(
                    "{}.pipe(Schema.startsWith(\"{}\", {{ message: () => `{}` must start with \"{}\" }})",
                    result, prefix, field_name_title, prefix
                ),
                StringValidator::EndsWith(suffix) => format!(
                    "{}.pipe(Schema.endsWith(\"{}\", {{ message: () => `{}` must end with \"{}\" }})",
                    result, suffix, field_name_title, suffix
                ),
                StringValidator::Includes(substring) => format!(
                    "{}.pipe(Schema.includes(\"{}\", {{ message: () => `{}` must include \"{}\" }})",
                    result, substring, field_name_title, substring
                ),
                StringValidator::Trimmed => format!("{}.pipe(Schema.trimmed)", result),
                StringValidator::Lowercased => format!("{}.pipe(Schema.toLowerCase", result),
                StringValidator::Uppercased => format!("{}.pipe(Schema.toUpperCase", result),
                StringValidator::Capitalized => format!("{}.pipe(Schema.capitalize", result),
                StringValidator::Uncapitalized => format!("{}.pipe(Schema.uncapitalize", result),
                StringValidator::RegexLiteral(format_variant) => format!(
                    "{}.pipe(Schema.pattern(/{}/, {{ message: () => `{}` has an invalid format` }}))",
                    result,
                    format_variant.to_owned().into_regex().as_str(),
                    field_name_title
                ),
                _ => result,
            },

            // Number validators
            Validator::NumberValidator(nv) => match nv {
                NumberValidator::GreaterThan(value) => format!(
                    "{}.pipe(Schema.greaterThan({}, {{ message: () => `{}` must be greater than {}` }}))",
                    result, value.0, field_name_title, value.0
                ),
                NumberValidator::GreaterThanOrEqualTo(value) => format!(
                    "{}.pipe(Schema.greaterThanOrEqualTo({}, {{ message: () => `{}` must be greater than or equal to {}` }}))",
                    result, value.0, field_name_title, value.0
                ),
                NumberValidator::LessThan(value) => format!(
                    "{}.pipe(Schema.lessThan({}, {{ message: () => `{}` must be less than {}` }}))",
                    result, value.0, field_name_title, value.0
                ),
                NumberValidator::LessThanOrEqualTo(value) => format!(
                    "{}.pipe(Schema.lessThanOrEqualTo({}, {{ message: () => `{}` must be less than or equal to {}` }}))",
                    result, value.0, field_name_title, value.0
                ),
                NumberValidator::Between(start, end) => format!(
                    "{}.pipe(Schema.between({}, {}, {{ message: () => `{}` must be between {} and {}` }}))",
                    result, start.0, end.0, field_name_title, start.0, end.0
                ),
                NumberValidator::Int => format!(
                    "{}.pipe(Schema.int({{ message: () => `{}` must be an integer` }}))",
                    result, field_name_title
                ),
                NumberValidator::NonNaN => format!(
                    "{}.pipe(Schema.nonNaN({{ message: () => `{}` must not be NaN` }}))",
                    result, field_name_title
                ),
                NumberValidator::Finite => format!(
                    "{}.pipe(Schema.finite({{ message: () => `{}` must be a finite number` }}))",
                    result, field_name_title
                ),
                NumberValidator::Positive => format!(
                    "{}.pipe(Schema.positive({{ message: () => `{}` must be a positive number` }}))",
                    result, field_name_title
                ),
                NumberValidator::NonNegative => format!(
                    "{}.pipe(Schema.nonNegative({{ message: () => `{}` must be a non-negative number` }}))",
                    result, field_name_title
                ),
                NumberValidator::Negative => format!(
                    "{}.pipe(Schema.negative({{ message: () => `{}` must be a negative number` }}))",
                    result, field_name_title
                ),
                NumberValidator::NonPositive => format!(
                    "{}.pipe(Schema.nonPositive({{ message: () => `{}` must be a non-positive number` }}))",
                    result, field_name_title
                ),
                NumberValidator::MultipleOf(value) => format!(
                    "{}.pipe(Schema.multipleOf({}, {{ message: () => `{}` must be a multiple of {}` }}))",
                    result, value.0, field_name_title, value.0
                ),
                NumberValidator::Uint8 => result,
            },

            // Array validators
            Validator::ArrayValidator(av) => match av {
                ArrayValidator::MinItems(count) => format!(
                    "{}.pipe(Schema.minItems({}, {{ message: () => `{}` must contain at least {} items` }}))",
                    result, count, field_name_title, count
                ),
                ArrayValidator::MaxItems(count) => format!(
                    "{}.pipe(Schema.maxItems({}, {{ message: () => `{}` must contain at most {} items` }}))",
                    result, count, field_name_title, count
                ),
                ArrayValidator::ItemsCount(count) => format!(
                    "{}.pipe(Schema.itemsCount({}, {{ message: () => `{}` must contain exactly {} items` }}))",
                    result, count, field_name_title, count
                ),
            },

            // Date validators
            Validator::DateValidator(dv) => match dv {
                DateValidator::ValidDate => format!("{}.pipe(Schema.ValidDate)", result),
                DateValidator::GreaterThanDate(date) => format!(
                    "{}.pipe(Schema.greaterThan(new Date(\"{}\"), {{ message: () => `{}` must be after `{}` }}))",
                    result, date, field_name_title, date
                ),
                DateValidator::GreaterThanOrEqualToDate(date) => format!(
                    "{}.pipe(Schema.greaterThanOrEqualTo(new Date(\"{}\"), {{ message: () => `{}` must be on or after `{}` }}))",
                    result, date, field_name_title, date
                ),
                DateValidator::LessThanDate(date) => format!(
                    "{}.pipe(Schema.lessThan(new Date(\"{}\"), {{ message: () => `{}` must be before `{}` }}))",
                    result, date, field_name_title, date
                ),
                DateValidator::LessThanOrEqualToDate(date) => format!(
                    "{}.pipe(Schema.lessThanOrEqualTo(new Date(\"{}\"), {{ message: () => `{}` must be on or before `{}` }}))",
                    result, date, field_name_title, date
                ),
                DateValidator::BetweenDate(start, end) => format!(
                    "{}.pipe(Schema.between(new Date(\"{}\"), new Date(\"{}\"), {{ message: () => `{}` must be between `{}` and `{}` }}))",
                    result, start, end, field_name_title, start, end
                ),
            },

            // BigInt validators
            Validator::BigIntValidator(biv) => match biv {
                BigIntValidator::GreaterThanBigInt(value) => format!(
                    "{}.pipe(Schema.greaterThanBigInt({}n, {{ message: () => `{}` must be greater than {}` }}))",
                    result, value, field_name_title, value
                ),
                BigIntValidator::GreaterThanOrEqualToBigInt(value) => format!(
                    "{}.pipe(Schema.greaterThanOrEqualToBigInt({}n, {{ message: () => `{}` must be greater than or equal to {}` }}))",
                    result, value, field_name_title, value
                ),
                BigIntValidator::LessThanBigInt(value) => format!(
                    "{}.pipe(Schema.lessThanBigInt({}n, {{ message: () => `{}` must be less than {}` }}))",
                    result, value, field_name_title, value
                ),
                BigIntValidator::LessThanOrEqualToBigInt(value) => format!(
                    "{}.pipe(Schema.lessThanOrEqualToBigInt({}n, {{ message: () => `{}` must be less than or equal to {}` }}))",
                    result, value, field_name_title, value
                ),
                BigIntValidator::BetweenBigInt(start, end) => format!(
                    "{}.pipe(Schema.betweenBigInt({}n, {}n, {{ message: () => `{}` must be between {} and {}` }}))",
                    result, start, end, field_name_title, start, end
                ),
                BigIntValidator::PositiveBigInt => format!(
                    "{}.pipe(Schema.positiveBigInt({{ message: () => `{}` must be a positive BigInt` }}))",
                    result, field_name_title
                ),
                BigIntValidator::NonNegativeBigInt => format!(
                    "{}.pipe(Schema.nonNegativeBigInt({{ message: () => `{}` must be a non-negative BigInt` }}))",
                    result, field_name_title
                ),
                BigIntValidator::NegativeBigInt => format!(
                    "{}.pipe(Schema.negativeBigInt({{ message: () => `{}` must be a negative BigInt` }}))",
                    result, field_name_title
                ),
                BigIntValidator::NonPositiveBigInt => format!(
                    "{}.pipe(Schema.nonPositiveBigInt({{ message: () => `{}` must be a non-positive BigInt` }}))",
                    result, field_name_title
                ),
            },

            // BigDecimal validators
            Validator::BigDecimalValidator(bdv) => match bdv {
                BigDecimalValidator::GreaterThanBigDecimal(value) => format!(
                    "{}.pipe(Schema.greaterThanBigDecimal(BigDecimal.fromNumber({}), {{ message: () => `{}` must be greater than {}` }}))",
                    result, value, field_name_title, value
                ),
                BigDecimalValidator::GreaterThanOrEqualToBigDecimal(value) => format!(
                    "{}.pipe(Schema.greaterThanOrEqualToBigDecimal(BigDecimal.fromNumber({}), {{ message: () => `{}` must be greater than or equal to {}` }}))",
                    result, value, field_name_title, value
                ),
                BigDecimalValidator::LessThanBigDecimal(value) => format!(
                    "{}.pipe(Schema.lessThanBigDecimal(BigDecimal.fromNumber({}), {{ message: () => `{}` must be less than {}` }}))",
                    result, value, field_name_title, value
                ),
                BigDecimalValidator::LessThanOrEqualToBigDecimal(value) => format!(
                    "{}.pipe(Schema.lessThanOrEqualToBigDecimal(BigDecimal.fromNumber({}), {{ message: () => `{}` must be less than or equal to {}` }}))",
                    result, value, field_name_title, value
                ),
                BigDecimalValidator::BetweenBigDecimal(start, end) => format!(
                    "{}.pipe(Schema.betweenBigDecimal(BigDecimal.fromNumber({}), BigDecimal.fromNumber({}), {{ message: () => `{}` must be between {} and {}` }}))",
                    result, start, end, field_name_title, start, end
                ),
                BigDecimalValidator::PositiveBigDecimal => format!(
                    "{}.pipe(Schema.positiveBigDecimal({{ message: () => `{}` must be a positive BigDecimal` }}))",
                    result, field_name_title
                ),
                BigDecimalValidator::NonNegativeBigDecimal => format!(
                    "{}.pipe(Schema.nonNegativeBigDecimal({{ message: () => `{}` must be a non-negative BigDecimal` }}))",
                    result, field_name_title
                ),
                BigDecimalValidator::NegativeBigDecimal => format!(
                    "{}.pipe(Schema.negativeBigDecimal({{ message: () => `{}` must be a negative BigDecimal` }}))",
                    result, field_name_title
                ),
                BigDecimalValidator::NonPositiveBigDecimal => format!(
                    "{}.pipe(Schema.nonPositiveBigDecimal({{ message: () => `{}` must be a non-positive BigDecimal` }}))",
                    result, field_name_title
                ),
            },

            // Duration validators
            Validator::DurationValidator(dv) => match dv {
                DurationValidator::GreaterThanDuration(value) => format!(
                    "{}.pipe(Schema.greaterThanDuration(\"{}\", {{ message: () => `{}` must be longer than `{}` }}))",
                    result, value, field_name_title, value
                ),
                DurationValidator::GreaterThanOrEqualToDuration(value) => format!(
                    "{}.pipe(Schema.greaterThanOrEqualToDuration(\"{}\", {{ message: () => `{}` must be at least `{}` long` }}))",
                    result, value, field_name_title, value
                ),
                DurationValidator::LessThanDuration(value) => format!(
                    "{}.pipe(Schema.lessThanDuration(\"{}\", {{ message: () => `{}` must be shorter than `{}` }}))",
                    result, value, field_name_title, value
                ),
                DurationValidator::LessThanOrEqualToDuration(value) => format!(
                    "{}.pipe(Schema.lessThanOrEqualToDuration(\"{}\", {{ message: () => `{}` must be at most `{}` long` }}))",
                    result, value, field_name_title, value
                ),
                DurationValidator::BetweenDuration(start, end) => format!(
                    "{}.pipe(Schema.betweenDuration(\"{}\", \"{}\", {{ message: () => `{}` must be between `{}` and `{}` long` }}))",
                    result, start, end, field_name_title, start, end
                ),
            },
        };
    }

    result
}
