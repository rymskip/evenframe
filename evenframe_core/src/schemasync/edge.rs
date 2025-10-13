use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, quote};
use std::fmt;
use std::str::FromStr;
use syn::parenthesized;
use syn::spanned::Spanned;
use syn::{Expr, ExprArray, ExprLit, Lit};
use tracing::{debug, error, info, trace, warn};

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct EdgeConfig {
    pub edge_name: String,

    pub from: Vec<String>,

    pub to: Vec<String>,

    /* Table implementations of EdgeConfig shouldn't have direction */
    pub direction: Option<Direction>,
}

impl ToTokens for EdgeConfig {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let edge_name = &self.edge_name;
        let from_values = self.from.iter();
        let to_values = self.to.iter();
        let direction_tokens = match &self.direction {
            Some(direction) => quote! { Some(#direction) },
            None => quote! { None },
        };

        tokens.extend(quote! {
            ::evenframe::schemasync::EdgeConfig {
                edge_name: #edge_name.to_string(),
                from: vec![#(#from_values.to_string()),*],
                to: vec![#(#to_values.to_string()),*],
                direction: #direction_tokens
            }
        });
    }
}

impl EdgeConfig {
    fn normalize_table_name(name: &str) -> String {
        name.chars()
            .filter(|c| !matches!(c, '_' | '-' | ' '))
            .flat_map(|c| c.to_lowercase())
            .collect()
    }

    pub fn matches_from_table(&self, table_name: &str) -> bool {
        let normalized = Self::normalize_table_name(table_name);
        self.from
            .iter()
            .any(|candidate| Self::normalize_table_name(candidate) == normalized)
    }

    pub fn matches_to_table(&self, table_name: &str) -> bool {
        let normalized = Self::normalize_table_name(table_name);
        self.to
            .iter()
            .any(|candidate| Self::normalize_table_name(candidate) == normalized)
    }

    pub fn resolve_direction_for_table(&self, table_name: &str) -> Direction {
        if let Some(direction) = self.direction {
            return direction;
        }

        let from_match = self.matches_from_table(table_name);
        let to_match = self.matches_to_table(table_name);

        match (from_match, to_match) {
            (true, true) => Direction::Both,
            (true, false) => Direction::From,
            (false, true) => Direction::To,
            (false, false) => {
                warn!(
                    "Unable to infer direction for table '{}' in edge '{}'. Defaulting to Both.",
                    table_name, self.edge_name
                );
                Direction::Both
            }
        }
    }

    fn parse_strings_from_expr(
        expr: Expr,
        field_name: &str,
        attr_name: &str,
    ) -> syn::Result<Vec<String>> {
        match expr {
            Expr::Lit(ExprLit {
                lit: Lit::Str(lit), ..
            }) => Ok(vec![lit.value()]),
            Expr::Array(ExprArray { elems, .. }) => {
                let mut values = Vec::new();
                for elem in elems {
                    match elem {
                        Expr::Lit(ExprLit {
                            lit: Lit::Str(lit), ..
                        }) => values.push(lit.value()),
                        other => {
                            return Err(syn::Error::new(
                                other.span(),
                                format!(
                                    "Each element in '{}' on field '{}' must be a string literal.\nExample: {} = [\"value\"]",
                                    attr_name, field_name, attr_name
                                ),
                            ));
                        }
                    }
                }
                if values.is_empty() {
                    return Err(syn::Error::new(
                        Span::call_site(),
                        format!(
                            "The '{}' array on field '{}' must contain at least one entry.",
                            attr_name, field_name
                        ),
                    ));
                }
                Ok(values)
            }
            other => Err(syn::Error::new(
                other.span(),
                format!(
                    "The '{}' attribute on field '{}' must be a string literal or array of string literals.",
                    attr_name, field_name
                ),
            )),
        }
    }

    fn parse_single_string(expr: Expr, field_name: &str, attr_name: &str) -> syn::Result<String> {
        let mut values = Self::parse_strings_from_expr(expr, field_name, attr_name)?;
        if values.len() != 1 {
            return Err(syn::Error::new(
                Span::call_site(),
                format!(
                    "The '{}' attribute on field '{}' expects a single value.",
                    attr_name, field_name
                ),
            ));
        }
        Ok(values.remove(0))
    }

    pub fn parse(field: &syn::Field) -> syn::Result<Option<EdgeConfig>> {
        debug!("Parsing edge configuration from field");
        let field_name = field
            .ident
            .as_ref()
            .map(|i| i.to_string())
            .unwrap_or_else(|| "<unnamed>".to_string());
        trace!("Processing field: {}", field_name);

        let mut edge_name: Option<String> = None;
        let mut from: Vec<String> = Vec::new();
        let mut to: Vec<String> = Vec::new();
        let mut direction: Option<Direction> = None;

        // Iterate over all attributes of the field.
        debug!(
            "Found {} attributes on field {}",
            field.attrs.len(),
            field_name
        );
        for (i, attr) in field.attrs.iter().enumerate() {
            trace!("Processing attribute {} of {}", i + 1, field.attrs.len());
            // Check if the attribute is an "edge" attribute.
            if attr.path().is_ident("edge") {
                debug!("Found edge attribute on field {}", field_name);
                attr.parse_nested_meta(|meta| {
                    let ident = meta.path.get_ident().map(|ident| ident.to_string());
                    match ident.as_deref() {
                        Some("edge_name") | Some("name") => {
                            trace!("Parsing edge_name attribute");
                            if edge_name.is_some() {
                                warn!(
                                    "Duplicate edge name attribute found on field {}",
                                    field_name
                                );
                                return Err(meta.error("duplicate edge name attribute"));
                            }
                            let expr = if meta.input.peek(syn::token::Paren) {
                                let content;
                                parenthesized!(content in meta.input);
                                content.parse::<Expr>()?
                            } else {
                                meta.value()?.parse::<Expr>()?
                            };
                            let parsed_name =
                                Self::parse_single_string(expr, &field_name, "edge_name")?;
                            trace!("Parsed edge_name: {}", parsed_name);
                            edge_name = Some(parsed_name);
                            Ok(())
                        }
                        Some("from") => {
                            trace!("Parsing from attribute");
                            let expr = if meta.input.peek(syn::token::Paren) {
                                let content;
                                parenthesized!(content in meta.input);
                                content.parse::<Expr>()?
                            } else {
                                meta.value()?.parse::<Expr>()?
                            };
                            let mut values =
                                Self::parse_strings_from_expr(expr, &field_name, "from")?;
                            trace!("Parsed from values: {:?}", values);
                            from.append(&mut values);
                            Ok(())
                        }
                        Some("to") => {
                            trace!("Parsing to attribute");
                            let expr = if meta.input.peek(syn::token::Paren) {
                                let content;
                                parenthesized!(content in meta.input);
                                content.parse::<Expr>()?
                            } else {
                                meta.value()?.parse::<Expr>()?
                            };
                            let mut values =
                                Self::parse_strings_from_expr(expr, &field_name, "to")?;
                            trace!("Parsed to values: {:?}", values);
                            to.append(&mut values);
                            Ok(())
                        }
                        Some("direction") => {
                            trace!("Parsing direction attribute");
                            if direction.is_some() {
                                warn!(
                                    "Duplicate direction attribute found on field {}",
                                    field_name
                                );
                                return Err(meta.error("duplicate direction attribute"));
                            }
                            let expr = if meta.input.peek(syn::token::Paren) {
                                let content;
                                parenthesized!(content in meta.input);
                                content.parse::<Expr>()?
                            } else {
                                meta.value()?.parse::<Expr>()?
                            };
                            let direction_str =
                                Self::parse_single_string(expr, &field_name, "direction")?;
                            trace!("Parsed direction string: {}", direction_str);
                            let parsed_direction =
                                direction_str.parse::<Direction>().map_err(|e| {
                                    warn!(
                                        "Invalid direction '{}' on field {}: {}",
                                        direction_str, field_name, e
                                    );
                                    meta.error(e)
                                })?;
                            direction = Some(parsed_direction);
                            Ok(())
                        }
                        _ => {
                            let path = meta.path.to_token_stream().to_string();
                            warn!(
                                "Unrecognized edge detail '{}' on field {}",
                                path, field_name
                            );
                            Err(meta.error("unrecognized edge detail"))
                        }
                    }
                })?;
                // If any of the required attributes is missing, return an error indicating which one.
                debug!("Validating parsed edge attributes for field {}", field_name);
                let edge_name = edge_name.ok_or_else(|| {
                    error!("Missing edge_name/name attribute on field {}", field_name);
                    syn::Error::new(field.span(), "missing edge_name (or name) attribute")
                })?;
                if from.is_empty() {
                    error!("Missing from attribute on field {}", field_name);
                    return Err(syn::Error::new(field.span(), "missing from attribute"));
                }
                if to.is_empty() {
                    error!("Missing to attribute on field {}", field_name);
                    return Err(syn::Error::new(field.span(), "missing to attribute"));
                }

                let edge_config = EdgeConfig {
                    edge_name: edge_name.clone(),
                    from: from.clone(),
                    to: to.clone(),
                    direction,
                };
                info!(
                    "Successfully parsed edge configuration for field {}: {:?} -> {} -> {:?}, direction: {:?}",
                    field_name, from, edge_name, to, direction
                );
                return Ok(Some(edge_config));
            }
        }

        debug!("No edge attribute found on field {}", field_name);
        Ok(None)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Subquery {
    pub text: String,
}

impl Subquery {
    pub fn parse(field: &syn::Field) -> syn::Result<Option<Subquery>> {
        let field_name = field
            .ident
            .as_ref()
            .map(|i| i.to_string())
            .unwrap_or_else(|| "<unnamed>".to_string());
        debug!("Parsing subquery configuration from field {}", field_name);

        for (i, attr) in field.attrs.iter().enumerate() {
            trace!(
                "Processing attribute {} of {} on field {}",
                i + 1,
                field.attrs.len(),
                field_name
            );
            if attr.path().is_ident("subquery") {
                debug!("Found subquery attribute on field {}", field_name);
                // Parse the attribute content as a literal string.
                let lit: syn::LitStr = attr.parse_args()?;
                let text = lit.value();
                trace!("Parsed subquery text with length: {}", text.len());
                info!("Successfully parsed subquery for field {}", field_name);
                return Ok(Some(Subquery { text }));
            }
        }
        // If no subquery attribute is found, return None.
        debug!("No subquery attribute found on field {}", field_name);
        Ok(None)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Direction {
    From,
    To,
    Both,
}

impl ToTokens for Direction {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            Direction::From => tokens.extend(quote! { ::evenframe::schemasync::Direction::From }),
            Direction::To => tokens.extend(quote! { ::evenframe::schemasync::Direction::To }),
            Direction::Both => tokens.extend(quote! { ::evenframe::schemasync::Direction::Both }),
        }
    }
}

impl FromStr for Direction {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        trace!("Parsing direction from string: '{}'", s);
        let normalized = s.to_lowercase();
        match normalized.as_str() {
            "from" => {
                trace!("Parsed direction: From");
                Ok(Direction::From)
            }
            "to" => {
                trace!("Parsed direction: To");
                Ok(Direction::To)
            }
            "both" => {
                trace!("Parsed direction: Both (mapped to To)");
                Ok(Direction::Both)
            }
            _ => {
                error!("Invalid direction string: '{}'", s);
                Err(format!("Invalid direction: {}", s))
            }
        }
    }
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Direction::From => write!(f, "From"),
            Direction::To => write!(f, "To"),
            Direction::Both => write!(f, "Both"),
        }
    }
}
