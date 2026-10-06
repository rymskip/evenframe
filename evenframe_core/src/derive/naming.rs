//! The names a type's fields and variants take outside Rust: JSON, TypeScript,
//! and the database. The derive and the scanner
//! both resolve them here, so the two cannot disagree.

use crate::derive::schemasync_attributes::SchemasyncAttributes;
pub use crate::derive::surreal_attributes::UnitValue;
use crate::derive::surreal_attributes::{
    Position, SkipContent, SurrealAttributes, SurrealCasing, SurrealDefault,
};
use crate::types::{ContentStorage, EnumRepresentation, Storage, Wire};
use convert_case::{Case, Casing};
use heck::{
    ToKebabCase, ToLowerCamelCase, ToShoutyKebabCase, ToShoutySnakeCase, ToSnakeCase,
    ToUpperCamelCase,
};
use proc_macro2::Span;
use serde_derive_internals::{
    Ctxt, Derive,
    ast::{Container, Data, Field, Style},
    attr::TagType,
};
use syn::{Attribute, DeriveInput, Ident, LitStr, Token, spanned::Spanned};

/// The wire form of one struct or enum, in declaration order.
#[derive(Default)]
pub struct ItemWire {
    /// What serde writes a struct as.
    pub shape: ItemShape,
    /// The type `#[serde(into = "...")]` writes the struct as, in place of
    /// its fields.
    pub wire_as: Option<syn::Type>,
    /// A struct's named fields.
    pub fields: Vec<Wire>,
    /// How serde writes and reads each of a struct's named fields.
    pub handling: Vec<FieldHandling>,
    pub variants: Vec<VariantWire>,
    /// An enum's representation in serde's JSON.
    pub representation: EnumRepresentation,
    /// What a struct's missing fields take under `#[serde(default)]`.
    pub container_default: FieldDefault,
    /// What a stored struct's missing fields take: `#[surreal(default)]`'s,
    /// else serde's.
    pub stored_container_default: FieldDefault,
    /// A tuple struct of one field stored as an array, by `#[surreal(tuple)]`.
    pub stored_tuple: bool,
    /// A unit struct's `#[surreal(value)]`.
    pub unit_value: Option<UnitValue>,
    /// Each element of a tuple struct stored through serde.
    pub opaque_elements: Vec<bool>,
    pub deny_unknown_fields: bool,
}

/// What serde writes a struct as.
#[derive(Default, Clone, PartialEq, Eq)]
pub enum ItemShape {
    /// An object of its named fields. Every enum has this shape.
    #[default]
    Named,
    /// Its one field's value: a single-field tuple struct, or a
    /// `#[serde(transparent)]` struct, whose `member` is the field serde reads.
    Newtype { member: syn::Member },
    /// An array of its fields.
    Tuple(usize),
    /// `null`.
    Unit,
}

// `syn::Member` has no `Debug` without syn's `extra-traits`, so the member is
// shown as written.
impl std::fmt::Debug for ItemShape {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Named => formatter.write_str("Named"),
            Self::Newtype { member } => {
                let member = quote::ToTokens::to_token_stream(member).to_string();
                formatter
                    .debug_struct("Newtype")
                    .field("member", &member)
                    .finish()
            }
            Self::Tuple(count) => formatter.debug_tuple("Tuple").field(count).finish(),
            Self::Unit => formatter.write_str("Unit"),
        }
    }
}

#[derive(Default)]
pub struct VariantWire {
    pub wire: Wire,
    /// A struct variant's named fields.
    pub fields: Vec<Wire>,
    pub handling: Vec<FieldHandling>,
    /// The other names serde reads the variant by, its deserialize name among
    /// them when that differs from the name it writes.
    pub aliases: Vec<String>,
    /// `#[serde(skip_serializing)]`: serde never writes it.
    pub skip_serializing: bool,
    /// The predicate `#[surreal(skip_content_if)]` names.
    pub skip_content_if: Option<syn::Path>,
    /// A unit variant's `#[surreal(value)]`.
    pub unit_value: Option<UnitValue>,
}

/// How serde writes and reads one named field, beyond its name.
#[derive(Default, Clone)]
pub struct FieldHandling {
    /// What it gives the field when the input lacks it.
    pub default: FieldDefault,
    /// `#[serde(flatten)]`: the field's own fields sit beside its siblings.
    pub flatten: bool,
    /// The other names serde reads the field by, its deserialize name among
    /// them when that differs from the name it writes.
    pub aliases: Vec<String>,
    /// `#[serde(skip_serializing)]`: serde never writes it.
    pub skip_serializing: bool,
    /// `#[serde(skip_deserializing)]`: serde never reads it, giving it its
    /// default instead.
    pub skip_deserializing: bool,
    /// Serde writes or reads it with functions of its own, `with`,
    /// `serialize_with` or `deserialize_with`.
    pub custom_serde: bool,
    /// Whether a stored record's value is read, rather than its default.
    pub storage_reads: bool,
    /// What a stored record missing it gives it: `#[surreal(default)]`'s,
    /// else serde's.
    pub stored_default: FieldDefault,
}

