use quote::quote;
use syn::{Attribute, Expr, ExprArray, ExprLit, Lit, LitStr, Meta, spanned::Spanned};
use tracing::{debug, error, info, trace};

use crate::{
    format::Format,
    mockmake::{MockGenerationConfig, coordinate::Coordination},
    schemasync::{Direction, EdgeConfig},
    types::StructField,
};
use std::{collections::HashMap, convert::TryFrom};

// Remove unused imports - these are only used in the macro implementation, not generated code

pub fn parse_mock_data_attribute(
    attrs: &[Attribute],
) -> Result<Option<MockGenerationConfig>, syn::Error> {
    info!(
        "Starting mock_data attribute parsing for {} attributes",
        attrs.len()
    );
    trace!(
        "Processing attributes: {:?}",
        attrs
            .iter()
            .map(|a| a
                .path()
                .get_ident()
                .map(|i| i.to_string())
                .unwrap_or_else(|| "unknown".to_string()))
            .collect::<Vec<_>>()
    );

    for (index, attr) in attrs.iter().enumerate() {
        trace!("Processing attribute {} of {}", index + 1, attrs.len());
        if attr.path().is_ident("mock_data") {
            debug!("Found mock_data attribute, parsing arguments");
            let result: Result<syn::punctuated::Punctuated<Meta, syn::Token![,]>, _> =
                attr.parse_args_with(syn::punctuated::Punctuated::parse_terminated);

            match result {
                Ok(metas) => {
                    debug!("Successfully parsed {} meta arguments", metas.len());
                    // Start with defaults from MockGenerationConfig::default()
                    let mut base_config = MockGenerationConfig::default();
                    let mut overrides_name = None;

                    for (meta_index, meta) in metas.iter().enumerate() {
                        trace!("Processing meta {} of {}", meta_index + 1, metas.len());
                        match meta {
                            Meta::NameValue(nv) if nv.path.is_ident("n") => {
                                debug!("Processing 'n' parameter");
                                if let Expr::Lit(ExprLit {
                                    lit: Lit::Int(lit), ..
                                }) = &nv.value
                                {
                                    match lit.base10_parse::<usize>() {
                                        Ok(value) => {
                                            debug!("Successfully parsed n value: {}", value);
                                            base_config.n = value;
                                        }
                                        Err(_) => {
                                            error!(
                                                "Failed to parse 'n' value: {}",
                                                lit.base10_digits()
                                            );
                                            return Err(syn::Error::new(
                                                lit.span(),
                                                format!(
                                                    "Invalid value for 'n': '{}'. Expected a positive integer.\n\nExample: #[mock_data(n = 1000)]",
                                                    lit.base10_digits()
                                                ),
                                            ));
                                        }
                                    }
                                } else {
                                    return Err(syn::Error::new(
                                        nv.value.span(),
                                        "The 'n' parameter must be an integer literal.\n\nExample: #[mock_data(n = 1000)]",
                                    ));
                                }
                            }
                            Meta::NameValue(nv) if nv.path.is_ident("overrides") => {
                                if let Expr::Lit(ExprLit {
                                    lit: Lit::Str(lit), ..
                                }) = &nv.value
                                {
                                    overrides_name = Some(lit.value());
                                } else {
                                    return Err(syn::Error::new(
                                        nv.value.span(),
                                        "The 'overrides' parameter must be a string literal.\n\nExample: #[mock_data(overrides = \"custom_config\")]",
                                    ));
                                }
                            }
                            Meta::NameValue(nv) if nv.path.is_ident("coordinate") => {
                                // Skip here - coordinate is parsed separately by coordinate_parser
                            }
                            Meta::NameValue(nv) => {
                                let param_name = nv
                                    .path
                                    .get_ident()
                                    .map(|i| i.to_string())
                                    .unwrap_or_else(|| "unknown".to_string());
                                return Err(syn::Error::new(
                                    nv.path.span(),
                                    format!(
                                        "Unknown parameter '{}' in mock_data attribute.\n\nValid parameters are: n, overrides, coordinate\n\nExample: #[mock_data(n = 1000, overrides = \"config\", coordinate = [InitializeEqual([\"field1\", \"field2\"])])]",
                                        param_name
                                    ),
                                ));
                            }
                            _ => {
                                return Err(syn::Error::new(
                                    meta.span(),
                                    "Invalid syntax in mock_data attribute.\n\nExpected format: #[mock_data(n = 1000, overrides = \"config\")]",
                                ));
                            }
                        }
                    }

                    // Parse coordination rules directly from the attributes
                    let mut coordination_rules = Vec::new();

                    // Look for coordinate parameter in the metas we already have
                    for meta in metas.iter() {
                        if let Meta::NameValue(nv) = meta
                            && nv.path.is_ident("coordinate")
                        {
                            // coordinate = [...]
                            if let Expr::Array(ExprArray { elems, .. }) = &nv.value {
                                for elem in elems {
                                    match Coordination::try_from(elem) {
                                        Ok(coord) => {
                                            debug!(
                                                "Successfully parsed coordination rule: {:?}",
                                                coord
                                            );
                                            coordination_rules.push(coord);
                                        }
                                        Err(e) => {
                                            error!("Failed to parse coordination rule: {}", e);
                                            return Err(syn::Error::new(
                                                elem.span(),
                                                format!("Failed to parse coordination rule: {}", e),
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }

                    info!(
                        "Successfully parsed mock_data attribute: n={}, overrides={:?}, coordination_rules_count={}",
                        base_config.n,
                        overrides_name,
                        coordination_rules.len()
                    );

                    // Parse overrides from config if specified
                    let table_level_override: Option<HashMap<StructField, Format>> =
                        if let Some(override_name) = overrides_name {
                            // TODO: Load format overrides from configuration based on override_name
                            // For now, return None - this would be loaded from a config file
                            debug!(
                                "Override '{}' specified but override loading not yet implemented",
                                override_name
                            );
                            None
                        } else {
                            None
                        };

                    // Apply parsed values to the base config
                    base_config.table_level_override = table_level_override;
                    base_config.coordination_rules = coordination_rules;

                    return Ok(Some(base_config));
                }
                Err(err) => {
                    error!("Failed to parse mock_data attribute arguments: {}", err);
                    return Err(syn::Error::new(
                        attr.span(),
                        format!(
                            "Failed to parse mock_data attribute: {}\n\nExample usage:\n#[mock_data(n = 1000)]\n#[mock_data(n = 500, overrides = \"custom_config\")]",
                            err
                        ),
                    ));
                }
            }
        }
    }
    debug!("No mock_data attribute found");
    Ok(None)
}

pub fn parse_event_attributes(attrs: &[Attribute]) -> Result<Vec<String>, syn::Error> {
    info!(
        "Starting event attribute parsing for {} attributes",
        attrs.len()
    );

    let mut events = Vec::new();

    for attr in attrs {
        if attr.path().is_ident("event") {
            debug!("Found event attribute");
            let lit: LitStr = attr.parse_args().map_err(|e| {
                syn::Error::new(
                    attr.span(),
                    format!(
                        "Failed to parse event attribute: {}\n\nExpected usage: #[event(\"DEFINE EVENT name ON TABLE table WHEN ... THEN ...\")]",
                        e
                    ),
                )
            })?;

            let value = lit.value();
            trace!(event_statement = %value, "Parsed event attribute");

            if value.trim().is_empty() {
                return Err(syn::Error::new(
                    lit.span(),
                    "Event statement cannot be empty.\n\nExample: #[event(\"DEFINE EVENT my_event ON TABLE user WHEN $before != $after THEN ...\")]",
                ));
            }

            events.push(value);
        }
    }

    debug!(
        event_count = events.len(),
        "Completed event attribute parsing"
    );
    Ok(events)
}

pub fn parse_table_validators(attrs: &[Attribute]) -> Result<Vec<String>, syn::Error> {
    info!(
        "Starting table validators parsing for {} attributes",
        attrs.len()
    );
    let mut validators = Vec::new();

    for attr in attrs {
        if attr.path().is_ident("validators") {
            debug!("Found validators attribute");
            let result: Result<syn::punctuated::Punctuated<Meta, syn::Token![,]>, _> =
                attr.parse_args_with(syn::punctuated::Punctuated::parse_terminated);

            match result {
                Ok(metas) => {
                    for meta in metas {
                        match meta {
                            Meta::NameValue(nv) if nv.path.is_ident("custom") => {
                                if let Expr::Lit(ExprLit {
                                    lit: Lit::Str(lit), ..
                                }) = &nv.value
                                {
                                    let validator_value = lit.value();
                                    debug!("Adding custom validator: {}", validator_value);
                                    validators.push(validator_value);
                                } else {
                                    return Err(syn::Error::new(
                                        nv.value.span(),
                                        "The 'custom' parameter must be a string literal containing a validation expression.\n\nExample: #[validators(custom = \"$value > 0 AND $value < 100\")]",
                                    ));
                                }
                            }
                            Meta::NameValue(nv) => {
                                let param_name = nv
                                    .path
                                    .get_ident()
                                    .map(|i| i.to_string())
                                    .unwrap_or_else(|| "unknown".to_string());
                                return Err(syn::Error::new(
                                    nv.path.span(),
                                    format!(
                                        "Unknown parameter '{}' in validators attribute.\n\nValid parameter is: custom\n\nExample: #[validators(custom = \"$value > 0\")]",
                                        param_name
                                    ),
                                ));
                            }
                            _ => {
                                return Err(syn::Error::new(
                                    meta.span(),
                                    "Invalid syntax in validators attribute.\n\nExpected format: #[validators(custom = \"validation_expression\")]",
                                ));
                            }
                        }
                    }
                }
                Err(err) => {
                    return Err(syn::Error::new(
                        attr.span(),
                        format!(
                            "Failed to parse validators attribute: {}\n\nExample usage:\n#[validators(custom = \"$value > 0\")]\n#[validators(custom = \"string::len($value) > 5\")]",
                            err
                        ),
                    ));
                }
            }
        }
    }

    info!("Successfully parsed {} table validators", validators.len());
    Ok(validators)
}

pub fn parse_relation_attribute(attrs: &[Attribute]) -> Result<Option<EdgeConfig>, syn::Error> {
    info!(
        "Starting relation attribute parsing for {} attributes",
        attrs.len()
    );
    for attr in attrs {
        if attr.path().is_ident("relation") {
            debug!("Found relation attribute");
            let result: Result<syn::punctuated::Punctuated<Meta, syn::Token![,]>, _> =
                attr.parse_args_with(syn::punctuated::Punctuated::parse_terminated);

            match result {
                Ok(metas) => {
                    let mut edge_name = None;
                    let mut from = None;
                    let mut to = None;
                    let mut direction: Option<Direction> = None;

                    for meta in metas {
                        match meta {
                            Meta::NameValue(nv) if nv.path.is_ident("edge_name") => {
                                if let Expr::Lit(ExprLit {
                                    lit: Lit::Str(lit), ..
                                }) = &nv.value
                                {
                                    edge_name = Some(lit.value());
                                } else {
                                    return Err(syn::Error::new(
                                        nv.value.span(),
                                        "The 'edge_name' parameter must be a string literal.\n\nExample: edge_name = \"has_user\"",
                                    ));
                                }
                            }
                            Meta::NameValue(nv) if nv.path.is_ident("from") => {
                                if let Expr::Lit(ExprLit {
                                    lit: Lit::Str(lit), ..
                                }) = &nv.value
                                {
                                    from = Some(lit.value());
                                } else {
                                    return Err(syn::Error::new(
                                        nv.value.span(),
                                        "The 'from' parameter must be a string literal.\n\nExample: from = \"Order\"",
                                    ));
                                }
                            }
                            Meta::NameValue(nv) if nv.path.is_ident("to") => {
                                if let Expr::Lit(ExprLit {
                                    lit: Lit::Str(lit), ..
                                }) = &nv.value
                                {
                                    to = Some(lit.value());
                                } else {
                                    return Err(syn::Error::new(
                                        nv.value.span(),
                                        "The 'to' parameter must be a string literal.\n\nExample: to = \"User\"",
                                    ));
                                }
                            }
                            Meta::NameValue(nv) if nv.path.is_ident("direction") => {
                                if let Expr::Lit(ExprLit {
                                    lit: Lit::Str(lit), ..
                                }) = &nv.value
                                {
                                    direction = match lit.value().as_str() {
                                        "from" => Some(Direction::From),
                                        "to" => Some(Direction::To),
                                        "both" => Some(Direction::Both),
                                        other => {
                                            return Err(syn::Error::new(
                                                lit.span(),
                                                format!(
                                                    "Invalid direction '{}'. Valid values are: \"from\", \"to\", \"both\"\n\nExample: direction = \"from\"",
                                                    other
                                                ),
                                            ));
                                        }
                                    };
                                } else {
                                    return Err(syn::Error::new(
                                        nv.value.span(),
                                        "The 'direction' parameter must be a string literal with value \"from\" or \"to\".\n\nExample: direction = \"from\"",
                                    ));
                                }
                            }
                            Meta::NameValue(nv) => {
                                let param_name = nv
                                    .path
                                    .get_ident()
                                    .map(|i| i.to_string())
                                    .unwrap_or_else(|| "unknown".to_string());
                                return Err(syn::Error::new(
                                    nv.path.span(),
                                    format!(
                                        "Unknown parameter '{}' in relation attribute.\n\nValid parameters are: edge_name, from, to, direction\n\nExample: #[relation(edge_name = \"has_user\", from = \"Order\", to = \"User\", direction = \"from\")]",
                                        param_name
                                    ),
                                ));
                            }
                            _ => {
                                return Err(syn::Error::new(
                                    meta.span(),
                                    "Invalid syntax in relation attribute.\n\nExpected format: #[relation(edge_name = \"...\", from = \"...\", to = \"...\", direction = \"...\")]",
                                ));
                            }
                        }
                    }

                    match (&edge_name, &from, &to, &direction) {
                        (Some(edge_name), Some(from), Some(to), Some(direction)) => {
                            info!(
                                "Successfully parsed relation attribute: edge_name={}, from={}, to={}, direction={:?}",
                                edge_name, from, to, direction
                            );
                            return Ok(Some(EdgeConfig {
                                edge_name: edge_name.to_owned(),
                                from: from.to_owned(),
                                to: to.to_owned(),
                                direction: direction.to_owned(),
                            }));
                        }
                        _ => {
                            let missing = vec![
                                edge_name.is_none().then_some("edge_name"),
                                from.is_none().then_some("from"),
                                to.is_none().then_some("to"),
                                direction.is_none().then_some("direction"),
                            ]
                            .into_iter()
                            .flatten()
                            .collect::<Vec<_>>()
                            .join(", ");

                            return Err(syn::Error::new(
                                attr.span(),
                                format!(
                                    "Missing required parameters in relation attribute: {}\n\nAll parameters are required:\n#[relation(\n    edge_name = \"has_user\",\n    from = \"Order\",\n    to = \"User\",\n    direction = \"from\"\n)]",
                                    missing
                                ),
                            ));
                        }
                    }
                }
                Err(err) => {
                    return Err(syn::Error::new(
                        attr.span(),
                        format!(
                            "Failed to parse relation attribute: {}\n\nExample usage:\n#[relation(\n    edge_name = \"has_user\",\n    from = \"Order\",\n    to = \"User\",\n    direction = \"from\"\n)]",
                            err
                        ),
                    ));
                }
            }
        }
    }
    debug!("No relation attribute found");
    Ok(None)
}

pub fn parse_format_attribute(
    attrs: &[Attribute],
) -> Result<Option<proc_macro2::TokenStream>, syn::Error> {
    use syn::{Expr, ExprCall, ExprPath, Path, PathSegment};

    info!(
        "Starting format attribute parsing for {} attributes",
        attrs.len()
    );
    for attr in attrs {
        if attr.path().is_ident("format") {
            debug!("Found format attribute");
            // Parse the attribute content as an expression
            let expr: syn::Expr = attr.parse_args()
                .map_err(|e| syn::Error::new(
                    attr.span(),
                    format!("Failed to parse format attribute: {}\n\nExamples:\n#[format(DateTime)]\n#[format(Url(\"example.com\"))]", e)
                ))?;

            // Transform the expression to add Format:: prefix if needed
            let format_expr = match &expr {
                // If it's just an identifier like DateTime, convert to Format::DateTime
                Expr::Path(path_expr) if path_expr.path.segments.len() == 1 => {
                    let variant = &path_expr.path.segments[0];
                    let mut segments = syn::punctuated::Punctuated::new();
                    segments.push(PathSegment::from(syn::Ident::new("Format", variant.span())));
                    segments.push(variant.clone());
                    Expr::Path(ExprPath {
                        attrs: vec![],
                        qself: None,
                        path: Path {
                            leading_colon: None,
                            segments,
                        },
                    })
                }
                // If it's a call like Url("domain"), convert to Format::Url("domain")
                Expr::Call(call_expr) => {
                    if let Expr::Path(path_expr) = &*call_expr.func {
                        if path_expr.path.segments.len() == 1 {
                            let variant = &path_expr.path.segments[0];
                            let mut segments = syn::punctuated::Punctuated::new();
                            segments
                                .push(PathSegment::from(syn::Ident::new("Format", variant.span())));
                            segments.push(variant.clone());
                            Expr::Call(ExprCall {
                                attrs: call_expr.attrs.clone(),
                                func: Box::new(Expr::Path(ExprPath {
                                    attrs: vec![],
                                    qself: None,
                                    path: Path {
                                        leading_colon: None,
                                        segments,
                                    },
                                })),
                                paren_token: call_expr.paren_token,
                                args: call_expr.args.clone(),
                            })
                        } else {
                            expr.clone()
                        }
                    } else {
                        expr.clone()
                    }
                }
                // Otherwise keep as is
                _ => expr.clone(),
            };

            // Use the TryFrom implementation to parse the Format
            match Format::try_from(&format_expr) {
                Ok(format) => {
                    debug!("Successfully parsed format: {:?}", format);
                    // Since Format implements ToTokens, we can just quote it directly
                    return Ok(Some(quote! { #format }));
                }
                Err(e) => {
                    error!("Failed to parse format expression: {}", e);
                    return Err(syn::Error::new(
                        expr.span(),
                        format!(
                            "{}\n\nValid formats:\n- Simple: DateTime, Date, Time, Currency, Percentage, Phone, Email, FirstName, LastName, CompanyName, PhoneNumber, ColorHex, JwtToken, Oklch, PostalCode\n- With parameter: Url(\"domain.com\")",
                            e
                        ),
                    ));
                }
            }
        }
    }
    debug!("No format attribute found");
    Ok(None)
}

pub fn parse_format_attribute_bin(attrs: &[Attribute]) -> Result<Option<Format>, syn::Error> {
    use syn::{Expr, ExprCall, ExprPath, Path, PathSegment};

    info!(
        "Starting format attribute parsing for {} attributes",
        attrs.len()
    );
    for attr in attrs {
        if attr.path().is_ident("format") {
            debug!("Found format attribute");
            // Parse the attribute content as an expression
            let expr: syn::Expr = attr.parse_args()
                .map_err(|e| syn::Error::new(
                    attr.span(),
                    format!("Failed to parse format attribute: {}\n\nExamples:\n#[format(DateTime)]\n#[format(Url(\"example.com\"))]", e)
                ))?;

            // Transform the expression to add Format:: prefix if needed
            let format_expr = match &expr {
                // If it's just an identifier like DateTime, convert to Format::DateTime
                Expr::Path(path_expr) if path_expr.path.segments.len() == 1 => {
                    let variant = &path_expr.path.segments[0];
                    let mut segments = syn::punctuated::Punctuated::new();
                    segments.push(PathSegment::from(syn::Ident::new("Format", variant.span())));
                    segments.push(variant.clone());
                    Expr::Path(ExprPath {
                        attrs: vec![],
                        qself: None,
                        path: Path {
                            leading_colon: None,
                            segments,
                        },
                    })
                }
                // If it's a call like Url("domain"), convert to Format::Url("domain")
                Expr::Call(call_expr) => {
                    if let Expr::Path(path_expr) = &*call_expr.func {
                        if path_expr.path.segments.len() == 1 {
                            let variant = &path_expr.path.segments[0];
                            let mut segments = syn::punctuated::Punctuated::new();
                            segments
                                .push(PathSegment::from(syn::Ident::new("Format", variant.span())));
                            segments.push(variant.clone());
                            Expr::Call(ExprCall {
                                attrs: call_expr.attrs.clone(),
                                func: Box::new(Expr::Path(ExprPath {
                                    attrs: vec![],
                                    qself: None,
                                    path: Path {
                                        leading_colon: None,
                                        segments,
                                    },
                                })),
                                paren_token: call_expr.paren_token,
                                args: call_expr.args.clone(),
                            })
                        } else {
                            expr.clone()
                        }
                    } else {
                        expr.clone()
                    }
                }
                // Otherwise keep as is
                _ => expr.clone(),
            };

            // Use the TryFrom implementation to parse the Format
            match Format::try_from(&format_expr) {
                Ok(format) => {
                    debug!("Successfully parsed format: {:?}", format);
                    // Since Format implements ToTokens, we can just quote it directly
                    return Ok(Some(format));
                }
                Err(e) => {
                    error!("Failed to parse format expression: {}", e);
                    return Err(syn::Error::new(
                        expr.span(),
                        format!(
                            "{}\n\nValid formats:\n- Simple: DateTime, Date, Time, Currency, Percentage, Phone, Email, FirstName, LastName, CompanyName, PhoneNumber, ColorHex, JwtToken, Oklch, PostalCode\n- With parameter: Url(\"domain.com\")",
                            e
                        ),
                    ));
                }
            }
        }
    }
    debug!("No format attribute found");
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn parse_event_attributes_collects_events() {
        let attrs: Vec<Attribute> = vec![
            parse_quote!(#[mock_data(n = 10)]),
            parse_quote!(#[event("DEFINE EVENT foo ON TABLE user WHEN true THEN { RETURN true }")]),
            parse_quote!(#[event("DEFINE EVENT bar ON TABLE user WHEN true THEN { RETURN false }")]),
        ];

        let events = parse_event_attributes(&attrs).expect("expected events to parse");
        assert_eq!(events.len(), 2);
        assert_eq!(
            events,
            vec![
                "DEFINE EVENT foo ON TABLE user WHEN true THEN { RETURN true }".to_string(),
                "DEFINE EVENT bar ON TABLE user WHEN true THEN { RETURN false }".to_string(),
            ]
        );
    }

    #[test]
    fn parse_event_attributes_rejects_empty_statements() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[event("")])];
        let result = parse_event_attributes(&attrs);
        assert!(result.is_err());
    }
}
