use crate::{
    schemasync::TableConfig,
    types::{
        EnumRepresentation, FieldType, Pipeline, Storage, StructConfig, StructField, TaggedUnion,
        Variant, VariantData, Wire,
    },
    validator::{Validator, ValidatorOverrides, morph::Morph},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// A struct serde writes as another type rather than as an object of fields.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewtypeConfig {
    pub name: String,
    /// What serde writes: the one field's type, an array of a tuple struct's
    /// fields, or `Unit` (null) for a unit struct.
    pub inner: FieldType,
    #[serde(default)]
    pub kind: NewtypeKind,
    /// Rewrite the inner value, before `validators` check it.
    #[serde(default)]
    pub morphs: Vec<Morph>,
    /// Checked on the inner value, before any validators of a field holding it.
    #[serde(default)]
    pub validators: Vec<Validator>,
    /// Lists replacing `validators` in one pipeline.
    #[serde(default, skip_serializing_if = "ValidatorOverrides::is_empty")]
    pub validator_overrides: ValidatorOverrides,
    /// A tuple struct's morphs, one list per element.
    #[serde(default)]
    pub element_morphs: Vec<Vec<Morph>>,
    /// A tuple struct's validators, one list per element.
    #[serde(default)]
    pub element_validators: Vec<Vec<Validator>>,
    /// Lists replacing each element's validators in one pipeline.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub element_validator_overrides: Vec<ValidatorOverrides>,
    /// How the database stores it, where its `#[surreal]` keys differ from
    /// serde: an array by `tuple`, an element through serde by `wrap`, a unit
    /// struct as its `value`.
    #[serde(default)]
    pub storage: Storage,
    #[serde(default)]
    pub doccom: Option<String>,
    #[serde(default)]
    pub annotations: Vec<String>,
    #[serde(default)]
    pub macroforge_derives: Vec<String>,
    #[serde(default)]
    pub rust_derives: Vec<String>,
    #[serde(default)]
    pub pipeline: Pipeline,
    /// Registered for field-type resolution only, never emitted.
    #[serde(default)]
    pub resolve_only: bool,
    #[serde(default)]
    pub raw_attributes: BTreeMap<String, Vec<String>>,
    /// What an output rule plugin made of it, read through `effective`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_override: Option<Box<Self>>,
}

/// How a newtype is described.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NewtypeKind {
    /// A single-field tuple struct or a `#[serde(transparent)]` struct: its own
    /// type, branded over the field's, so a plain value of the field's type is
    /// not one.
    #[default]
    Branded,
    /// A multi-field tuple struct or a unit struct: just the shape serde writes.
    Alias,
}

impl NewtypeConfig {
    /// Resolve `output_override` recursively. See [`crate::types::StructConfig::effective`].
    pub fn effective(&self) -> &Self {
        self.output_override
            .as_deref()
            .map_or(self, Self::effective)
    }

    /// Whether its own `#[surreal]` keys store it in another shape than its
    /// inner type's.
    pub fn stores_own_shape(&self) -> bool {
        self.storage.tuple
            || self.storage.value.is_some()
            || self.storage.opaque_elements.contains(&true)
    }

    /// The newtype as a one-variant untagged enum carrying its storage, which
    /// the schema, mock data and defaults render wherever it is held. Its
    /// validators check the value Rust reads, not a stored form the schema
    /// could assert.
    fn stored_enum(&self) -> TaggedUnion {
        TaggedUnion {
            enum_name: self.name.clone(),
            variants: vec![Variant {
                name: self.name.clone(),
                data: (self.inner != FieldType::Unit)
                    .then(|| VariantData::DataStructureRef(self.inner.clone())),
                wire: Wire {
                    storage: self.storage.clone(),
                    ..Wire::default()
                },
                doccom: None,
                annotations: Vec::new(),
                output_override: None,
                raw_attributes: BTreeMap::new(),
                is_default: true,
                element_morphs: self.element_morphs.clone(),
                element_validators: self.element_validators.clone(),
                element_validator_overrides: Vec::new(),
            }],
            representation: EnumRepresentation::Untagged,
            doccom: self.doccom.clone(),
            macroforge_derives: Vec::new(),
            annotations: Vec::new(),
            pipeline: self.pipeline,
            rust_derives: Vec::new(),
            output_override: None,
            resolve_only: self.resolve_only,
            raw_attributes: BTreeMap::new(),
        }
    }
}

