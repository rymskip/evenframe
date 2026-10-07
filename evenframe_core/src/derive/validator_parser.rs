use crate::schemasync::mockmake::format::{CustomPattern, Format, PatternDialect};
use crate::validator::morph::Morph;
use crate::validator::text_pattern::TextPattern;
use crate::validator::{StringValidator, Validator, ValidatorOverrides};
use proc_macro2::TokenStream;
use quote::quote;
use syn::punctuated::Punctuated;
use syn::{Attribute, Error, Result};

/// A hint for a validator expression that failed to parse.
fn suggest_validator_correction(expression: &str) -> String {
    let lower = expression.to_lowercase();
    let suggestions = [
        (
            "integer_parse",
            "a `FromText<i64>` field (or another integer), which reads the integer from its text",
        ),
        (
            "numeric_parse",
            "a `FromText<f64>` field, which reads the number from its text",
        ),
        (
            "url_parse",
            "a `FromText<url::Url>` field, which reads the URL from its text",
        ),
        (
            "date_iso_parse",
            "an `IsoDate` field, which reads the instant from ISO 8601 text",
        ),
        (
            "date_epoch_parse",
            "an `EpochMillis` field, which reads the instant from text of epoch milliseconds",
        ),
        (
            "date_parse",
            "an `IsoDate` field, which reads the instant from ISO 8601 text",
        ),
        (
            "json_parse",
            "a `JsonText<T>` field, which reads the value from JSON text",
        ),
        ("minlength", "min_length = n"),
        ("maxlength", "max_length = n"),
        ("min_length", "min_length = n"),
        ("max_length", "max_length = n"),
        ("pattern", "regex_literal = format(custom = \"^...$\")"),
        ("regex", "regex_literal = format(custom = \"^...$\")"),
        ("min", "greater_than_or_equal_to = n, or min_length = n"),
        ("max", "less_than_or_equal_to = n, or max_length = n"),
        ("between", "between = (min, max)"),
        ("range", "between = (min, max)"),
        ("minitems", "min_items = n"),
        ("maxitems", "max_items = n"),
        ("min_items", "min_items = n"),
        ("max_items", "max_items = n"),
        (
            "required",
            "This is typically handled by Option<T> types, not validators",
        ),
    ];
    suggestions
        .iter()
        .find(|(pattern, _)| lower.contains(pattern))
        .map(|(_, suggestion)| format!("\n\nDid you mean: {suggestion}?"))
        .unwrap_or_default()
}

/// A field's `#[morphs(...)]` and `#[validators(...)]`, parsed once.
#[derive(Debug, Clone, Default)]
pub struct FieldValidators {
    /// Rewrite the value before the validators check it.
    pub morphs: Vec<Morph>,
    pub validators: Vec<Validator>,
}

impl FieldValidators {
    pub fn is_empty(&self) -> bool {
        self.morphs.is_empty() && self.validators.is_empty()
    }

