use crate::validator::Validator;
use crate::validator::string_rules::{StringParse, StringRule};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::punctuated::Punctuated;
use syn::{Attribute, Error, Result};

/// Whether `ty` is an `Option<T>`.
fn is_option_type(ty: &syn::Type) -> bool {
    if let syn::Type::Path(type_path) = ty
        && let Some(segment) = type_path.path.segments.last()
    {
        return segment.ident == "Option";
    }
    false
}

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

    /// Statements that read the field from `map` into `temp` and validate
    /// it. A parse morph reads a string and parses it into the field's type;
    /// on an `Option` field every validator applies to the present value.
    pub fn read_tokens(
        &self,
        temp: &syn::Ident,
        field_type: &syn::Type,
        field_name: &str,
    ) -> Result<TokenStream> {
        let optional = is_option_type(field_type);
        let rejection = quote! {
            |rejection| ::serde::de::Error::custom(::std::format!("{}: {}", #field_name, rejection))
        };
        let read = match self.parse {
            None => quote! { map.next_value()? },
            Some(parse) => {
                let function = format_ident!("{}", parse.runtime_function());
                if optional {
                    quote! {
                        map.next_value::<::std::option::Option<::std::string::String>>()?
                            .as_deref()
                            .map(::evenframe::validator::runtime::#function)
                            .transpose()
                            .map_err(#rejection)?
                    }
                } else {
                    quote! {
                        ::evenframe::validator::runtime::#function(
                            &map.next_value::<::std::string::String>()?,
                        )
                        .map_err(#rejection)?
                    }
                }
            }
        };

        let checked = if self.parse.is_some() {
            &self.validators[1..]
        } else {
            &self.validators[..]
        };
        let transforms = checked.iter().any(|validator| {
            matches!(
                validator,
                Validator::StringValidator(string_validator)
                    if matches!(string_validator.rule(), StringRule::Transform(_))
            )
        });
        let inner = format_ident!("{}_inner", temp);
        let place = if optional {
            quote! { (*#inner) }
        } else {
            quote! { #temp }
        };
        let checks = checked
            .iter()
            .map(|validator| {
                validator
                    .validation_tokens(&place, field_name)
                    .map_err(|message| Error::new_spanned(field_type, message))
            })
            .collect::<Result<Vec<TokenStream>>>()?;

        let binding = if transforms {
            quote! { let mut #temp: #field_type = #read; }
        } else {
            quote! { let #temp: #field_type = #read; }
        };
        let validation = match (optional, checks.is_empty(), transforms) {
            (_, true, _) => TokenStream::new(),
            (false, false, _) => quote! { #(#checks)* },
            (true, false, true) => quote! {
                if let ::std::option::Option::Some(#inner) = &mut #temp { #(#checks)* }
            },
            (true, false, false) => quote! {
                if let ::std::option::Option::Some(#inner) = &#temp { #(#checks)* }
            },
        };
        Ok(quote! { #binding #validation })
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