/// What serde gives a field missing from its input.
#[derive(Default, Clone)]
pub enum FieldDefault {
    #[default]
    None,
    /// `Default::default()`.
    Trait,
    /// The function `#[serde(default = "...")]` names.
    Function(syn::ExprPath),
}

impl From<&SurrealDefault> for FieldDefault {
    fn from(default: &SurrealDefault) -> Self {
        match default {
            SurrealDefault::Trait => Self::Trait,
            SurrealDefault::Function(function) => Self::Function(function.clone()),
        }
    }
}

impl From<&serde_derive_internals::attr::Default> for FieldDefault {
    fn from(default: &serde_derive_internals::attr::Default) -> Self {
        match default {
            serde_derive_internals::attr::Default::None => Self::None,
            serde_derive_internals::attr::Default::Default => Self::Trait,
            serde_derive_internals::attr::Default::Path(function) => {
                Self::Function(function.clone())
            }
        }
    }
}

/// Resolves the serde and SurrealValue names of `input`'s fields and
/// variants, and how serde writes and reads each.
pub fn resolve(input: &DeriveInput) -> syn::Result<ItemWire> {
    let mut item = serde_wire(input)?;
    apply_surreal(input, &mut item)?;
    apply_typescript(input, &mut item).map_err(|error| {
        syn::Error::new(
            error.span(),
            format!("TypeScript naming for '{}': {error}", input.ident),
        )
    })?;
    Ok(item)
}

/// A Rust identifier as serde and SurrealValue name it, without `r#`.
pub fn unraw(ident: &Ident) -> String {
    let name = ident.to_string();
    name.strip_prefix("r#").map(str::to_owned).unwrap_or(name)
}

fn serde_wire(input: &DeriveInput) -> syn::Result<ItemWire> {
    let context = Ctxt::new();
    let private = Ident::new("__private", Span::call_site());
    let item = Container::from_ast(&context, input, Derive::Serialize, &private)
        .map(|container| container_wire(&container));
    context.check()?;
    // `from_ast` returns nothing only after recording an error, which `check` returned.
    item.ok_or_else(|| syn::Error::new(input.ident.span(), "serde cannot describe a union"))
}

fn container_wire(container: &Container) -> ItemWire {
    let item = container_shape(container);
    ItemWire {
        wire_as: container.attrs.type_into().cloned(),
        ..item
    }
}

fn container_shape(container: &Container) -> ItemWire {
    match &container.data {
        Data::Struct(style, fields) => {
            let shape = if container.attrs.transparent() {
                // serde_derive_internals has already checked that exactly one
                // field is left once the skipped ones are set aside.
                fields
                    .iter()
                    .find(|field| !field.attrs.skip_deserializing())
                    .map(|field| ItemShape::Newtype {
                        member: field.member.clone(),
                    })
                    .unwrap_or(ItemShape::Unit)
            } else {
                match style {
                    Style::Struct => ItemShape::Named,
                    Style::Newtype => ItemShape::Newtype {
                        member: syn::Member::Unnamed(syn::Index::from(0)),
                    },
                    Style::Tuple => ItemShape::Tuple(fields.len()),
                    Style::Unit => ItemShape::Unit,
                }
            };
            match style {
                Style::Struct => ItemWire {
                    shape,
                    fields: fields.iter().map(field_wire).collect(),
                    handling: fields.iter().map(field_handling).collect(),
                    container_default: container.attrs.default().into(),
                    deny_unknown_fields: container.attrs.deny_unknown_fields(),
                    ..ItemWire::default()
                },
                Style::Newtype | Style::Tuple | Style::Unit => ItemWire {
                    shape,
                    ..ItemWire::default()
                },
            }
        }
        Data::Enum(variants) => ItemWire {
            variants: variants
                .iter()
                .map(|variant| VariantWire {
                    wire: Wire {
                        serde: renamed(&variant.ident, variant.attrs.name()),
                        serde_skipped: variant.attrs.skip_serializing()
                            && variant.attrs.skip_deserializing(),
                        serde_untagged: variant.attrs.untagged(),
                        storage: Storage {
                            skipped: variant.attrs.skip_serializing()
                                && variant.attrs.skip_deserializing(),
                            ..Storage::default()
                        },
                        ..Wire::default()
                    },
                    fields: match variant.style {
                        Style::Struct => variant.fields.iter().map(field_wire).collect(),
                        Style::Tuple | Style::Newtype | Style::Unit => Vec::new(),
                    },
                    handling: variant.fields.iter().map(field_handling).collect(),
                    aliases: aliases(variant.attrs.aliases(), variant.attrs.name()),
                    skip_serializing: variant.attrs.skip_serializing(),
                    ..VariantWire::default()
                })
                .collect(),
            representation: match container.attrs.tag() {
                TagType::External => EnumRepresentation::ExternallyTagged,
                TagType::Internal { tag } => {
                    EnumRepresentation::InternallyTagged { tag: tag.clone() }
                }
                TagType::Adjacent { tag, content } => EnumRepresentation::AdjacentlyTagged {
                    tag: tag.clone(),
                    content: content.clone(),
                },
                TagType::None => EnumRepresentation::Untagged,
            },
            deny_unknown_fields: container.attrs.deny_unknown_fields(),
            ..ItemWire::default()
        },
    }
}

