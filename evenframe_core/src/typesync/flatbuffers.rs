//! FlatBuffers schema generation with validator attributes.
//!
//! This module generates FlatBuffers schema files (.fbs) with
//! `(validate: "...")` attributes at the field level for validators.

use crate::error::{EvenframeError, Result};
use crate::schemasync::format::Format;
use crate::types::{FieldType, StructConfig, StructField, TaggedUnion, VariantData};
use crate::typesync::doc_comment::format_triple_slash;
use crate::typesync::map_key::MapKey;
use crate::validator::{
    ArrayValidator, BigDecimalValidator, BigIntValidator, DateValidator, DurationValidator,
    NumberValidator, StringValidator, Validator,
};
use convert_case::{Case, Casing};
use std::collections::{BTreeMap, BTreeSet};

/// FlatBuffers' scalar types, which alone can be optional with `= null`.
const SCALARS: [&str; 11] = [
    "bool", "int8", "uint8", "int16", "uint16", "int32", "uint32", "int64", "uint64", "float",
    "double",
];

/// Main entry point for generating FlatBuffers schema.
pub fn generate_flatbuffers_schema_string(
    structs: &BTreeMap<String, StructConfig>,
    enums: &BTreeMap<String, TaggedUnion>,
    namespace: Option<&str>,
    registry: &crate::types::ForeignTypeRegistry,
) -> Result<String> {
    tracing::info!(
        struct_count = structs.len(),
        enum_count = enums.len(),
        "Generating FlatBuffers schema"
    );

    // Deduplicate structs by PascalCase name
    let mut seen_structs = BTreeSet::new();
    let unique_structs: Vec<&StructConfig> = structs
        .values()
        .filter(|s| {
            // `resolve_only` types are registered for resolution but not emitted.
            if s.resolve_only {
                return false;
            }
            let name = s.struct_name.to_case(Case::Pascal);
            if seen_structs.contains(&name) {
                false
            } else {
                seen_structs.insert(name);
                true
            }
        })
        .collect();

    // Deduplicate enums by PascalCase name
    let mut seen_enums = BTreeSet::new();
    let unique_enums: Vec<&TaggedUnion> = enums
        .values()
        .filter(|e| {
            if e.resolve_only {
                return false;
            }
            let name = e.enum_name.to_case(Case::Pascal);
            if seen_enums.contains(&name) {
                false
            } else {
                seen_enums.insert(name);
                true
            }
        })
        .collect();

    let mut fbs = Fbs {
        registry,
        declared: seen_structs.union(&seen_enums).cloned().collect(),
        scalar_enums: unique_enums
            .iter()
            .map(|e| e.effective())
            .filter(|e| e.variants.iter().all(|v| v.effective().data.is_none()))
            .map(|e| e.enum_name.to_case(Case::Pascal))
            .collect(),
        definitions: Vec::new(),
        uses_validate: false,
    };

    let mut body = String::new();
    // Generate enums first (they may be referenced by tables)
    for enum_def in &unique_enums {
        body.push_str(&fbs.enum_definition(enum_def.effective())?);
        body.push('\n');
    }
    // Generate tables
    for struct_config in &unique_structs {
        let struct_config = struct_config.effective();
        let name = struct_config.struct_name.to_case(Case::Pascal);
        if let Some(ref doc) = struct_config.doccom {
            body.push_str(&format_triple_slash(doc, ""));
        }
        body.push_str(&fbs.table(&name, &struct_config.fields)?);
        body.push('\n');
    }

    let mut output = String::new();
    if let Some(ns) = namespace {
        output.push_str(&format!("namespace {};\n\n", ns));
    }
    if fbs.uses_validate {
        output.push_str("attribute \"validate\";\n\n");
    }
    output.push_str(&body);
    for definition in &fbs.definitions {
        output.push_str(definition);
        output.push('\n');
    }

    tracing::info!(
        output_length = output.len(),
        "FlatBuffers schema generation complete"
    );
    Ok(output)
}

/// A table field's type, and `= null` when it is an optional scalar.
struct FbsField {
    type_name: String,
    optional_scalar: bool,
}

impl FbsField {
    fn declaration(&self, name: &str) -> String {
        let default = if self.optional_scalar { " = null" } else { "" };
        format!("{name}: {}{default}", self.type_name)
    }
}

/// Renders FlatBuffers definitions. A shape FlatBuffers has no syntax for is
/// held in a table named after where it appears, declared after the scanned
/// types.
struct Fbs<'a> {
    registry: &'a crate::types::ForeignTypeRegistry,
    /// Every type name already taken, scanned or generated.
    declared: BTreeSet<String>,
    /// Scanned enums of unit variants, which are FlatBuffers scalars.
    scalar_enums: BTreeSet<String>,
    definitions: Vec<String>,
    uses_validate: bool,
}

