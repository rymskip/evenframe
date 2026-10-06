//! The `#[surreal(...)]` keys the SurrealDB SDK's `SurrealValue` derive
//! defines, read at each position they apply to. Every key the SDK defines is
//! accepted where it applies and overrides how the database stores the item;
//! any other key is an error, as are the combinations the SDK refuses.

use proc_macro2::Span;
use syn::{Attribute, Ident, Lit, LitStr, Path, Token};

/// Where an attribute sits, which decides the keys it takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Position {
    /// A struct of named fields.
    Struct,
    /// A tuple struct.
    TupleStruct,
    /// A unit struct.
    UnitStruct,
    Enum,
    /// A variant of named fields.
    StructVariant,
    TupleVariant,
    UnitVariant,
    /// A named field.
    Field,
    /// An element of a tuple struct or variant.
    Element,
}

impl Position {
    fn keys(self) -> &'static [&'static str] {
        match self {
            Position::Struct => &["crate", "rename", "rename_all", "default"],
            Position::TupleStruct => &["crate", "rename", "tuple"],
            Position::UnitStruct => &["crate", "rename", "value"],
            Position::Enum => &[
                "crate",
                "untagged",
                "tag",
                "content",
                "skip_content",
                "skip_content_if",
                "rename_all",
                "uppercase",
                "lowercase",
            ],
            Position::StructVariant => &[
                "rename",
                "rename_all",
                "default",
                "skip_content",
                "skip_content_if",
            ],
            Position::TupleVariant => &["rename", "tuple", "skip_content", "skip_content_if"],
            Position::UnitVariant => &["rename", "value", "other", "skip_content"],
            Position::Field => &["rename", "default", "wrap", "flatten", "skip"],
            Position::Element => &["wrap"],
        }
    }

    fn describe(self) -> &'static str {
        match self {
            Position::Struct => "a struct",
            Position::TupleStruct => "a tuple struct",
            Position::UnitStruct => "a unit struct",
            Position::Enum => "an enum",
            Position::StructVariant => "a struct variant",
            Position::TupleVariant => "a tuple variant",
            Position::UnitVariant => "a unit variant",
            Position::Field => "a named field",
            Position::Element => "a tuple element",
        }
    }
}

/// When an adjacently tagged variant's content key is left out.
#[derive(Clone)]
pub enum SkipContent {
    /// `skip_content`: never written.
    Always,
    /// `skip_content_if = "predicate"`: left out when the predicate holds for
    /// the content's value.
    If(Path),
}

/// What a field missing from the record takes, under `#[surreal(default)]`.
#[derive(Clone)]
pub enum SurrealDefault {
    /// `Default::default()`.
    Trait,
    /// The function `default = "..."` names.
    Function(syn::ExprPath),
}

/// A unit variant's `value`, the literal it is stored as.
#[derive(Debug, Clone, PartialEq)]
pub enum UnitValue {
    None,
    Null,
    Bool(bool),
    String(String),
    Int(i64),
    Float(f64),
}

impl UnitValue {
    /// The literal as SurrealQL writes it.
    pub fn surql(&self) -> String {
        match self {
            UnitValue::None => "NONE".to_owned(),
            UnitValue::Null => "NULL".to_owned(),
            UnitValue::Bool(value) => value.to_string(),
            UnitValue::String(value) => crate::schemasync::table::surql_string_literal(value),
            UnitValue::Int(value) => value.to_string(),
            UnitValue::Float(value) => format!("{value:?}f"),
        }
    }

    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        if input.peek(Ident) {
            let ident: Ident = input.parse()?;
            return match ident.to_string().to_lowercase().as_str() {
                "none" => Ok(UnitValue::None),
                "null" => Ok(UnitValue::Null),
                _ => Err(syn::Error::new(
                    ident.span(),
                    "a unit variant's value is a literal, `null` or `none`",
                )),
            };
        }
        let literal: Lit = input.parse()?;
        Ok(match &literal {
            Lit::Bool(value) => UnitValue::Bool(value.value),
            Lit::Str(value) => UnitValue::String(value.value()),
            Lit::Int(value) => UnitValue::Int(value.base10_parse()?),
            Lit::Float(value) => UnitValue::Float(value.base10_parse()?),
            other => {
                return Err(syn::Error::new_spanned(
                    other,
                    "a unit variant's value is a boolean, string, integer or float literal, \
                     `null` or `none`",
                ));
            }
        })
    }
}

