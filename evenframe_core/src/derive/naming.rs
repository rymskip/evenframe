//! The names a type's fields and variants take outside Rust: serde's in JSON
//! and TypeScript, SurrealValue's in the database. The derive and the scanner
//! both resolve them here, so the two cannot disagree.

use crate::types::{EnumRepresentation, Wire};
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
#[derive(Debug, Default)]
pub struct ItemWire {
    /// A struct's named fields.
    pub fields: Vec<Wire>,
    pub variants: Vec<VariantWire>,
    /// An enum's representation in serde's JSON.
    pub representation: EnumRepresentation,
}

#[derive(Debug, Default)]
pub struct VariantWire {
    pub wire: Wire,
    /// A struct variant's named fields.
    pub fields: Vec<Wire>,
}

/// Resolves the serde and SurrealValue names of `input`'s fields and
/// variants, rejecting the attributes that give a field no single key.
pub fn resolve(input: &DeriveInput) -> syn::Result<ItemWire> {
    let mut item = serde_wire(input)?;
    apply_surreal(input, &mut item)?;
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
        .map(|container| container_wire(&context, &container));
    context.check()?;
    // `from_ast` returns nothing only after recording an error, which `check` returned.
    item.ok_or_else(|| syn::Error::new(input.ident.span(), "serde cannot describe a union"))
}

