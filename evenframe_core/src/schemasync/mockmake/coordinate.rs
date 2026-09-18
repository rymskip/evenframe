use bon::Builder;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use try_from_expr::TryFromExpr;
use uuid::Uuid;

#[cfg(feature = "schemasync")]
use crate::error::EvenframeError;
#[cfg(feature = "schemasync")]
use crate::schemasync::mockmake::field_value::FieldValueGenerator;
#[cfg(feature = "schemasync")]
use crate::schemasync::mockmake::{Mockmaker, format::Format};
#[cfg(feature = "schemasync")]
use crate::types::{FieldType, StructField};
#[cfg(feature = "schemasync")]
use chrono::{Duration, NaiveDate, Utc};
#[cfg(feature = "schemasync")]
use rand::RngExt;

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize, Builder)]
pub struct CoordinationId {
    pub table_name: String,
    pub field_name: String,
}

#[cfg(feature = "schemasync")]
impl CoordinationId {
    /// The field this id names: a field of its table, or one nested in
    /// objects along a dotted path.
    pub fn get_field(&self, mockmaker: &Mockmaker<'_>) -> Result<StructField, EvenframeError> {
        let unknown = |reason: String| {
            EvenframeError::mock_generation(format!(
                "coordinated field `{}.{}` {reason}",
                self.table_name, self.field_name
            ))
        };
        let table = mockmaker
            .tables
            .get(&self.table_name)
            .ok_or_else(|| unknown("is on an unknown table".into()))?
            .effective();
        let mut fields = &table.struct_config.fields;
        let mut segments = self.field_name.split('.').peekable();
        while let Some(segment) = segments.next() {
            let field = fields
                .iter()
                .find(|f| f.field_name == segment)
                .ok_or_else(|| unknown(format!("has no field `{segment}`")))?;
            if segments.peek().is_none() {
                return Ok(field.clone());
            }
            fields = &object_name(&field.field_type)
                .and_then(|name| mockmaker.objects.get(name))
                .ok_or_else(|| {
                    unknown(format!("goes through `{segment}`, which is not an object"))
                })?
                .effective()
                .fields;
        }
        Err(unknown("has an empty path".into()))
    }
}

/// The object type a field holds, looking through `Option`.
#[cfg(feature = "schemasync")]
fn object_name(field_type: &FieldType) -> Option<&str> {
    match field_type {
        FieldType::Other(name) => Some(name),
        FieldType::Option(inner) => object_name(inner),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Builder)]
pub struct CoordinationPair {
    pub coordinated_fields: Vec<CoordinationId>,
    pub coordination: Coordination,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, Builder)]
pub struct CoordinationGroup {
    #[builder(default)]
    pub id: Uuid,
    #[builder(default)]
    pub tables: BTreeSet<String>,
    #[builder(default)]
    pub coordination_pairs: Vec<CoordinationPair>,
}
/// Render a raw coordinated value as the SurrealQL literal `FieldValueGenerator`
/// would produce for `field`: bare numbers for numeric fields (floats with an
/// `f` suffix), `d'…'` for datetime-formatted non-string fields, and a quoted
/// string otherwise.
/// Whether a field holds a date or a date and time.
#[cfg(feature = "schemasync")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DateKind {
    Date,
    DateTime,
}

/// The kind of date a field holds, from its format or else its type.
#[cfg(feature = "schemasync")]
pub(crate) fn date_kind(field: &StructField) -> Option<DateKind> {
    match field.format {
        Some(Format::Date) => return Some(DateKind::Date),
        Some(Format::DateTime | Format::DateWithinDays(_) | Format::AppointmentDateTime) => {
            return Some(DateKind::DateTime);
        }
        _ => {}
    }
    let mut ty = &field.field_type;
    while let FieldType::Option(inner) = ty {
        ty = inner;
    }
    matches!(ty, FieldType::Other(name) if is_datetime_like(name)).then_some(DateKind::DateTime)
}

/// The first day of generated date sequences and ranges.
#[cfg(feature = "schemasync")]
const BASE_DATE: NaiveDate = NaiveDate::from_ymd_opt(2024, 1, 1).expect("2024-01-01 is a date");

#[cfg(feature = "schemasync")]
pub(crate) fn coordinated_literal(field: &StructField, raw: &str) -> String {
    let mut scalar = &field.field_type;
    while let FieldType::Option(inner) = scalar {
        scalar = inner;
    }
    if scalar.is_numeric()
        && let Ok(number) = raw.trim().parse::<f64>()
    {
        return match scalar {
            FieldType::F32 | FieldType::F64 => format!("{number}f"),
            _ => format!("{}", number.round() as i64),
        };
    }
    let escaped = raw.replace('\\', "\\\\").replace('\'', "\\'");
    if date_kind(field).is_some() && !matches!(scalar, FieldType::String) {
        format!("d'{escaped}'")
    } else {
        format!("'{escaped}'")
    }
}

/// The raw text of a SurrealQL string or datetime literal (`'a'`, `d'…'`);
/// other literals are returned unchanged.
#[cfg(feature = "schemasync")]
pub(crate) fn literal_to_raw(literal: &str) -> String {
    let body = literal
        .strip_prefix("d'")
        .or_else(|| literal.strip_prefix('\''))
        .and_then(|rest| rest.strip_suffix('\''));
    match body {
        Some(body) => body.replace("\\'", "'").replace("\\\\", "\\"),
        None => literal.to_string(),
    }
}

