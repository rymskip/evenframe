use crate::{
    error::EvenframeError,
    schemasync::TableConfig,
    schemasync::mockmake::Mockmaker,
    schemasync::mockmake::format::Format,
    schemasync::mockmake::validator_gen,
    schemasync::table::surql_ident,
    types::{EnumRepresentation, FieldType, ForeignTypeRegistry, StructField, VariantData},
    validator::{MockValue, Validator},
};
use bon::Builder;
#[cfg(feature = "mockmake")]
use chrono_tz::TZ_VARIANTS;
use convert_case::{Case, Casing};
use rand::{RngExt, rngs::ThreadRng, seq::IndexedRandom};
use std::fmt;
use std::rc::Rc;
use tracing;

/// One value to generate, and where it sits in the field.
#[derive(Clone)]
struct Frame<'a> {
    field: &'a StructField,
    table_config: &'a TableConfig,
    field_type: &'a FieldType,
    /// The dotted path of the value, shared by the frames that keep it.
    field_path: Rc<str>,
    /// The object and enum types around this value, to stop at a type that
    /// holds itself.
    visited_types: Option<Rc<Visited<'a>>>,
}

/// A type the value is nested in, linked to the types around it, so a child
/// frame extends its parent's chain instead of copying it.
struct Visited<'a> {
    type_name: &'a str,
    outer: Option<Rc<Visited<'a>>>,
}

impl<'a> Frame<'a> {
    fn visits(&self, type_name: &str) -> bool {
        let mut current = self.visited_types.as_deref();
        while let Some(visited) = current {
            if visited.type_name == type_name {
                return true;
            }
            current = visited.outer.as_deref();
        }
        false
    }

    /// The visited chain of a child nested in `type_name`.
    fn inside(&self, type_name: &'a str) -> Option<Rc<Visited<'a>>> {
        Some(Rc::new(Visited {
            type_name,
            outer: self.visited_types.clone(),
        }))
    }
}

/// Where a generated value goes, written out only when an error names it.
#[derive(Clone, Copy)]
struct FieldLocation<'l> {
    table_name: &'l str,
    field_path: &'l str,
}

impl fmt::Display for FieldLocation<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}", self.table_name, self.field_path)
    }
}

enum WorkItem<'a> {
    Generate(Frame<'a>),
    AssembleVec {
        count: usize,
    },
    AssembleTuple {
        count: usize,
    },
    AssembleStruct {
        field_names: Vec<String>,
    },
    AssembleMap {
        count: usize,
    },
    AssembleEnum,
    WrapInVariantKey {
        variant_name: String,
    },
    AssembleTaggedStruct {
        tag_key: String,
        tag_value: String,
        field_names: Vec<String>,
    },
}

#[derive(Debug, Builder)]
pub struct FieldValueGenerator<'a> {
    mockmaker: &'a Mockmaker<'a>,
    table_config: &'a TableConfig,
    field: &'a StructField,
    id_index: &'a usize,
    registry: &'a ForeignTypeRegistry,
}

