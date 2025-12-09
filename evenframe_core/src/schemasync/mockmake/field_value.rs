use crate::{
    coordinate::CoordinationId,
    format::Format,
    mockmake::Mockmaker,
    schemasync::TableConfig,
    types::{FieldType, StructField, VariantData},
};
use bon::Builder;
use chrono_tz::TZ_VARIANTS;
use convert_case::{Case, Casing};
use rand::{Rng, rngs::ThreadRng, seq::IndexedRandom};
use std::collections::HashSet;
use tracing;

// The context struct is now simple again, with a direct reference.
#[derive(Clone)]
struct Frame<'a> {
    field: &'a StructField,
    table_config: &'a TableConfig,
    field_type: &'a FieldType,
    field_path: String,             // Track the full path for nested fields
    visited_types: HashSet<String>, // Track visited types to avoid infinite recursion
}

enum WorkItem<'a> {
    Generate(Frame<'a>),
    AssembleVec { count: usize },
    AssembleTuple { count: usize },
    AssembleStruct { field_names: Vec<String> },
    AssembleMap { count: usize },
    AssembleEnum,
}

#[derive(Debug, Builder)]
pub struct FieldValueGenerator<'a> {
    mockmaker: &'a Mockmaker<'a>,
    table_config: &'a TableConfig,
    field: &'a StructField,
    id_index: &'a usize,
}

