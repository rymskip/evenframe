use crate::schemasync::mockmake::format::{CustomPattern, Format, PatternDialect};
use crate::validator::string_rules::{StringParse, StringRule};
use crate::validator::{StringValidator, Validator, ValidatorOverrides};
use proc_macro2::TokenStream;
use quote::quote;
use syn::punctuated::Punctuated;
use syn::{Attribute, Error, Result};

/// A hint for a validator expression that failed to parse.
fn suggest_validator_correction(expression: &str) -> String {
    let lower = expression.to_lowercase();
    let suggestions = [
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

/// A field's `#[validators(...)]`, parsed once.
#[derive(Debug, Clone, Default)]
pub struct FieldValidators {
    pub validators: Vec<Validator>,
    /// The parse morph the field is read through, when its first validator
    /// is one.
    pub parse: Option<StringParse>,
}

impl FieldValidators {
    pub fn is_empty(&self) -> bool {
        self.validators.is_empty()
    }

    /// The validators as tokens for the field's static config.
    pub fn config_tokens(&self) -> Vec<TokenStream> {
        self.validators
            .iter()
            .map(|validator| quote! { #validator })
            .collect()
    }

    /// The validators the field's value goes through after any parse morph,
    /// in declared order.
    pub fn steps(&self) -> &[Validator] {
        match (self.parse, self.validators.split_first()) {
            (Some(_), Some((_, rest))) => rest,
            _ => &self.validators,
        }
    }
}

/// Parses every `#[validators(...)]` on `attrs`, rejecting misspelled
/// attributes, unknown validators, unparsable bounds and a parse morph that
/// is not first.
pub fn parse_field_validators(attrs: &[Attribute]) -> Result<FieldValidators> {
    for attr in attrs {
        let misspelling = ["validator", "validate", "validation"]
            .into_iter()
            .find(|name| attr.path().is_ident(name));
        if let Some(name) = misspelling {
            return Err(Error::new_spanned(
                attr,
                format!(
                    "Invalid attribute name '{name}'. Did you mean 'validators'?\n\n\
                    Example: #[validators(StringValidator::Email)]"
                ),
            ));
        }
    }

    let mut spanned = Vec::new();
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
                        - #[validators(trim, min_length = 5, max_length = 50)]\n\n\
                        Parse error: {parse_error}"
                    ),
                )
            })?;
        for expression in &expressions {
            collect_validators(expression, PatternDialect::Portable, &mut spanned)?;
        }
    }
    finish(spanned)
}

/// Each tuple element's validators and overrides, one entry per element, or
/// none when no element has any.
pub fn parse_element_validators<'a>(
    fields: impl IntoIterator<Item = &'a syn::Field>,
) -> Result<(Vec<Vec<Validator>>, Vec<ValidatorOverrides>)> {
    let (validators, overrides): (Vec<Vec<Validator>>, Vec<ValidatorOverrides>) = fields
        .into_iter()
        .map(|field| {
            Ok((
                parse_field_validators(&field.attrs)?.validators,
                crate::derive::schemasync_attributes::parse_validator_overrides(&field.attrs)?,
            ))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .unzip();
    Ok(
        if validators.iter().all(Vec::is_empty)
            && overrides.iter().all(ValidatorOverrides::is_empty)
        {
            (Vec::new(), Vec::new())
        } else {
            (validators, overrides)
        },
    )
}

/// The validators an override such as `#[typesync(validators(...))]` lists,
/// whose custom patterns run only in `dialect`'s engines.
pub fn parse_validator_list(
    expressions: &Punctuated<syn::Expr, syn::Token![,]>,
    dialect: PatternDialect,
) -> Result<FieldValidators> {
    let mut spanned = Vec::new();
    for expression in expressions {
        collect_validators(expression, dialect, &mut spanned)?;
    }
    finish(spanned)
}

/// Refuses a parse morph anywhere but first, and records the one there.
fn finish(spanned: Vec<(Validator, proc_macro2::Span)>) -> Result<FieldValidators> {
    let is_parse = |validator: &Validator| {
        matches!(
            validator,
            Validator::StringValidator(string_validator)
                if matches!(string_validator.rule(), StringRule::Parse(_))
        )
    };
    if let Some((misplaced, span)) = spanned
        .iter()
        .skip(1)
        .find(|(validator, _)| is_parse(validator))
    {
        return Err(Error::new(
            *span,
            format!(
                "{misplaced:?} parses the field's input, so it must be the first validator and appear once"
            ),
        ));
    }
    let validators: Vec<Validator> = spanned
        .into_iter()
        .map(|(validator, _)| validator)
        .collect();
    let parse = match validators.first() {
        Some(Validator::StringValidator(first)) => match first.rule() {
            StringRule::Parse(parse) => Some(parse),
            StringRule::Check | StringRule::Transform(_) | StringRule::Carrier => None,
        },
        _ => None,
    };
    Ok(FieldValidators { validators, parse })
}

/// Adds the validators `expression` names, with their spans: one validator,
/// or an array or parenthesized group of them.
fn collect_validators(
    expression: &syn::Expr,
    dialect: PatternDialect,
    validators: &mut Vec<(Validator, proc_macro2::Span)>,
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
                        - lower\n\
                        - integer_parse\n\
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
            validators.push((validator, syn::spanned::Spanned::span(expression)));
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_field_validators;
    use crate::validator::Validator;

    fn validators(source: &str) -> syn::Result<Vec<Validator>> {
        let item: syn::DeriveInput =
            syn::parse_str(&format!("{source} struct Item;")).expect("the attributes parse");
        parse_field_validators(&item.attrs).map(|parsed| parsed.validators)
    }

    #[test]
    fn idiomatic_names_read_as_the_validators_their_paths_name() {
        for (idiomatic, path) in [
            (
                "#[validators(trim, non_empty, max_length = 80)]",
                "#[validators(StringValidator::Trim, StringValidator::NonEmpty, StringValidator::MaxLength(80))]",
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
                "#[validators(trim, lower)]",
                "#[validators(StringValidator::Trim, StringValidator::Lower)]",
            ),
            (
                "#[validators(integer_parse, greater_than_or_equal_to = 13.0)]",
                "#[validators(StringValidator::IntegerParse, NumberValidator::GreaterThanOrEqualTo(13.0))]",
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
    fn an_idiomatic_pattern_meets_the_same_dialect_check() {
        let error = validators(r#"#[validators(regex_literal = format(custom = r"^\d+$"))]"#)
            .expect_err("`\\d` is not portable");
        assert!(error.to_string().contains("[0-9]"), "{error}");
    }
}
