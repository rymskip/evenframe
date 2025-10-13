use crate::{
    format::Format,
    mockmake::Mockmaker,
    schemasync::TableConfig,
    types::{FieldType, StructConfig, StructField, TaggedUnion, VariantData},
};
use bon::Builder;
use chrono_tz::TZ_VARIANTS;
use convert_case::{Case, Casing};
use rand::{Rng, rngs::ThreadRng, seq::IndexedRandom};
use std::collections::HashMap;

#[derive(Debug, Builder)]
pub struct FieldValueGenerator<'a> {
    mockmaker: &'a Mockmaker,
    table_config: &'a TableConfig,
    field: &'a StructField,
    id_index: &'a usize,
    coordinated_values: &'a HashMap<String, String>,
}

impl<'a> FieldValueGenerator<'a> {
    pub fn run(&self) -> String {
        if let Some(format) = &self.field.format {
            return self.handle_format(format);
        }
        self.generate_field_value(&self.field.field_type)
    }

    pub fn generate_field_value(&self, field_type: &FieldType) -> String {
        tracing::trace!(
            field_name = %self.field.field_name,
            field_type = ?self.field.field_type,
            "Generating field value"
        );
        let mut rng = rand::rng();
        match field_type {
            FieldType::String => format!("'{}'", Mockmaker::random_string(8)),

            FieldType::Char => {
                let c = rng.random_range(32u8..=126u8) as char;
                format!("'{}'", c)
            }
            FieldType::Bool => format!("{}", rng.random_bool(0.5)),
            FieldType::Unit => "NONE".to_string(),
            FieldType::Decimal => format!("{:.3}dec", rng.random_range(0.0..100.0)),
            FieldType::F32 | FieldType::F64 | FieldType::OrderedFloat(_) => {
                format!("{:.2}f", rng.random_range(0.0..100.0))
            }
            // Combine signed integer types
            FieldType::I8
            | FieldType::I16
            | FieldType::I32
            | FieldType::I64
            | FieldType::I128
            | FieldType::Isize => {
                format!("{}", rng.random_range(0..100))
            }
            // Combine unsigned integer types
            FieldType::U8
            | FieldType::U16
            | FieldType::U32
            | FieldType::U64
            | FieldType::U128
            | FieldType::Usize => format!("{}", rng.random_range(0..100)),

            FieldType::DateTime => format!("d'{}'", chrono::Utc::now().to_rfc3339()),

            FieldType::EvenframeDuration => {
                // Generate random duration in nanoseconds (0 to 1 day in nanos)
                let nanos = rng.random_range(0..86_400_000_000_000i64); // 0 to 24 hours
                format!("duration::from::nanos({})", nanos)
            }
            FieldType::Timezone => {
                // Generate random IANA timezone string from chrono_tz
                let tz = &TZ_VARIANTS[rng.random_range(0..TZ_VARIANTS.len())];
                format!("'{}'", tz.name())
            }
            FieldType::EvenframeRecordId => self.handle_record_id(
                &self.field.field_name,
                &self.table_config.table_name,
                &mut rng,
            ),
            // For an Option, randomly decide whether to generate a value or use NULL.
            FieldType::Option(inner_type) => self.handle_option(inner_type, &mut rng),
            // For a vector, generate a dummy array with a couple of elements.
            FieldType::Vec(inner_type) => self.handle_vec(inner_type),
            // For a tuple, recursively generate values for each component.
            FieldType::Tuple(types) => self.handle_tuple(types),
            // For a struct (named fields), create a JSON-like object.
            FieldType::Struct(fields) => self.handle_struct(fields),
            FieldType::HashMap(key, value) => self.handle_hash_map(key, value),
            FieldType::BTreeMap(key, value) => self.handle_b_tree_map(key, value),
            FieldType::RecordLink(inner_type) => self.generate_field_value(inner_type),
            // For other types, try to see if the type is actually a reference to another db table/app struct, a app-only struct, or an enum.
            FieldType::Other(type_name) => self.handle_other(type_name, &mut rng),
        }
    }