impl<'a> FieldValueGenerator<'a> {
    /// A SurrealQL literal for the field, built iteratively: recursion
    /// overflowed the stack on deep types.
    pub fn run(&self) -> Result<String, EvenframeError> {
        let mut work_stack: Vec<WorkItem<'a>> = Vec::new();
        let mut value_stack: Vec<String> = Vec::new();
        let mut rng = rand::rng();

        let initial_context = Frame {
            field: self.field,
            table_config: self.table_config,
            field_type: &self.field.field_type,
            field_path: Rc::from(self.field.field_name.as_str()),
            visited_types: None,
        };
        work_stack.push(WorkItem::Generate(initial_context));

        while let Some(work_item) = work_stack.pop() {
            match work_item {
                WorkItem::Generate(ctx) => {
                    // Tier 0: the table's mock-data plugin, when it gives a value.
                    #[cfg(feature = "wasm-plugins")]
                    if let Some(plugin_name) = self
                        .table_config
                        .mock_generation_config
                        .as_ref()
                        .and_then(|config| config.plugin.as_ref())
                    {
                        let pm_cell = self.mockmaker.plugin_manager.as_ref().ok_or_else(|| {
                            EvenframeError::mock_generation(format!(
                                "`{}` uses mock-data plugin `{plugin_name}`, but no plugins were loaded",
                                self.table_config.table_name
                            ))
                        })?;
                        let record_id = self
                            .mockmaker
                            .id_map
                            .get(&self.table_config.table_name)
                            .and_then(|ids| ids.get(*self.id_index))
                            .cloned()
                            .ok_or_else(|| {
                                EvenframeError::mock_generation(format!(
                                    "no record id at index {} of `{}`",
                                    self.id_index, self.table_config.table_name
                                ))
                            })?;
                        let input = super::plugin_types::PluginFieldInput {
                            table_name: self.table_config.table_name.to_string(),
                            field_name: ctx.field_path.to_string(),
                            field_type: format!("{:?}", ctx.field_type),
                            record_index: *self.id_index,
                            total_records: self.mockmaker.record_count(self.table_config),
                            record_id,
                        };
                        let generated = pm_cell
                            .borrow_mut()
                            .generate_field_value(plugin_name, &input)
                            .map_err(|error| {
                                EvenframeError::mock_generation(format!(
                                    "`{}`: {error}",
                                    ctx.field_path
                                ))
                            })?;
                        if let Some(value) = generated {
                            value_stack.push(value);
                            continue;
                        }
                    }

                    let location = FieldLocation {
                        table_name: &self.table_config.table_name,
                        field_path: &ctx.field_path,
                    };
                    if let Some(coordinated_value) = self.mockmaker.coordinated_values.get(
                        &self.table_config.table_name,
                        &ctx.field_path,
                        *self.id_index,
                    ) {
                        value_stack.push(coordinated_value.to_string());
                    } else if let Some(format) = &ctx.field.format {
                        value_stack.push(self.handle_format(
                            format,
                            ctx.field_type,
                            &ctx.field.validators,
                            location,
                        )?);
                    } else if let Some(value) = validator_gen::generate_with_validators(
                        ctx.field_type,
                        &ctx.field.validators,
                        &mut rng,
                    ) {
                        value_stack.push(value);
                    } else {
                        match ctx.field_type {
                            FieldType::String => value_stack
                                .push(generate_string_with_retry(&ctx.field.validators, location)?),
                            FieldType::Char => value_stack
                                .push(format!("'{}'", rng.random_range(32u8..=126u8) as char)),
                            FieldType::Bool => {
                                value_stack.push(format!("{}", rng.random_bool(0.5)))
                            }
                            FieldType::Unit => value_stack.push("NONE".to_string()),
                            FieldType::Duration => {
                                // The generator draws inside every duration
                                // bound, so bounds it could not meet are disjoint.
                                if ctx.field.validators.iter().any(|validator| {
                                    matches!(validator, Validator::DurationValidator(_))
                                }) {
                                    return Err(disjoint_durations(
                                        location,
                                        &ctx.field.validators,
                                    ));
                                }
                                value_stack.push(validator_gen::duration_literal(
                                    rng.random_range(0..validator_gen::DAY_NANOS),
                                ));
                            }
                            FieldType::F32 | FieldType::F64 => {
                                value_stack.push(generate_float_with_retry(
                                    &ctx.field.validators,
                                    location,
                                    &mut rng,
                                )?)
                            }
                            FieldType::I8
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
                            | FieldType::Usize => value_stack.push(generate_integer_with_retry(
                                ctx.field_type,
                                &ctx.field.validators,
                                location,
                                &mut rng,
                            )?),
                            FieldType::Option(inner_type) => {
                                // An optional value holding a link with nothing to
                                // point at stays null.
                                if rng.random_bool(0.5)
                                    || self.mockmaker.has_unfillable_link(inner_type)
                                {
                                    value_stack.push("null".to_string());
                                } else {
                                    work_stack.push(WorkItem::Generate(Frame {
                                        field_type: inner_type,
                                        ..ctx.clone()
                                    }));
                                }
                            }
                            FieldType::Vec(inner_type) => {
                                let (lo, hi) =
                                    validator_gen::array_count_range(&ctx.field.validators, 2, 9);
                                // A list of values holding a link with nothing to
                                // point at stays empty.
                                let count = if self.mockmaker.has_unfillable_link(inner_type) {
                                    0
                                } else if lo == hi {
                                    lo
                                } else {
                                    rng.random_range(lo..=hi)
                                };
                                work_stack.push(WorkItem::AssembleVec { count });
                                for _ in 0..count {
                                    work_stack.push(WorkItem::Generate(Frame {
                                        field_type: inner_type,
                                        ..ctx.clone()
                                    }));
                                }
                            }
                            FieldType::Tuple(types) => {
                                work_stack.push(WorkItem::AssembleTuple { count: types.len() });
                                for inner_type in types.iter().rev() {
                                    work_stack.push(WorkItem::Generate(Frame {
                                        field_type: inner_type,
                                        ..ctx.clone()
                                    }));
                                }
                            }
                            FieldType::Struct(fields) => {
                                let field_names: Vec<String> =
                                    fields.iter().map(|(name, _)| name.clone()).collect();
                                work_stack.push(WorkItem::AssembleStruct { field_names });

                                for (nested_field_name, ftype) in fields.iter().rev() {
                                    work_stack.push(WorkItem::Generate(Frame {
                                        field_type: ftype,
                                        field_path: Rc::from(format!(
                                            "{}.{nested_field_name}",
                                            ctx.field_path
                                        )),
                                        ..ctx.clone()
                                    }));
                                }
                            }
                            FieldType::HashMap(key_ft, value_ft)
                            | FieldType::BTreeMap(key_ft, value_ft) => {
                                let count = rng.random_range(0..3);
                                work_stack.push(WorkItem::AssembleMap { count });
                                for _ in 0..count {
                                    work_stack.push(WorkItem::Generate(Frame {
                                        field_type: value_ft,
                                        ..ctx.clone()
                                    }));
                                    work_stack.push(WorkItem::Generate(Frame {
                                        field_type: key_ft,
                                        ..ctx.clone()
                                    }));
                                }
                            }
                            FieldType::RecordLink(inner_type) => {
                                // For relation tables, in/out fields must use relation.from/to
                                // to stay in sync with the DEFINE TABLE ... FROM ... TO ... clause.
                                if ctx.table_config.relation.is_some()
                                    && matches!(ctx.field.db_name(), "in" | "out")
                                {
                                    value_stack.push(self.handle_record_id(
                                        ctx.field,
                                        &ctx.table_config.table_name,
                                        ctx.table_config,
                                        &mut rng,
                                    )?);
                                    continue;
                                }

                                // RecordLink should ultimately reference a persistable table.
                                // If the inner type is an enum (persistable struct union), choose a variant that maps to a table.
                                match inner_type.as_ref() {
                                    FieldType::Other(type_name) => {
                                        let targets = self.mockmaker.link_target_tables(type_name);
                                        if targets.is_empty() {
                                            return Err(EvenframeError::mock_generation(format!(
                                                "`{}` links to `{type_name}`, which is not a table or a union of tables",
                                                ctx.field_path
                                            )));
                                        }
                                        value_stack.push(self.random_link(
                                            &targets,
                                            &ctx.field_path,
                                            &mut rng,
                                        )?);
                                    }
                                    _ => {
                                        return Err(EvenframeError::mock_generation(format!(
                                            "`{}` links to {inner_type:?}; a link names a table or a union of tables",
                                            ctx.field_path
                                        )));
                                    }
                                }
                            }
                            FieldType::Other(type_name) => {
                                // Check if this is a foreign type with a mock strategy
                                if let Some(ftc) = self.registry.lookup(type_name) {
                                    let strategy = ftc.mock_strategy.as_str();
                                    match strategy {
                                        // SurrealQL-specific formats that need special encoding
                                        "datetime" => {
                                            value_stack.push(format!(
                                                "d'{}'",
                                                chrono::Utc::now().to_rfc3339()
                                            ));
                                            continue;
                                        }
                                        "duration" => {
                                            value_stack.push(format!(
                                                "duration::from_nanos({})",
                                                rng.random_range(0..86_400_000_000_000i64)
                                            ));
                                            continue;
                                        }
                                        "timezone" => {
                                            #[cfg(feature = "mockmake")]
                                            {
                                                let tz = &TZ_VARIANTS
                                                    [rng.random_range(0..TZ_VARIANTS.len())];
                                                value_stack.push(format!("'{}'", tz.name()));
                                            }
                                            #[cfg(not(feature = "mockmake"))]
                                            {
                                                let timezones = [
                                                    "UTC",
                                                    "America/New_York",
                                                    "Europe/London",
                                                    "Asia/Tokyo",
                                                ];
                                                value_stack.push(format!(
                                                    "'{}'",
                                                    timezones[rng.random_range(0..timezones.len())]
                                                ));
                                            }
                                            continue;
                                        }
                                        "decimal" => {
                                            value_stack.push(format!(
                                                "{:.3}dec",
                                                rng.random_range(0.0..100.0)
                                            ));
                                            continue;
                                        }
                                        "float" => {
                                            value_stack.push(format!(
                                                "{:.2}f",
                                                rng.random_range(0.0..100.0)
                                            ));
                                            continue;
                                        }
                                        "record_id" => {
                                            value_stack.push(self.handle_record_id(
                                                ctx.field,
                                                &ctx.table_config.table_name,
                                                ctx.table_config,
                                                &mut rng,
                                            )?);
                                            continue;
                                        }
                                        "object" => {
                                            value_stack.push("{}".to_string());
                                            continue;
                                        }
                                        _ => {
                                            if let Ok(fmt) = strategy.parse::<Format>() {
                                                let val = fmt.generate_formatted_value()?;
                                                value_stack.push(format!("'{}'", val));
                                                continue;
                                            }
                                            // Fall through to existing Other logic
                                        }
                                    }
                                }

                                // Check if we've already visited this type to avoid infinite recursion
                                if ctx.visits(type_name) {
                                    tracing::debug!(
                                        type_name = %type_name,
                                        field_path = %ctx.field_path,
                                        "Detected circular reference, generating null"
                                    );
                                    value_stack.push("null".to_string());
                                    continue;
                                }

                                let snake_case_name = type_name.to_case(Case::Snake);
                                // A table held by value is stored as a link to it, as the
                                // schema defines it; an enum held by value is written inline.
                                let table_targets = if self.mockmaker.enums.contains_key(type_name)
                                {
                                    std::rc::Rc::from([])
                                } else {
                                    self.mockmaker.link_target_tables(type_name)
                                };
                                if !table_targets.is_empty() {
                                    value_stack.push(self.random_link(
                                        &table_targets,
                                        &ctx.field_path,
                                        &mut rng,
                                    )?);
                                } else if let Some(struct_config) = self
                                    .mockmaker
                                    .objects
                                    .get(type_name)
                                    .or_else(|| self.mockmaker.objects.get(&snake_case_name))
                                {
                                    let struct_config = struct_config.effective();
                                    let field_names: Vec<String> = struct_config
                                        .fields
                                        .iter()
                                        .map(|field| field.effective().db_name().to_owned())
                                        .collect();
                                    work_stack.push(WorkItem::AssembleStruct { field_names });

                                    let visited_types = ctx.inside(type_name);
                                    for struct_field in struct_config.fields.iter().rev() {
                                        work_stack.push(WorkItem::Generate(Frame {
                                            field: struct_field,
                                            field_type: &struct_field.field_type,
                                            field_path: Rc::from(format!(
                                                "{}.{}",
                                                ctx.field_path, struct_field.field_name
                                            )),
                                            table_config: ctx.table_config,
                                            visited_types: visited_types.clone(),
                                        }));
                                    }
                                } else if let Some(tagged_union) =
                                    self.mockmaker.enums.get(type_name)
                                {
                                    let variant =
                                        tagged_union.variants.choose(&mut rng).ok_or_else(|| {
                                            EvenframeError::mock_generation(format!(
                                                "`{}` is the enum `{type_name}`, which has no variants",
                                                ctx.field_path
                                            ))
                                        })?;
                                    let repr = &tagged_union.representation;
                                    if let Some(ref variant_data) = variant.data {
                                        match variant_data {
                                            VariantData::InlineStruct(enum_struct) => {
                                                let struct_config = enum_struct.effective();
                                                let field_names: Vec<String> = struct_config
                                                    .fields
                                                    .iter()
                                                    .map(|field| {
                                                        field.effective().db_name().to_owned()
                                                    })
                                                    .collect();

                                                match repr {
                                                    EnumRepresentation::ExternallyTagged => {
                                                        work_stack.push(
                                                            WorkItem::WrapInVariantKey {
                                                                variant_name: variant
                                                                    .db_name()
                                                                    .to_owned(),
                                                            },
                                                        );
                                                        work_stack.push(WorkItem::AssembleStruct {
                                                            field_names,
                                                        });
                                                    }
                                                    EnumRepresentation::InternallyTagged {
                                                        tag,
                                                    } => {
                                                        let mut names_with_tag = vec![tag.clone()];
                                                        names_with_tag.extend(field_names);
                                                        work_stack.push(
                                                            WorkItem::AssembleTaggedStruct {
                                                                tag_key: tag.clone(),
                                                                tag_value: variant
                                                                    .db_name()
                                                                    .to_owned(),
                                                                field_names: names_with_tag,
                                                            },
                                                        );
                                                    }
                                                    EnumRepresentation::AdjacentlyTagged {
                                                        tag,
                                                        content,
                                                    } => {
                                                        work_stack.push(WorkItem::AssembleStruct {
                                                            field_names: vec![
                                                                tag.clone(),
                                                                content.clone(),
                                                            ],
                                                        });
                                                        work_stack.push(WorkItem::AssembleStruct {
                                                            field_names,
                                                        });
                                                        // tag value will be pushed after struct fields
                                                    }
                                                    EnumRepresentation::Untagged => {
                                                        work_stack.push(WorkItem::AssembleStruct {
                                                            field_names,
                                                        });
                                                    }
                                                }

                                                let visited_types = ctx.inside(type_name);
                                                for struct_field in
                                                    struct_config.fields.iter().rev()
                                                {
                                                    work_stack.push(WorkItem::Generate(Frame {
                                                        field: struct_field,
                                                        field_type: &struct_field.field_type,
                                                        field_path: Rc::from(format!(
                                                            "{}.{}",
                                                            ctx.field_path, struct_field.field_name
                                                        )),
                                                        table_config: ctx.table_config,
                                                        visited_types: visited_types.clone(),
                                                    }));
                                                }

                                                // For adjacently tagged, push tag value after struct fields (processed first due to LIFO)
                                                if let EnumRepresentation::AdjacentlyTagged {
                                                    ..
                                                } = repr
                                                {
                                                    value_stack
                                                        .push(format!("'{}'", variant.db_name()));
                                                }
                                            }
                                            VariantData::DataStructureRef(field_type) => {
                                                match repr {
                                                    EnumRepresentation::ExternallyTagged
                                                    | EnumRepresentation::InternallyTagged {
                                                        ..
                                                    } => {
                                                        work_stack.push(
                                                            WorkItem::WrapInVariantKey {
                                                                variant_name: variant
                                                                    .db_name()
                                                                    .to_owned(),
                                                            },
                                                        );
                                                    }
                                                    EnumRepresentation::AdjacentlyTagged {
                                                        tag,
                                                        content,
                                                    } => {
                                                        work_stack.push(WorkItem::AssembleStruct {
                                                            field_names: vec![
                                                                tag.clone(),
                                                                content.clone(),
                                                            ],
                                                        });
                                                        // Push the tag value directly; inner value comes from Generate
                                                        value_stack.push(format!(
                                                            "'{}'",
                                                            variant.db_name()
                                                        ));
                                                    }
                                                    EnumRepresentation::Untagged => {
                                                        work_stack.push(WorkItem::AssembleEnum);
                                                    }
                                                }
                                                work_stack.push(WorkItem::Generate(Frame {
                                                    field_type,
                                                    ..ctx.clone()
                                                }));
                                            }
                                        }
                                    } else {
                                        // Unit variant
                                        match repr {
                                            EnumRepresentation::InternallyTagged { tag }
                                            | EnumRepresentation::AdjacentlyTagged {
                                                tag, ..
                                            } => {
                                                value_stack.push(format!(
                                                    "{{ {}: '{}' }}",
                                                    surql_ident(tag),
                                                    variant.db_name()
                                                ));
                                            }
                                            _ => {
                                                value_stack
                                                    .push(format!("'{}'", variant.db_name()));
                                            }
                                        }
                                    }
                                } else {
                                    return Err(EvenframeError::mock_generation(format!(
                                        "`{}.{}` has the type `{type_name}`, which is not a table, object, enum or foreign type",
                                        ctx.table_config.table_name, ctx.field_path
                                    )));
                                }
                            }
                        }
                    }
                }
                WorkItem::AssembleVec { count } | WorkItem::AssembleTuple { count } => {
                    let items = take_last(&mut value_stack, count)?;
                    value_stack.push(format!("[{}]", items.join(", ")));
                }
                WorkItem::AssembleStruct { field_names } => {
                    let values = take_last(&mut value_stack, field_names.len())?;
                    let assignments: Vec<String> = field_names
                        .into_iter()
                        .zip(values)
                        .map(|(name, value)| format!("{}: {}", surql_ident(&name), value))
                        .collect();
                    value_stack.push(format!("{{ {} }}", assignments.join(", ")));
                }
                WorkItem::AssembleMap { count } => {
                    let entries: Vec<String> = take_last(&mut value_stack, count * 2)?
                        .chunks(2)
                        .map(|pair| pair.join(": "))
                        .collect();
                    value_stack.push(format!("{{ {} }}", entries.join(", ")));
                }
                WorkItem::AssembleEnum { .. } => {
                    // No action needed; the generated value just stays on the stack.
                }
                WorkItem::WrapInVariantKey { variant_name } => {
                    let inner = take_last(&mut value_stack, 1)?.join("");
                    value_stack.push(format!("{{ {}: {} }}", surql_ident(&variant_name), inner));
                }
                WorkItem::AssembleTaggedStruct {
                    tag_key,
                    tag_value,
                    field_names,
                } => {
                    // Like AssembleStruct but the first field is the tag with a known value
                    let data_values =
                        take_last(&mut value_stack, field_names.len().saturating_sub(1))?;
                    let mut assignments: Vec<String> =
                        vec![format!("{}: '{}'", surql_ident(&tag_key), tag_value)];
                    for (name, value) in field_names.into_iter().skip(1).zip(data_values) {
                        assignments.push(format!("{}: {}", surql_ident(&name), value));
                    }
                    value_stack.push(format!("{{ {} }}", assignments.join(", ")));
                }
            }
        }

        match value_stack.as_slice() {
            [value] => Ok(value.clone()),
            values => Err(EvenframeError::mock_generation(format!(
                "generating `{}` left {} values instead of one",
                self.field.field_name,
                values.len()
            ))),
        }
    }