fn field_handling(field: &Field) -> FieldHandling {
    FieldHandling {
        default: field.attrs.default().into(),
        flatten: field.attrs.flatten(),
        aliases: aliases(field.attrs.aliases(), field.attrs.name()),
        skip_serializing: field.attrs.skip_serializing(),
        skip_deserializing: field.attrs.skip_deserializing(),
        custom_serde: field.attrs.serialize_with().is_some()
            || field.attrs.deserialize_with().is_some(),
        storage_reads: !field.attrs.skip_deserializing(),
        stored_default: field.attrs.default().into(),
    }
}

/// Every name serde reads an item by other than the one it writes: its
/// `#[serde(alias)]` names and a deserialize name of its own.
fn aliases(
    names: &std::collections::BTreeSet<serde_derive_internals::name::Name>,
    name: &serde_derive_internals::name::MultiName,
) -> Vec<String> {
    names
        .iter()
        .map(|alias| alias.value.clone())
        .filter(|alias| *alias != name.serialize_name().value)
        .collect()
}

fn field_wire(field: &Field) -> Wire {
    let serde = match &field.member {
        syn::Member::Named(ident) => renamed(ident, field.attrs.name()),
        syn::Member::Unnamed(_) => None,
    };
    let skip_serializing = field.attrs.skip_serializing();
    let skip_deserializing = field.attrs.skip_deserializing();
    Wire {
        serde,
        serde_skipped: skip_serializing && skip_deserializing,
        // A key serde leaves out of what it writes, or ignores in what it
        // reads, is one the JSON may lack.
        serde_optional: field.attrs.skip_serializing_if().is_some()
            || skip_serializing != skip_deserializing,
        serde_untagged: false,
        serde_flatten: field.attrs.flatten(),
        // The database stores what serde writes.
        storage: Storage {
            skipped: skip_serializing,
            ..Storage::default()
        },
        surreal: None,
        typescript: None,
    }
}

/// The name serde writes an item by, when it differs from the Rust name. A
/// deserialize name of its own is read as an alias.
fn renamed(ident: &Ident, name: &serde_derive_internals::name::MultiName) -> Option<String> {
    let serialized = &name.serialize_name().value;
    (*serialized != unraw(ident)).then(|| serialized.clone())
}

// ----- SurrealValue ----------------------------------------------------------

#[derive(Clone, Copy)]
enum NameCase {
    Lowercase,
    Uppercase,
    PascalCase,
    CamelCase,
    SnakeCase,
    ScreamingSnake,
    KebabCase,
    ScreamingKebab,
}

impl NameCase {
    fn parse(literal: &LitStr) -> syn::Result<Self> {
        Ok(match literal.value().as_str() {
            "lowercase" => Self::Lowercase,
            "UPPERCASE" => Self::Uppercase,
            "PascalCase" => Self::PascalCase,
            "camelCase" => Self::CamelCase,
            "snake_case" => Self::SnakeCase,
            "SCREAMING_SNAKE_CASE" => Self::ScreamingSnake,
            "kebab-case" => Self::KebabCase,
            "SCREAMING-KEBAB-CASE" => Self::ScreamingKebab,
            other => {
                return Err(syn::Error::new(
                    literal.span(),
                    format!(
                        "unknown casing \"{other}\"; expected lowercase, UPPERCASE, PascalCase, camelCase, snake_case, SCREAMING_SNAKE_CASE, kebab-case or SCREAMING-KEBAB-CASE"
                    ),
                ));
            }
        })
    }

    fn apply(self, name: &str) -> String {
        match self {
            Self::Lowercase => name.to_lowercase(),
            Self::Uppercase => name.to_uppercase(),
            Self::PascalCase => name.to_upper_camel_case(),
            Self::CamelCase => name.to_lower_camel_case(),
            Self::SnakeCase => name.to_snake_case(),
            Self::ScreamingSnake => name.to_shouty_snake_case(),
            Self::KebabCase => name.to_kebab_case(),
            Self::ScreamingKebab => name.to_shouty_kebab_case(),
        }
    }

    /// TS camel/Pascal casing keeps the generators' word boundaries, including digits.
    fn apply_ts(self, name: &str) -> String {
        match self {
            Self::CamelCase => name.to_case(Case::Camel),
            Self::PascalCase => name.to_case(Case::Pascal),
            Self::Lowercase
            | Self::Uppercase
            | Self::SnakeCase
            | Self::ScreamingSnake
            | Self::KebabCase
            | Self::ScreamingKebab => self.apply(name),
        }
    }
}