    pub fn handle_format(&self, format: &Format) -> String {
        let generated = format.generate_formatted_value();

        // Check if format generates numeric or boolean values that shouldn't be quoted
        match format {
            // These formats generate numeric values, don't quote
            Format::Percentage
            | Format::Latitude
            | Format::Longitude
            | Format::CurrencyAmount
            | Format::AppointmentDurationNs => generated,

            Format::DateTime | Format::AppointmentDateTime | Format::DateWithinDays(_) => {
                format!("d'{}'", generated)
            }

            // Most formats generate strings, quote them
            _ => format!("'{}'", generated),
        }
    }

    fn handle_record_id(
        &self,
        field_name: &String,
        table_name: &String,
        rng: &mut ThreadRng,
    ) -> String {
        {
            if self.table_config.relation.is_none() && field_name == "id" {
                match self.mockmaker.id_map.get(table_name) {
                    Some(ids) => return format!("r'{}'", ids[*self.id_index].clone()),
                    None => panic!(
                        "{}",
                        format!(
                            "There were no ids for the table {}, field {}\ncurrent id_map: {:#?}",
                            table_name, field_name, self.mockmaker.id_map
                        )
                    ),
                };
            } else if field_name == "in" {
                // safe unwrap, we check if relation is some at the beginning of the function
                let relation = self.table_config.relation.as_ref().unwrap();
                for candidate in &relation.from {
                    if let Some(ids) = self.mockmaker.id_map.get(candidate) {
                        if ids.is_empty() {
                            panic!(
                                "{}",
                                format!(
                                    "There were no id's for the table {}, field {}",
                                    candidate, field_name
                                )
                            )
                        }
                        return format!("r'{}'", ids[rng.random_range(0..ids.len())].clone());
                    }
                }
                panic!(
                    "{}",
                    format!(
                        "There were no id's for any of the tables {:?}, field {}",
                        relation.from, field_name
                    )
                );
            } else if field_name == "out" {
                // safe unwrap, we check if relation is some at the beginning of the function
                let relation = self.table_config.relation.as_ref().unwrap();
                for candidate in &relation.to {
                    if let Some(ids) = self.mockmaker.id_map.get(candidate) {
                        if ids.is_empty() {
                            panic!(
                                "{}",
                                format!(
                                    "There were no id's for the table {}, field {}",
                                    candidate, field_name
                                )
                            )
                        }
                        return format!("r'{}'", ids[rng.random_range(0..ids.len())].clone());
                    }
                }
                panic!(
                    "{}",
                    format!(
                        "There were no id's for any of the tables {:?}, field {}",
                        relation.to, field_name
                    )
                );
            }

            panic!(
                "EvenframeRecordId used for field other than in, out, or id. Should use RecordLink type"
            )
        }
    }

    fn handle_option(&self, inner_type: &FieldType, rng: &mut ThreadRng) -> String {
        if rng.random_bool(0.5) {
            "null".to_string()
        } else {
            self.generate_field_value(inner_type)
        }
    }

    fn handle_vec(&self, inner_type: &FieldType) -> String {
        let count = rand::rng().random_range(2..10);

        let items: Vec<String> = (0..count)
            .map(|_| self.generate_field_value(inner_type))
            .collect();
        format!("[{}]", items.join(", "))
    }

    fn handle_tuple(&self, types: &[FieldType]) -> String {
        let values: Vec<String> = types
            .iter()
            .map(|inner_type| self.generate_field_value(inner_type))
            .collect();
        format!("({})", values.join(", "))
    }

    fn handle_struct(&self, fields: &[(String, FieldType)]) -> String {
        // Build nested coordination context
        let mut nested_coordinated_values = HashMap::new();
        let field_prefix = format!("{}.", self.field.field_name);

        // Extract coordinated values for nested fields
        for (coord_field, coord_value) in self.coordinated_values {
            if coord_field.starts_with(&field_prefix) {
                let nested_field = &coord_field[field_prefix.len()..];
                nested_coordinated_values.insert(nested_field.to_string(), coord_value.clone());
            }
        }

        let field_values: Vec<String> = fields
            .iter()
            .map(|(fname, ftype)| {
                // Check if we have a coordinated value for this nested field
                let value = if let Some(coord_value) = nested_coordinated_values.get(fname) {
                    // Use the coordinated value
                    match ftype {
                        FieldType::DateTime => format!("d'{}'", coord_value),
                        FieldType::String => format!("'{}'", coord_value),
                        _ => coord_value.clone(),
                    }
                } else {
                    self.generate_field_value(ftype)
                };
                format!("{}: {}", fname, value)
            })
            .collect();
        format!("{{ {} }}", field_values.join(", "))
    }