    fn handle_format(
        &self,
        format: &Format,
        target: &FieldType,
        validators: &[Validator],
        location: FieldLocation<'_>,
    ) -> Result<String, EvenframeError> {
        let mut scalar = target;
        while let FieldType::Option(inner) = scalar {
            scalar = inner;
        }

        // Currency and percentage formats describe string presentation
        // ("$12.34", "42.5%"). When the declared field type is numeric the
        // schema stores a bare number (with ASSERTs built from the
        // validators), so the type wins: generate through the validator
        // path, falling back to a bounded bare number.
        if matches!(format, Format::CurrencyAmount | Format::Percentage) && scalar.is_numeric() {
            let mut rng = rand::rng();
            if !validators.is_empty() {
                return generate_scalar_with_retry(scalar, validators, location, &mut rng);
            }
            return Ok(match format {
                Format::CurrencyAmount => format!("{:.2}", rng.random_range(0.0..1000.0)),
                _ => format!("{:.1}", rng.random_range(0.0..100.0)),
            });
        }

        let generated = format.generate_formatted_value()?;

        // A format hint can contradict the field's validators, and the
        // validators are what the database enforces (they become ASSERT
        // clauses). Check the value in the domain the database will see
        // (numeric fields as numbers, everything else as strings), and
        // regenerate through the validator path on a mismatch.
        if !validators.is_empty() {
            let satisfied = if scalar.is_numeric() {
                generated.parse::<f64>().is_ok_and(|number| {
                    validators
                        .iter()
                        .all(|validator| validator.matches(&MockValue::Num(number)))
                })
            } else {
                validators
                    .iter()
                    .all(|validator| validator.matches(&MockValue::Str(&generated)))
            };
            if !satisfied {
                return generate_scalar_with_retry(scalar, validators, location, &mut rand::rng());
            }
        }

        Ok(match format {
            Format::CurrencyAmount | Format::Percentage => format!("'{}'", generated),
            Format::Latitude | Format::Longitude | Format::AppointmentDurationNs => generated,
            Format::DateTime | Format::AppointmentDateTime | Format::DateWithinDays(_) => {
                // A Rust `String` field maps to surql TYPE string, where a
                // d'…' datetime literal fails coercion. Only datetime-typed
                // fields (foreign types like chrono) take the literal form.
                if matches!(scalar, FieldType::String) {
                    format!("'{}'", generated)
                } else {
                    format!("d'{}'", generated)
                }
            }
            _ => format!("'{}'", generated),
        })
    }