/// SurrealValue's name for an item when it differs from the Rust name.
fn surreal_name(
    ident: &Ident,
    own: &SurrealAttributes,
    casing: Option<NameCase>,
) -> Option<String> {
    let rust = unraw(ident);
    let name = own
        .rename
        .clone()
        .or_else(|| casing.map(|casing| casing.apply(&rust)))?;
    (name != rust).then_some(name)
}

fn named_fields(fields: &syn::Fields) -> Vec<&syn::Field> {
    match fields {
        syn::Fields::Named(named) => named.named.iter().collect(),
        syn::Fields::Unnamed(_) | syn::Fields::Unit => Vec::new(),
    }
}

fn ts_casing(attrs: &[Attribute], key: &str) -> syn::Result<Option<NameCase>> {
    let mut casing = None;
    for attr in attrs
        .iter()
        .filter(|attr| attr.path().is_ident("evenframe"))
    {
        attr.parse_nested_meta(|meta| {
            if !meta.path.is_ident(key) {
                return Err(meta.error(format!("expected #[evenframe({key} = \"...\")] here")));
            }
            if casing.is_some() {
                return Err(meta.error(format!("duplicate evenframe {key}")));
            }
            casing = Some(
                meta.value()
                    .and_then(|value| value.parse::<LitStr>())
                    .and_then(|literal| NameCase::parse(&literal))
                    .map_err(|error| meta.error(format!("invalid {key}: {error}")))?,
            );
            Ok(())
        })
        .map_err(|error| {
            syn::Error::new(
                error.span(),
                format!("invalid Evenframe naming attribute: {error}"),
            )
        })?;
    }
    Ok(casing)
}