#[cfg(feature = "schemasync")]
impl Mockmaker<'_> {
    pub fn generate_coordinated_values(&mut self) -> Result<(), EvenframeError> {
        tracing::debug!("Generating coordinated values for all tables");

        // Build coordination groups from the tables
        let coordination_groups = self.build_coordination_groups()?;

        // Clear existing coordinated values
        self.coordinated_values.clear();

        // Process each coordination group
        for coordination_group in coordination_groups {
            // Get the maximum record count from all tables in this group
            let n = coordination_group
                .tables
                .iter()
                .filter_map(|table_name| self.tables.get(table_name))
                .map(|table| self.record_count(table.effective()))
                .max()
                .unwrap_or(self.schemasync_config.mock_gen_config.default_record_count);

            // Process coordinated values for this group
            for index in 0..n {
                for coordination_pair in &coordination_group.coordination_pairs {
                    match &coordination_pair.coordination {
                        Coordination::InitializeEqual(_) => {
                            let Some(first_coord) = coordination_pair.coordinated_fields.first()
                            else {
                                continue;
                            };
                            let value = self.generate_coordinated_field(first_coord, index)?;

                            for coordination_id in &coordination_pair.coordinated_fields {
                                self.coordinated_values
                                    .insert((index, coordination_id.clone()), value.clone());
                            }
                        }

                        Coordination::InitializeSequential {
                            field_names: _,
                            increment,
                        } => {
                            // Collect the fields for this coordination
                            let fields: Vec<StructField> = coordination_pair
                                .coordinated_fields
                                .iter()
                                .map(|coord_id| coord_id.get_field(self))
                                .collect::<Result<_, _>>()?;
                            let field_refs: Vec<&StructField> = fields.iter().collect();

                            // Generate sequential values using the dedicated function
                            let values = Self::generate_sequential_values(&field_refs, increment);

                            // Store the generated values
                            for coordination_id in &coordination_pair.coordinated_fields {
                                let field_name = coordination_id
                                    .field_name
                                    .split('.')
                                    .next_back()
                                    .unwrap_or(&coordination_id.field_name);

                                if let Some(value) = values.get(field_name) {
                                    self.store_raw_coordinated_value(
                                        index,
                                        coordination_id,
                                        value,
                                    )?;
                                }
                            }
                        }
                        Coordination::InitializeSum {
                            field_names: _,
                            total,
                        } => {
                            // Collect the fields for this coordination
                            let fields: Vec<StructField> = coordination_pair
                                .coordinated_fields
                                .iter()
                                .map(|coord_id| coord_id.get_field(self))
                                .collect::<Result<_, _>>()?;
                            let field_refs: Vec<&StructField> = fields.iter().collect();

                            // Generate sum values using the dedicated function
                            let values = Self::generate_sum_values(&field_refs, *total);

                            // Store the generated values
                            for coordination_id in &coordination_pair.coordinated_fields {
                                let field_name = coordination_id
                                    .field_name
                                    .split('.')
                                    .next_back()
                                    .unwrap_or(&coordination_id.field_name);

                                if let Some(value) = values.get(field_name) {
                                    self.store_raw_coordinated_value(
                                        index,
                                        coordination_id,
                                        value,
                                    )?;
                                }
                            }
                        }
                        Coordination::InitializeDerive {
                            source_field_names,
                            target_field_name,
                            derivation,
                        } => {
                            // Separate source and target fields
                            let mut source_fields = Vec::new();
                            let mut source_coord_ids = Vec::new();
                            let mut target_coord_id = None;

                            for coordination_id in &coordination_pair.coordinated_fields {
                                let field_name = coordination_id
                                    .field_name
                                    .split('.')
                                    .next_back()
                                    .unwrap_or(&coordination_id.field_name);

                                if source_field_names.contains(&field_name.to_string()) {
                                    source_fields.push(coordination_id.get_field(self)?);
                                    source_coord_ids.push(coordination_id.clone());
                                } else if field_name == target_field_name {
                                    target_coord_id = Some(coordination_id.clone());
                                }
                            }

                            // Generate source values first
                            let mut source_values_map = BTreeMap::new();
                            for (coord_id, field) in
                                source_coord_ids.iter().zip(source_fields.iter())
                            {
                                let value = self.generate_coordinated_field(coord_id, index)?;

                                let field_name = field.field_name.clone();
                                source_values_map.insert(field_name, literal_to_raw(&value));
                                self.coordinated_values
                                    .insert((index, coord_id.clone()), value);
                            }

                            // Generate derived value using the dedicated function
                            let source_field_refs: Vec<&StructField> =
                                source_fields.iter().collect();
                            let derived_values = Self::generate_derive_values(
                                &source_field_refs,
                                target_field_name,
                                derivation,
                                &source_values_map,
                            );

                            // Store the derived value
                            if let (Some(target_id), Some(value)) =
                                (target_coord_id, derived_values.get(target_field_name))
                            {
                                self.store_raw_coordinated_value(index, &target_id, value)?;
                            }
                        }
                        Coordination::OneToOne(_) => {
                            // OneToOne is handled directly in handle_record_id
                            // at generation time, not through pre-computed values.
                        }
                        Coordination::InitializeCoherent(coherent_dataset) => {
                            let values = Self::generate_coherent_values(coherent_dataset, index);

                            // Store the generated values
                            // For coherent datasets, we need to match the field name from the dataset
                            // to the actual field name which might include a path
                            for coordination_id in &coordination_pair.coordinated_fields {
                                // Try to match by the last part of the field name
                                let field_key = coordination_id
                                    .field_name
                                    .split('.')
                                    .next_back()
                                    .unwrap_or(&coordination_id.field_name);

                                // Try exact match first
                                if let Some(value) = values.get(field_key) {
                                    self.store_raw_coordinated_value(
                                        index,
                                        coordination_id,
                                        value,
                                    )?;
                                } else {
                                    // Try to find a matching key in the values map
                                    for (key, value) in &values {
                                        if coordination_id.field_name.ends_with(key)
                                            || key == field_key
                                        {
                                            self.store_raw_coordinated_value(
                                                index,
                                                coordination_id,
                                                value,
                                            )?;
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// A generated value for the coordinated field `id` of record `index`.
    fn generate_coordinated_field(
        &self,
        id: &CoordinationId,
        index: usize,
    ) -> Result<String, EvenframeError> {
        let field = id.get_field(self)?;
        let table_config = self.table(&id.table_name)?;
        FieldValueGenerator::builder()
            .field(&field)
            .id_index(&index)
            .mockmaker(self)
            .table_config(table_config)
            .registry(self.registry)
            .build()
            .run()
    }

    /// Store a raw (unquoted) coordinated value for record `index`, rendered
    /// as a SurrealQL literal for its field.
    fn store_raw_coordinated_value(
        &mut self,
        index: usize,
        coordination_id: &CoordinationId,
        raw: &str,
    ) -> Result<(), EvenframeError> {
        let field = coordination_id.get_field(self)?;
        self.coordinated_values.insert(
            (index, coordination_id.clone()),
            coordinated_literal(&field, raw),
        );
        Ok(())
    }

    /// Values for `fields` that step by `increment`: dates or datetimes
    /// from a base, or numbers from a random start.
    pub fn generate_sequential_values(
        fields: &[&StructField],
        increment: &CoordinateIncrement,
    ) -> BTreeMap<String, String> {
        tracing::trace!(field_count = fields.len(), "Generating sequential values");
        let Some(first_field) = fields.first() else {
            return BTreeMap::new();
        };
        let kind = date_kind(first_field);
        let now = Utc::now();
        let base: f64 = rand::rng().random_range(0.0..100.0);
        let step = |i: usize| {
            let i = i as i64;
            match increment {
                CoordinateIncrement::Days(d) => Duration::days(i64::from(*d) * i),
                CoordinateIncrement::Hours(h) => Duration::hours(i64::from(*h) * i),
                CoordinateIncrement::Minutes(m) => Duration::minutes(i64::from(*m) * i),
                CoordinateIncrement::Numeric(_) => Duration::zero(),
            }
        };
        fields
            .iter()
            .enumerate()
            .map(|(i, field)| {
                let value = match (kind, increment) {
                    (Some(DateKind::Date), _) => (BASE_DATE + step(i)).to_string(),
                    (Some(DateKind::DateTime), _) => (now + step(i)).to_rfc3339(),
                    (None, CoordinateIncrement::Numeric(n)) => (base + n * i as f64).to_string(),
                    (None, _) => (base + i as f64).to_string(),
                };
                (field.field_name.clone(), value)
            })
            .collect()
    }

    /// Generate values that sum to a total
    fn generate_sum_values(fields: &[&StructField], total: f64) -> BTreeMap<String, String> {
        tracing::trace!(
            field_count = fields.len(),
            total = total,
            "Generating sum values"
        );
        let mut values = BTreeMap::new();
        let mut rng = rand::rng();

        if fields.is_empty() {
            return values;
        }

        // Generate random percentages that sum to total
        let mut remaining = total;
        let mut generated_values = Vec::new();

        for i in 0..fields.len() - 1 {
            let max_value = remaining / (fields.len() - i) as f64 * 1.5; // Allow some variance
            let value = rng.random_range(0.0..max_value.min(remaining));
            generated_values.push(value);
            remaining -= value;
        }

        // Last value gets the remainder to ensure exact sum
        generated_values.push(remaining);

        // Assign values to fields, but handle rounding carefully for percentages
        let is_percentage = fields
            .iter()
            .any(|f| matches!(f.format, Some(Format::Percentage)));

        if is_percentage {
            // Round all but the last value, which takes the rest, so the
            // rounded values still sum to the total.
            let mut formatted_values = Vec::new();
            let mut formatted_sum = 0.0;
            for value in &generated_values[..generated_values.len() - 1] {
                let rounded = (value * 10.0).round() / 10.0;
                formatted_sum += rounded;
                formatted_values.push(format!("{rounded:.1}"));
            }
            formatted_values.push(format!("{:.1}", total - formatted_sum));

            // Assign the formatted values
            for (field, formatted_value) in fields.iter().zip(formatted_values.iter()) {
                values.insert(field.field_name.clone(), formatted_value.clone());
            }
        } else {
            // For non-percentage fields, use the original logic
            for (field, value) in fields.iter().zip(generated_values.iter()) {
                // Coordinated values are interpolated into SurrealQL verbatim,
                // so the declared field type decides the shape: numeric fields
                // get a bare number, string fields a quoted "$…" literal.
                let formatted_value = match &field.format {
                    Some(Format::CurrencyAmount) if !field.field_type.is_numeric() => {
                        format!("'${:.2}'", value)
                    }
                    _ => format!("{:.2}", value),
                };
                values.insert(field.field_name.clone(), formatted_value);
            }
        }

        values
    }

    /// Generate derived values from source fields
    fn generate_derive_values(
        source_fields: &[&StructField],
        target_field: &str,
        derivation: &DerivationType,
        source_values: &BTreeMap<String, String>,
    ) -> BTreeMap<String, String> {
        tracing::trace!(target_field = %target_field, "Generating derived values");
        let mut values = BTreeMap::new();

        match derivation {
            DerivationType::Concatenate(separator) => {
                let concatenated = source_fields
                    .iter()
                    .filter_map(|field| source_values.get(&field.field_name))
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(separator);
                values.insert(target_field.to_string(), concatenated);
            }
            DerivationType::Extract(extract_type) => {
                if let Some(first_field) = source_fields.first()
                    && let Some(source_value) = source_values.get(&first_field.field_name)
                {
                    let extracted = match extract_type {
                        ExtractType::FirstWord => source_value
                            .split_whitespace()
                            .next()
                            .unwrap_or("")
                            .to_string(),
                        ExtractType::LastWord => source_value
                            .split_whitespace()
                            .last()
                            .unwrap_or("")
                            .to_string(),
                        ExtractType::Domain => {
                            // Extract domain from email
                            source_value.split('@').nth(1).unwrap_or("").to_string()
                        }
                        ExtractType::Username => {
                            // Extract username from email
                            source_value.split('@').next().unwrap_or("").to_string()
                        }
                        ExtractType::Initials => {
                            // Extract initials from words
                            source_value
                                .split_whitespace()
                                .filter_map(|word| word.chars().next())
                                .collect::<String>()
                                .to_uppercase()
                        }
                    };
                    values.insert(target_field.to_string(), extracted);
                }
            }
            DerivationType::Transform(transform_type) => {
                if let Some(first_field) = source_fields.first()
                    && let Some(source_value) = source_values.get(&first_field.field_name)
                {
                    let transformed = match transform_type {
                        TransformType::Uppercase => source_value.to_uppercase(),
                        TransformType::Lowercase => source_value.to_lowercase(),
                        TransformType::Capitalize => {
                            let mut chars = source_value.chars();
                            match chars.next() {
                                None => String::new(),
                                Some(first) => {
                                    first.to_uppercase().collect::<String>() + chars.as_str()
                                }
                            }
                        }
                        TransformType::Truncate(len) => source_value.chars().take(*len).collect(),
                        TransformType::Hash => {
                            // Simple hash representation
                            format!(
                                "{:x}",
                                source_value.len() * 31
                                    + source_value.chars().map(|c| c as usize).sum::<usize>()
                            )
                        }
                    };
                    values.insert(target_field.to_string(), transformed);
                }
            }
        }

        values
    }

    /// Generate coherent values from predefined datasets
    fn generate_coherent_values(
        dataset: &crate::schemasync::mockmake::coordinate::CoherentDataset,
        index: usize,
    ) -> BTreeMap<String, String> {
        tracing::trace!(index = index, "Generating coherent values");
        use crate::schemasync::mockmake::coordinate::*;

        /// Coherent address data
        const COHERENT_ADDRESSES: &[(&str, &str, &str, &str)] = &[
            ("New York", "NY", "10001", "USA"),
            ("Los Angeles", "CA", "90001", "USA"),
            ("Chicago", "IL", "60601", "USA"),
            ("Houston", "TX", "77001", "USA"),
            ("Phoenix", "AZ", "85001", "USA"),
            ("Philadelphia", "PA", "19101", "USA"),
            ("San Antonio", "TX", "78201", "USA"),
            ("San Diego", "CA", "92101", "USA"),
            ("Dallas", "TX", "75201", "USA"),
            ("San Jose", "CA", "95101", "USA"),
        ];

        /// Coherent geo location data
        const CITY_COORDINATES: &[(&str, f64, f64, &str)] = &[
            ("New York", 40.7128, -74.0060, "USA"),
            ("Los Angeles", 34.0522, -118.2437, "USA"),
            ("Chicago", 41.8781, -87.6298, "USA"),
            ("Houston", 29.7604, -95.3698, "USA"),
            ("Phoenix", 33.4484, -112.0740, "USA"),
            ("Philadelphia", 39.9526, -75.1652, "USA"),
            ("San Antonio", 29.4241, -98.4936, "USA"),
            ("San Diego", 32.7157, -117.1611, "USA"),
            ("Dallas", 32.7767, -96.7970, "USA"),
            ("San Jose", 37.3382, -121.8863, "USA"),
        ];

        match dataset {
            CoherentDataset::Address {
                city,
                state,
                zip,
                country,
            } => {
                let (city_val, state_val, zip_val, country_val) =
                    COHERENT_ADDRESSES[index % COHERENT_ADDRESSES.len()];
                let mut values = BTreeMap::new();
                values.insert(city.clone(), city_val.to_string());
                values.insert(state.clone(), state_val.to_string());
                values.insert(zip.clone(), zip_val.to_string());
                values.insert(country.clone(), country_val.to_string());
                values
            }
            CoherentDataset::PersonName {
                first_name,
                last_name,
                full_name,
            } => {
                // Use the extended person names from coordinate.rs
                let names = crate::schemasync::mockmake::coordinate::EXTENDED_PERSON_NAMES;
                let (first, last, _gender) = names[index % names.len()];
                let mut values = BTreeMap::new();
                values.insert(first_name.clone(), first.to_string());
                values.insert(last_name.clone(), last.to_string());
                values.insert(full_name.clone(), format!("{} {}", first, last));
                values
            }
            CoherentDataset::GeoLocation {
                latitude,
                longitude,
                city,
                country,
            } => {
                let (city_val, lat, lng, country_val) =
                    CITY_COORDINATES[index % CITY_COORDINATES.len()];
                let mut values = BTreeMap::new();
                values.insert(latitude.clone(), lat.to_string());
                values.insert(longitude.clone(), lng.to_string());
                values.insert(city.clone(), city_val.to_string());
                values.insert(country.clone(), country_val.to_string());
                values
            }
            CoherentDataset::DateRange {
                start_date,
                end_date,
            } => {
                // Generate coherent start/end dates
                let base = BASE_DATE;
                let start_offset = (index * 7) as i64; // Weekly intervals
                let duration_days = 14; // 2 week duration

                let start = base + Duration::days(start_offset);
                let end = start + Duration::days(duration_days);

                let mut values = BTreeMap::new();
                values.insert(start_date.clone(), start.to_string());
                values.insert(end_date.clone(), end.to_string());
                values
            }
            CoherentDataset::GeoRadius {
                latitude,
                longitude,
                center_lat,
                center_lng,
                radius_km,
            } => {
                let mut rng = rand::rng();
                let earth_radius_km = 6371.0_f64;

                // sqrt for uniform area distribution within circle
                let distance_km = radius_km * rng.random::<f64>().sqrt();
                let bearing = rng.random::<f64>() * 2.0 * std::f64::consts::PI;

                // Haversine inverse formula
                let angular_dist = distance_km / earth_radius_km;
                let lat1 = center_lat.to_radians();
                let lng1 = center_lng.to_radians();

                let lat2 = (lat1.sin() * angular_dist.cos()
                    + lat1.cos() * angular_dist.sin() * bearing.cos())
                .asin();
                let lng2 = lng1
                    + (bearing.sin() * angular_dist.sin() * lat1.cos())
                        .atan2(angular_dist.cos() - lat1.sin() * lat2.sin());

                let mut values = BTreeMap::new();
                values.insert(latitude.clone(), format!("{:.6}", lat2.to_degrees()));
                values.insert(longitude.clone(), format!("{:.6}", lng2.to_degrees()));
                values
            }
        }
    }
}
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, TryFromExpr)]
pub enum CoordinatedValue {
    String(String),
    F64(f64),
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, TryFromExpr)]
pub enum Coordination {
    /// Initialize multiple fields with the same value
    InitializeEqual(Vec<String>),

    /// Initialize fields in sequence (e.g., start < end dates)
    InitializeSequential {
        field_names: Vec<String>,
        increment: CoordinateIncrement,
    },

    /// Fields must sum to a total (e.g., percentage fields = 100)
    InitializeSum {
        field_names: Vec<String>,
        total: f64,
    },

    /// One field derives from another (e.g., full_name from first + last)
    InitializeDerive {
        source_field_names: Vec<String>,
        target_field_name: String,
        derivation: DerivationType,
    },

    /// Ensure fields are from same dataset (e.g., matching city/state/zip)
    InitializeCoherent(CoherentDataset),

    /// Map a relation field 1:1 to target table records sequentially.
    /// Use on relation tables to ensure every target record gets exactly one edge.
    /// The string is the field name (e.g., "out").
    OneToOne(String),
}

#[cfg(feature = "schemasync")]
impl Coordination {
    /// Validate that this coordination can be applied to the given fields
    pub fn validate(
        &self,
        mockmaker: &Mockmaker<'_>,
        coordination_ids: &[CoordinationId],
    ) -> Result<(), EvenframeError> {
        // First validate that all field paths exist and are valid
        let mut fields = Vec::new();
        for coord_id in coordination_ids {
            // Validate field path exists
            let parts: Vec<&str> = coord_id.field_name.split('.').collect();
            let table_config = mockmaker.tables.get(&coord_id.table_name).ok_or_else(|| {
                EvenframeError::Validation(format!(
                    "Table '{}' not found for coordination field '{}'",
                    coord_id.table_name, coord_id.field_name
                ))
            })?;

            // Traverse the field path to validate it exists
            let mut current_fields = &table_config.struct_config.fields;
            let mut current_field: Option<&StructField> = None;

            for (i, part) in parts.iter().enumerate() {
                let field = current_fields
                    .iter()
                    .find(|f| &f.field_name == part)
                    .ok_or_else(|| {
                        EvenframeError::Validation(format!(
                            "Field path '{}' is invalid: '{}' not found in '{}'",
                            coord_id.field_name, part, coord_id.table_name
                        ))
                    })?;

                current_field = Some(field);

                // If not the last part, we need to traverse into a struct
                if i < parts.len() - 1 {
                    match &field.field_type {
                        FieldType::Struct(_nested_fields) => {
                            // For inline structs, we'd need to handle this differently
                            // For now, we'll validate against objects
                            if let FieldType::Other(type_name) = &field.field_type {
                                let obj = mockmaker.objects.get(type_name).ok_or_else(|| {
                                    EvenframeError::Validation(format!(
                                        "Nested object '{}' not found for field path '{}'",
                                        type_name, coord_id.field_name
                                    ))
                                })?;
                                current_fields = &obj.fields;
                            } else {
                                return Err(EvenframeError::Validation(format!(
                                    "Field '{}' in path '{}' is not a struct type",
                                    part, coord_id.field_name
                                )));
                            }
                        }
                        FieldType::Other(type_name) => {
                            let obj = mockmaker.objects.get(type_name).ok_or_else(|| {
                                EvenframeError::Validation(format!(
                                    "Nested object '{}' not found for field path '{}'",
                                    type_name, coord_id.field_name
                                ))
                            })?;
                            current_fields = &obj.fields;
                        }
                        _ => {
                            return Err(EvenframeError::Validation(format!(
                                "Field '{}' in path '{}' is not a struct type and cannot have nested fields",
                                part, coord_id.field_name
                            )));
                        }
                    }
                }
            }

            if let Some(field) = current_field {
                fields.push((coord_id.clone(), field.clone()));
            }
        }

        // Now validate based on coordination type
        match self {
            Coordination::InitializeEqual(_) => {
                // All fields must have the same type AND format
                if fields.is_empty() {
                    return Err(EvenframeError::Validation(
                        "InitializeEqual requires at least one field".to_string(),
                    ));
                }

                let first_field = &fields[0].1;
                let first_type = &first_field.field_type;
                let first_format = &first_field.format;

                for (coord_id, field) in &fields[1..] {
                    if !field_types_compatible(first_type, &field.field_type) {
                        return Err(EvenframeError::Validation(format!(
                            "InitializeEqual: Field '{}' has incompatible type {:?}, expected type compatible with {:?}",
                            coord_id.field_name, field.field_type, first_type
                        )));
                    }

                    if first_format != &field.format {
                        return Err(EvenframeError::Validation(format!(
                            "InitializeEqual: Field '{}' has format {:?}, but expected format {:?} to match other fields",
                            coord_id.field_name, field.format, first_format
                        )));
                    }
                }
            }

            Coordination::InitializeSequential { increment, .. } => {
                for (coord_id, field) in &fields {
                    let fits = match increment {
                        CoordinateIncrement::Days(_) => date_kind(field).is_some(),
                        CoordinateIncrement::Hours(_) | CoordinateIncrement::Minutes(_) => {
                            date_kind(field) == Some(DateKind::DateTime)
                        }
                        CoordinateIncrement::Numeric(_) => is_numeric_type(&field.field_type),
                    };
                    if !fits {
                        return Err(EvenframeError::Validation(format!(
                            "InitializeSequential: field '{}' ({:?}, format {:?}) cannot step by {increment:?}; \
                             days step dates and datetimes, hours and minutes step datetimes, \
                             and numeric steps need a numeric field",
                            coord_id.field_name, field.field_type, field.format
                        )));
                    }
                    if let Some((_, first)) = fields.first()
                        && field.format != first.format
                    {
                        return Err(EvenframeError::Validation(format!(
                            "InitializeSequential: field '{}' has format {:?}, but the first field has {:?}",
                            coord_id.field_name, field.format, first.format
                        )));
                    }
                }
            }

            Coordination::InitializeSum { total, .. } => {
                // All fields must be numeric
                for (coord_id, field) in &fields {
                    if !is_numeric_type(&field.field_type) {
                        return Err(EvenframeError::Validation(format!(
                            "InitializeSum: Field '{}' must be numeric type to participate in sum, got {:?}",
                            coord_id.field_name, field.field_type
                        )));
                    }
                }

                if *total <= 0.0 {
                    return Err(EvenframeError::Validation(format!(
                        "InitializeSum: Total must be positive, got {}",
                        total
                    )));
                }
            }

            Coordination::InitializeDerive {
                source_field_names,
                target_field_name,
                derivation,
            } => {
                // Validate based on derivation type
                match derivation {
                    DerivationType::Concatenate(_) => {
                        // Source fields should be string-like
                        for source_name in source_field_names {
                            let source_field = fields
                                .iter()
                                .find(|(id, _)| id.field_name.ends_with(source_name))
                                .ok_or_else(|| {
                                    EvenframeError::Validation(format!(
                                        "Source field '{}' not found in coordination fields",
                                        source_name
                                    ))
                                })?;

                            if !is_string_like(&source_field.1.field_type) {
                                return Err(EvenframeError::Validation(format!(
                                    "InitializeDerive with Concatenate: Source field '{}' should be string-like, got {:?}",
                                    source_name, source_field.1.field_type
                                )));
                            }
                        }

                        // Target should be string
                        let target_field = fields
                            .iter()
                            .find(|(id, _)| id.field_name.ends_with(target_field_name))
                            .ok_or_else(|| {
                                EvenframeError::Validation(format!(
                                    "Target field '{}' not found in coordination fields",
                                    target_field_name
                                ))
                            })?;

                        match &target_field.1.field_type {
                            FieldType::String => {}
                            FieldType::Option(inner) if matches!(**inner, FieldType::String) => {}
                            _ => {
                                return Err(EvenframeError::Validation(format!(
                                    "InitializeDerive target field '{}' must be String type, got {:?}",
                                    target_field_name, target_field.1.field_type
                                )));
                            }
                        }
                    }
                    DerivationType::Extract(extract_type) => {
                        match extract_type {
                            ExtractType::Domain | ExtractType::Username => {
                                // Source should be email (string with email format)
                                for source_name in source_field_names {
                                    let source_field = fields
                                        .iter()
                                        .find(|(id, _)| id.field_name.ends_with(source_name))
                                        .ok_or_else(|| {
                                            EvenframeError::Validation(format!(
                                                "Source field '{}' not found",
                                                source_name
                                            ))
                                        })?;

                                    if !is_string_like(&source_field.1.field_type) {
                                        return Err(EvenframeError::Validation(format!(
                                            "Extract Domain/Username requires string field, got {:?}",
                                            source_field.1.field_type
                                        )));
                                    }

                                    // Ideally should have Email format
                                    if !matches!(&source_field.1.format, Some(Format::Email)) {
                                        tracing::warn!(
                                            "Field '{}' used for email extraction but doesn't have Email format",
                                            source_name
                                        );
                                    }
                                }
                            }
                            _ => {
                                // Other extract types need string sources
                                for source_name in source_field_names {
                                    let source_field = fields
                                        .iter()
                                        .find(|(id, _)| id.field_name.ends_with(source_name))
                                        .ok_or_else(|| {
                                            EvenframeError::Validation(format!(
                                                "Source field '{}' not found",
                                                source_name
                                            ))
                                        })?;

                                    if !is_string_like(&source_field.1.field_type) {
                                        return Err(EvenframeError::Validation(format!(
                                            "Extract operation requires string field, got {:?}",
                                            source_field.1.field_type
                                        )));
                                    }
                                }
                            }
                        }
                    }
                    DerivationType::Transform(_) => {
                        // Transform operations are flexible, just ensure fields exist
                        if source_field_names.is_empty() {
                            return Err(EvenframeError::Validation(
                                "InitializeDerive requires at least one source field".to_string(),
                            ));
                        }
                    }
                }
            }

            Coordination::InitializeCoherent(dataset) => {
                // Validate that field types match expected types for the dataset
                match dataset {
                    CoherentDataset::Address {
                        city,
                        state,
                        zip,
                        country,
                    } => {
                        validate_string_field(&fields, city, "city")?;
                        validate_string_field(&fields, state, "state")?;
                        validate_string_field(&fields, zip, "zip")?;
                        validate_string_field(&fields, country, "country")?;
                    }
                    CoherentDataset::PersonName {
                        first_name,
                        last_name,
                        full_name,
                    } => {
                        validate_string_field(&fields, first_name, "first_name")?;
                        validate_string_field(&fields, last_name, "last_name")?;
                        validate_string_field(&fields, full_name, "full_name")?;
                    }
                    CoherentDataset::GeoLocation {
                        latitude,
                        longitude,
                        city,
                        country,
                    } => {
                        validate_numeric_field(&fields, latitude, "latitude")?;
                        validate_numeric_field(&fields, longitude, "longitude")?;
                        validate_string_field(&fields, city, "city")?;
                        validate_string_field(&fields, country, "country")?;
                    }
                    CoherentDataset::DateRange {
                        start_date,
                        end_date,
                    } => {
                        validate_datetime_field(&fields, start_date, "start_date")?;
                        validate_datetime_field(&fields, end_date, "end_date")?;
                    }
                    CoherentDataset::GeoRadius {
                        latitude,
                        longitude,
                        center_lat,
                        center_lng,
                        radius_km,
                    } => {
                        validate_numeric_field(&fields, latitude, "latitude")?;
                        validate_numeric_field(&fields, longitude, "longitude")?;
                        if !(-90.0..=90.0).contains(center_lat) {
                            return Err(EvenframeError::mock_generation(format!(
                                "center_lat must be between -90 and 90, got {}",
                                center_lat
                            )));
                        }
                        if !(-180.0..=180.0).contains(center_lng) {
                            return Err(EvenframeError::mock_generation(format!(
                                "center_lng must be between -180 and 180, got {}",
                                center_lng
                            )));
                        }
                        if *radius_km <= 0.0 {
                            return Err(EvenframeError::mock_generation(format!(
                                "radius_km must be positive, got {}",
                                radius_km
                            )));
                        }
                    }
                }
            }

            Coordination::OneToOne(field_name) => {
                // Target field must exist and be a RecordLink
                let target = fields
                    .iter()
                    .find(|(id, _)| id.field_name.ends_with(field_name))
                    .ok_or_else(|| {
                        EvenframeError::Validation(format!(
                            "OneToOne: Field '{}' not found in coordination fields",
                            field_name
                        ))
                    })?;

                if !matches!(&target.1.field_type, FieldType::RecordLink(_)) {
                    return Err(EvenframeError::Validation(format!(
                        "OneToOne: Field '{}' must be a RecordLink type, got {:?}",
                        field_name, target.1.field_type
                    )));
                }
            }
        }

        Ok(())
    }
}

// Helper functions for type checking
#[cfg(feature = "schemasync")]
fn field_types_compatible(type1: &FieldType, type2: &FieldType) -> bool {
    match (type1, type2) {
        (FieldType::Option(inner1), FieldType::Option(inner2)) => {
            field_types_compatible(inner1, inner2)
        }
        (FieldType::Option(inner), other) | (other, FieldType::Option(inner)) => {
            field_types_compatible(inner, other)
        }
        (t1, t2) => t1 == t2,
    }
}

#[cfg(feature = "schemasync")]
fn is_numeric_type(field_type: &FieldType) -> bool {
    match field_type {
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
        | FieldType::Usize => true,
        FieldType::Option(inner) => is_numeric_type(inner),
        _ => false,
    }
}

/// Check if a type name represents a datetime-like type (used for foreign types that replaced FieldType::DateTime)
#[cfg(feature = "schemasync")]
fn is_datetime_like(name: &str) -> bool {
    name == "DateTime" || name.contains("DateTime")
}

#[cfg(feature = "schemasync")]
fn is_string_like(field_type: &FieldType) -> bool {
    match field_type {
        FieldType::String | FieldType::Char => true,
        FieldType::Option(inner) => is_string_like(inner),
        _ => false,
    }
}

#[cfg(feature = "schemasync")]
fn validate_string_field(
    fields: &[(CoordinationId, StructField)],
    field_name: &str,
    label: &str,
) -> Result<(), EvenframeError> {
    if !field_name.is_empty() {
        let field = fields
            .iter()
            .find(|(id, _)| id.field_name.ends_with(field_name))
            .ok_or_else(|| {
                EvenframeError::Validation(format!(
                    "Coherent dataset field '{}' ('{}') not found in coordination fields",
                    label, field_name
                ))
            })?;

        if !is_string_like(&field.1.field_type) {
            return Err(EvenframeError::Validation(format!(
                "Coherent dataset field '{}' must be string type, got {:?}",
                label, field.1.field_type
            )));
        }
    }
    Ok(())
}

#[cfg(feature = "schemasync")]
fn validate_numeric_field(
    fields: &[(CoordinationId, StructField)],
    field_name: &str,
    label: &str,
) -> Result<(), EvenframeError> {
    if !field_name.is_empty() {
        let field = fields
            .iter()
            .find(|(id, _)| id.field_name.ends_with(field_name))
            .ok_or_else(|| {
                EvenframeError::Validation(format!(
                    "Coherent dataset field '{}' ('{}') not found in coordination fields",
                    label, field_name
                ))
            })?;

        if !is_numeric_type(&field.1.field_type) {
            return Err(EvenframeError::Validation(format!(
                "Coherent dataset field '{}' must be numeric type, got {:?}",
                label, field.1.field_type
            )));
        }
    }
    Ok(())
}

#[cfg(feature = "schemasync")]
fn validate_datetime_field(
    fields: &[(CoordinationId, StructField)],
    field_name: &str,
    label: &str,
) -> Result<(), EvenframeError> {
    if !field_name.is_empty() {
        let field = fields
            .iter()
            .find(|(id, _)| id.field_name.ends_with(field_name))
            .ok_or_else(|| {
                EvenframeError::Validation(format!(
                    "Coherent dataset field '{}' ('{}') not found in coordination fields",
                    label, field_name
                ))
            })?;

        match date_kind(&field.1) {
            Some(_) => {}
            None => {
                return Err(EvenframeError::Validation(format!(
                    "Coherent dataset field '{}' must be a date or datetime, got {:?} with format {:?}",
                    label, field.1.field_type, field.1.format
                )));
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, TryFromExpr)]
pub enum CoordinateIncrement {
    Days(i32),
    Hours(i32),
    Minutes(i32),
    Numeric(f64),
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, TryFromExpr)]
pub enum DerivationType {
    Concatenate(String), // separator
    Extract(ExtractType),
    Transform(TransformType),
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, TryFromExpr)]
pub enum ExtractType {
    FirstWord,
    LastWord,
    Domain,   // from email
    Username, // from email
    Initials,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, TryFromExpr)]
pub enum TransformType {
    Uppercase,
    Lowercase,
    Capitalize,
    Truncate(usize),
    Hash,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize, TryFromExpr)]
pub enum CoherentDataset {
    Address {
        city: String,
        state: String,
        zip: String,
        country: String,
    },
    PersonName {
        first_name: String,
        last_name: String,
        full_name: String,
    },
    GeoLocation {
        latitude: String,
        longitude: String,
        city: String,
        country: String,
    },
    DateRange {
        start_date: String,
        end_date: String,
    },
    /// Generate random coordinates within a radius of a center point.
    GeoRadius {
        latitude: String,
        longitude: String,
        center_lat: f64,
        center_lng: f64,
        radius_km: f64,
    },
}

/// Trait for custom coordinators
pub trait CustomCoordinator: Send + Sync {
    fn generate(&self, fields: &[&str], index: usize) -> BTreeMap<String, String>;
}

// Extended address dataset with more US cities
pub const EXTENDED_ADDRESSES: &[(&str, &str, &str, &str)] = &[
    // Original addresses
    ("New York", "NY", "10001", "USA"),
    ("Los Angeles", "CA", "90001", "USA"),
    ("Chicago", "IL", "60601", "USA"),
    ("Houston", "TX", "77001", "USA"),
    ("Phoenix", "AZ", "85001", "USA"),
    ("Philadelphia", "PA", "19101", "USA"),
    ("San Antonio", "TX", "78201", "USA"),
    ("San Diego", "CA", "92101", "USA"),
    ("Dallas", "TX", "75201", "USA"),
    ("San Jose", "CA", "95101", "USA"),
    // Additional cities
    ("Austin", "TX", "78701", "USA"),
    ("Jacksonville", "FL", "32099", "USA"),
    ("Fort Worth", "TX", "76101", "USA"),
    ("Columbus", "OH", "43085", "USA"),
    ("Charlotte", "NC", "28201", "USA"),
    ("San Francisco", "CA", "94101", "USA"),
    ("Indianapolis", "IN", "46201", "USA"),
    ("Seattle", "WA", "98101", "USA"),
    ("Denver", "CO", "80201", "USA"),
    ("Boston", "MA", "02101", "USA"),
    ("El Paso", "TX", "79901", "USA"),
    ("Nashville", "TN", "37201", "USA"),
    ("Detroit", "MI", "48201", "USA"),
    ("Oklahoma City", "OK", "73101", "USA"),
    ("Portland", "OR", "97201", "USA"),
    ("Las Vegas", "NV", "89101", "USA"),
    ("Memphis", "TN", "38101", "USA"),
    ("Louisville", "KY", "40201", "USA"),
    ("Baltimore", "MD", "21201", "USA"),
    ("Milwaukee", "WI", "53201", "USA"),
    ("Albuquerque", "NM", "87101", "USA"),
    ("Tucson", "AZ", "85701", "USA"),
    ("Fresno", "CA", "93701", "USA"),
    ("Mesa", "AZ", "85201", "USA"),
    ("Sacramento", "CA", "94203", "USA"),
    ("Atlanta", "GA", "30301", "USA"),
    ("Kansas City", "MO", "64101", "USA"),
    ("Colorado Springs", "CO", "80901", "USA"),
    ("Omaha", "NE", "68101", "USA"),
    ("Raleigh", "NC", "27601", "USA"),
    ("Miami", "FL", "33101", "USA"),
    ("Long Beach", "CA", "90801", "USA"),
    ("Virginia Beach", "VA", "23450", "USA"),
    ("Oakland", "CA", "94601", "USA"),
    ("Minneapolis", "MN", "55401", "USA"),
    ("Tulsa", "OK", "74101", "USA"),
    ("Tampa", "FL", "33601", "USA"),
    ("Arlington", "TX", "76001", "USA"),
    ("New Orleans", "LA", "70112", "USA"),
];

// Extended geo location dataset with international cities
pub const EXTENDED_GEO_LOCATIONS: &[(&str, f64, f64, &str)] = &[
    // US Cities
    ("New York", 40.7128, -74.0060, "USA"),
    ("Los Angeles", 34.0522, -118.2437, "USA"),
    ("Chicago", 41.8781, -87.6298, "USA"),
    ("Houston", 29.7604, -95.3698, "USA"),
    ("Phoenix", 33.4484, -112.0740, "USA"),
    ("Philadelphia", 39.9526, -75.1652, "USA"),
    ("San Antonio", 29.4241, -98.4936, "USA"),
    ("San Diego", 32.7157, -117.1611, "USA"),
    ("Dallas", 32.7767, -96.7970, "USA"),
    ("San Jose", 37.3382, -121.8863, "USA"),
    ("Austin", 30.2672, -97.7431, "USA"),
    ("San Francisco", 37.7749, -122.4194, "USA"),
    ("Seattle", 47.6062, -122.3321, "USA"),
    ("Denver", 39.7392, -104.9903, "USA"),
    ("Boston", 42.3601, -71.0589, "USA"),
    ("Miami", 25.7617, -80.1918, "USA"),
    // International Cities
    ("London", 51.5074, -0.1278, "UK"),
    ("Paris", 48.8566, 2.3522, "France"),
    ("Tokyo", 35.6762, 139.6503, "Japan"),
    ("Sydney", -33.8688, 151.2093, "Australia"),
    ("Berlin", 52.5200, 13.4050, "Germany"),
    ("Madrid", 40.4168, -3.7038, "Spain"),
    ("Rome", 41.9028, 12.4964, "Italy"),
    ("Toronto", 43.6532, -79.3832, "Canada"),
    ("Amsterdam", 52.3676, 4.9041, "Netherlands"),
    ("Stockholm", 59.3293, 18.0686, "Sweden"),
    ("Oslo", 59.9139, 10.7522, "Norway"),
    ("Copenhagen", 55.6761, 12.5683, "Denmark"),
    ("Helsinki", 60.1699, 24.9384, "Finland"),
    ("Vienna", 48.2082, 16.3738, "Austria"),
    ("Prague", 50.0755, 14.4378, "Czech Republic"),
    ("Warsaw", 52.2297, 21.0122, "Poland"),
    ("Budapest", 47.4979, 19.0402, "Hungary"),
    ("Athens", 37.9838, 23.7275, "Greece"),
    ("Lisbon", 38.7223, -9.1393, "Portugal"),
    ("Dublin", 53.3498, -6.2603, "Ireland"),
    ("Brussels", 50.8503, 4.3517, "Belgium"),
    ("Zurich", 47.3769, 8.5417, "Switzerland"),
    ("Mumbai", 19.0760, 72.8777, "India"),
    ("Singapore", 1.3521, 103.8198, "Singapore"),
    ("Hong Kong", 22.3193, 114.1694, "Hong Kong"),
    ("Shanghai", 31.2304, 121.4737, "China"),
    ("Beijing", 39.9042, 116.4074, "China"),
    ("Seoul", 37.5665, 126.9780, "South Korea"),
    ("Bangkok", 13.7563, 100.5018, "Thailand"),
    ("Dubai", 25.2048, 55.2708, "UAE"),
    ("Cairo", 30.0444, 31.2357, "Egypt"),
    ("Istanbul", 41.0082, 28.9784, "Turkey"),
    ("Moscow", 55.7558, 37.6173, "Russia"),
    ("São Paulo", -23.5505, -46.6333, "Brazil"),
    ("Buenos Aires", -34.6037, -58.3816, "Argentina"),
    ("Mexico City", 19.4326, -99.1332, "Mexico"),
    ("Lima", -12.0464, -77.0428, "Peru"),
    ("Bogotá", 4.7110, -74.0721, "Colombia"),
    ("Santiago", -33.4489, -70.6693, "Chile"),
    ("Cape Town", -33.9249, 18.4241, "South Africa"),
    ("Johannesburg", -26.2041, 28.0473, "South Africa"),
    ("Lagos", 6.5244, 3.3792, "Nigeria"),
    ("Nairobi", -1.2921, 36.8219, "Kenya"),
];

// Extended person name combinations
pub const EXTENDED_PERSON_NAMES: &[(&str, &str, &str)] = &[
    // Common American names
    ("John", "Smith", "Male"),
    ("Jane", "Johnson", "Female"),
    ("Michael", "Williams", "Male"),
    ("Sarah", "Brown", "Female"),
    ("David", "Jones", "Male"),
    ("Emma", "Garcia", "Female"),
    ("James", "Miller", "Male"),
    ("Lisa", "Davis", "Female"),
    ("Robert", "Rodriguez", "Male"),
    ("Mary", "Martinez", "Female"),
    ("William", "Hernandez", "Male"),
    ("Patricia", "Lopez", "Female"),
    ("Richard", "Gonzalez", "Male"),
    ("Jennifer", "Wilson", "Female"),
    ("Thomas", "Anderson", "Male"),
    ("Linda", "Thomas", "Female"),
    ("Charles", "Taylor", "Male"),
    ("Elizabeth", "Moore", "Female"),
    ("Joseph", "Jackson", "Male"),
    ("Barbara", "Martin", "Female"),
    ("Christopher", "Lee", "Male"),
    ("Susan", "Perez", "Female"),
    ("Daniel", "Thompson", "Male"),
    ("Jessica", "White", "Female"),
    ("Matthew", "Harris", "Male"),
    ("Karen", "Sanchez", "Female"),
    ("Anthony", "Clark", "Male"),
    ("Nancy", "Ramirez", "Female"),
    ("Mark", "Lewis", "Male"),
    ("Betty", "Robinson", "Female"),
    ("Donald", "Walker", "Male"),
    ("Helen", "Young", "Female"),
    ("Kenneth", "Allen", "Male"),
    ("Sandra", "King", "Female"),
    ("Steven", "Wright", "Male"),
    ("Donna", "Scott", "Female"),
    ("Paul", "Torres", "Male"),
    ("Carol", "Nguyen", "Female"),
    ("Joshua", "Hill", "Male"),
    ("Michelle", "Flores", "Female"),
    ("Andrew", "Green", "Male"),
    ("Laura", "Adams", "Female"),
    ("George", "Nelson", "Male"),
    ("Dorothy", "Baker", "Female"),
    ("Kevin", "Hall", "Male"),
    ("Maria", "Rivera", "Female"),
    ("Brian", "Campbell", "Male"),
    ("Amy", "Mitchell", "Female"),
    ("Edward", "Carter", "Male"),
    ("Shirley", "Roberts", "Female"),
];

// Financial test scenarios
pub const FINANCIAL_SCENARIOS: &[(&str, f64, f64, f64)] = &[
    ("Retail Purchase", 99.99, 8.00, 107.99),
    ("Restaurant Bill", 85.00, 6.80, 91.80),
    ("Online Order", 249.99, 20.00, 269.99),
    ("Grocery Shopping", 156.43, 12.51, 168.94),
    ("Electronics", 599.00, 47.92, 646.92),
    ("Clothing", 125.50, 10.04, 135.54),
    ("Books", 45.99, 3.68, 49.67),
    ("Subscription", 19.99, 1.60, 21.59),
    ("Software License", 299.00, 23.92, 322.92),
    ("Hardware", 1299.00, 103.92, 1402.92),
    ("Office Supplies", 67.89, 5.43, 73.32),
    ("Fuel Purchase", 45.00, 3.60, 48.60),
    ("Pharmacy", 34.99, 2.80, 37.79),
    ("Entertainment", 150.00, 12.00, 162.00),
    ("Home Improvement", 425.00, 34.00, 459.00),
    ("Automotive Parts", 189.99, 15.20, 205.19),
    ("Pet Supplies", 78.50, 6.31, 84.78),
    ("Sports Equipment", 299.99, 24.00, 323.99),
    ("Garden Supplies", 112.50, 9.00, 121.50),
    ("Art Supplies", 89.99, 7.20, 97.19),
    ("Musical Instruments", 699.00, 55.92, 754.92),
    ("Furniture", 899.00, 71.92, 970.92),
    ("Appliances", 1599.00, 127.92, 1726.92),
    ("Jewelry", 450.00, 36.00, 486.00),
    ("Cosmetics", 65.50, 5.24, 70.74),
    ("Toys", 39.99, 3.20, 43.19),
    ("Video Games", 59.99, 4.80, 64.79),
    ("Streaming Service", 14.99, 1.20, 16.19),
    ("Cloud Storage", 9.99, 0.80, 10.79),
    ("Phone Bill", 85.00, 6.80, 91.80),
];

// Date range test scenarios
pub const DATE_RANGE_SCENARIOS: &[(&str, i64, &str)] = &[
    ("Sprint", 14, "2 week sprint"),
    ("Quarter", 90, "Financial quarter"),
    ("Semester", 120, "Academic semester"),
    ("Project Phase", 30, "Monthly phase"),
    ("Trial Period", 7, "Weekly trial"),
    ("Contract", 365, "Annual contract"),
    ("Warranty", 730, "2-year warranty"),
    ("Subscription", 30, "Monthly subscription"),
    ("Campaign", 45, "Marketing campaign"),
    ("Event", 3, "3-day event"),
    ("Weekend", 2, "Weekend getaway"),
    ("Workweek", 5, "Business week"),
    ("Fortnight", 14, "Two weeks"),
    ("Billing Cycle", 30, "Monthly billing"),
    ("Academic Year", 280, "School year"),
    ("Summer Break", 90, "Summer vacation"),
    ("Holiday Season", 45, "Holiday period"),
    ("Training Program", 60, "2-month training"),
    ("Probation Period", 90, "3-month probation"),
    ("Notice Period", 30, "1-month notice"),
    ("Lease Term", 365, "1-year lease"),
    ("Conference", 4, "4-day conference"),
    ("Workshop", 1, "1-day workshop"),
    ("Internship", 90, "3-month internship"),
    ("Product Launch", 21, "3-week launch"),
    ("Beta Test", 30, "1-month beta"),
    ("Evaluation Period", 15, "2-week evaluation"),
    ("Certification", 180, "6-month certification"),
    ("Membership", 365, "Annual membership"),
    ("Insurance Term", 180, "6-month term"),
];

// Company and job title combinations
pub const COMPANY_JOB_COMBINATIONS: &[(&str, &str, &str)] = &[
    ("TechCorp Inc", "Software Engineer", "Technology"),
    ("DataSoft LLC", "Data Scientist", "Analytics"),
    ("CloudVision Corp", "DevOps Engineer", "Infrastructure"),
    ("InnovateTech Ltd", "Product Manager", "Product"),
    ("NextGen Co", "UX Designer", "Design"),
    ("ProSystems Inc", "Backend Developer", "Engineering"),
    ("Digital Solutions LLC", "Frontend Developer", "Engineering"),
    ("Global Analytics Corp", "Business Analyst", "Business"),
    ("Enterprise Systems Ltd", "System Administrator", "IT"),
    ("Creative Studios Co", "Graphic Designer", "Design"),
    ("Marketing Plus Inc", "Marketing Manager", "Marketing"),
    ("Sales Force LLC", "Sales Representative", "Sales"),
    ("Finance Pro Corp", "Financial Analyst", "Finance"),
    ("Legal Associates Ltd", "Legal Counsel", "Legal"),
    ("HR Solutions Co", "HR Manager", "Human Resources"),
    ("Operations Hub Inc", "Operations Manager", "Operations"),
    ("Quality First LLC", "QA Engineer", "Quality"),
    ("Security Shield Corp", "Security Analyst", "Security"),
    ("Mobile Apps Ltd", "Mobile Developer", "Engineering"),
    ("AI Innovations Co", "ML Engineer", "AI/ML"),
    ("Web Dynamics Inc", "Full Stack Developer", "Engineering"),
    ("Data Insights LLC", "Data Engineer", "Analytics"),
    ("Cloud Native Corp", "Cloud Architect", "Infrastructure"),
    ("Product Vision Ltd", "Product Designer", "Design"),
    ("Tech Startup Co", "CTO", "Executive"),
    (
        "Enterprise Cloud Inc",
        "Solutions Architect",
        "Architecture",
    ),
    ("Digital Marketing LLC", "SEO Specialist", "Marketing"),
    ("Sales Tech Corp", "Account Executive", "Sales"),
    ("FinTech Solutions Ltd", "Blockchain Developer", "Finance"),
    ("Legal Tech Co", "Compliance Officer", "Legal"),
    ("People First Inc", "Talent Acquisition", "Human Resources"),
    ("Supply Chain LLC", "Logistics Manager", "Operations"),
    ("Test Automation Corp", "SDET", "Quality"),
    ("CyberSec Ltd", "Penetration Tester", "Security"),
    ("App Innovations Co", "iOS Developer", "Engineering"),
    ("Research Labs Inc", "Research Scientist", "R&D"),
    (
        "Platform Solutions LLC",
        "Platform Engineer",
        "Infrastructure",
    ),
    ("Growth Hacking Corp", "Growth Manager", "Marketing"),
    (
        "Customer Success Ltd",
        "Customer Success Manager",
        "Support",
    ),
    ("Tech Consulting Co", "Technical Consultant", "Consulting"),
];

// Product catalog test data
pub const PRODUCT_CATALOG: &[(&str, &str, f64, &str)] = &[
    ("Premium Widget", "WDG-001", 29.99, "Electronics"),
    ("Deluxe Gadget", "GDG-002", 49.99, "Electronics"),
    ("Pro Device", "DEV-003", 99.99, "Hardware"),
    ("Ultra Tool", "TUL-004", 19.99, "Tools"),
    ("Super System", "SYS-005", 299.99, "Software"),
    ("Advanced Platform", "PLT-006", 499.99, "Software"),
    ("Professional Solution", "SOL-007", 999.99, "Enterprise"),
    ("Basic Widget", "WDG-008", 9.99, "Electronics"),
    ("Standard Gadget", "GDG-009", 24.99, "Electronics"),
    ("Essential Device", "DEV-010", 39.99, "Hardware"),
    ("Smart Sensor", "SNS-011", 79.99, "IoT"),
    ("Power Bank", "PWR-012", 34.99, "Accessories"),
    ("Wireless Charger", "CHG-013", 44.99, "Accessories"),
    ("USB Hub", "HUB-014", 24.99, "Accessories"),
    ("Memory Card", "MEM-015", 19.99, "Storage"),
    ("External Drive", "DRV-016", 89.99, "Storage"),
    ("Network Switch", "NET-017", 149.99, "Networking"),
    ("Router Pro", "RTR-018", 199.99, "Networking"),
    ("Security Camera", "CAM-019", 129.99, "Security"),
    ("Smart Lock", "LCK-020", 249.99, "Security"),
    ("Development Kit", "DEV-021", 399.99, "Development"),
    ("API Gateway", "API-022", 599.99, "Software"),
    ("Database Tool", "DBT-023", 799.99, "Software"),
    ("Analytics Suite", "ANL-024", 1299.99, "Enterprise"),
    ("Monitoring System", "MON-025", 899.99, "Enterprise"),
    ("Backup Solution", "BKP-026", 499.99, "Software"),
    ("Cloud Service", "CLD-027", 299.99, "Services"),
    ("Support Package", "SUP-028", 199.99, "Services"),
    ("Training Course", "TRN-029", 399.99, "Education"),
    ("Certification Exam", "CRT-030", 299.99, "Education"),
];

#[cfg(all(test, feature = "surrealdb"))]
mod tests {
    use super::*;

    fn field(field_type: FieldType, format: Option<Format>) -> StructField {
        StructField {
            field_name: "f".to_string(),
            field_type,
            format,
            ..Default::default()
        }
    }

    #[test]
    fn coordinated_literals_match_field_types() {
        assert_eq!(
            coordinated_literal(&field(FieldType::I32, None), "41.6"),
            "42"
        );
        assert_eq!(
            coordinated_literal(
                &field(FieldType::Option(Box::new(FieldType::F64)), None),
                "40.7128"
            ),
            "40.7128f"
        );
        assert_eq!(
            coordinated_literal(&field(FieldType::String, None), "O'Brien \\ Co"),
            "'O\\'Brien \\\\ Co'"
        );
        assert_eq!(
            coordinated_literal(&field(FieldType::String, Some(Format::Date)), "2024-01-08"),
            "'2024-01-08'"
        );
        assert_eq!(
            coordinated_literal(
                &field(
                    FieldType::Other("DateTime".to_string()),
                    Some(Format::DateTime)
                ),
                "2024-01-08T00:00:00Z"
            ),
            "d'2024-01-08T00:00:00Z'"
        );
        // A non-numeric value for a numeric field falls back to a string
        assert_eq!(
            coordinated_literal(&field(FieldType::I32, None), "n/a"),
            "'n/a'"
        );
    }

    #[test]
    fn literal_to_raw_round_trips() {
        for raw in ["plain", "O'Brien \\ Co", ""] {
            let literal = coordinated_literal(&field(FieldType::String, None), raw);
            assert_eq!(literal_to_raw(&literal), raw);
        }
        assert_eq!(
            literal_to_raw("d'2024-01-08T00:00:00Z'"),
            "2024-01-08T00:00:00Z"
        );
        assert_eq!(literal_to_raw("42"), "42");
    }

    /// Haversine distance in km between two (lat, lng) points in degrees.
    fn haversine_km(lat1: f64, lng1: f64, lat2: f64, lng2: f64) -> f64 {
        let r = 6371.0_f64;
        let dlat = (lat2 - lat1).to_radians();
        let dlng = (lng2 - lng1).to_radians();
        let a = (dlat / 2.0).sin().powi(2)
            + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlng / 2.0).sin().powi(2);
        r * 2.0 * a.sqrt().asin()
    }

    #[test]
    fn test_geo_radius_generates_within_radius() {
        let dataset = CoherentDataset::GeoRadius {
            latitude: "lat".to_string(),
            longitude: "lng".to_string(),
            center_lat: 52.52,
            center_lng: 13.405,
            radius_km: 25.0,
        };

        for i in 0..100 {
            let values = Mockmaker::generate_coherent_values(&dataset, i);
            let lat: f64 = values["lat"].parse().expect("lat should be a valid f64");
            let lng: f64 = values["lng"].parse().expect("lng should be a valid f64");

            assert!((-90.0..=90.0).contains(&lat), "lat out of range: {}", lat);
            assert!((-180.0..=180.0).contains(&lng), "lng out of range: {}", lng);

            let dist = haversine_km(52.52, 13.405, lat, lng);
            assert!(
                dist <= 25.5, // small tolerance for floating point
                "Point ({}, {}) is {:.2}km from center, expected <= 25km",
                lat,
                lng,
                dist
            );
        }
    }

    #[test]
    fn test_geo_radius_produces_varied_output() {
        let dataset = CoherentDataset::GeoRadius {
            latitude: "lat".to_string(),
            longitude: "lng".to_string(),
            center_lat: 0.0,
            center_lng: 0.0,
            radius_km: 100.0,
        };

        let mut lats = std::collections::BTreeSet::new();
        for i in 0..20 {
            let values = Mockmaker::generate_coherent_values(&dataset, i);
            lats.insert(values["lat"].clone());
        }
        assert!(
            lats.len() > 1,
            "GeoRadius should produce varied coordinates, got {} unique values",
            lats.len()
        );
    }
}

/// The path generated code names coordination types by.
fn coordinate_path(name: &str) -> proc_macro2::TokenStream {
    let ident = proc_macro2::Ident::new(name, proc_macro2::Span::call_site());
    quote::quote! { ::evenframe::schemasync::coordinate::#ident }
}

impl quote::ToTokens for Coordination {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let ty = coordinate_path("Coordination");
        tokens.extend(match self {
            Coordination::InitializeEqual(fields) => {
                quote::quote! { #ty::InitializeEqual(vec![#(#fields.to_string()),*]) }
            }
            Coordination::InitializeSequential {
                field_names,
                increment,
            } => quote::quote! {
                #ty::InitializeSequential {
                    field_names: vec![#(#field_names.to_string()),*],
                    increment: #increment,
                }
            },
            Coordination::InitializeSum { field_names, total } => quote::quote! {
                #ty::InitializeSum {
                    field_names: vec![#(#field_names.to_string()),*],
                    total: #total,
                }
            },
            Coordination::InitializeDerive {
                source_field_names,
                target_field_name,
                derivation,
            } => quote::quote! {
                #ty::InitializeDerive {
                    source_field_names: vec![#(#source_field_names.to_string()),*],
                    target_field_name: #target_field_name.to_string(),
                    derivation: #derivation,
                }
            },
            Coordination::InitializeCoherent(dataset) => {
                quote::quote! { #ty::InitializeCoherent(#dataset) }
            }
            Coordination::OneToOne(field) => {
                quote::quote! { #ty::OneToOne(#field.to_string()) }
            }
        });
    }
}

impl quote::ToTokens for CoordinateIncrement {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let ty = coordinate_path("CoordinateIncrement");
        tokens.extend(match self {
            CoordinateIncrement::Days(n) => quote::quote! { #ty::Days(#n) },
            CoordinateIncrement::Hours(n) => quote::quote! { #ty::Hours(#n) },
            CoordinateIncrement::Minutes(n) => quote::quote! { #ty::Minutes(#n) },
            CoordinateIncrement::Numeric(n) => quote::quote! { #ty::Numeric(#n) },
        });
    }
}

impl quote::ToTokens for DerivationType {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let ty = coordinate_path("DerivationType");
        tokens.extend(match self {
            DerivationType::Concatenate(separator) => {
                quote::quote! { #ty::Concatenate(#separator.to_string()) }
            }
            DerivationType::Extract(extract) => {
                let variant = proc_macro2::Ident::new(
                    match extract {
                        ExtractType::FirstWord => "FirstWord",
                        ExtractType::LastWord => "LastWord",
                        ExtractType::Domain => "Domain",
                        ExtractType::Username => "Username",
                        ExtractType::Initials => "Initials",
                    },
                    proc_macro2::Span::call_site(),
                );
                let extract_ty = coordinate_path("ExtractType");
                quote::quote! { #ty::Extract(#extract_ty::#variant) }
            }
            DerivationType::Transform(transform) => {
                let transform_ty = coordinate_path("TransformType");
                let transform = match transform {
                    TransformType::Uppercase => quote::quote! { #transform_ty::Uppercase },
                    TransformType::Lowercase => quote::quote! { #transform_ty::Lowercase },
                    TransformType::Capitalize => quote::quote! { #transform_ty::Capitalize },
                    TransformType::Truncate(n) => quote::quote! { #transform_ty::Truncate(#n) },
                    TransformType::Hash => quote::quote! { #transform_ty::Hash },
                };
                quote::quote! { #ty::Transform(#transform) }
            }
        });
    }
}

impl quote::ToTokens for CoherentDataset {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let ty = coordinate_path("CoherentDataset");
        tokens.extend(match self {
            CoherentDataset::Address {
                city,
                state,
                zip,
                country,
            } => quote::quote! {
                #ty::Address {
                    city: #city.to_string(),
                    state: #state.to_string(),
                    zip: #zip.to_string(),
                    country: #country.to_string(),
                }
            },
            CoherentDataset::PersonName {
                first_name,
                last_name,
                full_name,
            } => quote::quote! {
                #ty::PersonName {
                    first_name: #first_name.to_string(),
                    last_name: #last_name.to_string(),
                    full_name: #full_name.to_string(),
                }
            },
            CoherentDataset::GeoLocation {
                latitude,
                longitude,
                city,
                country,
            } => quote::quote! {
                #ty::GeoLocation {
                    latitude: #latitude.to_string(),
                    longitude: #longitude.to_string(),
                    city: #city.to_string(),
                    country: #country.to_string(),
                }
            },
            CoherentDataset::DateRange {
                start_date,
                end_date,
            } => quote::quote! {
                #ty::DateRange {
                    start_date: #start_date.to_string(),
                    end_date: #end_date.to_string(),
                }
            },
            CoherentDataset::GeoRadius {
                latitude,
                longitude,
                center_lat,
                center_lng,
                radius_km,
            } => quote::quote! {
                #ty::GeoRadius {
                    latitude: #latitude.to_string(),
                    longitude: #longitude.to_string(),
                    center_lat: #center_lat,
                    center_lng: #center_lng,
                    radius_km: #radius_km,
                }
            },
        });
    }
}