    /// The morphs as tokens for the field's static config.
    pub fn morph_tokens(&self) -> Vec<TokenStream> {
        self.morphs.iter().map(|morph| quote! { #morph }).collect()
    }

    /// The validators as tokens for the field's static config.
    pub fn config_tokens(&self) -> Vec<TokenStream> {
        self.validators
            .iter()
            .map(|validator| quote! { #validator })
            .collect()
    }
}

/// Parses every `#[morphs(...)]` and `#[validators(...)]` on `attrs`,
/// rejecting misspelled attributes, unknown morphs and validators, a morph
/// among the validators and unparsable bounds.
pub fn parse_field_validators(attrs: &[Attribute]) -> Result<FieldValidators> {
    for attr in attrs {
        let misspelling = [
            ("validator", "validators"),
            ("validate", "validators"),
            ("validation", "validators"),
            ("morph", "morphs"),
            ("transform", "morphs"),
            ("transforms", "morphs"),
            ("normalize", "morphs"),
        ]
        .into_iter()
        .find(|(name, _)| attr.path().is_ident(name));
        if let Some((name, meant)) = misspelling {
            return Err(Error::new_spanned(
                attr,
                format!("Invalid attribute name '{name}'. Did you mean '{meant}'?"),
            ));
        }
    }

    let mut morphs = Vec::new();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("morphs")) {
        let expressions = attr
            .parse_args_with(Punctuated::<syn::Expr, syn::Token![,]>::parse_separated_nonempty)
            .map_err(|parse_error| {
                Error::new_spanned(
                    attr,
                    format!(
                        "Failed to parse morphs attribute: expected a comma-separated list of morphs, \
                         such as #[morphs(trim, lower)].\n\nParse error: {parse_error}"
                    ),
                )
            })?;
        for expression in &expressions {
            let morph = Morph::try_from(expression).map_err(|error| {
                Error::new_spanned(
                    expression,
                    format!(
                        "Failed to parse morph expression: {error}\n\n\
                         Morphs: trim, collapse_whitespace, lower, upper, capitalize, normalize, \
                         normalize_nfc, normalize_nfd, normalize_nfkc, normalize_nfkd, \
                         round = 2, clamp = (\"0\", \"100\"), sort, unique"
                    ),
                )
            })?;
            morph
                .check_bounds()
                .map_err(|message| Error::new_spanned(expression, message))?;
            morphs.push(morph);
        }
    }

    let mut validators = Vec::new();
    for attr in attrs
        .iter()
        .filter(|attr| attr.path().is_ident("validators"))
    {
        let expressions = attr
            .parse_args_with(Punctuated::<syn::Expr, syn::Token![,]>::parse_separated_nonempty)
            .map_err(|parse_error| {
                Error::new_spanned(
                    attr,
                    format!(
                        "Failed to parse validators attribute: expected a comma-separated list of validators.\n\n\
                        Examples:\n\
                        - #[validators(email)]\n\
                        - #[validators(non_empty, min_length = 5, max_length = 50)]\n\n\
                        Parse error: {parse_error}"
                    ),
                )
            })?;
        for expression in &expressions {
            collect_validators(expression, PatternDialect::Portable, &mut validators)?;
        }
    }
    Ok(FieldValidators { morphs, validators })
}

/// Each tuple element's morphs, validators and overrides, one entry per
/// element, or none when no element has any.
#[derive(Debug, Clone, Default)]
pub struct ElementValidators {
    pub morphs: Vec<Vec<Morph>>,
    pub validators: Vec<Vec<Validator>>,
    pub overrides: Vec<ValidatorOverrides>,
}

/// Each tuple element's `#[morphs]`, `#[validators]` and overrides.
pub fn parse_element_validators<'a>(
    fields: impl IntoIterator<Item = &'a syn::Field>,
) -> Result<ElementValidators> {
    let mut elements = ElementValidators::default();
    for field in fields {
        let parsed = parse_field_validators(&field.attrs)?;
        elements.morphs.push(parsed.morphs);
        elements.validators.push(parsed.validators);
        elements
            .overrides
            .push(crate::derive::schemasync_attributes::parse_validator_overrides(&field.attrs)?);
    }
    Ok(
        if elements.morphs.iter().all(Vec::is_empty)
            && elements.validators.iter().all(Vec::is_empty)
            && elements.overrides.iter().all(ValidatorOverrides::is_empty)
        {
            ElementValidators::default()
        } else {
            elements
        },
    )
}

/// The validators an override such as `#[typesync(validators(...))]` lists,
/// whose custom patterns run only in `dialect`'s engines.
pub fn parse_validator_list(
    expressions: &Punctuated<syn::Expr, syn::Token![,]>,
    dialect: PatternDialect,
) -> Result<FieldValidators> {
    let mut validators = Vec::new();
    for expression in expressions {
        collect_validators(expression, dialect, &mut validators)?;
    }
    Ok(FieldValidators {
        morphs: Vec::new(),
        validators,
    })
}

/// Adds the validators `expression` names: one validator, or an array or
/// parenthesized group of them.
fn collect_validators(
    expression: &syn::Expr,
    dialect: PatternDialect,
    validators: &mut Vec<Validator>,
) -> Result<()> {
    match expression {
        syn::Expr::Array(array) if array.elems.is_empty() => Err(Error::new_spanned(
            expression,
            "Empty validator array. Please provide at least one validator.\n\n\
            Example: #[validators([email, min_length = 5])]",
        )),
        syn::Expr::Array(array) => array
            .elems
            .iter()
            .try_for_each(|element| collect_validators(element, dialect, validators)),
        syn::Expr::Paren(paren) => collect_validators(&paren.expr, dialect, validators),
        _ => {
            if Morph::try_from(expression).is_ok() {
                return Err(Error::new_spanned(
                    expression,
                    format!(
                        "`{}` rewrites the value rather than checking it: write it in \
                         `#[morphs(...)]`, which runs before the validators",
                        quote!(#expression)
                    ),
                ));
            }
            let mut validator = Validator::try_from(expression).map_err(|error| {
                let suggestion = suggest_validator_correction(&quote!(#expression).to_string());
                Error::new_spanned(
                    expression,
                    format!(
                        "Failed to parse validator expression: {error}{suggestion}\n\n\
                        Common validators:\n\
                        - email\n\
                        - min_length = 5\n\
                        - max_length = 100\n\
                        - greater_than_or_equal_to = 0.0\n\
                        - between = (0.0, 100.0)\n\
                        - min_items = 1\n\
                        - regex_literal = format(custom = \"^[a-z]+$\")\n\n\
                        A name two validator kinds share takes the kind, as in \
                        `string_validator(min_length = 5)`, and the path form, as in \
                        `StringValidator::MinLength(5)`, reads the same."
                    ),
                )
            })?;
            validator
                .check_bounds()
                .map_err(|message| Error::new_spanned(expression, message))?;
            if let Validator::StringValidator(StringValidator::RegexLiteral(Format::Custom(
                custom,
            ))) = &mut validator
            {
                *custom = CustomPattern::parse(custom.as_str(), dialect)
                    .map_err(|message| Error::new_spanned(expression, message))?;
            }
            // The validator anchors a text argument itself.
            if let Validator::StringValidator(
                StringValidator::StartsWith(TextPattern::Format(Format::Custom(custom)))
                | StringValidator::EndsWith(TextPattern::Format(Format::Custom(custom)))
                | StringValidator::Includes(TextPattern::Format(Format::Custom(custom))),
            ) = &mut validator
            {
                *custom = CustomPattern::parse(custom.as_str(), dialect)
                    .map_err(|message| Error::new_spanned(expression, message))?;
                custom
                    .refuse_anchors()
                    .map_err(|message| Error::new_spanned(expression, message))?;
            }
            validators.push(validator);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_field_validators;
    use crate::validator::Validator;
    use crate::validator::morph::{Morph, NumberMorph, StringMorph};

    fn validators(source: &str) -> syn::Result<Vec<Validator>> {
        let item: syn::DeriveInput =
            syn::parse_str(&format!("{source} struct Item;")).expect("the attributes parse");
        parse_field_validators(&item.attrs).map(|parsed| parsed.validators)
    }

    #[test]
    fn idiomatic_names_read_as_the_validators_their_paths_name() {
        for (idiomatic, path) in [
            (
                "#[validators(non_empty, max_length = 80)]",
                "#[validators(StringValidator::NonEmpty, StringValidator::MaxLength(80))]",
            ),
            (
                r#"#[validators(regex_literal = format(custom = "^[a-z]+$"))]"#,
                r#"#[validators(StringValidator::RegexLiteral(Format::Custom("^[a-z]+$")))]"#,
            ),
            (
                "#[validators(email, string_validator(min_length = 3))]",
                "#[validators(StringValidator::Email, StringValidator::MinLength(3))]",
            ),
            (
                "#[validators(max_items = 2)]",
                "#[validators(ArrayValidator::MaxItems(2))]",
            ),
            (
                "#[validators(greater_than_or_equal_to = 0.0, between = (0.0, 100.0))]",
                "#[validators(NumberValidator::GreaterThanOrEqualTo(0.0), NumberValidator::Between(0.0, 100.0))]",
            ),
        ] {
            assert_eq!(
                validators(idiomatic).expect(idiomatic),
                validators(path).expect(path),
                "{idiomatic}"
            );
        }
    }

    #[test]
    fn morphs_are_read_from_their_own_attribute() {
        let item: syn::DeriveInput =
            syn::parse_str("#[morphs(trim, round = 2)] #[validators(non_empty)] struct Item;")
                .expect("the attributes parse");
        let parsed = parse_field_validators(&item.attrs).expect("they are valid");
        assert_eq!(
            parsed.morphs,
            vec![
                Morph::StringMorph(StringMorph::Trim),
                Morph::NumberMorph(NumberMorph::Round(2)),
            ]
        );
        let error = validators("#[validators(trim)]").expect_err("a morph is not a validator");
        assert!(error.to_string().contains("#[morphs(...)]"), "{error}");
    }

    #[test]
    fn a_format_argument_reads_and_may_not_anchor_itself() {
        use crate::schemasync::mockmake::format::Format;
        use crate::validator::StringValidator;
        use crate::validator::text_pattern::TextPattern;
        assert_eq!(
            validators("#[validators(includes = format(uppercase))]").expect("a format argument"),
            vec![Validator::StringValidator(StringValidator::Includes(
                TextPattern::Format(Format::Uppercase)
            ))]
        );
        let error = validators(r#"#[validators(starts_with = format(custom = "^ab"))]"#)
            .expect_err("anchored");
        assert!(error.to_string().contains("anchors itself"), "{error}");
        assert!(validators(r#"#[validators(includes = format(custom = "[A-Z]"))]"#).is_ok());
    }

    #[test]
    fn a_removed_parse_points_at_its_type() {
        let error = validators("#[validators(integer_parse)]").expect_err("parses are types");
        assert!(error.to_string().contains("FromText<i64>"), "{error}");
    }

    #[test]
    fn an_idiomatic_pattern_meets_the_same_dialect_check() {
        let error = validators(r#"#[validators(regex_literal = format(custom = r"^\d+$"))]"#)
            .expect_err("`\\d` is not portable");
        assert!(error.to_string().contains("[0-9]"), "{error}");
    }
}
