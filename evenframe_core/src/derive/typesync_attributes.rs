//! `#[typesync(...)]`: what the TypeScript outputs add to an item beside what
//! evenframe describes for it.
//!
//! - `annotation("...")` is written as `/** ... */` before the item. A
//!   Macroforge `@derive(...)` written this way is warned about: derives go in
//!   `macroforge(derives = [...])`, which replaces the output's `default_derives`.
//! - `macroforge(derives = [A, B], attributes = [...])` adds Macroforge derives
//!   to the item's `@derive(...)`, and writes each attribute as a JSDoc
//!   annotation: `name` as `@name`, `name(key = value, flag)` as
//!   `@name({ key: value, flag: true })`, with each key in camelCase.
//! - `validators(...)` replaces a field's, element's or newtype's
//!   `#[validators(...)]` in the TypeScript outputs. Its custom patterns run
//!   only in JavaScript, so each is a regex literal such as `"/^\p{L}+$/u"`.

use crate::{
    derive::validator_parser::parse_validator_list, schemasync::format::PatternDialect,
    validator::Validator,
};
use convert_case::{Case, Casing};
use proc_macro2::{Span, TokenStream};
use quote::quote_spanned;
use syn::{
    Attribute, Data, DeriveInput, Expr, Fields, Ident, Lit, LitStr, Meta, Token, UnOp, bracketed,
    punctuated::Punctuated,
};

/// Where an attribute sits, which decides whether it takes derives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    /// A struct or enum.
    Container,
    /// A variant of named fields, which the outputs declare as a type of its
    /// own.
    StructVariant,
    /// A tuple or unit variant.
    Variant,
    Field,
}

impl Position {
    fn takes_derives(self) -> bool {
        matches!(self, Position::Container | Position::StructVariant)
    }

    /// A value's validators sit on a field, a tuple element or a newtype.
    fn takes_validators(self) -> bool {
        matches!(self, Position::Container | Position::Field)
    }
}

/// An entry evenframe reads but steers away from, reported as a compile
/// warning where it is written.
#[derive(Debug, Clone)]
pub struct Warning {
    pub span: Span,
    pub note: String,
}

impl Warning {
    /// Tokens that make rustc warn with the note at the entry: a use of a
    /// deprecated constant, since a derive on stable Rust has no warning of its
    /// own.
    pub fn to_tokens(&self) -> TokenStream {
        let note = &self.note;
        quote_spanned! {self.span=>
            const _: () = {
                #[deprecated(note = #note)]
                const TYPESYNC: () = ();
                TYPESYNC
            };
        }
    }
}

/// An item's `#[typesync(...)]` entries.
#[derive(Debug, Clone, Default)]
pub struct TypesyncAttributes {
    /// Macroforge derives, in the order written.
    pub macroforge_derives: Vec<String>,
    /// JSDoc annotations, each written as `/** {annotation} */`: every
    /// `annotation("...")` and every Macroforge attribute, in the order written.
    pub annotations: Vec<String>,
    /// `validators(...)`, replacing `#[validators(...)]` in the TypeScript
    /// outputs, its custom patterns JavaScript regex literals.
    pub validators: Option<Vec<Validator>>,
    /// Entries written in a form evenframe steers away from.
    pub warnings: Vec<Warning>,
}