/// An item's `#[surreal(...)]` keys.
#[derive(Clone, Default)]
pub struct SurrealAttributes {
    /// Whether the item has any `#[surreal(...)]` attribute, which opts a
    /// serde-skipped item into storage.
    pub present: bool,
    pub rename: Option<String>,
    pub rename_all: Option<String>,
    pub uppercase: bool,
    pub lowercase: bool,
    pub tag: Option<String>,
    pub content: Option<String>,
    pub untagged: bool,
    pub default: Option<SurrealDefault>,
    pub flatten: bool,
    pub wrap: bool,
    pub skip: bool,
    pub tuple: bool,
    pub value: Option<UnitValue>,
    pub other: bool,
    pub skip_content: Option<SkipContent>,
}

impl SurrealAttributes {
    /// Reads every `#[surreal(...)]` on `attrs` at `position`.
    pub fn parse(attrs: &[Attribute], position: Position) -> syn::Result<Self> {
        let mut parsed = Self::default();
        for attr in attrs.iter().filter(|attr| attr.path().is_ident("surreal")) {
            parsed.present = true;
            attr.parse_nested_meta(|meta| {
                let key = meta
                    .path
                    .get_ident()
                    .map(Ident::to_string)
                    .unwrap_or_default();
                if !position.keys().contains(&key.as_str()) {
                    return Err(meta.error(format!(
                        "#[surreal({key})] does not apply to {}; it takes {}",
                        position.describe(),
                        position.keys().join(", ")
                    )));
                }
                let string = |meta: &syn::meta::ParseNestedMeta| -> syn::Result<String> {
                    Ok(meta.value()?.parse::<LitStr>()?.value())
                };
                match key.as_str() {
                    "crate" => {
                        meta.value()?.parse::<LitStr>()?;
                    }
                    "rename" => parsed.rename = Some(string(&meta)?),
                    "rename_all" => parsed.rename_all = Some(string(&meta)?),
                    "uppercase" => parsed.uppercase = true,
                    "lowercase" => parsed.lowercase = true,
                    "tag" => parsed.tag = Some(string(&meta)?),
                    "content" => parsed.content = Some(string(&meta)?),
                    "untagged" => parsed.untagged = true,
                    "default" => {
                        parsed.default = Some(if meta.input.peek(Token![=]) {
                            SurrealDefault::Function(
                                meta.value()?.parse::<LitStr>()?.parse::<syn::ExprPath>()?,
                            )
                        } else {
                            SurrealDefault::Trait
                        });
                    }
                    "flatten" => parsed.flatten = true,
                    "wrap" => parsed.wrap = true,
                    "skip" => parsed.skip = true,
                    "tuple" => parsed.tuple = true,
                    "value" => parsed.value = Some(UnitValue::parse(meta.value()?)?),
                    "other" => parsed.other = true,
                    "skip_content" | "skip_content_if" => {
                        if parsed.skip_content.is_some() {
                            return Err(meta.error(
                                "skip_content and skip_content_if cannot both apply to one item",
                            ));
                        }
                        parsed.skip_content = Some(if key == "skip_content" {
                            SkipContent::Always
                        } else {
                            SkipContent::If(meta.value()?.parse::<LitStr>()?.parse::<Path>()?)
                        });
                    }
                    other => {
                        return Err(meta.error(format!("#[surreal({other})] is not a key")));
                    }
                }
                Ok(())
            })?;
        }
        parsed.refuse_contradictions(attrs)?;
        Ok(parsed)
    }

    /// The combinations the SDK's derive refuses.
    fn refuse_contradictions(&self, attrs: &[Attribute]) -> syn::Result<()> {
        let span = attrs
            .iter()
            .find(|attr| attr.path().is_ident("surreal"))
            .map(syn::spanned::Spanned::span)
            .unwrap_or_else(Span::call_site);
        let refuse = |message: &str| Err(syn::Error::new(span, message));
        if self.rename_all.is_some() && (self.uppercase || self.lowercase) {
            return refuse(
                "#[surreal(rename_all)] and the legacy uppercase or lowercase cannot both name the \
                 variants",
            );
        }
        if self.untagged && (self.tag.is_some() || self.content.is_some()) {
            return refuse("an untagged enum has no tag or content key");
        }
        if self.content.is_some() && self.tag.is_none() {
            return refuse("#[surreal(content)] needs a tag key");
        }
        if self.rename.is_some() && self.value.is_some() {
            return refuse(
                "#[surreal(value)] replaces the variant's whole stored form, so #[surreal(rename)] \
                 would name nothing",
            );
        }
        if self.other && (self.value.is_some() || self.rename.is_some()) {
            return refuse(
                "#[surreal(other)] reads anything no other variant reads, so it has no value or \
                 name of its own",
            );
        }
        if self.flatten && self.rename.is_some() {
            return refuse(
                "#[surreal(flatten)] stores the field's contents beside its siblings, so there is \
                 no key for #[surreal(rename)] to name",
            );
        }
        Ok(())
    }