fn container_wire(context: &Ctxt, container: &Container) -> ItemWire {
    if container.attrs.transparent() {
        context.error_spanned_by(
            &container.ident,
            "#[serde(transparent)] is not supported: evenframe describes a type by its own fields, \
             and serde writes this one as its inner field",
        );
    }
    if container.attrs.type_into().is_some() {
        context.error_spanned_by(
            &container.ident,
            "#[serde(into = \"...\")] is not supported: serde writes this type as another type, \
             which evenframe cannot describe from these fields",
        );
    }
    match &container.data {
        Data::Struct(Style::Struct, fields) => ItemWire {
            fields: fields
                .iter()
                .map(|field| field_wire(context, field))
                .collect(),
            ..ItemWire::default()
        },
        Data::Struct(_, _) => ItemWire::default(),
        Data::Enum(variants) => ItemWire {
            variants: variants
                .iter()
                .map(|variant| {
                    if variant.attrs.untagged() {
                        context.error_spanned_by(
                            variant.original,
                            "#[serde(untagged)] on a single variant is not supported: \
                             evenframe gives every variant of an enum the same representation",
                        );
                    }
                    let skipped = skipped(
                        context,
                        variant.original,
                        variant.attrs.skip_serializing(),
                        variant.attrs.skip_deserializing(),
                    );
                    VariantWire {
                        wire: Wire {
                            serde: renamed(
                                context,
                                variant.original,
                                &variant.ident,
                                variant.attrs.name(),
                            ),
                            serde_skipped: skipped,
                            ..Wire::default()
                        },
                        fields: match variant.style {
                            Style::Struct => variant
                                .fields
                                .iter()
                                .map(|field| field_wire(context, field))
                                .collect(),
                            Style::Tuple | Style::Newtype | Style::Unit => Vec::new(),
                        },
                    }
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
            ..ItemWire::default()
        },
    }
}

fn field_wire(context: &Ctxt, field: &Field) -> Wire {
    if field.attrs.flatten() {
        context.error_spanned_by(
            field.original,
            "#[serde(flatten)] is not supported: evenframe gives every field its own key",
        );
    }
    let serde = match &field.member {
        syn::Member::Named(ident) => renamed(context, field.original, ident, field.attrs.name()),
        syn::Member::Unnamed(_) => None,
    };
    Wire {
        serde,
        serde_skipped: skipped(
            context,
            field.original,
            field.attrs.skip_serializing(),
            field.attrs.skip_deserializing(),
        ),
        serde_optional: field.attrs.skip_serializing_if().is_some(),
        surreal: None,
    }
}

/// Serde's name for an item when it differs from the Rust name. TypeScript
/// describes JSON in both directions, so the two names must agree.
fn renamed<T: quote::ToTokens>(
    context: &Ctxt,
    span: T,
    ident: &Ident,
    name: &serde_derive_internals::name::MultiName,
) -> Option<String> {
    let serialized = &name.serialize_name().value;
    if *serialized != name.deserialize_name().value {
        context.error_spanned_by(
            span,
            format!(
                "serde writes this as \"{serialized}\" but reads \"{}\"; evenframe needs one name \
                 for both, so use #[serde(rename = \"...\")] with #[serde(alias = \"...\")] instead",
                name.deserialize_name().value
            ),
        );
    }
    (*serialized != unraw(ident)).then(|| serialized.clone())
}

/// Whether serde skips an item entirely. Skipping it one way only leaves JSON
/// that cannot round trip, which no TypeScript type describes.
fn skipped<T: quote::ToTokens>(
    context: &Ctxt,
    span: T,
    skip_serializing: bool,
    skip_deserializing: bool,
) -> bool {
    if skip_serializing != skip_deserializing {
        context.error_spanned_by(
            span,
            "skipping only one direction is not supported: serde would write and read different \
             shapes; use #[serde(skip)], or #[serde(skip_serializing_if = \"...\")] for an \
             optional key",
        );
    }
    skip_serializing && skip_deserializing
}

// ----- SurrealValue ----------------------------------------------------------

#[derive(Clone, Copy)]
enum Casing {
    Lowercase,
    Uppercase,
    PascalCase,
    CamelCase,
    SnakeCase,
    ScreamingSnake,
    KebabCase,
    ScreamingKebab,
}

impl Casing {
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
                    format!("unknown #[surreal(rename_all = \"{other}\")] casing"),
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
}

/// The `#[surreal(...)]` keys that decide names and shape. Keys that change
/// neither, such as `crate`, `default` or `skip_content`, are accepted as is.
#[derive(Default)]
struct SurrealAttributes {
    rename: Option<String>,
    rename_all: Option<Casing>,
    tag: Option<String>,
    content: Option<String>,
    untagged: bool,
}

impl SurrealAttributes {
    fn parse(attrs: &[Attribute]) -> syn::Result<Self> {
        let mut parsed = Self::default();
        for attr in attrs.iter().filter(|attr| attr.path().is_ident("surreal")) {
            attr.parse_nested_meta(|meta| {
                let key = meta
                    .path
                    .get_ident()
                    .map(Ident::to_string)
                    .unwrap_or_default();
                match key.as_str() {
                    "rename" => parsed.rename = Some(meta.value()?.parse::<LitStr>()?.value()),
                    "rename_all" => {
                        parsed.rename_all = Some(Casing::parse(&meta.value()?.parse()?)?)
                    }
                    "lowercase" => parsed.rename_all = Some(Casing::Lowercase),
                    "uppercase" => parsed.rename_all = Some(Casing::Uppercase),
                    "tag" => parsed.tag = Some(meta.value()?.parse::<LitStr>()?.value()),
                    "content" => parsed.content = Some(meta.value()?.parse::<LitStr>()?.value()),
                    "untagged" => parsed.untagged = true,
                    "flatten" => {
                        return Err(meta.error(
                            "#[surreal(flatten)] is not supported: evenframe gives every field \
                             its own key",
                        ));
                    }
                    "value" | "tuple" => {
                        return Err(meta.error(format!(
                            "#[surreal({key})] is not supported: it stores the variant in a shape \
                             evenframe's schema does not describe"
                        )));
                    }
                    _ => {
                        if meta.input.peek(Token![=]) {
                            meta.value()?.parse::<syn::Expr>()?;
                        }
                    }
                }
                Ok(())
            })?;
        }
        Ok(parsed)
    }

    fn representation(&self) -> Option<EnumRepresentation> {
        if self.untagged {
            return Some(EnumRepresentation::Untagged);
        }
        match (&self.tag, &self.content) {
            (Some(tag), Some(content)) => Some(EnumRepresentation::AdjacentlyTagged {
                tag: tag.clone(),
                content: content.clone(),
            }),
            (Some(tag), None) => Some(EnumRepresentation::InternallyTagged { tag: tag.clone() }),
            (None, _) => None,
        }
    }
}

/// SurrealValue's name for an item when it differs from the Rust name.
fn surreal_name(ident: &Ident, own: &SurrealAttributes, casing: Option<Casing>) -> Option<String> {
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

fn apply_fields(
    fields: &syn::Fields,
    casing: Option<Casing>,
    wires: &mut [Wire],
) -> syn::Result<()> {
    for (field, wire) in named_fields(fields).into_iter().zip(wires) {
        let own = SurrealAttributes::parse(&field.attrs)?;
        if let Some(ident) = &field.ident {
            wire.surreal = surreal_name(ident, &own, casing);
        }
    }
    Ok(())
}

fn apply_surreal(input: &DeriveInput, item: &mut ItemWire) -> syn::Result<()> {
    let container = SurrealAttributes::parse(&input.attrs)?;
    match &input.data {
        syn::Data::Struct(data) => {
            apply_fields(&data.fields, container.rename_all, &mut item.fields)
        }
        syn::Data::Enum(data) => {
            if let Some(surreal) = container.representation()
                && surreal != item.representation
            {
                return Err(syn::Error::new(
                    input.ident.span(),
                    format!(
                        "SurrealValue stores this enum as {surreal:?} but serde writes it as {:?}; \
                         evenframe describes one representation, so give both the same \
                         tag, content or untagged attributes",
                        item.representation
                    ),
                ));
            }
            for (variant, wire) in data.variants.iter().zip(&mut item.variants) {
                let own = SurrealAttributes::parse(&variant.attrs)?;
                if own.representation().is_some() {
                    return Err(syn::Error::new(
                        variant.span(),
                        "a variant cannot set its own SurrealValue representation",
                    ));
                }
                wire.wire.surreal = surreal_name(&variant.ident, &own, container.rename_all);
                apply_fields(&variant.fields, own.rename_all, &mut wire.fields)?;
            }
            Ok(())
        }
        syn::Data::Union(_) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::{ItemWire, resolve};
    use crate::types::EnumRepresentation;

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
    fn shapes_without_one_key_per_field_are_rejected() {
        assert!(rejection("struct Outer { #[serde(flatten)] inner: Inner }").contains("flatten"));
        assert!(rejection("struct Outer { #[surreal(flatten)] inner: Inner }").contains("flatten"));
        assert!(
            rejection(r#"struct User { #[serde(rename(serialize = "a", deserialize = "b"))] name: String }"#)
                .contains("writes this as \"a\" but reads \"b\"")
        );
        assert!(
            rejection("struct User { #[serde(skip_serializing)] name: String }")
                .contains("only one direction")
        );
        assert!(
            rejection("#[serde(transparent)] struct Id { value: String }").contains("transparent")
        );
        assert!(
            rejection(r#"#[serde(into = "String")] struct Id { value: String }"#).contains("into")
        );
        assert!(
            rejection(r#"#[serde(tag = "kind")] #[surreal(tag = "type")] enum Shape { Circle { radius: f64 } }"#)
                .contains("SurrealValue stores this enum")
        );
        assert!(rejection(r#"enum Code { #[surreal(value = 1)] One }"#).contains("value"));
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