    /// A random record from `targets`, the tables a link at `field_path` can
    /// point at.
    fn random_link(
        &self,
        targets: &[String],
        field_path: &str,
        rng: &mut ThreadRng,
    ) -> Result<String, EvenframeError> {
        let pools: Vec<&[String]> = targets
            .iter()
            .filter_map(|table| self.mockmaker.id_map.get(table))
            .map(Vec::as_slice)
            .collect();
        let id = draw_from_pools(&pools, rng).ok_or_else(|| {
            EvenframeError::mock_generation(format!(
                "`{field_path}` must link to {}, which has no records",
                targets.join(" or ")
            ))
        })?;
        Ok(format!("r'{id}'"))
    }

    /// A link for `field`. A relation's `in` and `out` are SurrealDB's keys,
    /// so its database name decides them; coordination rules name the Rust
    /// field.
    fn handle_record_id(
        &self,
        field: &StructField,
        table_name: &str,
        table_config: &TableConfig,
        rng: &mut ThreadRng,
    ) -> Result<String, EvenframeError> {
        let field_name = field.field_name.as_str();
        if let Some(relation) = &table_config.relation
            && matches!(field.db_name(), "in" | "out")
        {
            // A OneToOne coordination maps record `i` to target record `i`.
            let one_to_one = table_config
                .mock_generation_config
                .as_ref()
                .is_some_and(|config| {
                    config.coordination_rules.iter().any(|rule| {
                        matches!(rule, crate::schemasync::mockmake::coordinate::Coordination::OneToOne(coordinated) if coordinated == field_name)
                    })
                });
            let tables = if field.db_name() == "in" {
                &relation.from
            } else {
                &relation.to
            };
            let ids = tables
                .iter()
                .find_map(|table| self.mockmaker.id_map.get(table))
                .filter(|ids| !ids.is_empty())
                .ok_or_else(|| {
                    EvenframeError::mock_generation(format!(
                        "`{table_name}.{field_name}` must link to {}, which has no records",
                        tables.join(" or ")
                    ))
                })?;
            let id = if one_to_one {
                &ids[*self.id_index % ids.len()]
            } else {
                &ids[rng.random_range(0..ids.len())]
            };
            return Ok(format!("r'{id}'"));
        }

        self.mockmaker
            .id_map
            .get(table_name)
            .and_then(|ids| ids.get(*self.id_index))
            .map(|id| format!("r'{id}'"))
            .ok_or_else(|| {
                EvenframeError::mock_generation(format!(
                    "`{table_name}` has no record id at index {} for `{field_name}`",
                    self.id_index
                ))
            })
    }
}