    /// The legacy or `rename_all` casing, as one value.
    pub fn casing(&self) -> Option<SurrealCasing<'_>> {
        if self.uppercase {
            Some(SurrealCasing::Uppercase)
        } else if self.lowercase {
            Some(SurrealCasing::Lowercase)
        } else {
            self.rename_all.as_deref().map(SurrealCasing::Named)
        }
    }
}

/// A casing the attributes name.
#[derive(Debug, Clone, Copy)]
pub enum SurrealCasing<'a> {
    Uppercase,
    Lowercase,
    /// A `rename_all` value, such as `"camelCase"`.
    Named(&'a str),
}

#[cfg(test)]
mod tests {
    use super::{Position, SkipContent, SurrealAttributes, UnitValue};
    use syn::Attribute;

    fn attributes(source: &str) -> Vec<Attribute> {
        let item: syn::DeriveInput =
            syn::parse_str(&format!("{source} struct Item;")).expect("the attributes parse");
        item.attrs
    }

    fn parse(source: &str, position: Position) -> syn::Result<SurrealAttributes> {
        SurrealAttributes::parse(&attributes(source), position)
    }

    fn rejection(source: &str, position: Position) -> String {
        match parse(source, position) {
            Ok(_) => panic!("expected {source} to be refused at {position:?}"),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn every_key_is_read_where_it_applies() {
        let field = parse(
            "#[surreal(rename = \"full_name\", default = \"zero\", wrap)] #[surreal(skip)]",
            Position::Field,
        )
        .expect("field keys parse");
        assert!(field.present && field.wrap && field.skip);
        assert_eq!(field.rename.as_deref(), Some("full_name"));
        assert!(field.default.is_some());

        let enumeration = parse(
            "#[surreal(crate = \"surrealdb::types\", tag = \"type\", content = \"data\", \
             skip_content_if = \"blank\", rename_all = \"camelCase\")]",
            Position::Enum,
        )
        .expect("enum keys parse");
        assert_eq!(enumeration.tag.as_deref(), Some("type"));
        assert_eq!(enumeration.content.as_deref(), Some("data"));
        assert!(matches!(enumeration.skip_content, Some(SkipContent::If(_))));
        assert!(enumeration.casing().is_some());

        for (source, expected) in [
            ("none", UnitValue::None),
            ("null", UnitValue::Null),
            ("true", UnitValue::Bool(true)),
            ("\"on\"", UnitValue::String("on".to_owned())),
            ("3", UnitValue::Int(3)),
            ("1.5", UnitValue::Float(1.5)),
        ] {
            let unit = parse(
                &format!("#[surreal(value = {source})]"),
                Position::UnitVariant,
            )
            .expect("a unit value parses");
            assert_eq!(unit.value, Some(expected));
        }
        assert!(
            parse("#[surreal(tuple)]", Position::TupleStruct)
                .expect("tuple parses")
                .tuple
        );
        assert!(
            parse("#[surreal(other, skip_content)]", Position::UnitVariant)
                .expect("other parses")
                .other
        );
    }

    #[test]
    fn a_key_outside_its_positions_is_refused_with_the_keys_that_apply() {
        let message = rejection("#[surreal(skip)]", Position::Element);
        assert!(
            message.contains("does not apply to a tuple element"),
            "{message}"
        );
        assert!(message.contains("it takes wrap"), "{message}");
        let message = rejection("#[surreal(tag = \"kind\")]", Position::Struct);
        assert!(message.contains("a struct"), "{message}");
        let message = rejection("#[surreal(serialize_with = \"f\")]", Position::Field);
        assert!(message.contains("serialize_with"), "{message}");
    }

    #[test]
    fn contradictory_keys_are_refused() {
        for (source, position, expected) in [
            (
                "#[surreal(rename_all = \"camelCase\", uppercase)]",
                Position::Enum,
                "rename_all",
            ),
            (
                "#[surreal(untagged, tag = \"kind\")]",
                Position::Enum,
                "untagged",
            ),
            (
                "#[surreal(content = \"data\")]",
                Position::Enum,
                "needs a tag",
            ),
            (
                "#[surreal(rename = \"a\", value = 1)]",
                Position::UnitVariant,
                "replaces",
            ),
            (
                "#[surreal(other, value = 1)]",
                Position::UnitVariant,
                "other",
            ),
            (
                "#[surreal(flatten, rename = \"a\")]",
                Position::Field,
                "flatten",
            ),
            (
                "#[surreal(skip_content, skip_content_if = \"blank\")]",
                Position::TupleVariant,
                "cannot both",
            ),
            ("#[surreal(value = [1])]", Position::UnitStruct, "literal"),
        ] {
            let message = rejection(source, position);
            assert!(message.contains(expected), "{source}: {message}");
        }
    }
}