    fn handle_hash_map(&self, key_ft: &FieldType, value_ft: &FieldType) -> String {
        let count = rand::rng().random_range(0..3);
        let entries: Vec<String> = (0..count)
            .map(|_| {
                let key_string = self.generate_field_value(key_ft);
                let value_string = self.generate_field_value(value_ft);
                format!("{}: {}", key_string, value_string)
            })
            .collect();
        format!("{{ {} }}", entries.join(", "))
    }

    fn handle_b_tree_map(&self, key_ft: &FieldType, value_ft: &FieldType) -> String {
        let count = rand::rng().random_range(0..3);
        let entries: Vec<String> = (0..count)
            .map(|_| {
                let key_string = self.generate_field_value(key_ft);
                let value_string = self.generate_field_value(value_ft);
                format!("{}: {}", key_string, value_string)
            })
            .collect();
        format!("{{ {} }}", entries.join(", "))
    }

    fn handle_other(&self, type_name: &String, rng: &mut ThreadRng) -> String {
        let snake_case_name = type_name.to_case(Case::Snake);
        // First try to find by matching table-struct name
        if let Some((table_name, _)) = self
            .mockmaker
            .tables
            .iter()
            .find(|(_, table_config)| &table_config.table_name == type_name)
        {
            self.handle_table(table_name, rng)
        } else if let Some(struct_config) = self
            .mockmaker
            .objects
            .get(type_name)
            .or_else(|| self.mockmaker.objects.get(&snake_case_name))
        {
            self.handle_object(struct_config)
        } else if let Some(tagged_union) = self.mockmaker.enums.get(type_name) {
            self.handle_enum(tagged_union, rng)
        } else {
            panic!(
                "{}",
                format!(
                    "This type could not be parsed table {}, field {}",
                    self.table_config.table_name, self.field.field_name
                )
            )
        }
    }

    fn handle_table(&self, table_name: &String, rng: &mut ThreadRng) -> String {
        if let Some(possible_ids) = self.mockmaker.id_map.get(table_name) {
            let idx = rng.random_range(0..possible_ids.len());
            format!("r'{}'", possible_ids[idx])
        } else {
            // Fallback if no ids were generated for this table
            panic!(
                "{}",
                format!(
                    "There were no id's for the table {}, field {}",
                    table_name, self.field.field_name
                )
            )
        }
    }

    fn handle_enum(&self, tagged_union: &TaggedUnion, rng: &mut ThreadRng) -> String {
        let variant = tagged_union
            .variants
            .choose(rng)
            .expect("Something went wrong selecting a random enum variant, returned None");
        if let Some(ref variant_data) = variant.data {
            // Generate dummy value for the enum variant's data, if available.
            let variant_data_field_type = match variant_data {
                VariantData::InlineStruct(enum_struct) => {
                    &FieldType::Other(enum_struct.struct_name.clone())
                }
                VariantData::DataStructureRef(field_type) => field_type,
            };
            self.generate_field_value(variant_data_field_type)
        } else {
            format!("'{}'", variant.name)
        }
    }

    fn handle_object(&self, struct_config: &StructConfig) -> String {
        let mut assignments = Vec::new();
        for struct_field in &struct_config.fields {
            let val = Self::builder()
                .coordinated_values(self.coordinated_values)
                .field(struct_field)
                .id_index(self.id_index)
                .mockmaker(self.mockmaker)
                .table_config(self.table_config)
                .build()
                .run();

            assignments.push(format!("{}: {val}", struct_field.field_name));
        }
        // Surreal accepts JSON-like objects with unquoted keys:
        format!("{{ {} }}", assignments.join(", "))
    }
}