/// The last `count` values on `stack`, in order.
fn take_last(stack: &mut Vec<String>, count: usize) -> Result<Vec<String>, EvenframeError> {
    let start = stack.len().checked_sub(count).ok_or_else(|| {
        EvenframeError::mock_generation(format!(
            "assembling a value needed {count} parts but {} were generated",
            stack.len()
        ))
    })?;
    Ok(stack.split_off(start))
}

/// Cap on retry attempts when a generator produces a value that fails the
/// field's validator set.
const RETRY_ATTEMPTS: usize = 32;

fn unsatisfied(location: FieldLocation<'_>, validators: &[Validator]) -> EvenframeError {
    EvenframeError::mock_generation(format!(
        "no value generated for `{location}` in {RETRY_ATTEMPTS} attempts is all of: {}. \
         Check that these validators can hold at once",
        describe_all(validators)
    ))
}

fn disjoint_durations(location: FieldLocation<'_>, validators: &[Validator]) -> EvenframeError {
    EvenframeError::mock_generation(format!(
        "no duration for `{location}` is all of: {}. These bounds do not overlap",
        describe_all(validators)
    ))
}

fn describe_all(validators: &[Validator]) -> String {
    validators
        .iter()
        .map(Validator::describe)
        .collect::<Vec<_>>()
        .join(", ")
}