impl TypesyncAttributes {
    /// Reads every `#[typesync(...)]` on `attrs` at `position`.
    pub fn parse(attrs: &[Attribute], position: Position) -> syn::Result<Self> {
        let mut parsed = Self::default();
        for attr in attrs.iter().filter(|attr| attr.path().is_ident("typesync")) {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("annotation") {
                    let content;
                    syn::parenthesized!(content in meta.input);
                    let annotation: LitStr = content.parse()?;
                    if annotation.value().trim().is_empty() {
                        return Err(syn::Error::new(
                            annotation.span(),
                            "an annotation is the text written inside its JSDoc comment, such \
                             as `annotation(\"@hidden\")`",
                        ));
                    }
                    if annotation.value().trim_start().starts_with("@derive(") {
                        parsed.warnings.push(Warning {
                            span: annotation.span(),
                            note: "a Macroforge derive is not an annotation: list it in \
                                   `#[typesync(macroforge(derives = [...]))]`, which replaces \
                                   the output's `default_derives`"
                                .to_string(),
                        });
                    }
                    parsed.annotations.push(annotation.value());
                    Ok(())
                } else if meta.path.is_ident("validators") {
                    if !position.takes_validators() {
                        return Err(meta.error(
                            "validators check a value: put them on a field, a tuple element or \
                             a newtype",
                        ));
                    }
                    if parsed.validators.is_some() {
                        return Err(meta.error("#[typesync(validators(...))] is given twice"));
                    }
                    let content;
                    syn::parenthesized!(content in meta.input);
                    let list = Punctuated::<syn::Expr, Token![,]>::parse_terminated(&content)?;
                    parsed.validators =
                        Some(parse_validator_list(&list, PatternDialect::JavaScript)?.validators);
                    Ok(())
                } else if meta.path.is_ident("macroforge") {
                    meta.parse_nested_meta(|entry| {
                        if entry.path.is_ident("derives") {
                            let derives = bracketed_list::<Ident>(&entry)?;
                            if !position.takes_derives() {
                                return Err(entry.error(
                                    "Macroforge derives apply to a type: put them on the struct, \
                                     enum or struct variant",
                                ));
                            }
                            for derive in derives {
                                let name = derive.to_string();
                                if parsed.macroforge_derives.contains(&name) {
                                    return Err(syn::Error::new(
                                        derive.span(),
                                        format!("the Macroforge derive `{name}` is listed twice"),
                                    ));
                                }
                                parsed.macroforge_derives.push(name);
                            }
                            Ok(())
                        } else if entry.path.is_ident("attributes") {
                            for attribute in bracketed_list::<Meta>(&entry)? {
                                parsed.annotations.push(macroforge_attribute(&attribute)?);
                            }
                            Ok(())
                        } else {
                            Err(entry.error(
                                "#[typesync(macroforge(...))] takes `derives = [...]` and \
                                 `attributes = [...]`",
                            ))
                        }
                    })
                } else {
                    Err(meta.error(
                        "#[typesync(...)] takes `annotation(\"...\")`, \
                         `macroforge(derives = [...], attributes = [...])` and \
                         `validators(...)`",
                    ))
                }
            })?;
        }
        Ok(parsed)
    }
}

/// The warnings for every `#[typesync(...)]` on `input`, its variants and
/// their fields. Attributes that do not parse are left to the derive, which
/// reports them as errors.
pub fn warnings(input: &DeriveInput) -> TokenStream {
    let field_attrs = |fields: &Fields| {
        fields
            .iter()
            .map(|field| (field.attrs.clone(), Position::Field))
            .collect::<Vec<_>>()
    };
    let mut sites = vec![(input.attrs.clone(), Position::Container)];
    match &input.data {
        Data::Struct(data) => sites.extend(field_attrs(&data.fields)),
        Data::Enum(data) => {
            for variant in &data.variants {
                let position = match variant.fields {
                    Fields::Named(_) => Position::StructVariant,
                    Fields::Unnamed(_) | Fields::Unit => Position::Variant,
                };
                sites.push((variant.attrs.clone(), position));
                sites.extend(field_attrs(&variant.fields));
            }
        }
        Data::Union(_) => {}
    }
    sites
        .iter()
        .filter_map(|(attrs, position)| TypesyncAttributes::parse(attrs, *position).ok())
        .flat_map(|parsed| parsed.warnings)
        .map(|warning| warning.to_tokens())
        .collect()
}

/// The comma-separated items of `key = [...]`.
fn bracketed_list<Item: syn::parse::Parse>(
    entry: &syn::meta::ParseNestedMeta,
) -> syn::Result<Punctuated<Item, Token![,]>> {
    let input = entry.value()?;
    let content;
    bracketed!(content in input);
    Punctuated::parse_terminated(&content)
}