/// Rewrites every type the scan describes so that a field holding a newtype
/// holds its inner type instead, which is how the database stores it, and
/// returns the types as declared wherever that rewrote one.
///
/// A field typed as a newtype, or as an `Option` of one, takes the newtype's
/// validators (the innermost newtype's first, as Rust reads them) ahead of its
/// own. Deeper positions, such as a `Vec` of one, take the inner type alone,
/// and the declared type recorded for the field gives the schema and mock
/// data the newtype's validators there. A newtype whose own
/// `#[surreal]` keys change its stored shape becomes an enum carrying that
/// storage instead. A newtype that holds itself has no storable form and is
/// refused.
pub fn desugar_newtypes(
    newtypes: &BTreeMap<String, NewtypeConfig>,
    enums: &mut BTreeMap<String, TaggedUnion>,
    tables: &mut BTreeMap<String, TableConfig>,
    objects: &mut BTreeMap<String, StructConfig>,
) -> crate::Result<DeclaredTypes> {
    let mut desugar = Desugar {
        newtypes,
        declared: DeclaredTypes::default(),
    };
    if newtypes.is_empty() {
        return Ok(desugar.declared);
    }
    for newtype in newtypes
        .values()
        .filter(|newtype| newtype.stores_own_shape())
    {
        enums.insert(newtype.name.clone(), newtype.stored_enum());
    }
    // A table's struct is among the objects too, but its fields are reached
    // through the table: held by value elsewhere, it is stored as a link.
    let table_structs: BTreeSet<String> = tables
        .values()
        .map(|table| table.struct_config.struct_name.clone())
        .collect();
    for (name, struct_config) in objects.iter_mut() {
        let owner = (!table_structs.contains(name)).then(|| FieldOwner::Object(name.clone()));
        desugar.struct_config(struct_config, owner.as_ref())?;
    }
    for table in tables.values_mut() {
        desugar.table(table)?;
    }
    for (name, tagged_union) in enums.iter_mut() {
        desugar.tagged_union(name, tagged_union, true)?;
    }
    desugar.declared.chains = newtypes
        .values()
        .filter(|newtype| !newtype.stores_own_shape())
        .map(|newtype| Ok((newtype.name.clone(), desugar.chain(&newtype.name)?)))
        .collect::<crate::Result<_>>()?;
    Ok(desugar.declared)
}

/// What holds a field, as the generators that walk the stored types reach it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum FieldOwner {
    /// A table, by the name its effective configuration defines it under.
    Table(String),
    /// An object, by its key among the objects.
    Object(String),
    /// A struct variant's payload, by its enum's key and the variant's name.
    Variant { enum_name: String, variant: String },
}

impl std::fmt::Display for FieldOwner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FieldOwner::Table(name) | FieldOwner::Object(name) => formatter.write_str(name),
            FieldOwner::Variant { enum_name, variant } => {
                write!(formatter, "{enum_name}::{variant}")
            }
        }
    }
}

/// The types as declared wherever desugaring replaced a newtype, with each
/// newtype's validators, for a generator that must meet the validators of a
/// newtype the stored type no longer names. Only effective configurations are
/// recorded.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct DeclaredTypes {
    fields: BTreeMap<FieldOwner, BTreeMap<String, FieldType>>,
    /// A tuple variant's payload, by its enum's key and the variant's name.
    payloads: BTreeMap<(String, String), FieldType>,
    /// Each newtype's innermost declared type, beneath any newtypes it holds,
    /// and the validators checked on the way down, innermost first.
    chains: BTreeMap<String, (FieldType, Chain)>,
}

