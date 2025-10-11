use crate::validator::Validator;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Attribute, Error, Result};
use tracing;

/// Helper function to suggest validator corrections based on common mistakes
fn suggest_validator_correction(expr_str: &str) -> String {
    let lower = expr_str.to_lowercase();

    // Common typos and their corrections
    let suggestions = vec![
        ("email", "StringValidator::Email"),
        ("minlength", "StringValidator::MinLength(n)"),
        ("maxlength", "StringValidator::MaxLength(n)"),
        ("min_length", "StringValidator::MinLength(n)"),
        ("max_length", "StringValidator::MaxLength(n)"),
        ("pattern", "StringValidator::Pattern(\"regex\")"),
        ("regex", "StringValidator::Pattern(\"regex\")"),
        (
            "min",
            "NumberValidator::Min(n) or StringValidator::MinLength(n)",
        ),
        (
            "max",
            "NumberValidator::Max(n) or StringValidator::MaxLength(n)",
        ),
        ("between", "NumberValidator::Between(min, max)"),
        ("range", "NumberValidator::Between(min, max)"),
        ("minitems", "ArrayValidator::MinItems(n)"),
        ("maxitems", "ArrayValidator::MaxItems(n)"),
        ("min_items", "ArrayValidator::MinItems(n)"),
        ("max_items", "ArrayValidator::MaxItems(n)"),
        ("unique", "ArrayValidator::Unique"),
        (
            "required",
            "This is typically handled by Option<T> types, not validators",
        ),
    ];

    for (pattern, suggestion) in suggestions {
        if lower.contains(pattern) {
            return format!("\n\nDid you mean: {}?", suggestion);
        }
    }

    String::new()
}

pub fn parse_field_validators_with_logic(
    attrs: &[Attribute],
    value_ident: &str,
) -> Result<(Vec<TokenStream>, Vec<TokenStream>)> {
    tracing::debug!(attr_count = attrs.len(), value_ident = %value_ident, "Parsing field validators with logic");
    // Check for common attribute mistakes
    for attr in attrs {
        if attr.path().is_ident("validator") {
            return Err(Error::new_spanned(
                attr,
                "Invalid attribute name 'validator'. Did you mean 'validators' (plural)?\n\n\
                Example: #[validators(StringValidator::Email)]",
            ));
        }
        if attr.path().is_ident("validate") {
            return Err(Error::new_spanned(
                attr,
                "Invalid attribute name 'validate'. Did you mean 'validators'?\n\n\
                Example: #[validators(StringValidator::MinLength(5))]",
            ));
        }
        if attr.path().is_ident("validation") {
            return Err(Error::new_spanned(
                attr,
                "Invalid attribute name 'validation'. Did you mean 'validators'?\n\n\
                Example: #[validators(NumberValidator::Min(0.0))]",
            ));
        }
    }

    for attr in attrs {
        if attr.path().is_ident("validators") {
            // Parse the validator expression
            let parse_result = attr.parse_args_with(|input: syn::parse::ParseStream| {
                // Try to parse as a comma-separated list of expressions
                syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_separated_nonempty(
                    input,
                )
            });

            match parse_result {
                Ok(validators_list) => {
                    let mut validator_tokens = Vec::new();
                    let mut logic_tokens = Vec::new();
                    for validator_expr in validators_list {
                        let (val_tokens, log_tokens) =
                            parse_validator_enum_with_logic(&validator_expr, value_ident)?;
                        validator_tokens.extend(val_tokens);
                        logic_tokens.extend(log_tokens);
                    }
                    return Ok((validator_tokens, logic_tokens));
                }
                Err(_err) => {
                    // Try parsing as a single expression for backwards compatibility
                    match attr.parse_args::<syn::Expr>() {
                        Ok(expr) => return parse_validator_enum_with_logic(&expr, value_ident),
                        Err(parse_err) => {
                            return Err(Error::new_spanned(
                                attr,
                                format!(
                                    "Failed to parse validators attribute. Expected either a single validator \
                                    expression or a comma-separated list of validators. \n\n\
                                    Examples:\n\
                                    - #[validators(StringValidator::Email)]\n\
                                    - #[validators(StringValidator::MinLength(5), StringValidator::MaxLength(50))]\n\n\
                                    Parse error: {}",
                                    parse_err
                                ),
                            ));
                        }
                    }
                }
            }
        }
    }
    Ok((vec![], vec![]))
}

pub fn parse_field_validators(attrs: &[Attribute]) -> Result<Vec<TokenStream>> {
    tracing::debug!(attr_count = attrs.len(), "Parsing field validators");
    let (validator_tokens, _) = parse_field_validators_with_logic(attrs, "value")?;
    Ok(validator_tokens)
}

// Parse a validator enum expression and return both validator tokens and validation logic
pub fn parse_validator_enum_with_logic(
    expr: &syn::Expr,
    value_ident: &str,
) -> Result<(Vec<TokenStream>, Vec<TokenStream>)> {
    tracing::trace!(value_ident = %value_ident, "Parsing validator enum with logic");
    let mut validator_tokens = Vec::new();
    let mut logic_tokens = Vec::new();

    // Handle array of validators
    if let syn::Expr::Array(array_expr) = expr {
        if array_expr.elems.is_empty() {
            return Err(Error::new_spanned(
                expr,
                "Empty validator array. Please provide at least one validator.\n\n\
                Example: #[validators([StringValidator::Email, StringValidator::MinLength(5)])]",
            ));
        }

        for (idx, elem) in array_expr.elems.iter().enumerate() {
            match parse_validator_enum_with_logic(elem, value_ident) {
                Ok((val_tokens, log_tokens)) => {
                    validator_tokens.extend(val_tokens);
                    logic_tokens.extend(log_tokens);
                }
                Err(err) => {
                    return Err(Error::new_spanned(
                        elem,
                        format!("Error in validator at index {}: {}", idx, err),
                    ));
                }
            }
        }
        return Ok((validator_tokens, logic_tokens));
    }

    // Handle parenthesized expressions
    if let syn::Expr::Paren(paren) = expr {
        return parse_validator_enum_with_logic(&paren.expr, value_ident);
    }

    // Try to parse the expression into a Validator enum using the SynEnum derive
    match Validator::try_from(expr) {
        Ok(validator) => {
            // Get the validation logic tokens
            let validation_logic = validator.get_validation_logic_tokens(value_ident);
            logic_tokens.push(validation_logic);

            validator_tokens.push(quote! {#validator});
        }
        Err(err) => {
            // Provide more specific error messages based on the expression type
            let expr_str = quote!(#expr).to_string();
            let suggestion = suggest_validator_correction(&expr_str);

            return Err(Error::new_spanned(
                expr,
                format!(
                    "Failed to parse validator expression: {}{}\n\n\
                    Common validator examples:\n\
                    - StringValidator::Email\n\
                    - StringValidator::MinLength(5)\n\
                    - StringValidator::MaxLength(100)\n\
                    - StringValidator::Pattern(\"^[A-Z]\")\n\
                    - NumberValidator::Min(0.0)\n\
                    - NumberValidator::Max(100.0)\n\
                    - NumberValidator::Between(0.0, 100.0)\n\
                    - ArrayValidator::MinItems(1)\n\
                    - ArrayValidator::MaxItems(10)\n\n\
                    Make sure the validator enum is imported and spelled correctly.",
                    err, suggestion
                ),
            ));
        }
    }

    Ok((validator_tokens, logic_tokens))
}