impl Fbs<'_> {
    /// Takes a generated type name, rejecting one already taken.
    fn claim(&mut self, name: &str) -> Result<()> {
        if self.declared.insert(name.to_string()) {
            return Ok(());
        }
        Err(EvenframeError::type_sync(format!(
            "the FlatBuffers table `{name}` generated for a field shape collides with a type of \
             the same name; rename the type, field or variant it comes from"
        )))
    }

    /// Declares a generated table named `name` with `fields`.
    fn declare(&mut self, name: &str, fields: &[StructField]) -> Result<()> {
        self.claim(name)?;
        let table = self.table(name, fields)?;
        self.definitions.push(table);
        Ok(())
    }

    /// A table named `name` with `fields`.
    fn table(&mut self, name: &str, fields: &[StructField]) -> Result<String> {
        let mut output = format!("table {name} {{\n");
        for field in fields {
            let field = field.effective();
            if let Some(ref doc) = field.doccom {
                output.push_str(&format_triple_slash(doc, "    "));
            }
            let fbs_field = self.field(
                &field.field_type,
                &format!("{name}{}", field.field_name.to_case(Case::Pascal)),
            )?;
            output.push_str(&format!(
                "    {}",
                fbs_field.declaration(&field.field_name.to_case(Case::Snake))
            ));
            let validators = collect_validators_for_field(&field.validators);
            if !validators.is_empty() {
                self.uses_validate = true;
                output.push_str(&format!(" (validate: \"{validators}\")"));
            }
            output.push_str(";\n");
        }
        output.push_str("}\n");
        Ok(output)
    }

    /// A table field of `field_type`. `hint` names any table it needs.
    fn field(&mut self, field_type: &FieldType, hint: &str) -> Result<FbsField> {
        match field_type {
            // Table, string and vector fields are optional already; an
            // `Option<Option<T>>` needs a table to tell its two `None`s apart.
            FieldType::Option(inner) if matches!(**inner, FieldType::Option(_)) => Ok(FbsField {
                type_name: self.wrapper(inner, hint)?,
                optional_scalar: false,
            }),
            FieldType::Option(inner) => {
                let type_name = self.value(inner, hint)?;
                Ok(FbsField {
                    optional_scalar: SCALARS.contains(&type_name.as_str())
                        || self.scalar_enums.contains(&type_name),
                    type_name,
                })
            }
            _ => Ok(FbsField {
                type_name: self.value(field_type, hint)?,
                optional_scalar: false,
            }),
        }
    }

    /// `field_type` as a present value.
    fn value(&mut self, field_type: &FieldType, hint: &str) -> Result<String> {
        Ok(match field_type {
            FieldType::String | FieldType::Char => "string".to_string(),
            FieldType::Bool => "bool".to_string(),
            FieldType::F32 => "float".to_string(),
            FieldType::F64 => "double".to_string(),
            FieldType::I8 => "int8".to_string(),
            FieldType::I16 => "int16".to_string(),
            FieldType::I32 => "int32".to_string(),
            FieldType::I64 | FieldType::Isize => "int64".to_string(),
            FieldType::U8 => "uint8".to_string(),
            FieldType::U16 => "uint16".to_string(),
            FieldType::U32 => "uint32".to_string(),
            FieldType::U64 | FieldType::Usize => "uint64".to_string(),
            // FlatBuffers has no 128-bit integers; the decimal text is exact.
            FieldType::I128 | FieldType::U128 => "string".to_string(),
            FieldType::Option(_) => self.wrapper(field_type, hint)?,
            FieldType::Unit => {
                self.declare(hint, &[])?;
                hint.to_string()
            }
            FieldType::Vec(inner) => format!("[{}]", self.element(inner, &format!("{hint}Item"))?),
            FieldType::HashMap(key, value) | FieldType::BTreeMap(key, value) => {
                // A map is a vector of entry tables sorted by key.
                let entry = format!("{hint}Entry");
                self.claim(&entry)?;
                let key_type = self.map_key(key)?;
                let value_declaration = self
                    .field(value, &format!("{hint}Value"))?
                    .declaration("value");
                self.definitions.push(format!(
                    "table {entry} {{\n    key: {key_type} (key);\n    {value_declaration};\n}}\n"
                ));
                format!("[{entry}]")
            }
            FieldType::Tuple(items) => {
                let fields: Vec<StructField> = items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| StructField {
                        field_name: format!("item{index}"),
                        field_type: item.clone(),
                        ..Default::default()
                    })
                    .collect();
                self.declare(hint, &fields)?;
                hint.to_string()
            }
            FieldType::Struct(members) => {
                let fields: Vec<StructField> = members
                    .iter()
                    .map(|(name, member)| StructField {
                        field_name: name.clone(),
                        field_type: member.clone(),
                        ..Default::default()
                    })
                    .collect();
                self.declare(hint, &fields)?;
                hint.to_string()
            }
            FieldType::Duration => self.value(&FieldType::serde_duration(), hint)?,
            FieldType::RecordLink(inner) => self.value(inner, hint)?,
            FieldType::Other(type_name) => match self.registry.lookup(type_name) {
                Some(foreign) if !foreign.flatbuffers.is_empty() => foreign.flatbuffers.clone(),
                _ => type_name.to_case(Case::Pascal),
            },
        })
    }

    /// A vector element. A vector cannot hold a vector, a map (a vector of
    /// entries) or a missing value, so those are held in a table.
    fn element(&mut self, field_type: &FieldType, hint: &str) -> Result<String> {
        match field_type {
            FieldType::Vec(_)
            | FieldType::HashMap(..)
            | FieldType::BTreeMap(..)
            | FieldType::Option(_) => self.wrapper(field_type, hint),
            _ => self.value(field_type, hint),
        }
    }

    /// A table holding `field_type` as its one `value` field.
    fn wrapper(&mut self, field_type: &FieldType, hint: &str) -> Result<String> {
        let value = StructField {
            field_name: "value".to_string(),
            field_type: field_type.clone(),
            ..Default::default()
        };
        self.declare(hint, std::slice::from_ref(&value))?;
        Ok(hint.to_string())
    }

    /// A map key as the scalar or string an entry table's `(key)` field holds.
    fn map_key(&mut self, key: &FieldType) -> Result<String> {
        match MapKey::require(key)? {
            MapKey::Named(name) => match self.registry.lookup(name) {
                Some(foreign)
                    if foreign.flatbuffers == "string"
                        || SCALARS.contains(&foreign.flatbuffers.as_str()) =>
                {
                    Ok(foreign.flatbuffers.clone())
                }
                Some(foreign) => Err(EvenframeError::type_sync(format!(
                    "map key `{name}` maps to the FlatBuffers type {:?}, but a key must be a \
                     scalar or a string",
                    foreign.flatbuffers
                ))),
                None => Ok(name.to_case(Case::Pascal)),
            },
            MapKey::Text | MapKey::Char | MapKey::Integer | MapKey::Bool => self.value(key, ""),
        }
    }

    /// An enum: a FlatBuffers enum when every variant is a unit variant,
    /// else a union of one table per variant.
    fn enum_definition(&mut self, enum_def: &TaggedUnion) -> Result<String> {
        let name = enum_def.enum_name.to_case(Case::Pascal);
        let mut output = String::new();
        if let Some(ref doc) = enum_def.doccom {
            output.push_str(&format_triple_slash(doc, ""));
        }
        let variants: Vec<_> = enum_def.variants.iter().map(|v| v.effective()).collect();

        if self.scalar_enums.contains(&name) {
            let underlying = match variants.len() {
                0..=256 => "ubyte",
                257..=65_536 => "ushort",
                _ => "uint",
            };
            let values = variants
                .iter()
                .enumerate()
                .map(|(index, variant)| format!("    {} = {index}", variant.name))
                .collect::<Vec<_>>()
                .join(",\n");
            output.push_str(&format!("enum {name} : {underlying} {{\n{values}\n}}\n"));
            return Ok(output);
        }

        // A union holds only tables, so each variant's payload is a table.
        let mut members = Vec::new();
        for variant in &variants {
            let table = format!("{name}{}", variant.name.to_case(Case::Pascal));
            match &variant.data {
                None => self.declare(&table, &[])?,
                Some(VariantData::InlineStruct(inline)) => self.declare(&table, &inline.fields)?,
                Some(VariantData::DataStructureRef(field_type)) => {
                    let value = StructField {
                        field_name: "value".to_string(),
                        field_type: field_type.clone(),
                        ..Default::default()
                    };
                    self.declare(&table, std::slice::from_ref(&value))?;
                }
            }
            members.push(format!("    {table}"));
        }
        output.push_str(&format!("union {name} {{\n{}\n}}\n", members.join(",\n")));
        Ok(output)
    }
}