/// A Macroforge attribute as the JSDoc annotation Macroforge reads.
fn macroforge_attribute(attribute: &Meta) -> syn::Result<String> {
    match attribute {
        Meta::Path(path) => Ok(format!("@{}", single_ident(path)?)),
        Meta::List(list) => {
            let options = list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
            Ok(format!(
                "@{}({})",
                single_ident(&list.path)?,
                object(&options)?
            ))
        }
        Meta::NameValue(name_value) => Err(syn::Error::new_spanned(
            name_value,
            "a Macroforge attribute is `name` or `name(key = value, ...)`",
        )),
    }
}

/// `key = value` and `flag` options as an object literal, each key in
/// camelCase, a flag `true` and a nested list an object of its own.
fn object(options: &Punctuated<Meta, Token![,]>) -> syn::Result<String> {
    let entries = options
        .iter()
        .map(|option| {
            let (key, value) = match option {
                Meta::Path(path) => (path, "true".to_owned()),
                Meta::NameValue(name_value) => (&name_value.path, value(&name_value.value)?),
                Meta::List(list) => (
                    &list.path,
                    object(
                        &list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?,
                    )?,
                ),
            };
            Ok(format!(
                "{}: {value}",
                single_ident(key)?.to_string().to_case(Case::Camel)
            ))
        })
        .collect::<syn::Result<Vec<_>>>()?;
    Ok(if entries.is_empty() {
        "{}".to_owned()
    } else {
        format!("{{ {} }}", entries.join(", "))
    })
}

/// A literal, a negated number or an array of them, as TypeScript writes it.
fn value(expr: &Expr) -> syn::Result<String> {
    match expr {
        Expr::Lit(literal) => match &literal.lit {
            Lit::Str(text) => string_literal(&text.value()),
            Lit::Int(number) => Ok(number.base10_digits().to_owned()),
            Lit::Float(number) => Ok(number.base10_digits().to_owned()),
            Lit::Bool(flag) => Ok(flag.value.to_string()),
            other => Err(unsupported_value(other)),
        },
        Expr::Unary(unary)
            if matches!(unary.op, UnOp::Neg(_))
                && matches!(
                    &*unary.expr,
                    Expr::Lit(literal) if matches!(literal.lit, Lit::Int(_) | Lit::Float(_))
                ) =>
        {
            Ok(format!("-{}", value(&unary.expr)?))
        }
        Expr::Array(array) => Ok(format!(
            "[{}]",
            array
                .elems
                .iter()
                .map(value)
                .collect::<syn::Result<Vec<_>>>()?
                .join(", ")
        )),
        other => Err(unsupported_value(other)),
    }
}

fn unsupported_value(value: &impl quote::ToTokens) -> syn::Error {
    syn::Error::new_spanned(
        value,
        "a Macroforge attribute's value is a string, number or boolean literal, or an array of \
         them",
    )
}

/// `text` as a TypeScript string inside a JSDoc comment, which a `*/` would
/// end.
fn string_literal(text: &str) -> syn::Result<String> {
    serde_json::to_string(text)
        .map(|quoted| quoted.replace("*/", "*\\/"))
        .map_err(|error| {
            syn::Error::new(
                proc_macro2::Span::call_site(),
                format!("the string cannot be written as TypeScript: {error}"),
            )
        })
}

fn single_ident(path: &syn::Path) -> syn::Result<&Ident> {
    path.get_ident().ok_or_else(|| {
        syn::Error::new_spanned(path, "a Macroforge attribute name or key is one identifier")
    })
}

#[cfg(test)]
mod tests {
    use super::{Position, TypesyncAttributes, warnings};

    fn parse(source: &str, position: Position) -> syn::Result<TypesyncAttributes> {
        let item: syn::DeriveInput =
            syn::parse_str(&format!("{source} struct Item;")).expect("the attributes parse");
        TypesyncAttributes::parse(&item.attrs, position)
    }

