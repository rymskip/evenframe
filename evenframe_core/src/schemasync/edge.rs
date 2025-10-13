use proc_macro2::TokenStream;
use quote::{ToTokens, quote};
use std::fmt;
use std::str::FromStr;
use syn::parenthesized;
use syn::spanned::Spanned;
use tracing::{debug, error, info, trace, warn};

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct EdgeConfig {
    pub edge_name: String,
    pub from: String,
    pub to: String,
    pub direction: Direction,
}

impl ToTokens for EdgeConfig {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let edge_name = &self.edge_name;
        let from = &self.from;
        let to = &self.to;
        let direction = &self.direction;

        tokens.extend(quote! {
            ::evenframe::schemasync::EdgeConfig {
                edge_name: #edge_name.to_string(),
                from: #from.to_string(),
                to: #to.to_string(),
                direction: #direction
            }
        });
    }
}

impl EdgeConfig {
    pub fn parse(field: &syn::Field) -> syn::Result<Option<EdgeConfig>> {
        debug!("Parsing edge configuration from field");
        let field_name = field
            .ident
            .as_ref()
            .map(|i| i.to_string())
            .unwrap_or_else(|| "<unnamed>".to_string());
        trace!("Processing field: {}", field_name);

        let mut edge_name: Option<String> = None;
        let mut from: Option<String> = None;
        let mut to: Option<String> = None;
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
                    // For "edge_name", ensure we only set it once.
                    if meta.path.is_ident("edge_name") {
                        trace!("Parsing edge_name attribute");
                        let content;
                        parenthesized!(content in meta.input);
                        if edge_name.is_some() {
                            warn!(
                                "Duplicate edge_name attribute found on field {}",
                                field_name
                            );
                            return Err(meta.error("duplicate edge_name attribute"));
                        }
                        let parsed_name = content.parse::<syn::LitStr>()?.value();
                        trace!("Parsed edge_name: {}", parsed_name);
                        edge_name = Some(parsed_name);
                        return Ok(());
                    }
                    // For "from", ensure we only set it once.
                    if meta.path.is_ident("from") {
                        trace!("Parsing from attribute");
                        let content;
                        parenthesized!(content in meta.input);
                        if from.is_some() {
                            warn!("Duplicate from attribute found on field {}", field_name);
                            return Err(meta.error("duplicate from attribute"));
                        }
                        let parsed_from = content.parse::<syn::LitStr>()?.value();
                        trace!("Parsed from: {}", parsed_from);
                        from = Some(parsed_from);
                        return Ok(());
                    }
                    // For "to", ensure we only set it once.
                    if meta.path.is_ident("to") {
                        trace!("Parsing to attribute");
                        let content;
                        parenthesized!(content in meta.input);
                        if to.is_some() {
                            warn!("Duplicate to attribute found on field {}", field_name);
                            return Err(meta.error("duplicate to attribute"));
                        }
                        let parsed_to = content.parse::<syn::LitStr>()?.value();
                        trace!("Parsed to: {}", parsed_to);
                        to = Some(parsed_to);
                        return Ok(());
                    }

                    if meta.path.is_ident("direction") {
                        trace!("Parsing direction attribute");
                        let content;
                        parenthesized!(content in meta.input);
                        if direction.is_some() {
                            warn!(
                                "Duplicate direction attribute found on field {}",
                                field_name
                            );
                            return Err(meta.error("duplicate direction attribute"));
                        }
                        let lit: syn::LitStr = content.parse()?;
                        let direction_str = lit.value();
                        trace!("Parsing direction string: {}", direction_str);
                        // Convert the string into a Direction using FromStr.
                        let parsed_direction = direction_str.parse::<Direction>().map_err(|e| {
                            warn!(
                                "Invalid direction '{}' on field {}: {}",
                                direction_str, field_name, e
                            );
                            meta.error(e)
                        })?;
                        trace!("Parsed direction: {:?}", parsed_direction);
                        direction = Some(parsed_direction);
                        return Ok(());
                    }
                    // If an unexpected attribute is encountered, return an error.
                    let path = meta.path.to_token_stream().to_string();
                    warn!(
                        "Unrecognized edge detail '{}' on field {}",
                        path, field_name
                    );
                    Err(meta.error("unrecognized edge detail"))
                })?;
                // If any of the required attributes is missing, return an error indicating which one.
                debug!("Validating parsed edge attributes for field {}", field_name);
                let edge_name = edge_name.ok_or_else(|| {
                    error!("Missing edge_name attribute on field {}", field_name);
                    syn::Error::new(field.span(), "missing edge_name attribute")
                })?;
                let from = from.ok_or_else(|| {
                    error!("Missing from attribute on field {}", field_name);
                    syn::Error::new(field.span(), "missing from attribute")
                })?;
                let to = to.ok_or_else(|| {
                    error!("Missing to attribute on field {}", field_name);
                    syn::Error::new(field.span(), "missing to attribute")
                })?;
                let direction = direction.ok_or_else(|| {
                    error!("Missing direction attribute on field {}", field_name);
                    syn::Error::new(field.span(), "missing direction attribute")
                })?;

                let edge_config = EdgeConfig {
                    edge_name: edge_name.clone(),
                    from: from.clone(),
                    to: to.clone(),
                    direction: direction.clone(),
                };
                info!(
                    "Successfully parsed edge configuration for field {}: {} -> {} -> {}, direction: {:?}",
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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