/// A newtype chain's morphs and validators, innermost first, and the
/// TypeScript outputs' validators where any layer's differ, which mock data
/// meets as well.
#[derive(Debug, Default, Clone, PartialEq)]
struct Chain {
    morphs: Vec<Morph>,
    validators: Vec<Validator>,
    checks: Option<Vec<Validator>>,
}

impl Chain {
    /// The chain with `newtype` wrapped around it.
    fn layer(mut self, newtype: &NewtypeConfig) -> Self {
        let typesync = newtype.validator_overrides.typesync.as_ref();
        if self.checks.is_some() || typesync.is_some() {
            let mut checks = self
                .checks
                .take()
                .unwrap_or_else(|| self.validators.clone());
            checks.extend(typesync.unwrap_or(&newtype.validators).iter().cloned());
            self.checks = Some(checks);
        }
        self.morphs.extend(newtype.morphs.iter().cloned());
        self.validators.extend(newtype.validators.iter().cloned());
        self
    }
}

impl DeclaredTypes {
    /// The declared type of `field_name`, when a newtype in it was replaced.
    pub fn field(&self, owner: &FieldOwner, field_name: &str) -> Option<&FieldType> {
        self.fields.get(owner)?.get(field_name)
    }

    /// The declared payload of a tuple variant, when a newtype in it was
    /// replaced.
    pub fn payload(&self, enum_name: &str, variant: &str) -> Option<&FieldType> {
        self.payloads
            .get(&(enum_name.to_owned(), variant.to_owned()))
    }

    /// The declared type a newtype holds beneath any newtypes, and every
    /// validator the chain checks, innermost first.
    pub fn newtype(&self, field_type: &FieldType) -> Option<(&FieldType, &[Validator])> {
        let FieldType::Other(name) = field_type else {
            return None;
        };
        self.chains
            .get(name)
            .map(|(inner, chain)| (inner, chain.validators.as_slice()))
    }

    /// Every morph a newtype's chain applies, innermost first.
    pub fn newtype_morphs(&self, field_type: &FieldType) -> &[Morph] {
        match field_type {
            FieldType::Other(name) => self
                .chains
                .get(name)
                .map_or(&[], |(_, chain)| chain.morphs.as_slice()),
            _ => &[],
        }
    }

    /// The TypeScript outputs' validators for a newtype's chain, where they
    /// differ from the schema's.
    pub fn newtype_checks(&self, field_type: &FieldType) -> Option<&[Validator]> {
        let FieldType::Other(name) = field_type else {
            return None;
        };
        self.chains.get(name)?.1.checks.as_deref()
    }
}

struct Desugar<'n> {
    newtypes: &'n BTreeMap<String, NewtypeConfig>,
    declared: DeclaredTypes,
}