/// A value of `scalar` satisfying `validators`, generated from the validators
/// themselves where they can be solved and by rejection sampling otherwise.
fn generate_scalar_with_retry(
    scalar: &FieldType,
    validators: &[Validator],
    location: FieldLocation<'_>,
    rng: &mut ThreadRng,
) -> Result<String, EvenframeError> {
    match scalar {
        FieldType::String => validator_gen::generate_with_validators(scalar, validators, rng)
            .map_or_else(|| generate_string_with_retry(validators, location), Ok),
        FieldType::F32 | FieldType::F64 => generate_float_with_retry(validators, location, rng),
        FieldType::I8
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
        | FieldType::Usize => generate_integer_with_retry(scalar, validators, location, rng),
        _ => Err(unsatisfied(location, validators)),
    }
}

/// One id drawn uniformly from all of `pools` together, without gathering
/// them into one list, or `None` when they are all empty.
fn draw_from_pools<'p>(pools: &[&'p [String]], rng: &mut impl RngExt) -> Option<&'p String> {
    let total: usize = pools.iter().map(|pool| pool.len()).sum();
    if total == 0 {
        return None;
    }
    let mut drawn = rng.random_range(0..total);
    for pool in pools {
        match pool.get(drawn) {
            Some(id) => return Some(id),
            None => drawn -= pool.len(),
        }
    }
    None
}