/// Collect validators and format them as a comma-separated string for FlatBuffers attributes.
fn collect_validators_for_field(validators: &[Validator]) -> String {
    validators
        .iter()
        .filter_map(validator_to_flatbuffers_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Convert a Validator to its FlatBuffers attribute string representation.
/// Unlike macroforge, this includes ALL validators (including transformations).
fn validator_to_flatbuffers_string(validator: &Validator) -> Option<String> {
    match validator {
        Validator::StringValidator(sv) => string_validator_to_flatbuffers(sv),
        Validator::NumberValidator(nv) => number_validator_to_flatbuffers(nv),
        Validator::ArrayValidator(av) => array_validator_to_flatbuffers(av),
        Validator::DateValidator(dv) => date_validator_to_flatbuffers(dv),
        Validator::BigIntValidator(biv) => bigint_validator_to_flatbuffers(biv),
        Validator::BigDecimalValidator(bdv) => bigdecimal_validator_to_flatbuffers(bdv),
        Validator::DurationValidator(dv) => duration_validator_to_flatbuffers(dv),
    }
}

fn string_validator_to_flatbuffers(sv: &StringValidator) -> Option<String> {
    match sv {
        // Basic type validators
        StringValidator::String => Some("string".to_string()),
        StringValidator::Alpha => Some("alpha".to_string()),
        StringValidator::Alphanumeric => Some("alphanumeric".to_string()),
        StringValidator::Base64 => Some("base64".to_string()),
        StringValidator::Base64Url => Some("base64Url".to_string()),
        StringValidator::CreditCard => Some("creditCard".to_string()),
        StringValidator::Digits => Some("digits".to_string()),
        StringValidator::Email => Some("email".to_string()),
        StringValidator::Hex => Some("hex".to_string()),
        StringValidator::Integer => Some("integer".to_string()),
        StringValidator::Ip => Some("ip".to_string()),
        StringValidator::IpV4 => Some("ipv4".to_string()),
        StringValidator::IpV6 => Some("ipv6".to_string()),
        StringValidator::Json => Some("json".to_string()),
        StringValidator::Numeric => Some("numeric".to_string()),
        StringValidator::Regex => Some("regex".to_string()),
        StringValidator::Semver => Some("semver".to_string()),
        StringValidator::Url => Some("url".to_string()),

        // Date validators
        StringValidator::Date => Some("date".to_string()),
        StringValidator::DateEpoch => Some("dateEpoch".to_string()),
        StringValidator::DateIso => Some("dateIso".to_string()),

        // UUID validators
        StringValidator::Uuid => Some("uuid".to_string()),
        StringValidator::UuidV1 => Some("uuidV1".to_string()),
        StringValidator::UuidV2 => Some("uuidV2".to_string()),
        StringValidator::UuidV3 => Some("uuidV3".to_string()),
        StringValidator::UuidV4 => Some("uuidV4".to_string()),
        StringValidator::UuidV5 => Some("uuidV5".to_string()),
        StringValidator::UuidV6 => Some("uuidV6".to_string()),
        StringValidator::UuidV7 => Some("uuidV7".to_string()),
        StringValidator::UuidV8 => Some("uuidV8".to_string()),

        // Length validators
        StringValidator::MinLength(n) => Some(format!("minLength({})", n)),
        StringValidator::MaxLength(n) => Some(format!("maxLength({})", n)),
        StringValidator::Length(n) => Some(format!("length({})", n)),
        StringValidator::NonEmpty => Some("nonEmpty".to_string()),

        // Case/state validators
        StringValidator::Lowercased | StringValidator::LowerPreformatted => {
            Some("lowercase".to_string())
        }
        StringValidator::Uppercased | StringValidator::UpperPreformatted => {
            Some("uppercase".to_string())
        }
        StringValidator::Trimmed | StringValidator::TrimPreformatted => Some("trimmed".to_string()),
        StringValidator::Capitalized | StringValidator::CapitalizePreformatted => {
            Some("capitalized".to_string())
        }
        StringValidator::Uncapitalized => Some("uncapitalized".to_string()),

        // Transformation validators (INCLUDED per requirements)
        StringValidator::Capitalize => Some("capitalize".to_string()),
        StringValidator::Lower => Some("lower".to_string()),
        StringValidator::Upper => Some("upper".to_string()),
        StringValidator::Trim => Some("trim".to_string()),
        StringValidator::Normalize => Some("normalize".to_string()),
        StringValidator::NormalizeNFC => Some("normalizeNFC".to_string()),
        StringValidator::NormalizeNFD => Some("normalizeNFD".to_string()),
        StringValidator::NormalizeNFKC => Some("normalizeNFKC".to_string()),
        StringValidator::NormalizeNFKD => Some("normalizeNFKD".to_string()),
        StringValidator::NormalizeNFCPreformatted => Some("normalizedNFC".to_string()),
        StringValidator::NormalizeNFDPreformatted => Some("normalizedNFD".to_string()),
        StringValidator::NormalizeNFKCPreformatted => Some("normalizedNFKC".to_string()),
        StringValidator::NormalizeNFKDPreformatted => Some("normalizedNFKD".to_string()),

        // Parse validators (INCLUDED per requirements)
        StringValidator::DateParse => Some("dateParse".to_string()),
        StringValidator::DateEpochParse => Some("dateEpochParse".to_string()),
        StringValidator::DateIsoParse => Some("dateIsoParse".to_string()),
        StringValidator::IntegerParse => Some("integerParse".to_string()),
        StringValidator::NumericParse => Some("numericParse".to_string()),
        StringValidator::JsonParse => Some("jsonParse".to_string()),
        StringValidator::UrlParse => Some("urlParse".to_string()),

        // Substring validators
        StringValidator::StartsWith(s) => {
            Some(format!("startsWith(\\\"{}\\\")", escape_for_fbs(s)))
        }
        StringValidator::EndsWith(s) => Some(format!("endsWith(\\\"{}\\\")", escape_for_fbs(s))),
        StringValidator::Includes(s) => Some(format!("includes(\\\"{}\\\")", escape_for_fbs(s))),

        // Pattern validators
        StringValidator::RegexLiteral(Format::Custom(custom))
            if custom.flags().is_some_and(|flags| !flags.is_empty()) =>
        {
            Some(format!(
                "pattern(\\\"{}\\\", \\\"{}\\\")",
                escape_for_fbs(custom.as_str()),
                escape_for_fbs(custom.flags().unwrap_or_default())
            ))
        }
        StringValidator::RegexLiteral(format) => Some(format!(
            "pattern(\\\"{}\\\")",
            escape_for_fbs(&format.pattern())
        )),
        StringValidator::Literal(s) => Some(format!("literal(\\\"{}\\\")", escape_for_fbs(s))),

        // Special cases - skip internal validators
        StringValidator::StringEmbedded(_) => None,
    }
}

fn number_validator_to_flatbuffers(nv: &NumberValidator) -> Option<String> {
    match nv {
        NumberValidator::Int => Some("int".to_string()),
        NumberValidator::Finite => Some("finite".to_string()),
        NumberValidator::NonNaN => Some("nonNaN".to_string()),
        NumberValidator::Positive => Some("positive".to_string()),
        NumberValidator::Negative => Some("negative".to_string()),
        NumberValidator::NonPositive => Some("nonPositive".to_string()),
        NumberValidator::NonNegative => Some("nonNegative".to_string()),
        NumberValidator::GreaterThan(n) => Some(format!("greaterThan({})", n.0)),
        NumberValidator::GreaterThanOrEqualTo(n) => Some(format!("greaterThanOrEqualTo({})", n.0)),
        NumberValidator::LessThan(n) => Some(format!("lessThan({})", n.0)),
        NumberValidator::LessThanOrEqualTo(n) => Some(format!("lessThanOrEqualTo({})", n.0)),
        NumberValidator::Between(start, end) => Some(format!("between({}, {})", start.0, end.0)),
        NumberValidator::MultipleOf(n) => Some(format!("multipleOf({})", n.0)),
        NumberValidator::Uint8 => Some("uint8".to_string()),
    }
}

fn array_validator_to_flatbuffers(av: &ArrayValidator) -> Option<String> {
    match av {
        ArrayValidator::MinItems(n) => Some(format!("minItems({})", n)),
        ArrayValidator::MaxItems(n) => Some(format!("maxItems({})", n)),
        ArrayValidator::ItemsCount(n) => Some(format!("itemsCount({})", n)),
    }
}

fn date_validator_to_flatbuffers(dv: &DateValidator) -> Option<String> {
    match dv {
        DateValidator::ValidDate => Some("validDate".to_string()),
        DateValidator::GreaterThanDate(d) => {
            Some(format!("greaterThanDate(\\\"{}\\\")", escape_for_fbs(d)))
        }
        DateValidator::GreaterThanOrEqualToDate(d) => Some(format!(
            "greaterThanOrEqualToDate(\\\"{}\\\")",
            escape_for_fbs(d)
        )),
        DateValidator::LessThanDate(d) => {
            Some(format!("lessThanDate(\\\"{}\\\")", escape_for_fbs(d)))
        }
        DateValidator::LessThanOrEqualToDate(d) => Some(format!(
            "lessThanOrEqualToDate(\\\"{}\\\")",
            escape_for_fbs(d)
        )),
        DateValidator::BetweenDate(start, end) => Some(format!(
            "betweenDate(\\\"{}\\\", \\\"{}\\\")",
            escape_for_fbs(start),
            escape_for_fbs(end)
        )),
    }
}

fn bigint_validator_to_flatbuffers(biv: &BigIntValidator) -> Option<String> {
    match biv {
        BigIntValidator::PositiveBigInt => Some("positiveBigInt".to_string()),
        BigIntValidator::NegativeBigInt => Some("negativeBigInt".to_string()),
        BigIntValidator::NonPositiveBigInt => Some("nonPositiveBigInt".to_string()),
        BigIntValidator::NonNegativeBigInt => Some("nonNegativeBigInt".to_string()),
        BigIntValidator::GreaterThanBigInt(n) => {
            Some(format!("greaterThanBigInt(\\\"{}\\\")", escape_for_fbs(n)))
        }
        BigIntValidator::GreaterThanOrEqualToBigInt(n) => Some(format!(
            "greaterThanOrEqualToBigInt(\\\"{}\\\")",
            escape_for_fbs(n)
        )),
        BigIntValidator::LessThanBigInt(n) => {
            Some(format!("lessThanBigInt(\\\"{}\\\")", escape_for_fbs(n)))
        }
        BigIntValidator::LessThanOrEqualToBigInt(n) => Some(format!(
            "lessThanOrEqualToBigInt(\\\"{}\\\")",
            escape_for_fbs(n)
        )),
        BigIntValidator::BetweenBigInt(start, end) => Some(format!(
            "betweenBigInt(\\\"{}\\\", \\\"{}\\\")",
            escape_for_fbs(start),
            escape_for_fbs(end)
        )),
    }
}

fn bigdecimal_validator_to_flatbuffers(bdv: &BigDecimalValidator) -> Option<String> {
    match bdv {
        BigDecimalValidator::PositiveBigDecimal => Some("positiveBigDecimal".to_string()),
        BigDecimalValidator::NegativeBigDecimal => Some("negativeBigDecimal".to_string()),
        BigDecimalValidator::NonPositiveBigDecimal => Some("nonPositiveBigDecimal".to_string()),
        BigDecimalValidator::NonNegativeBigDecimal => Some("nonNegativeBigDecimal".to_string()),
        BigDecimalValidator::GreaterThanBigDecimal(n) => Some(format!(
            "greaterThanBigDecimal(\\\"{}\\\")",
            escape_for_fbs(n)
        )),
        BigDecimalValidator::GreaterThanOrEqualToBigDecimal(n) => Some(format!(
            "greaterThanOrEqualToBigDecimal(\\\"{}\\\")",
            escape_for_fbs(n)
        )),
        BigDecimalValidator::LessThanBigDecimal(n) => {
            Some(format!("lessThanBigDecimal(\\\"{}\\\")", escape_for_fbs(n)))
        }
        BigDecimalValidator::LessThanOrEqualToBigDecimal(n) => Some(format!(
            "lessThanOrEqualToBigDecimal(\\\"{}\\\")",
            escape_for_fbs(n)
        )),
        BigDecimalValidator::BetweenBigDecimal(start, end) => Some(format!(
            "betweenBigDecimal(\\\"{}\\\", \\\"{}\\\")",
            escape_for_fbs(start),
            escape_for_fbs(end)
        )),
    }
}

fn duration_validator_to_flatbuffers(dv: &DurationValidator) -> Option<String> {
    match dv {
        DurationValidator::GreaterThanDuration(d) => Some(format!(
            "greaterThanDuration(\\\"{}\\\")",
            escape_for_fbs(d)
        )),
        DurationValidator::GreaterThanOrEqualToDuration(d) => Some(format!(
            "greaterThanOrEqualToDuration(\\\"{}\\\")",
            escape_for_fbs(d)
        )),
        DurationValidator::LessThanDuration(d) => {
            Some(format!("lessThanDuration(\\\"{}\\\")", escape_for_fbs(d)))
        }
        DurationValidator::LessThanOrEqualToDuration(d) => Some(format!(
            "lessThanOrEqualToDuration(\\\"{}\\\")",
            escape_for_fbs(d)
        )),
        DurationValidator::BetweenDuration(start, end) => Some(format!(
            "betweenDuration(\\\"{}\\\", \\\"{}\\\")",
            escape_for_fbs(start),
            escape_for_fbs(end)
        )),
    }
}

/// Escape special characters for FlatBuffers attribute strings.
fn escape_for_fbs(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::{
        ArrayValidator, BTreeMap, BTreeSet, BigIntValidator, DateValidator, DurationValidator, Fbs,
        FieldType, NumberValidator, StringValidator, StructConfig, TaggedUnion, Validator,
        collect_validators_for_field, escape_for_fbs, generate_flatbuffers_schema_string,
        validator_to_flatbuffers_string,
    };
    use crate::types::{EnumRepresentation, StructField};
    use ordered_float::OrderedFloat;

    #[test]
    fn test_string_validators_to_flatbuffers() {
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::StringValidator(StringValidator::Email)),
            Some("email".to_string())
        );
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::StringValidator(
                StringValidator::MinLength(8)
            )),
            Some("minLength(8)".to_string())
        );
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::StringValidator(
                StringValidator::MaxLength(50)
            )),
            Some("maxLength(50)".to_string())
        );
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::StringValidator(StringValidator::Uuid)),
            Some("uuid".to_string())
        );
    }

    #[test]
    fn test_transformation_validators_included() {
        // Unlike macroforge, transformations ARE included in FlatBuffers
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::StringValidator(StringValidator::Lower)),
            Some("lower".to_string())
        );
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::StringValidator(StringValidator::Upper)),
            Some("upper".to_string())
        );
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::StringValidator(StringValidator::Trim)),
            Some("trim".to_string())
        );
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::StringValidator(
                StringValidator::Capitalize
            )),
            Some("capitalize".to_string())
        );
    }

    #[test]
    fn test_number_validators_to_flatbuffers() {
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::NumberValidator(NumberValidator::Int)),
            Some("int".to_string())
        );
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::NumberValidator(NumberValidator::Between(
                OrderedFloat(18.0),
                OrderedFloat(120.0)
            ))),
            Some("between(18, 120)".to_string())
        );
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::NumberValidator(NumberValidator::Positive)),
            Some("positive".to_string())
        );
    }

    #[test]
    fn test_array_validators_to_flatbuffers() {
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::ArrayValidator(ArrayValidator::MinItems(
                1
            ))),
            Some("minItems(1)".to_string())
        );
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::ArrayValidator(ArrayValidator::MaxItems(
                5
            ))),
            Some("maxItems(5)".to_string())
        );
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::ArrayValidator(
                ArrayValidator::ItemsCount(3)
            )),
            Some("itemsCount(3)".to_string())
        );
    }

    fn fbs(registry: &crate::types::ForeignTypeRegistry) -> Fbs<'_> {
        Fbs {
            registry,
            declared: BTreeSet::new(),
            scalar_enums: BTreeSet::from(["Role".to_string()]),
            definitions: Vec::new(),
            uses_validate: false,
        }
    }

    fn render(field_type: FieldType) -> (String, String) {
        let registry = crate::types::ForeignTypeRegistry::default();
        let mut fbs = fbs(&registry);
        let field = fbs.field(&field_type, "OwnerField").unwrap();
        (field.declaration("field"), fbs.definitions.concat())
    }

    #[test]
    fn scalars_map_to_flatbuffers_types() {
        for (field_type, expected) in [
            (FieldType::String, "string"),
            (FieldType::Char, "string"),
            (FieldType::Bool, "bool"),
            (FieldType::I8, "int8"),
            (FieldType::I16, "int16"),
            (FieldType::I32, "int32"),
            (FieldType::I64, "int64"),
            (FieldType::Isize, "int64"),
            (FieldType::I128, "string"),
            (FieldType::U8, "uint8"),
            (FieldType::U16, "uint16"),
            (FieldType::U32, "uint32"),
            (FieldType::U64, "uint64"),
            (FieldType::Usize, "uint64"),
            (FieldType::U128, "string"),
            (FieldType::F32, "float"),
            (FieldType::F64, "double"),
        ] {
            assert_eq!(
                render(field_type.clone()).0,
                format!("field: {expected}"),
                "{field_type:?}"
            );
        }
    }

    #[test]
    fn optional_scalars_default_to_null() {
        assert_eq!(
            render(FieldType::Option(Box::new(FieldType::I32))).0,
            "field: int32 = null"
        );
        assert_eq!(
            render(FieldType::Option(Box::new(FieldType::Other(
                "Role".to_string()
            ))))
            .0,
            "field: Role = null"
        );
        assert_eq!(
            render(FieldType::Option(Box::new(FieldType::String))).0,
            "field: string"
        );
    }

    #[test]
    fn vectors_and_scanned_types() {
        assert_eq!(
            render(FieldType::Vec(Box::new(FieldType::I32))).0,
            "field: [int32]"
        );
        assert_eq!(
            render(FieldType::Other("user_profile".to_string())).0,
            "field: UserProfile"
        );
    }

    #[test]
    fn shapes_flatbuffers_cannot_write_directly_get_tables() {
        let (declaration, tables) = render(FieldType::Vec(Box::new(FieldType::Vec(Box::new(
            FieldType::I32,
        )))));
        assert_eq!(declaration, "field: [OwnerFieldItem]");
        assert!(
            tables.contains("table OwnerFieldItem {\n    value: [int32];"),
            "{tables}"
        );

        let (declaration, tables) = render(FieldType::BTreeMap(
            Box::new(FieldType::String),
            Box::new(FieldType::Option(Box::new(FieldType::I64))),
        ));
        assert_eq!(declaration, "field: [OwnerFieldEntry]");
        assert!(
            tables.contains(
                "table OwnerFieldEntry {\n    key: string (key);\n    value: int64 = null;"
            ),
            "{tables}"
        );

        let (declaration, tables) = render(FieldType::Option(Box::new(FieldType::Option(
            Box::new(FieldType::Bool),
        ))));
        assert_eq!(declaration, "field: OwnerField");
        assert!(tables.contains("value: bool = null;"), "{tables}");

        let (declaration, tables) =
            render(FieldType::Tuple(vec![FieldType::String, FieldType::U8]));
        assert_eq!(declaration, "field: OwnerField");
        assert!(tables.contains("item_0: string;"), "{tables}");
        assert!(tables.contains("item_1: uint8;"), "{tables}");
    }

    #[test]
    fn a_generated_table_that_collides_is_rejected() {
        let registry = crate::types::ForeignTypeRegistry::default();
        let mut fbs = fbs(&registry);
        fbs.declared.insert("OwnerField".to_string());
        let error = fbs
            .field(&FieldType::Tuple(vec![FieldType::I32]), "OwnerField")
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(error.contains("`OwnerField`"), "{error}");
    }

    #[test]
    fn enum_keys_are_the_enum() {
        assert!(
            render(FieldType::HashMap(
                Box::new(FieldType::Other("Role".to_string())),
                Box::new(FieldType::String)
            ))
            .1
            .contains("key: Role (key);")
        );
    }

    #[test]
    fn test_collect_validators_for_field() {
        let validators = vec![
            Validator::StringValidator(StringValidator::Email),
            Validator::StringValidator(StringValidator::MinLength(5)),
        ];
        assert_eq!(
            collect_validators_for_field(&validators),
            "email, minLength(5)"
        );
    }

    #[test]
    fn test_collect_validators_empty() {
        let validators: Vec<Validator> = vec![];
        assert_eq!(collect_validators_for_field(&validators), "");
    }

    #[test]
    fn test_generate_simple_table() {
        let mut structs = BTreeMap::new();
        structs.insert(
            "user".to_string(),
            StructConfig {
                resolve_only: false,
                struct_name: "user".to_string(),
                fields: vec![
                    StructField {
                        field_name: "email".to_string(),
                        field_type: FieldType::String,
                        validators: vec![Validator::StringValidator(StringValidator::Email)],
                        ..Default::default()
                    },
                    StructField {
                        field_name: "age".to_string(),
                        field_type: FieldType::I32,
                        validators: vec![Validator::NumberValidator(NumberValidator::Between(
                            OrderedFloat(18.0),
                            OrderedFloat(120.0),
                        ))],
                        ..Default::default()
                    },
                ],
                validators: vec![],
                doccom: None,
                macroforge_derives: vec![],
                annotations: vec![],
                pipeline: crate::types::Pipeline::default(),
                rust_derives: vec![],
                output_override: None,
                raw_attributes: BTreeMap::new(),
            },
        );

        let output = generate_flatbuffers_schema_string(
            &structs,
            &BTreeMap::new(),
            None,
            &crate::types::ForeignTypeRegistry::default(),
        )
        .unwrap();

        assert!(output.contains("table User"));
        assert!(output.contains("email: string (validate: \"email\")"));
        assert!(output.contains("age: int32 (validate: \"between(18, 120)\")"));
    }

    #[test]
    fn test_generate_table_with_namespace() {
        let output = generate_flatbuffers_schema_string(
            &BTreeMap::new(),
            &BTreeMap::new(),
            Some("com.example.app"),
            &crate::types::ForeignTypeRegistry::default(),
        )
        .unwrap();
        assert!(output.starts_with("namespace com.example.app;"));
    }

    #[test]
    fn test_generate_simple_enum() {
        use crate::types::Variant;

        let mut enums = BTreeMap::new();
        enums.insert(
            "Status".to_string(),
            TaggedUnion {
                resolve_only: false,
                enum_name: "Status".to_string(),
                variants: vec![
                    Variant {
                        name: "Active".to_string(),
                        wire: Default::default(),
                        data: None,
                        doccom: None,
                        annotations: vec![],
                        output_override: None,
                        raw_attributes: BTreeMap::new(),
                        is_default: false,
                        element_validators: Vec::new(),
                        element_validator_overrides: Vec::new(),
                    },
                    Variant {
                        name: "Inactive".to_string(),
                        wire: Default::default(),
                        data: None,
                        doccom: None,
                        annotations: vec![],
                        output_override: None,
                        raw_attributes: BTreeMap::new(),
                        is_default: false,
                        element_validators: Vec::new(),
                        element_validator_overrides: Vec::new(),
                    },
                    Variant {
                        name: "Pending".to_string(),
                        wire: Default::default(),
                        data: None,
                        doccom: None,
                        annotations: vec![],
                        output_override: None,
                        raw_attributes: BTreeMap::new(),
                        is_default: false,
                        element_validators: Vec::new(),
                        element_validator_overrides: Vec::new(),
                    },
                ],
                representation: EnumRepresentation::default(),
                doccom: None,
                macroforge_derives: vec![],
                annotations: vec![],
                pipeline: crate::types::Pipeline::default(),
                rust_derives: vec![],
                output_override: None,
                raw_attributes: BTreeMap::new(),
            },
        );

        let output = generate_flatbuffers_schema_string(
            &BTreeMap::new(),
            &enums,
            None,
            &crate::types::ForeignTypeRegistry::default(),
        )
        .unwrap();

        assert!(output.contains("enum Status : ubyte"));
        assert!(output.contains("Active = 0"));
        assert!(output.contains("Inactive = 1"));
        assert!(output.contains("Pending = 2"));
    }

    #[test]
    fn test_generate_complete_schema() {
        use crate::types::Variant;

        let mut structs = BTreeMap::new();
        structs.insert(
            "user_registration_form".to_string(),
            StructConfig {
                resolve_only: false,
                struct_name: "user_registration_form".to_string(),
                fields: vec![
                    StructField {
                        field_name: "email".to_string(),
                        field_type: FieldType::String,
                        validators: vec![
                            Validator::StringValidator(StringValidator::Email),
                            Validator::StringValidator(StringValidator::NonEmpty),
                        ],
                        ..Default::default()
                    },
                    StructField {
                        field_name: "password".to_string(),
                        field_type: FieldType::String,
                        validators: vec![
                            Validator::StringValidator(StringValidator::MinLength(8)),
                            Validator::StringValidator(StringValidator::MaxLength(50)),
                        ],
                        ..Default::default()
                    },
                    StructField {
                        field_name: "age".to_string(),
                        field_type: FieldType::I32,
                        validators: vec![
                            Validator::NumberValidator(NumberValidator::Int),
                            Validator::NumberValidator(NumberValidator::Between(
                                OrderedFloat(18.0),
                                OrderedFloat(120.0),
                            )),
                        ],
                        ..Default::default()
                    },
                    StructField {
                        field_name: "tags".to_string(),
                        field_type: FieldType::Vec(Box::new(FieldType::String)),
                        validators: vec![],
                        ..Default::default()
                    },
                ],
                validators: vec![],
                doccom: None,
                macroforge_derives: vec![],
                annotations: vec![],
                pipeline: crate::types::Pipeline::default(),
                rust_derives: vec![],
                output_override: None,
                raw_attributes: BTreeMap::new(),
            },
        );

        let mut enums = BTreeMap::new();
        enums.insert(
            "Role".to_string(),
            TaggedUnion {
                resolve_only: false,
                enum_name: "Role".to_string(),
                variants: vec![
                    Variant {
                        name: "Admin".to_string(),
                        wire: Default::default(),
                        data: None,
                        doccom: None,
                        annotations: vec![],
                        output_override: None,
                        raw_attributes: BTreeMap::new(),
                        is_default: false,
                        element_validators: Vec::new(),
                        element_validator_overrides: Vec::new(),
                    },
                    Variant {
                        name: "User".to_string(),
                        wire: Default::default(),
                        data: None,
                        doccom: None,
                        annotations: vec![],
                        output_override: None,
                        raw_attributes: BTreeMap::new(),
                        is_default: false,
                        element_validators: Vec::new(),
                        element_validator_overrides: Vec::new(),
                    },
                ],
                representation: EnumRepresentation::default(),
                doccom: None,
                macroforge_derives: vec![],
                annotations: vec![],
                pipeline: crate::types::Pipeline::default(),
                rust_derives: vec![],
                output_override: None,
                raw_attributes: BTreeMap::new(),
            },
        );

        let output = generate_flatbuffers_schema_string(
            &structs,
            &enums,
            Some("com.example.users"),
            &crate::types::ForeignTypeRegistry::default(),
        )
        .unwrap();

        // Check namespace
        assert!(output.contains("namespace com.example.users;"));

        // Check enum
        assert!(output.contains("enum Role : ubyte"));
        assert!(output.contains("Admin = 0"));
        assert!(output.contains("User = 1"));

        // Check table
        assert!(output.contains("table UserRegistrationForm"));
        assert!(output.contains("email: string (validate: \"email, nonEmpty\")"));
        assert!(output.contains("password: string (validate: \"minLength(8), maxLength(50)\")"));
        assert!(output.contains("age: int32 (validate: \"int, between(18, 120)\")"));
        assert!(output.contains("tags: [string];"));
    }

    #[test]
    fn test_date_validator_to_flatbuffers() {
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::DateValidator(DateValidator::ValidDate)),
            Some("validDate".to_string())
        );
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::DateValidator(
                DateValidator::GreaterThanDate("2024-01-01".to_string())
            )),
            Some("greaterThanDate(\\\"2024-01-01\\\")".to_string())
        );
    }

    #[test]
    fn test_bigint_validator_to_flatbuffers() {
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::BigIntValidator(
                BigIntValidator::PositiveBigInt
            )),
            Some("positiveBigInt".to_string())
        );
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::BigIntValidator(
                BigIntValidator::GreaterThanBigInt("1000000".to_string())
            )),
            Some("greaterThanBigInt(\\\"1000000\\\")".to_string())
        );
    }

    #[test]
    fn test_duration_validator_to_flatbuffers() {
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::DurationValidator(
                DurationValidator::GreaterThanDuration("1h".to_string())
            )),
            Some("greaterThanDuration(\\\"1h\\\")".to_string())
        );
        assert_eq!(
            validator_to_flatbuffers_string(&Validator::DurationValidator(
                DurationValidator::BetweenDuration("1m".to_string(), "1h".to_string())
            )),
            Some("betweenDuration(\\\"1m\\\", \\\"1h\\\")".to_string())
        );
    }

    #[test]
    fn test_escape_for_fbs() {
        assert_eq!(escape_for_fbs("hello"), "hello");
        assert_eq!(escape_for_fbs("hello\"world"), "hello\\\"world");
        assert_eq!(escape_for_fbs("path\\to\\file"), "path\\\\to\\\\file");
    }
}
