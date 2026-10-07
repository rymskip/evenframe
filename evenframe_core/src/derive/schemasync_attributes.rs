//! `#[schemasync(...)]`: what the schema and mock data take for an item
//! beside what evenframe describes for it.
//!
//! - `retain` keeps an item serde skips in the database and its schema.
//! - `validators(...)` replaces a field's, element's or newtype's
//!   `#[validators(...)]` in the schema and mock data. Its custom patterns run
//!   only in Rust's engine, which SurrealDB's `string::matches` uses.

use crate::{
    derive::{typesync_attributes::TypesyncAttributes, validator_parser::parse_validator_list},
    schemasync::format::PatternDialect,
    validator::{Validator, ValidatorOverrides},
};
use syn::{Attribute, Token, punctuated::Punctuated};

/// An item's `#[schemasync(...)]` entries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SchemasyncAttributes {
    pub retain: bool,
    pub validators: Option<Vec<Validator>>,
}

impl SchemasyncAttributes {
    /// Reads every `#[schemasync(...)]` on `attrs`.
    pub fn parse(attrs: &[Attribute]) -> syn::Result<Self> {
        let mut parsed = Self::default();
        for attr in attrs
            .iter()
            .filter(|attr| attr.path().is_ident("schemasync"))
        {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("retain") {
                    parsed.retain = true;
                    Ok(())
                } else if meta.path.is_ident("validators") {
                    if parsed.validators.is_some() {
                        return Err(meta.error("#[schemasync(validators(...))] is given twice"));
                    }
                    let content;
                    syn::parenthesized!(content in meta.input);
                    let list = Punctuated::<syn::Expr, Token![,]>::parse_terminated(&content)?;
                    parsed.validators =
                        Some(parse_validator_list(&list, PatternDialect::Rust)?.validators);
                    Ok(())
                } else {
                    Err(meta.error(
                        "#[schemasync(...)] takes `retain`, which keeps an item serde skips in \
                         the database, and `validators(...)`, which replaces its validators in \
                         the schema",
                    ))
                }
            })?;
        }
        Ok(parsed)
    }
}

/// The validator overrides on a field, a tuple element or a newtype's
/// container: `#[typesync(validators(...))]` and
/// `#[schemasync(validators(...))]`.
pub fn parse_validator_overrides(attrs: &[Attribute]) -> syn::Result<ValidatorOverrides> {
    Ok(ValidatorOverrides {
        typesync: TypesyncAttributes::parse(
            attrs,
            crate::derive::typesync_attributes::Position::Field,
        )?
        .validators,
        schemasync: SchemasyncAttributes::parse(attrs)?.validators,
    })
}

/// Refuses validators and morphs on a struct of named fields, an enum or a
/// newtype of several values: none of them is one value, so a value's
/// validators and morphs sit on a field, a tuple element or a single-value
/// newtype.
pub fn refuse_container_validators(attrs: &[Attribute]) -> syn::Result<()> {
    let overrides = parse_container_validator_overrides(attrs)?;
    match attrs.iter().find(|attr| {
        attr.path().is_ident("validators")
            || attr.path().is_ident("morphs")
            || (!overrides.is_empty()
                && (attr.path().is_ident("typesync") || attr.path().is_ident("schemasync")))
    }) {
        Some(attr) => Err(syn::Error::new_spanned(
            attr,
            if attr.path().is_ident("morphs") {
                "morphs rewrite a value: put them on a field, a tuple element or a newtype"
            } else {
                "validators check a value: put them on a field, a tuple element or a newtype"
            },
        )),
        None => Ok(()),
    }
}

/// The validator overrides on a newtype's container, which check the value it
/// holds as its field's own do.
pub fn parse_container_validator_overrides(attrs: &[Attribute]) -> syn::Result<ValidatorOverrides> {
    Ok(ValidatorOverrides {
        typesync: TypesyncAttributes::parse(
            attrs,
            crate::derive::typesync_attributes::Position::Container,
        )?
        .validators,
        schemasync: SchemasyncAttributes::parse(attrs)?.validators,
    })
}

#[cfg(test)]
mod tests {
    use super::{SchemasyncAttributes, parse_validator_overrides};
    use crate::validator::{StringValidator, Validator};

    fn attributes(source: &str) -> Vec<syn::Attribute> {
        let item: syn::DeriveInput =
            syn::parse_str(&format!("{source} struct Item;")).expect("the attributes parse");
        item.attrs
    }

    #[test]
    fn each_pipeline_reads_its_own_validators_in_its_own_regex_syntax() {
        let overrides = parse_validator_overrides(&attributes(
            r#"#[typesync(validators(StringValidator::RegexLiteral(Format::Custom("/^\\p{L}+$/u"))))]
               #[schemasync(validators(StringValidator::RegexLiteral(Format::Custom(r"^\d+$")), StringValidator::NonEmpty))]"#,
        ))
        .expect("both lists parse");
        let Some(
            [
                Validator::StringValidator(StringValidator::RegexLiteral(
                    crate::schemasync::format::Format::Custom(typesync),
                )),
            ],
        ) = overrides.typesync.as_deref()
        else {
            panic!(
                "expected one typesync pattern, got {:?}",
                overrides.typesync
            );
        };
        assert_eq!(typesync.flags(), Some("u"));
        assert_eq!(overrides.schemasync.as_ref().map(Vec::len), Some(2));

        let refused = parse_validator_overrides(&attributes(
            r#"#[schemasync(validators(StringValidator::RegexLiteral(Format::Custom("^(?<=a)b$"))))]"#,
        ))
        .expect_err("SurrealDB's engine has no lookbehind");
        assert!(
            refused.to_string().contains("not a valid regex"),
            "{refused}"
        );
        let refused = parse_validator_overrides(&attributes(
            r#"#[typesync(validators(StringValidator::RegexLiteral(Format::Custom("^a$"))))]"#,
        ))
        .expect_err("a typesync pattern is a literal");
        assert!(refused.to_string().contains("regex literal"), "{refused}");
    }

    #[test]
    fn retain_and_validators_are_the_only_schemasync_keys() {
        let parsed = SchemasyncAttributes::parse(&attributes("#[schemasync(retain)]"))
            .expect("retain parses");
        assert!(parsed.retain);
        let error = SchemasyncAttributes::parse(&attributes("#[schemasync(keep)]"))
            .expect_err("keep is not a key");
        assert!(error.to_string().contains("validators(...)"), "{error}");
    }
}