impl Desugar<'_> {
    fn table(&mut self, table: &mut TableConfig) -> crate::Result<()> {
        match table.output_override.as_deref_mut() {
            Some(replacement) => {
                self.struct_config(&mut table.struct_config, None)?;
                self.table(replacement)
            }
            None => {
                let owner = FieldOwner::Table(table.table_name.clone());
                self.struct_config(&mut table.struct_config, Some(&owner))
            }
        }
    }

    /// Desugars `struct_config`, recording its fields under `owner` when it is
    /// the effective configuration.
    fn struct_config(
        &mut self,
        struct_config: &mut StructConfig,
        owner: Option<&FieldOwner>,
    ) -> crate::Result<()> {
        let effective = struct_config.output_override.is_none();
        for field in &mut struct_config.fields {
            let declared = self.field(field)?;
            if let (true, Some(owner), Some(declared)) = (effective, owner, declared) {
                self.declared
                    .fields
                    .entry(owner.clone())
                    .or_default()
                    .insert(field.field_name.clone(), declared);
            }
        }
        if let Some(replacement) = struct_config.output_override.as_deref_mut() {
            self.struct_config(replacement, owner)?;
        }
        Ok(())
    }

    fn tagged_union(
        &mut self,
        enum_name: &str,
        tagged_union: &mut TaggedUnion,
        effective: bool,
    ) -> crate::Result<()> {
        let variants_effective = effective && tagged_union.output_override.is_none();
        for variant in &mut tagged_union.variants {
            self.variant(enum_name, variant, variants_effective)?;
        }
        if let Some(replacement) = tagged_union.output_override.as_deref_mut() {
            self.tagged_union(enum_name, replacement, effective)?;
        }
        Ok(())
    }

    fn variant(
        &mut self,
        enum_name: &str,
        variant: &mut Variant,
        effective: bool,
    ) -> crate::Result<()> {
        let recorded = effective && variant.output_override.is_none();
        match &mut variant.data {
            Some(VariantData::InlineStruct(inline)) => {
                let owner = recorded.then(|| FieldOwner::Variant {
                    enum_name: enum_name.to_owned(),
                    variant: variant.name.clone(),
                });
                self.struct_config(inline, owner.as_ref())?;
            }
            Some(VariantData::DataStructureRef(field_type)) => {
                let stored = self.nested(field_type, &mut Vec::new())?;
                if stored != *field_type {
                    let declared = std::mem::replace(field_type, stored);
                    if recorded {
                        self.declared
                            .payloads
                            .insert((enum_name.to_owned(), variant.name.clone()), declared);
                    }
                }
            }
            None => {}
        }
        if let Some(replacement) = variant.output_override.as_deref_mut() {
            self.variant(enum_name, replacement, effective)?;
        }
        Ok(())
    }

    /// Desugars `field`, returning its declared type when a newtype in it
    /// was replaced.
    fn field(&mut self, field: &mut StructField) -> crate::Result<Option<FieldType>> {
        let (field_type, chain) = match &field.field_type {
            FieldType::Option(held) => match self.unwrapped(held, &mut Vec::new())? {
                Some((inner, chain)) => (FieldType::Option(Box::new(inner)), chain),
                None => (
                    self.nested(&field.field_type, &mut Vec::new())?,
                    Chain::default(),
                ),
            },
            held => match self.unwrapped(held, &mut Vec::new())? {
                Some((inner, chain)) => (inner, chain),
                None => (
                    self.nested(&field.field_type, &mut Vec::new())?,
                    Chain::default(),
                ),
            },
        };
        let declared = (field_type != field.field_type)
            .then(|| std::mem::replace(&mut field.field_type, field_type));
        if chain.checks.is_some() || field.validator_overrides.typesync.is_some() {
            let mut checks = chain
                .checks
                .clone()
                .unwrap_or_else(|| chain.validators.clone());
            checks.extend(
                field
                    .validator_overrides
                    .typesync
                    .take()
                    .unwrap_or_else(|| field.validators.clone()),
            );
            field.validator_overrides.typesync = Some(checks);
        }
        // The newtype's morphs and checks run first, then the field's morphs,
        // after which the newtype's checks hold again, so the stored value
        // has been through every morph and meets every check.
        if !chain.morphs.is_empty() {
            field.morphs = chain
                .morphs
                .into_iter()
                .chain(field.morphs.drain(..))
                .collect();
        }
        if !chain.validators.is_empty() {
            field.validators = chain
                .validators
                .into_iter()
                .chain(field.validators.drain(..))
                .collect();
        }
        if let Some(replacement) = field.output_override.as_deref_mut() {
            self.field(replacement)?;
        }
        Ok(declared)
    }

    /// The innermost declared type the newtype `name` holds and its chain's
    /// validators, innermost first.
    fn chain(&self, name: &str) -> crate::Result<(FieldType, Chain)> {
        let mut holding: Vec<String> = Vec::new();
        let mut current = name;
        let mut layers: Vec<&NewtypeConfig> = Vec::new();
        while let Some(newtype) = self.newtypes.get(current) {
            if holding.iter().any(|held| held == current) {
                return Err(self_holding(current, &holding));
            }
            holding.push(current.to_owned());
            layers.push(newtype);
            match &newtype.inner {
                FieldType::Other(inner)
                    if self
                        .newtypes
                        .get(inner)
                        .is_some_and(|held| !held.stores_own_shape()) =>
                {
                    current = inner;
                }
                inner => {
                    let chain = layers
                        .iter()
                        .rev()
                        .fold(Chain::default(), |chain, layer| chain.layer(layer));
                    return Ok((inner.clone(), chain));
                }
            }
        }
        Err(crate::EvenframeError::Config(format!(
            "`{name}` is not a newtype"
        )))
    }

    /// The storable form of `field_type` when it names a newtype, with that
    /// newtype's validators in the order Rust runs them. A newtype storing its
    /// own shape stays named, as the enum standing for it.
    fn unwrapped(
        &self,
        field_type: &FieldType,
        holding: &mut Vec<String>,
    ) -> crate::Result<Option<(FieldType, Chain)>> {
        let FieldType::Other(name) = field_type else {
            return Ok(None);
        };
        let Some(newtype) = self
            .newtypes
            .get(name)
            .filter(|newtype| !newtype.stores_own_shape())
        else {
            return Ok(None);
        };
        if holding.contains(name) {
            return Err(self_holding(name, holding));
        }
        holding.push(name.clone());
        let unwrapped = match self.unwrapped(&newtype.inner, holding)? {
            Some((inner, chain)) => (inner, chain.layer(newtype)),
            None => (
                self.nested(&newtype.inner, holding)?,
                Chain::default().layer(newtype),
            ),
        };
        holding.pop();
        Ok(Some(unwrapped))
    }

    /// `field_type` with every newtype it holds replaced by its storable form.
    fn nested(
        &self,
        field_type: &FieldType,
        holding: &mut Vec<String>,
    ) -> crate::Result<FieldType> {
        Ok(match field_type {
            FieldType::Other(_) => match self.unwrapped(field_type, holding)? {
                Some((inner, _)) => inner,
                None => field_type.clone(),
            },
            FieldType::Option(inner) => FieldType::Option(Box::new(self.nested(inner, holding)?)),
            FieldType::Vec(inner) => FieldType::Vec(Box::new(self.nested(inner, holding)?)),
            FieldType::JsonText(inner) => {
                FieldType::JsonText(Box::new(self.nested(inner, holding)?))
            }
            FieldType::HashMap(key, value) => FieldType::HashMap(
                Box::new(self.nested(key, holding)?),
                Box::new(self.nested(value, holding)?),
            ),
            FieldType::BTreeMap(key, value) => FieldType::BTreeMap(
                Box::new(self.nested(key, holding)?),
                Box::new(self.nested(value, holding)?),
            ),
            FieldType::Tuple(items) => FieldType::Tuple(
                items
                    .iter()
                    .map(|item| self.nested(item, holding))
                    .collect::<crate::Result<_>>()?,
            ),
            FieldType::Struct(members) => FieldType::Struct(
                members
                    .iter()
                    .map(|(name, member)| Ok((name.clone(), self.nested(member, holding)?)))
                    .collect::<crate::Result<_>>()?,
            ),
            // A link names the record's table, which a newtype never is.
            FieldType::RecordLink(_)
            | FieldType::String
            | FieldType::Char
            | FieldType::Bool
            | FieldType::Unit
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
            | FieldType::Usize
            | FieldType::Duration
            | FieldType::FromText(_)
            | FieldType::IsoDate
            | FieldType::EpochMillis => field_type.clone(),
        })
    }
}

fn self_holding(name: &str, holding: &[String]) -> crate::EvenframeError {
    let mut path = holding.to_vec();
    path.push(name.to_owned());
    crate::EvenframeError::Config(format!(
        "the newtype `{name}` holds itself ({}), so the database has no form to store it in",
        path.join(" → ")
    ))
}