impl<'a> FieldValueGenerator<'a> {
    // Was having stack overflow so created an iterative version
    pub fn run(&self) -> String {
        let mut work_stack: Vec<WorkItem<'a>> = Vec::new();
        let mut value_stack: Vec<String> = Vec::new();
        let mut rng = rand::rng();

        let initial_context = Frame {
            field: self.field,
            table_config: self.table_config,
            field_type: &self.field.field_type,
            field_path: self.field.field_name.clone(),
            visited_types: HashSet::new(),
        };
        work_stack.push(WorkItem::Generate(initial_context));

        while let Some(work_item) = work_stack.pop() {
            match work_item {
                WorkItem::Generate(ctx) => {
                    if let Some(coordinated_value) = self.mockmaker.coordinated_values.get(
                        &CoordinationId::builder()
                            .field_name(ctx.field_path.clone())
                            .table_name(self.table_config.table_name.to_string())
                            .build(),
                    ) {
                        value_stack.push(coordinated_value.to_string());
                    } else if let Some(format) = &ctx.field.format {
                        value_stack.push(self.handle_format(format));
                    } else {
                        match ctx.field_type {
                            FieldType::String => {
                                value_stack.push(format!("'{}'", Mockmaker::random_string(8)))
                            }
                            FieldType::Char => value_stack
                                .push(format!("'{}'", rng.random_range(32u8..=126u8) as char)),
                            FieldType::Bool => {
                                value_stack.push(format!("{}", rng.random_bool(0.5)))
                            }
                            FieldType::Unit => value_stack.push("NONE".to_string()),
                            FieldType::Decimal => {
                                value_stack.push(format!("{:.3}dec", rng.random_range(0.0..100.0)))
                            }
                            FieldType::F32 | FieldType::F64 | FieldType::OrderedFloat(_) => {
                                value_stack.push(format!("{:.2}f", rng.random_range(0.0..100.0)))
                            }
                            FieldType::I8
                            | FieldType::I16
                            | FieldType::I32
                            | FieldType::I64
                            | FieldType::I128
                            | FieldType::Isize => {
                                value_stack.push(format!("{}", rng.random_range(0..100)))
                            }
                            FieldType::U8
                            | FieldType::U16
                            | FieldType::U32
                            | FieldType::U64
                            | FieldType::U128
                            | FieldType::Usize => {
                                value_stack.push(format!("{}", rng.random_range(0..100)))
                            }
                            FieldType::DateTime => {
                                value_stack.push(format!("d'{}'", chrono::Utc::now().to_rfc3339()))
                            }
                            FieldType::EvenframeDuration => value_stack.push(format!(
                                "duration::from::nanos({})",
                                rng.random_range(0..86_400_000_000_000i64)
                            )),
                            FieldType::Timezone => {
                                let tz = &TZ_VARIANTS[rng.random_range(0..TZ_VARIANTS.len())];
                                value_stack.push(format!("'{}'", tz.name()));
                            }
                            FieldType::EvenframeRecordId => {
                                value_stack.push(self.handle_record_id(
                                    &ctx.field.field_name,
                                    &ctx.table_config.table_name,
                                    ctx.table_config,
                                    &mut rng,
                                ))
                            }
                            FieldType::Option(inner_type) => {
                                if rng.random_bool(0.5) {
                                    value_stack.push("null".to_string());
                                } else {
                                    work_stack.push(WorkItem::Generate(Frame {
                                        field_type: inner_type,
                                        ..ctx.clone()
                                    }));
                                }
                            }
                            FieldType::Vec(inner_type) => {
                                let count = rng.random_range(2..10);
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
                                        field_path: format!(
                                            "{}.{}",
                                            ctx.field_path.clone(),
                                            nested_field_name
                                        ),
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
                                // RecordLink should ultimately reference a persistable table.
                                // If the inner type is an enum (persistable struct union), choose a variant that maps to a table.
                                match inner_type.as_ref() {
                                    FieldType::Other(type_name) => {
                                        // Helper: resolve a type name to a table key in self.mockmaker.tables
                                        // Closure uses RNG; mark as mut so it implements FnMut
                                        let mut resolve_table = |name: &str,
                                                            tables: &std::collections::HashMap<String, crate::schemasync::table::TableConfig>,
                                                            enums: &std::collections::HashMap<String, crate::types::TaggedUnion>,
                                        | -> Option<String> {
                                            // 1) Direct match: type name corresponds to a table
                                            let snake = name.to_case(Case::Snake);
                                            if tables.contains_key(&snake) {
                                                return Some(snake);
                                            }
                                            // 2) Enum (persistable struct union): pick a variant that maps to a table
                                            if let Some(tagged) = enums.get(name) {
                                                // Collect candidate table names from variants
                                                let mut candidates: Vec<String> = Vec::new();
                                                for v in &tagged.variants {
                                                    if let Some(data) = &v.data {
                                                        match data {
                                                            crate::types::VariantData::InlineStruct(enum_struct) => {
                                                                let t = enum_struct.struct_name.to_case(Case::Snake);
                                                                if tables.contains_key(&t) {
                                                                    candidates.push(t);
                                                                }
                                                            }
                                                            crate::types::VariantData::DataStructureRef(fty) => {
                                                                if let crate::types::FieldType::Other(inner_name) = fty {
                                                                    let t = inner_name.to_case(Case::Snake);
                                                                    if tables.contains_key(&t) {
                                                                        candidates.push(t);
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                                if !candidates.is_empty() {
                                                    // Choose a random candidate
                                                    let idx = rng.random_range(0..candidates.len());
                                                    return Some(candidates[idx].clone());
                                                }
                                            }
                                            None
                                        };

                                        if let Some(table_key) = resolve_table(
                                            type_name,
                                            self.mockmaker.tables,
                                            self.mockmaker.enums,
                                        ) {
                                            // Generate a record ID for the resolved table
                                            if let Some(possible_ids) =
                                                self.mockmaker.id_map.get(&table_key)
                                            {
                                                if !possible_ids.is_empty() {
                                                    let id = format!(
                                                        "r'{}'",
                                                        possible_ids[rng
                                                            .random_range(0..possible_ids.len())]
                                                    );
                                                    value_stack.push(id);
                                                } else {
                                                    panic!(
                                                        "No IDs generated for table {} in RecordLink",
                                                        table_key
                                                    );
                                                }
                                            } else {
                                                // Fallback: synthesize a plausible ID using current index
                                                let id =
                                                    format!("r'{}:{}'", table_key, &self.id_index);
                                                value_stack.push(id);
                                            }
                                        } else {
                                            panic!(
                                                "RecordLink references type '{}' which does not map to a persistable table or persistable union in field {}",
                                                type_name, ctx.field.field_name
                                            );
                                        }
                                    }
                                    _ => {
                                        panic!(
                                            "RecordLink contains non-Other type {:?} in field {}. RecordLink should reference a type name or a persistable union.",
                                            inner_type, ctx.field.field_name
                                        );
                                    }
                                }
                            }
                            FieldType::Other(type_name) => {
                                // Check if we've already visited this type to avoid infinite recursion
                                if ctx.visited_types.contains(type_name) {
                                    tracing::debug!(
                                        type_name = %type_name,
                                        field_path = %ctx.field_path,
                                        "Detected circular reference, generating null"
                                    );
                                    value_stack.push("null".to_string());
                                    continue;
                                }

                                let snake_case_name = type_name.to_case(Case::Snake);
                                if let Some((table_name, _)) = self
                                    .mockmaker
                                    .tables
                                    .iter()
                                    .find(|(_, tc)| &tc.table_name == type_name)
                                {
                                    let value = if let Some(possible_ids) =
                                        self.mockmaker.id_map.get(table_name)
                                    {
                                        format!(
                                            "r'{}'",
                                            possible_ids[rng.random_range(0..possible_ids.len())]
                                        )
                                    } else {
                                        panic!(
                                            "There were no id's for the table {}, field {}",
                                            table_name, ctx.field.field_name
                                        );
                                    };
                                    value_stack.push(value);
                                } else if let Some(struct_config) = self
                                    .mockmaker
                                    .objects
                                    .get(type_name)
                                    .or_else(|| self.mockmaker.objects.get(&snake_case_name))
                                {
                                    let field_names: Vec<String> = struct_config
                                        .fields
                                        .iter()
                                        .map(|f| f.field_name.clone())
                                        .collect();
                                    work_stack.push(WorkItem::AssembleStruct { field_names });

                                    // Add current type to visited types for nested fields
                                    let mut new_visited = ctx.visited_types.clone();
                                    new_visited.insert(type_name.clone());

                                    for struct_field in struct_config.fields.iter().rev() {
                                        let new_ctx = Frame {
                                            field: struct_field,
                                            field_type: &struct_field.field_type,
                                            field_path: format!(
                                                "{}.{}",
                                                ctx.field_path.clone(),
                                                struct_field.field_name
                                            ),
                                            table_config: ctx.table_config,
                                            visited_types: new_visited.clone(),
                                        };
                                        work_stack.push(WorkItem::Generate(new_ctx));
                                    }
                                } else if let Some(tagged_union) =
                                    self.mockmaker.enums.get(type_name)
                                {
                                    let variant = tagged_union
                                        .variants
                                        .choose(&mut rng)
                                        .expect("Failed to select a random enum variant");
                                    if let Some(ref variant_data) = variant.data {
                                        // This logic is now restructured.
                                        match variant_data {
                                            VariantData::InlineStruct(enum_struct) => {
                                                let struct_config = self.mockmaker.objects.get(&enum_struct.struct_name).expect("Inline enum struct should have corresponding object definition");
                                                let field_names: Vec<String> = struct_config
                                                    .fields
                                                    .iter()
                                                    .map(|f| f.field_name.clone())
                                                    .collect();
                                                // Since the value of an enum with data replaces the enum, we just push the struct work items.
                                                work_stack
                                                    .push(WorkItem::AssembleStruct { field_names });

                                                // Add current enum type to visited types
                                                let mut new_visited = ctx.visited_types.clone();
                                                new_visited.insert(type_name.clone());

                                                for struct_field in
                                                    struct_config.fields.iter().rev()
                                                {
                                                    let new_ctx = Frame {
                                                        field: struct_field,
                                                        field_type: &struct_field.field_type,
                                                        field_path: format!(
                                                            "{}.{}",
                                                            ctx.field_path.clone(),
                                                            struct_field.field_name
                                                        ),
                                                        table_config: ctx.table_config,
                                                        visited_types: new_visited.clone(),
                                                    };
                                                    work_stack.push(WorkItem::Generate(new_ctx));
                                                }
                                            }
                                            VariantData::DataStructureRef(field_type) => {
                                                work_stack.push(WorkItem::AssembleEnum);
                                                work_stack.push(WorkItem::Generate(Frame {
                                                    field_type,
                                                    ..ctx.clone()
                                                }));
                                            }
                                        }
                                    } else {
                                        value_stack.push(format!("'{}'", variant.name));
                                    }
                                } else {
                                    panic!(
                                        "This type could not be parsed: table {}, field {}",
                                        ctx.table_config.table_name, ctx.field.field_name
                                    );
                                }
                            }
                        }
                    }
                }
                WorkItem::AssembleVec { count } => {
                    let items: Vec<_> = value_stack.drain(value_stack.len() - count..).collect();
                    value_stack.push(format!("[{}]", items.join(", ")));
                }
                WorkItem::AssembleTuple { count } => {
                    let items: Vec<_> = value_stack.drain(value_stack.len() - count..).collect();
                    value_stack.push(format!("[{}]", items.join(", ")));
                }
                WorkItem::AssembleStruct { field_names } => {
                    let count = field_names.len();
                    let values: Vec<_> = value_stack.drain(value_stack.len() - count..).collect();
                    let assignments: Vec<String> = field_names
                        .into_iter()
                        .zip(values.into_iter())
                        .map(|(name, value)| format!("{}: {}", name, value))
                        .collect();
                    value_stack.push(format!("{{ {} }}", assignments.join(", ")));
                }
                WorkItem::AssembleMap { count } => {
                    let mut entries = Vec::with_capacity(count);
                    for _ in 0..count {
                        let value = value_stack.pop().unwrap();
                        let key = value_stack.pop().unwrap();
                        entries.push(format!("{}: {}", key, value));
                    }
                    entries.reverse();
                    value_stack.push(format!("{{ {} }}", entries.join(", ")));
                }
                WorkItem::AssembleEnum { .. } => {
                    // No action needed; the generated value just stays on the stack.
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

    pub fn handle_format(&self, format: &Format) -> String {
        let generated = format.generate_formatted_value();
        match format {
            Format::Percentage
            | Format::Latitude
            | Format::Longitude
            | Format::CurrencyAmount
            | Format::AppointmentDurationNs => generated,
            Format::DateTime | Format::AppointmentDateTime | Format::DateWithinDays(_) => {
                format!("d'{}'", generated)
            }
            _ => format!("'{}'", generated),
        }
    }

    fn handle_record_id(
        &self,
        field_name: &str,
        table_name: &str,
        table_config: &TableConfig,
        rng: &mut ThreadRng,
    ) -> String {
        if let Some(relation) = &table_config.relation {
            let mut pick_relation_record = |tables: &[String], field_label: &str| -> String {
                for candidate in tables {
                    if let Some(ids) = self.mockmaker.id_map.get(candidate) {
                        if ids.is_empty() {
                            panic!(
                                "There were no id's for the table {}, field {}",
                                candidate, field_label
                            );
                        }
                        return format!("r'{}'", ids[rng.random_range(0..ids.len())].clone());
                    }
                }
                panic!(
                    "There were no id's for any of the tables {:?}, field {}",
                    tables, field_label
                );
            };

            if field_name == "in" {
                return pick_relation_record(&relation.from, field_name);
            } else if field_name == "out" {
                return pick_relation_record(&relation.to, field_name);
            }
        }

        if let Some(ids) = self.mockmaker.id_map.get(table_name) {
            if *self.id_index < ids.len() {
                format!("r'{}'", ids[*self.id_index].clone())
            } else {
                panic!("Out of bounds index for {table_name}, {field_name}")
            }
        } else {
            format!("r'{}:{}'", table_name, &self.id_index)
        }
    }
}