fn serde_rule(attrs: &[Attribute], key: &str) -> syn::Result<bool> {
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("serde")) {
        let metas = attr
            .parse_args_with(syn::punctuated::Punctuated::<syn::Meta, Token![,]>::parse_terminated)
            .map_err(|error| {
                syn::Error::new(error.span(), format!("reading serde {key}: {error}"))
            })?;
        if metas.iter().any(|meta| meta.path().is_ident(key)) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn ts_fields(
    fields: &syn::Fields,
    casing: Option<NameCase>,
    serde_explicit: bool,
    wires: &mut [Wire],
) -> syn::Result<()> {
    // An unnamed field has no name to case, so its `ts_name` names nothing,
    // as serde's `rename` there does.
    for field in fields.iter().filter(|field| field.ident.is_none()) {
        ts_casing(&field.attrs, "ts_name")
            .map_err(|error| syn::Error::new(error.span(), format!("unnamed field: {error}")))?;
    }
    for (field, wire) in named_fields(fields).into_iter().zip(wires) {
        if let Some(ident) = &field.ident {
            let own = ts_casing(&field.attrs, "ts_name").map_err(|error| {
                syn::Error::new(error.span(), format!("field '{ident}': {error}"))
            })?;
            let serde_named = serde_explicit
                || serde_rule(&field.attrs, "rename").map_err(|error| {
                    syn::Error::new(error.span(), format!("field '{ident}': {error}"))
                })?;
            wire.typescript = match own.or(casing) {
                Some(casing) => Some(casing.apply_ts(&unraw(ident))),
                None if serde_named => Some(wire.serde.clone().unwrap_or_else(|| unraw(ident))),
                None => None,
            };
        }
    }
    Ok(())
}

fn apply_typescript(input: &DeriveInput, item: &mut ItemWire) -> syn::Result<()> {
    let casing = ts_casing(&input.attrs, "all_ts_names")
        .map_err(|error| syn::Error::new(error.span(), format!("container attributes: {error}")))?;
    match &input.data {
        syn::Data::Struct(data) => ts_fields(
            &data.fields,
            casing,
            serde_rule(&input.attrs, "rename_all").map_err(|error| {
                syn::Error::new(error.span(), format!("struct serde naming: {error}"))
            })?,
            &mut item.fields,
        ),
        syn::Data::Enum(data) => {
            let serde_explicit =
                serde_rule(&input.attrs, "rename_all_fields").map_err(|error| {
                    syn::Error::new(error.span(), format!("enum serde naming: {error}"))
                })?;
            for (variant, wire) in data.variants.iter().zip(&mut item.variants) {
                let own = ts_casing(&variant.attrs, "all_ts_names").map_err(|error| {
                    syn::Error::new(
                        error.span(),
                        format!("variant '{}': {error}", variant.ident),
                    )
                })?;
                ts_fields(
                    &variant.fields,
                    own.or(casing),
                    serde_explicit
                        || serde_rule(&variant.attrs, "rename_all").map_err(|error| {
                            syn::Error::new(
                                error.span(),
                                format!("variant '{}': {error}", variant.ident),
                            )
                        })?,
                    &mut wire.fields,
                )
                .map_err(|error| {
                    syn::Error::new(
                        error.span(),
                        format!("variant '{}': {error}", variant.ident),
                    )
                })?;
            }
            Ok(())
        }
        syn::Data::Union(_) => Ok(()),
    }
}

/// The casing an item's `#[surreal]` keys give its fields or variants.
fn surreal_casing(own: &SurrealAttributes, span: Span) -> syn::Result<Option<NameCase>> {
    match own.casing() {
        None => Ok(None),
        Some(SurrealCasing::Uppercase) => Ok(Some(NameCase::Uppercase)),
        Some(SurrealCasing::Lowercase) => Ok(Some(NameCase::Lowercase)),
        Some(SurrealCasing::Named(casing)) => NameCase::parse(&LitStr::new(casing, span)).map(Some),
    }
}

/// Each named field's stored name and how the database stores it: serde's
/// shape, a serde-skipped field kept by its own `#[surreal]` keys,
/// `#[schemasync(retain)]` or its container's, and each `#[surreal]` key
/// overriding.
fn apply_fields(
    fields: &syn::Fields,
    casing: Option<NameCase>,
    wires: &mut [Wire],
    handling: &mut [FieldHandling],
    container_retained: bool,
) -> syn::Result<()> {
    for ((field, wire), handling) in named_fields(fields).into_iter().zip(wires).zip(handling) {
        let own = SurrealAttributes::parse(&field.attrs, Position::Field)?;
        let retained =
            container_retained || own.present || SchemasyncAttributes::parse(&field.attrs)?.retain;
        if let Some(ident) = &field.ident {
            wire.surreal = surreal_name(ident, &own, casing);
        }
        wire.storage.skipped = own.skip || (handling.skip_serializing && !retained);
        wire.storage.flatten = wire.serde_flatten || own.flatten;
        wire.storage.opaque = own.wrap || handling.custom_serde;
        handling.storage_reads = !own.skip && (!handling.skip_deserializing || retained);
        if let Some(default) = &own.default {
            handling.stored_default = default.into();
        }
    }
    Ok(())
}

/// Each element's `#[surreal(wrap)]`.
fn opaque_elements(fields: &syn::Fields) -> syn::Result<Vec<bool>> {
    match fields {
        syn::Fields::Unnamed(unnamed) => unnamed
            .unnamed
            .iter()
            .map(|field| Ok(SurrealAttributes::parse(&field.attrs, Position::Element)?.wrap))
            .collect(),
        syn::Fields::Named(_) | syn::Fields::Unit => Ok(Vec::new()),
    }
}

/// The representation an enum's `#[surreal]` keys store it in, if they name one.
fn surreal_representation(container: &SurrealAttributes) -> Option<EnumRepresentation> {
    if container.untagged {
        return Some(EnumRepresentation::Untagged);
    }
    match (&container.tag, &container.content) {
        (Some(tag), Some(content)) => Some(EnumRepresentation::AdjacentlyTagged {
            tag: tag.clone(),
            content: content.clone(),
        }),
        (Some(tag), None) => Some(EnumRepresentation::InternallyTagged { tag: tag.clone() }),
        (None, _) => None,
    }
}

fn apply_surreal(input: &DeriveInput, item: &mut ItemWire) -> syn::Result<()> {
    let container_retained = SchemasyncAttributes::parse(&input.attrs)?.retain;
    let span = input.ident.span();
    match &input.data {
        syn::Data::Struct(data) => {
            let position = match data.fields {
                syn::Fields::Named(_) => Position::Struct,
                syn::Fields::Unnamed(_) => Position::TupleStruct,
                syn::Fields::Unit => Position::UnitStruct,
            };
            let container = SurrealAttributes::parse(&input.attrs, position)?;
            item.stored_container_default = match &container.default {
                Some(default) => default.into(),
                None => item.container_default.clone(),
            };
            item.stored_tuple = container.tuple;
            item.unit_value = container.value.clone();
            item.opaque_elements = opaque_elements(&data.fields)?;
            apply_fields(
                &data.fields,
                surreal_casing(&container, span)?,
                &mut item.fields,
                &mut item.handling,
                container_retained,
            )
        }
        syn::Data::Enum(data) => {
            let container = SurrealAttributes::parse(&input.attrs, Position::Enum)?;
            let representation = surreal_representation(&container);
            let casing = surreal_casing(&container, span)?;
            let mut others = 0;
            for (variant, wire) in data.variants.iter().zip(&mut item.variants) {
                let position = match variant.fields {
                    syn::Fields::Named(_) => Position::StructVariant,
                    syn::Fields::Unnamed(_) => Position::TupleVariant,
                    syn::Fields::Unit => Position::UnitVariant,
                };
                let own = SurrealAttributes::parse(&variant.attrs, position)?;
                let retained = container_retained
                    || own.present
                    || SchemasyncAttributes::parse(&variant.attrs)?.retain;
                wire.wire.surreal = surreal_name(&variant.ident, &own, casing);
                let stored = match (&representation, wire.wire.serde_untagged) {
                    (Some(representation), _) => representation.clone(),
                    (None, true) => EnumRepresentation::Untagged,
                    (None, false) => item.representation.clone(),
                };
                let refuse = |message: &str| Err(syn::Error::new(variant.span(), message));
                let skip_content = own
                    .skip_content
                    .clone()
                    .or_else(|| container.skip_content.clone());
                let content = match (&skip_content, &stored) {
                    (None, _) => ContentStorage::Always,
                    (Some(SkipContent::Always), EnumRepresentation::AdjacentlyTagged { .. }) => {
                        ContentStorage::Never
                    }
                    (Some(SkipContent::If(_)), EnumRepresentation::AdjacentlyTagged { .. }) => {
                        ContentStorage::Sometimes
                    }
                    // An internally tagged variant has no content key to leave out.
                    (Some(_), EnumRepresentation::InternallyTagged { .. }) => {
                        ContentStorage::Always
                    }
                    (Some(_), _) => {
                        return refuse(
                            "#[surreal(skip_content)] leaves out a tagged variant's content key; \
                             this enum stores this variant without a tag",
                        );
                    }
                };
                let value = match (&own.value, &stored, &variant.fields) {
                    (Some(value), EnumRepresentation::Untagged, _) => Some(value.surql()),
                    (Some(_), _, _) => {
                        return refuse(
                            "#[surreal(value)] stores an untagged unit variant; this enum stores \
                             its variants tagged",
                        );
                    }
                    // serde writes an untagged unit variant as null.
                    (None, EnumRepresentation::Untagged, syn::Fields::Unit) => {
                        Some("NULL".to_owned())
                    }
                    (None, _, _) => None,
                };
                if own.other {
                    others += 1;
                    if others > 1 {
                        return refuse(
                            "only one variant can read what no other variant reads with \
                             #[surreal(other)]",
                        );
                    }
                }
                wire.wire.storage = Storage {
                    skipped: wire.skip_serializing && !retained,
                    representation: representation.clone(),
                    value,
                    other: own.other,
                    tuple: own.tuple,
                    content,
                    opaque_elements: opaque_elements(&variant.fields)?,
                    ..Storage::default()
                };
                wire.skip_content_if = match (skip_content, content) {
                    (Some(SkipContent::If(predicate)), ContentStorage::Sometimes) => {
                        Some(predicate)
                    }
                    _ => None,
                };
                wire.unit_value = own.value.clone();
                apply_fields(
                    &variant.fields,
                    surreal_casing(&own, variant.span())?,
                    &mut wire.fields,
                    &mut wire.handling,
                    container_retained,
                )?;
            }
            Ok(())
        }
        syn::Data::Union(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::{ItemShape, ItemWire, resolve};
    use crate::types::EnumRepresentation;
    use proc_macro2::Span;

    fn wire(source: &str) -> ItemWire {
        resolve(&syn::parse_str(source).expect("test item parses")).expect("names resolve")
    }

    fn rejection(source: &str) -> String {
        match resolve(&syn::parse_str(source).expect("test item parses")) {
            Ok(_) => panic!("expected a rejection for {source}"),
            Err(error) => error.to_string(),
        }
    }

    fn serde_names(item: &ItemWire) -> Vec<Option<&str>> {
        item.fields
            .iter()
            .map(|wire| wire.serde.as_deref())
            .collect()
    }

    fn surreal_names(item: &ItemWire) -> Vec<Option<&str>> {
        item.fields
            .iter()
            .map(|wire| wire.surreal.as_deref())
            .collect()
    }

    #[test]
    fn an_unrenamed_field_keeps_its_rust_name() {
        let item = wire("struct User { first_name: String, r#type: String }");
        assert_eq!(serde_names(&item), [None, None]);
        assert_eq!(surreal_names(&item), [None, None]);
    }

    #[test]
    fn typescript_overrides_support_every_case_style() {
        for (casing, expected) in [
            ("lowercase", "first_name"),
            ("UPPERCASE", "FIRST_NAME"),
            ("PascalCase", "FirstName"),
            ("camelCase", "firstName"),
            ("snake_case", "first_name"),
            ("SCREAMING_SNAKE_CASE", "FIRST_NAME"),
            ("kebab-case", "first-name"),
            ("SCREAMING-KEBAB-CASE", "FIRST-NAME"),
        ] {
            let item = wire(&format!(
                "#[evenframe(all_ts_names = \"{casing}\")] struct User {{ first_name: String }}"
            ));
            assert_eq!(item.fields[0].typescript.as_deref(), Some(expected));
            assert_eq!(item.fields[0].serde, None);
            assert_eq!(item.fields[0].surreal, None);
        }
    }

    #[test]
    fn ts_naming_on_unnamed_fields_names_nothing_as_serde_rename_there() {
        for source in [
            "struct User(#[evenframe(ts_name = \"snake_case\")] String);",
            "#[evenframe(all_ts_names = \"snake_case\")] struct User(String);",
            "#[evenframe(all_ts_names = \"snake_case\")] enum Status { Active }",
        ] {
            let item = wire(source);
            assert!(item.fields.iter().all(|field| field.typescript.is_none()));
        }
    }

    #[test]
    fn invalid_ts_naming_attributes_are_rejected() {
        for source in [
            "#[evenframe(ts_name = \"snake_case\")] struct User { first_name: String }",
            "struct User { #[evenframe(all_ts_names = \"snake_case\")] first_name: String }",
            "struct User { #[evenframe(ts_name = \"invalid\")] first_name: String }",
            "struct User(#[evenframe(ts_name = \"invalid\")] String);",
            "struct User { #[evenframe(ts_name = \"snake_case\", ts_name = \"camelCase\")] first_name: String }",
        ] {
            assert!(!rejection(source).is_empty());
        }
    }

    #[test]
    fn ts_and_database_casing_keep_their_respective_digit_boundaries() {
        let item = wire(
            r#"#[evenframe(all_ts_names = "camelCase")]
            #[surreal(rename_all = "camelCase")]
            struct Coordinates { point_2d: String }"#,
        );
        assert_eq!(item.fields[0].typescript.as_deref(), Some("point2D"));
        assert_eq!(item.fields[0].surreal.as_deref(), Some("point2d"));
    }

    #[test]
    fn serde_rename_all_and_rename_apply_as_serde_applies_them() {
        let item = wire(
            r#"#[serde(rename_all = "camelCase")]
            struct User { first_name: String, #[serde(rename = "LAST")] last_name: String, id: String }"#,
        );
        assert_eq!(serde_names(&item), [Some("firstName"), Some("LAST"), None]);
        assert_eq!(surreal_names(&item), [None, None, None]);
    }

    #[test]
    fn surreal_rename_all_and_rename_apply_as_surreal_value_applies_them() {
        let item = wire(
            r#"#[surreal(rename_all = "kebab-case")]
            struct User { first_name: String, #[surreal(rename = "surname")] last_name: String }"#,
        );
        assert_eq!(surreal_names(&item), [Some("first-name"), Some("surname")]);
        assert_eq!(serde_names(&item), [None, None]);
    }

    #[test]
    fn variants_follow_the_enum_and_their_fields_follow_the_variant() {
        let item = wire(
            r#"#[serde(rename_all = "snake_case", rename_all_fields = "camelCase")]
            #[surreal(rename_all = "lowercase")]
            enum Event {
                SignedUp { user_name: String },
                #[surreal(rename_all = "camelCase")]
                LoggedIn { user_name: String },
                #[serde(rename = "gone")]
                Deleted,
            }"#,
        );
        let serde: Vec<_> = item
            .variants
            .iter()
            .map(|variant| variant.wire.serde.as_deref())
            .collect();
        assert_eq!(serde, [Some("signed_up"), Some("logged_in"), Some("gone")]);
        let surreal: Vec<_> = item
            .variants
            .iter()
            .map(|variant| variant.wire.surreal.as_deref())
            .collect();
        assert_eq!(
            surreal,
            [Some("signedup"), Some("loggedin"), Some("deleted")]
        );
        assert_eq!(
            item.variants[0].fields[0].serde.as_deref(),
            Some("userName")
        );
        assert_eq!(item.variants[0].fields[0].surreal, None);
        assert_eq!(
            item.variants[1].fields[0].surreal.as_deref(),
            Some("userName")
        );
    }

    #[test]
    fn the_enum_representation_comes_from_serde() {
        let item = wire(r#"#[serde(tag = "kind", content = "data")] enum Shape { Circle(f64) }"#);
        assert_eq!(
            item.representation,
            EnumRepresentation::AdjacentlyTagged {
                tag: "kind".to_owned(),
                content: "data".to_owned()
            }
        );
    }

    #[test]
    fn skip_and_skip_serializing_if_are_recorded() {
        let item = wire(
            r#"struct User {
                #[serde(skip)] cache: String,
                #[serde(skip_serializing_if = "Option::is_none")] nickname: Option<String>,
            }"#,
        );
        assert!(item.fields[0].serde_skipped);
        assert!(item.fields[1].serde_optional);
    }

    #[test]
    fn a_key_skipped_one_way_is_optional_in_json() {
        let item = wire(
            r#"struct User {
                #[serde(skip_serializing)] password: String,
                #[serde(skip_deserializing)] created: String,
            }"#,
        );
        for (wire, handling) in item.fields.iter().zip(&item.handling) {
            assert!(!wire.serde_skipped);
            assert!(wire.serde_optional);
            assert!(handling.skip_serializing != handling.skip_deserializing);
        }
        assert!(item.handling[0].skip_serializing);
        assert!(item.handling[1].skip_deserializing);
        // Serde never writes the first, so the database never holds it.
        assert!(item.fields[0].storage.skipped);
        assert!(!item.fields[1].storage.skipped);
    }

    #[test]
    fn a_field_serde_writes_beside_its_siblings_is_marked_flattened() {
        let item = wire("struct Outer { name: String, #[serde(flatten)] inner: Inner }");
        assert!(!item.fields[0].serde_flatten);
        assert!(item.fields[1].serde_flatten);
        assert!(item.handling[1].flatten);
    }

    #[test]
    fn a_variant_serde_writes_bare_is_marked_untagged() {
        let item = wire(
            r#"#[serde(tag = "kind")] enum Contact { Phone { number: String }, #[serde(untagged)] Email(String) }"#,
        );
        assert!(!item.variants[0].wire.serde_untagged);
        assert!(item.variants[1].wire.serde_untagged);
    }

    #[test]
    fn a_split_rename_writes_one_name_and_reads_the_other_too() {
        let item = wire(
            r#"struct User {
                #[serde(rename(serialize = "fullName", deserialize = "full_name"))] name: String,
                #[serde(rename(serialize = "nick"), alias = "handle")] nickname: String,
            }"#,
        );
        assert_eq!(serde_names(&item), [Some("fullName"), Some("nick")]);
        assert_eq!(item.handling[0].aliases, ["full_name"]);
        assert_eq!(item.handling[1].aliases, ["handle", "nickname"]);

        let item = wire(
            r#"enum Shape { #[serde(rename(serialize = "circle", deserialize = "Circle"))] Circle }"#,
        );
        assert_eq!(item.variants[0].wire.serde.as_deref(), Some("circle"));
        assert_eq!(item.variants[0].aliases, ["Circle"]);
    }

    #[test]
    fn surreal_keys_override_storage_and_leave_serde_alone() {
        let outer = wire("struct Outer { #[surreal(flatten)] inner: Inner }");
        assert!(outer.fields[0].storage.flatten);
        assert!(!outer.fields[0].serde_flatten);

        let shape = wire(
            r#"#[serde(tag = "kind")] #[surreal(tag = "type")] enum Shape { Circle { radius: f64 } }"#,
        );
        assert_eq!(
            shape.representation,
            EnumRepresentation::InternallyTagged {
                tag: "kind".to_owned()
            }
        );
        assert_eq!(
            shape.variants[0].wire.storage.representation,
            Some(EnumRepresentation::InternallyTagged {
                tag: "type".to_owned()
            })
        );

        assert!(
            rejection(r#"enum Code { #[surreal(value = 1)] One }"#)
                .contains("untagged unit variant")
        );
        assert!(
            rejection(r#"enum Code { #[surreal(skip_content)] One(u8) }"#)
                .contains("without a tag")
        );
        assert!(
            rejection(r#"enum Code { #[surreal(other)] One, #[surreal(other)] Two }"#)
                .contains("only one variant")
        );
    }

    #[test]
    fn serde_skips_are_stored_only_when_retained() {
        let item = wire(
            r#"struct Account {
                #[serde(skip)] scratch: String,
                #[serde(skip)] #[schemasync(retain)] cache: String,
                #[serde(skip)] #[surreal(rename = "kept")] kept: String,
                #[surreal(skip)] session: String,
            }"#,
        );
        let skipped: Vec<bool> = item
            .fields
            .iter()
            .map(|wire| wire.storage.skipped)
            .collect();
        assert_eq!(skipped, [true, false, false, true]);

        let retained =
            wire(r#"#[schemasync(retain)] struct Draft { #[serde(skip)] note: String }"#);
        assert!(!retained.fields[0].storage.skipped);

        let phase = wire(
            r#"enum Phase { Open, #[serde(skip)] Hidden, #[serde(skip)] #[schemasync(retain)] Archived }"#,
        );
        let skipped: Vec<bool> = phase
            .variants
            .iter()
            .map(|variant| variant.wire.storage.skipped)
            .collect();
        assert_eq!(skipped, [false, true, false]);

        let amount = wire(r#"#[serde(untagged)] enum Amount { Whole(i64), Unknown }"#);
        assert_eq!(
            amount.variants[1].wire.storage.value.as_deref(),
            Some("NULL")
        );
    }

    #[test]
    fn a_struct_written_as_its_one_field_is_a_newtype() {
        let field = |name: &str| syn::Member::Named(syn::Ident::new(name, Span::call_site()));
        let first = syn::Member::Unnamed(syn::Index::from(0));
        assert_eq!(
            wire("struct NonEmptyString(String);").shape,
            ItemShape::Newtype {
                member: first.clone()
            }
        );
        assert_eq!(
            wire("#[serde(transparent)] struct Id { value: String }").shape,
            ItemShape::Newtype {
                member: field("value")
            }
        );
        assert_eq!(
            wire(
                "#[serde(transparent)] struct Tagged { #[serde(skip)] marker: (), value: String }"
            )
            .shape,
            ItemShape::Newtype {
                member: field("value")
            }
        );
        assert_eq!(
            wire("#[serde(transparent)] struct Wrapped(String);").shape,
            ItemShape::Newtype { member: first }
        );
        assert_eq!(wire("struct Pair(String, u32);").shape, ItemShape::Tuple(2));
        assert_eq!(wire("struct Marker;").shape, ItemShape::Unit);
        assert_eq!(wire("struct User { name: String }").shape, ItemShape::Named);
    }

    #[test]
    fn surreal_keys_that_keep_names_are_accepted() {
        let item = wire(
            r#"#[surreal(crate = "surrealdb::types")]
            struct User { #[surreal(default)] name: String, #[surreal(default = "zero")] count: u32 }"#,
        );
        assert_eq!(surreal_names(&item), [None, None]);
    }
}