    fn rejection(source: &str, position: Position) -> String {
        match parse(source, position) {
            Ok(parsed) => panic!("expected {source} to be refused, got {parsed:?}"),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn a_derive_written_as_an_annotation_is_warned_about() {
        let parsed = parse(
            r#"#[typesync(annotation("@derive(Encode, Decode)"), annotation("@hidden"))]"#,
            Position::Container,
        )
        .expect("the entries parse");
        let notes: Vec<&str> = parsed
            .warnings
            .iter()
            .map(|warning| warning.note.as_str())
            .collect();
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(
            notes
                .iter()
                .all(|note| note.contains("macroforge(derives = [...])")),
            "{notes:?}"
        );
    }

    #[test]
    fn every_warning_on_an_item_becomes_a_deprecated_use() {
        let item: syn::DeriveInput = syn::parse_str(
            r#"#[typesync(annotation("@derive(Encode)"))]
               enum Item {
                   #[typesync(annotation("@derive(Decode)"))]
                   One { #[typesync(annotation("@derive(Clone)"))] value: u8 },
                   #[typesync(annotation("@hidden"))]
                   Two,
               }"#,
        )
        .expect("the item parses");
        let tokens = warnings(&item).to_string();
        assert_eq!(tokens.matches("deprecated").count(), 3, "{tokens}");
    }

    #[test]
    fn derives_and_annotations_are_read_in_the_order_written() {
        let parsed = parse(
            r#"#[typesync(macroforge(derives = [Default, Encode]), annotation("@hidden"))]
               #[typesync(macroforge(derives = [Decode], attributes = [endec(tag = "kind")]))]"#,
            Position::Container,
        )
        .expect("the entries parse");
        assert_eq!(parsed.macroforge_derives, ["Default", "Encode", "Decode"]);
        assert_eq!(
            parsed.annotations,
            ["@hidden", r#"@endec({ tag: "kind" })"#]
        );
    }

    #[test]
    fn a_macroforge_attribute_renders_as_the_annotation_macroforge_reads() {
        let parsed = parse(
            r#"#[typesync(macroforge(attributes = [
                hidden,
                endec(rename_all = "camelCase", deny_unknown_fields),
                input(label = "Say \"hi\" */", rows = 3, step = -0.5, options = ["a", "b"]),
                layout(grid(columns = 2), empty()),
            ]))]"#,
            Position::Field,
        )
        .expect("the attributes parse");
        assert_eq!(
            parsed.annotations,
            [
                "@hidden",
                r#"@endec({ renameAll: "camelCase", denyUnknownFields: true })"#,
                r#"@input({ label: "Say \"hi\" *\/", rows: 3, step: -0.5, options: ["a", "b"] })"#,
                "@layout({ grid: { columns: 2 }, empty: {} })",
            ]
        );
    }

    #[test]
    fn derives_apply_only_to_a_type() {
        assert!(
            parse(
                "#[typesync(macroforge(derives = [Default]))]",
                Position::StructVariant
            )
            .is_ok()
        );
        for position in [Position::Field, Position::Variant] {
            let message = rejection("#[typesync(macroforge(derives = [Default]))]", position);
            assert!(message.contains("apply to a type"), "{message}");
        }
    }

    #[test]
    fn unknown_entries_and_values_are_refused() {
        for (source, expected) in [
            (
                "#[typesync(macroforge_derive(Default))]",
                "takes `annotation",
            ),
            (
                "#[typesync(macroforge(derive = [Default]))]",
                "takes `derives",
            ),
            ("#[typesync(annotation(\"\"))]", "an annotation is"),
            (
                "#[typesync(macroforge(derives = [Default, Default]))]",
                "listed twice",
            ),
            (
                "#[typesync(macroforge(attributes = [endec = \"x\"]))]",
                "`name` or `name(",
            ),
            (
                "#[typesync(macroforge(attributes = [endec(with = some::path)]))]",
                "string, number or boolean",
            ),
            (
                "#[typesync(macroforge(attributes = [a::b]))]",
                "one identifier",
            ),
        ] {
            let message = rejection(source, Position::Container);
            assert!(message.contains(expected), "{source}: {message}");
        }
    }
}