fn generate_string_with_retry(
    validators: &[Validator],
    location: FieldLocation<'_>,
) -> Result<String, EvenframeError> {
    (0..RETRY_ATTEMPTS)
        .map(|_| Mockmaker::random_string(8))
        .find(|candidate| {
            validators
                .iter()
                .all(|validator| validator.matches(&MockValue::Str(candidate)))
        })
        .map(|candidate| format!("'{candidate}'"))
        .ok_or_else(|| unsatisfied(location, validators))
}

fn generate_float_with_retry(
    validators: &[Validator],
    location: FieldLocation<'_>,
    rng: &mut ThreadRng,
) -> Result<String, EvenframeError> {
    if validators.is_empty() {
        return Ok(format!("{:.2}f", rng.random_range(0.0..100.0)));
    }
    // The validator-driven generator derives its sample range from the
    // validators themselves, so constraints a fixed 0..100 loop can never
    // hit (Negative, GreaterThan(1000), …) still converge.
    for _ in 0..RETRY_ATTEMPTS {
        if let Some(value) =
            validator_gen::generate_with_validators(&FieldType::F64, validators, rng)
        {
            return Ok(value);
        }
    }
    (0..RETRY_ATTEMPTS)
        .map(|_| rng.random_range(0.0..100.0))
        .find(|candidate| {
            validators
                .iter()
                .all(|validator| validator.matches(&MockValue::Num(*candidate)))
        })
        .map(|candidate| format!("{candidate:.2}f"))
        .ok_or_else(|| unsatisfied(location, validators))
}

