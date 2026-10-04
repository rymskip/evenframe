use crate::validator::Validator;
use crate::validator::string_rules::{StringParse, StringRule};
use proc_macro2::TokenStream;
use quote::quote;
use syn::punctuated::Punctuated;
use syn::{Attribute, Error, Result};

/// A hint for a validator expression that failed to parse.
fn suggest_validator_correction(expression: &str) -> String {
    let lower = expression.to_lowercase();
    let suggestions = [
        ("email", "StringValidator::Email"),
        ("minlength", "StringValidator::MinLength(n)"),
        ("maxlength", "StringValidator::MaxLength(n)"),
        ("min_length", "StringValidator::MinLength(n)"),
        ("max_length", "StringValidator::MaxLength(n)"),
        ("pattern", "StringValidator::RegexLiteral(Format::...)"),
        ("regex", "StringValidator::RegexLiteral(Format::...)"),
        (
            "min",
            "NumberValidator::GreaterThanOrEqualTo(n) or StringValidator::MinLength(n)",
        ),
        (
            "max",
            "NumberValidator::LessThanOrEqualTo(n) or StringValidator::MaxLength(n)",
        ),
        ("between", "NumberValidator::Between(min, max)"),
        ("range", "NumberValidator::Between(min, max)"),
        ("minitems", "ArrayValidator::MinItems(n)"),
        ("maxitems", "ArrayValidator::MaxItems(n)"),
        ("min_items", "ArrayValidator::MinItems(n)"),
        ("max_items", "ArrayValidator::MaxItems(n)"),
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
                        - #[validators(StringValidator::Email)]\n\
                        - #[validators(StringValidator::MinLength(5), StringValidator::MaxLength(50))]\n\n\
                        Parse error: {parse_error}"
                    ),
                )
            })?;
        for expression in &expressions {
            collect_validators(expression, &mut spanned)?;
        }
    }

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
    validators: &mut Vec<(Validator, proc_macro2::Span)>,
) -> Result<()> {
    match expression {
        syn::Expr::Array(array) if array.elems.is_empty() => Err(Error::new_spanned(
            expression,
            "Empty validator array. Please provide at least one validator.\n\n\
            Example: #[validators([StringValidator::Email, StringValidator::MinLength(5)])]",
        )),
        syn::Expr::Array(array) => array
            .elems
            .iter()
            .try_for_each(|element| collect_validators(element, validators)),
        syn::Expr::Paren(paren) => collect_validators(&paren.expr, validators),
        _ => {
            let validator = Validator::try_from(expression).map_err(|error| {
                let suggestion = suggest_validator_correction(&quote!(#expression).to_string());
                Error::new_spanned(
                    expression,
                    format!(
                        "Failed to parse validator expression: {error}{suggestion}\n\n\
                        Common validator examples:\n\
                        - StringValidator::Email\n\
                        - StringValidator::MinLength(5)\n\
                        - StringValidator::MaxLength(100)\n\
                        - StringValidator::Lower\n\
                        - StringValidator::IntegerParse\n\
                        - NumberValidator::GreaterThanOrEqualTo(0.0)\n\
                        - NumberValidator::Between(0.0, 100.0)\n\
                        - ArrayValidator::MinItems(1)\n\n\
                        Make sure the validator enum is imported and spelled correctly."
                    ),
                )
            })?;
            validator
                .check_bounds()
                .map_err(|message| Error::new_spanned(expression, message))?;
            validators.push((validator, syn::spanned::Spanned::span(expression)));
            Ok(())
        }
    }
}