fn generate_integer_with_retry(
    field_type: &FieldType,
    validators: &[Validator],
    location: FieldLocation<'_>,
    rng: &mut ThreadRng,
) -> Result<String, EvenframeError> {
    if validators.is_empty() {
        return Ok(format!("{}", rng.random_range(0..100)));
    }
    // The validator-driven generator derives its sample range from the
    // validators themselves, so constraints a fixed 0..100 loop can never
    // hit (Negative, GreaterThan(1000), …) still converge.
    for _ in 0..RETRY_ATTEMPTS {
        if let Some(value) = validator_gen::generate_with_validators(field_type, validators, rng) {
            return Ok(value);
        }
    }
    (0..RETRY_ATTEMPTS)
        .map(|_| rng.random_range(0i64..100))
        .find(|candidate| {
            validators
                .iter()
                .all(|validator| validator.matches(&MockValue::Num(*candidate as f64)))
        })
        .map(|candidate| candidate.to_string())
        .ok_or_else(|| unsatisfied(location, validators))
}

#[cfg(test)]
mod draw_tests {
    use super::draw_from_pools;
    use std::collections::BTreeSet;

    #[test]
    fn a_link_reaches_every_target_table_and_nothing_else() {
        let users = vec!["user:1".to_string(), "user:2".to_string()];
        let teams = vec!["team:1".to_string()];
        let pools = [users.as_slice(), &[][..], teams.as_slice()];
        let mut rng = rand::rng();
        let drawn: BTreeSet<&String> = (0..500)
            .filter_map(|_| draw_from_pools(&pools, &mut rng))
            .collect();
        let expected: BTreeSet<&String> = users.iter().chain(&teams).collect();
        assert_eq!(drawn, expected);
    }

    #[test]
    fn empty_pools_have_nothing_to_draw() {
        let mut rng = rand::rng();
        assert!(draw_from_pools(&[&[][..], &[][..]], &mut rng).is_none());
    }
}
