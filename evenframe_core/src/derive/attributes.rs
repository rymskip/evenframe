use quote::quote;
use syn::parse::Parse;
use syn::punctuated::Punctuated;
use syn::{
    Attribute, Expr, ExprArray, ExprLit, Ident, Lit, LitStr, Meta, Token, parenthesized,
    spanned::Spanned,
};
use tracing::{debug, error, info, trace};

use crate::{
    schemasync::{
        Direction, EdgeConfig, IndexConfig,
        mockmake::{MockGenerationConfig, coordinate::Coordination, format::Format},
    },
    types::{EnumRepresentation, StructField},
};
use std::collections::{BTreeSet, HashMap};
use std::convert::TryFrom;

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
                            Meta::NameValue(nv) if nv.path.is_ident("plugin") => {
                                debug!("Processing 'plugin' parameter");
                                if let Expr::Lit(ExprLit {
                                    lit: Lit::Str(lit), ..
                                }) = &nv.value
                                {
                                    base_config.plugin = Some(lit.value());
                                } else {
                                    return Err(syn::Error::new(
                                        nv.value.span(),
                                        "The 'plugin' parameter must be a string literal.\n\nExample: #[mock_data(plugin = \"my_plugin\")]",
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
                                        "Unknown parameter '{}' in mock_data attribute.\n\nValid parameters are: n, overrides, coordinate, plugin\n\nExample: #[mock_data(n = 1000, plugin = \"my_plugin\")]",
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
                            // Loading format overrides from config is not currently supported.
                            // This code runs inside a proc macro (compile time), so it cannot
                            // access runtime config. Compile-time file reading is possible but
                            // fragile (cargo doesn't track TOML changes as dependencies).
                            // For now, specify format overrides inline via field-level attributes.
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

/// Parse `#[mockmake(plugin = "name")]` attribute on a field.
pub fn parse_mockmake_attribute(attrs: &[Attribute]) -> Result<Option<String>, syn::Error> {
    for attr in attrs {
        if attr.path().is_ident("mockmake") {
            let result: Result<syn::punctuated::Punctuated<Meta, syn::Token![,]>, _> =
                attr.parse_args_with(syn::punctuated::Punctuated::parse_terminated);

            match result {
                Ok(metas) => {
                    for meta in &metas {
                        if let Meta::NameValue(nv) = meta
                            && nv.path.is_ident("plugin")
                        {
                            if let Expr::Lit(ExprLit {
                                lit: Lit::Str(lit), ..
                            }) = &nv.value
                            {
                                return Ok(Some(lit.value()));
                            } else {
                                return Err(syn::Error::new(
                                    nv.value.span(),
                                    "The 'plugin' parameter must be a string literal.\n\nExample: #[mockmake(plugin = \"my_plugin\")]",
                                ));
                            }
                        }
                    }
                    return Err(syn::Error::new(
                        attr.span(),
                        "Unknown parameter in mockmake attribute.\n\nValid parameter: plugin\n\nExample: #[mockmake(plugin = \"my_plugin\")]",
                    ));
                }
                Err(err) => {
                    return Err(syn::Error::new(
                        attr.span(),
                        format!(
                            "Failed to parse mockmake attribute: {}\n\nExample: #[mockmake(plugin = \"my_plugin\")]",
                            err
                        ),
                    ));
                }
            }
        }
    }
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

/// Parses every `#[index(fields(a, b, ...), unique?)]` attribute on a struct
/// into a `Vec<IndexConfig>`, validating that each ident inside `fields(...)`
/// names a real struct field (`known_fields` is the snake-cased field name set
/// with any `r#` prefix stripped).
pub fn parse_index_attributes(
    attrs: &[Attribute],
    known_fields: &BTreeSet<String>,
) -> Result<Vec<IndexConfig>, syn::Error> {
    let mut indexes = Vec::new();

    for attr in attrs.iter().filter(|a| a.path().is_ident("index")) {
        let mut fields: Option<Vec<Ident>> = None;
        let mut unique = false;

        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("fields") {
                if fields.is_some() {
                    return Err(meta.error("duplicate `fields(...)` in #[index(...)]"));
                }
                let content;
                parenthesized!(content in meta.input);
                let parsed: Punctuated<Ident, Token![,]> =
                    content.parse_terminated(Ident::parse, Token![,])?;
                let collected: Vec<Ident> = parsed.into_iter().collect();
                if collected.is_empty() {
                    return Err(meta.error(
                        "`fields(...)` must list at least one struct field identifier",
                    ));
                }
                fields = Some(collected);
                Ok(())
            } else if meta.path.is_ident("unique") {
                unique = true;
                Ok(())
            } else {
                Err(meta
                    .error("expected `fields(<ident>, ...)` or `unique` inside #[index(...)]"))
            }
        })?;

        let fields = fields.ok_or_else(|| {
            syn::Error::new(
                attr.path().span(),
                "`#[index(...)]` requires `fields(<ident>, ...)`\n\nExample: #[index(fields(user, message), unique)]",
            )
        })?;

        let mut field_names = Vec::with_capacity(fields.len());
        for ident in &fields {
            let raw = ident.to_string();
            let name = raw.trim_start_matches("r#").to_string();
            if !known_fields.contains(&name) {
                let mut all: Vec<&String> = known_fields.iter().collect();
                all.sort();
                let listed = all
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(syn::Error::new(
                    ident.span(),
                    format!(
                        "unknown field `{}` in #[index(...)]; struct has fields: {}",
                        name, listed
                    ),
                ));
            }
            field_names.push(name);
        }

        indexes.push(IndexConfig {
            fields: field_names,
            unique,
        });
    }

    Ok(indexes)
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

            // Handle bare #[relation] with no arguments
            let result: Result<syn::punctuated::Punctuated<Meta, syn::Token![,]>, _> =
                attr.parse_args_with(syn::punctuated::Punctuated::parse_terminated);

            match result {
                Ok(metas) => {
                    let mut edge_name = None;
                    let mut direction: Option<Direction> = None;

                    for meta in metas {
                        match meta {
                            Meta::NameValue(nv)
                                if nv.path.is_ident("edge_name") || nv.path.is_ident("name") =>
                            {
                                if let Expr::Lit(ExprLit {
                                    lit: Lit::Str(lit), ..
                                }) = &nv.value
                                {
                                    edge_name = Some(lit.value());
                                } else {
                                    return Err(syn::Error::new(
                                        nv.value.span(),
                                        "The 'edge_name' (or 'name') parameter must be a string literal.\n\nExample: #[relation(edge_name = \"has_user\")]",
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
                                        "The 'direction' parameter must be a string literal with value \"from\", \"to\", or \"both\".\n\nExample: direction = \"from\"",
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
                                        "Unknown parameter '{}' in relation attribute.\n\nValid parameters are: edge_name (or name), direction\n\nExamples:\n#[relation]\n#[relation(edge_name = \"custom_name\")]\n#[relation(edge_name = \"custom_name\", direction = \"from\")]",
                                        param_name
                                    ),
                                ));
                            }
                            _ => {
                                return Err(syn::Error::new(
                                    meta.span(),
                                    "Invalid syntax in relation attribute.\n\nExamples:\n#[relation]\n#[relation(edge_name = \"custom_name\")]\n#[relation(edge_name = \"custom_name\", direction = \"from\")]",
                                ));
                            }
                        }
                    }

                    info!(
                        "Successfully parsed relation attribute: edge_name={:?}, direction={:?}",
                        edge_name, direction
                    );
                    return Ok(Some(EdgeConfig {
                        edge_name: edge_name.unwrap_or_default(),
                        from: vec![],
                        to: vec![],
                        direction,
                    }));
                }
                Err(_) => {
                    // No arguments — bare #[relation]
                    info!("Parsed bare #[relation] attribute");
                    return Ok(Some(EdgeConfig {
                        edge_name: String::new(),
                        from: vec![],
                        to: vec![],
                        direction: None,
                    }));
                }
            }
        }
    }
    debug!("No relation attribute found");
    Ok(None)
}

pub fn parse_doccom_attribute(attrs: &[Attribute]) -> Result<Option<String>, syn::Error> {
    for attr in attrs {
        if attr.path().is_ident("doccom") {
            let lit: LitStr = attr.parse_args().map_err(|e| {
                syn::Error::new(
                    attr.span(),
                    format!(
                        "Failed to parse doccom attribute: {}\n\nExpected usage: #[doccom(\"Description text\")]",
                        e
                    ),
                )
            })?;

            let value = lit.value();

            if value.trim().is_empty() {
                return Err(syn::Error::new(
                    lit.span(),
                    "Doc comment cannot be empty or whitespace-only.\n\nExample: #[doccom(\"A user account in the system\")]",
                ));
            }

            return Ok(Some(value));
        }
    }
    Ok(None)
}

pub fn parse_macroforge_derive_attribute(attrs: &[Attribute]) -> Result<Vec<String>, syn::Error> {
    for attr in attrs {
        if attr.path().is_ident("macroforge_derive") {
            let result: Result<syn::punctuated::Punctuated<Meta, syn::Token![,]>, _> =
                attr.parse_args_with(syn::punctuated::Punctuated::parse_terminated);

            match result {
                Ok(metas) => {
                    let mut derives = Vec::new();
                    for meta in metas {
                        match meta {
                            Meta::Path(path) => {
                                if let Some(ident) = path.get_ident() {
                                    derives.push(ident.to_string());
                                } else {
                                    return Err(syn::Error::new(
                                        path.span(),
                                        "Expected a simple identifier in macroforge_derive.\n\nExample: #[macroforge_derive(Default, Serialize, Deserialize)]",
                                    ));
                                }
                            }
                            _ => {
                                return Err(syn::Error::new(
                                    meta.span(),
                                    "Expected bare identifiers in macroforge_derive.\n\nExample: #[macroforge_derive(Default, Serialize, Deserialize)]",
                                ));
                            }
                        }
                    }
                    return Ok(derives);
                }
                Err(err) => {
                    return Err(syn::Error::new(
                        attr.span(),
                        format!(
                            "Failed to parse macroforge_derive attribute: {}\n\nExample: #[macroforge_derive(Default, Serialize, Deserialize)]",
                            err
                        ),
                    ));
                }
            }
        }
    }
    Ok(Vec::new())
}

/// Extract all derive names from `#[derive(...)]` attributes.
///
/// Returns identifiers like `["Serialize", "Clone", "Debug", "Evenframe"]`.
pub fn parse_rust_derives(attrs: &[Attribute]) -> Vec<String> {
    let mut derives = Vec::new();
    for attr in attrs {
        if attr.path().is_ident("derive")
            && let Meta::List(meta_list) = &attr.meta
        {
            // Parse the token stream as comma-separated paths
            let result: Result<syn::punctuated::Punctuated<syn::Path, syn::Token![,]>, _> =
                meta_list.parse_args_with(syn::punctuated::Punctuated::parse_terminated);
            if let Ok(paths) = result {
                for path in paths {
                    // Use the last segment (e.g., "Serialize" from "serde::Serialize")
                    if let Some(segment) = path.segments.last() {
                        derives.push(segment.ident.to_string());
                    }
                }
            }
        }
    }
    derives
}

pub fn parse_annotation_attributes(attrs: &[Attribute]) -> Result<Vec<String>, syn::Error> {
    let mut annotations = Vec::new();

    for attr in attrs {
        if attr.path().is_ident("annotation") {
            let lit: LitStr = attr.parse_args().map_err(|e| {
                syn::Error::new(
                    attr.span(),
                    format!(
                        "Failed to parse annotation attribute: {}\n\nExpected usage: #[annotation(\"@decorator({{{{ key: \\\"value\\\" }}}})\")]",
                        e
                    ),
                )
            })?;

            let value = lit.value();

            if value.trim().is_empty() {
                return Err(syn::Error::new(
                    lit.span(),
                    "Annotation cannot be empty.\n\nExample: #[annotation(\"@default\")]",
                ));
            }

            annotations.push(value);
        }
    }

    Ok(annotations)
}

pub fn parse_serde_enum_representation(
    attrs: &[Attribute],
) -> Result<EnumRepresentation, syn::Error> {
    let mut tag: Option<String> = None;
    let mut content: Option<String> = None;
    let mut untagged = false;

    for attr in attrs {
        if attr.path().is_ident("serde") {
            let nested: syn::punctuated::Punctuated<Meta, syn::Token![,]> =
                attr.parse_args_with(syn::punctuated::Punctuated::parse_terminated)?;

            for meta in &nested {
                match meta {
                    Meta::NameValue(nv) if nv.path.is_ident("tag") => {
                        if let Expr::Lit(ExprLit {
                            lit: Lit::Str(lit), ..
                        }) = &nv.value
                        {
                            tag = Some(lit.value());
                        }
                    }
                    Meta::NameValue(nv) if nv.path.is_ident("content") => {
                        if let Expr::Lit(ExprLit {
                            lit: Lit::Str(lit), ..
                        }) = &nv.value
                        {
                            content = Some(lit.value());
                        }
                    }
                    Meta::Path(p) if p.is_ident("untagged") => {
                        untagged = true;
                    }
                    _ => {}
                }
            }
        }
    }

    if untagged {
        return Ok(EnumRepresentation::Untagged);
    }

    match (tag, content) {
        (Some(t), Some(c)) => Ok(EnumRepresentation::AdjacentlyTagged { tag: t, content: c }),
        (Some(t), None) => Ok(EnumRepresentation::InternallyTagged { tag: t }),
        (None, Some(_)) => Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "#[serde(content = \"...\")] requires #[serde(tag = \"...\")]",
        )),
        (None, None) => Ok(EnumRepresentation::ExternallyTagged),
    }
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

    #[test]
    fn parse_serde_no_attrs_returns_externally_tagged() {
        let attrs: Vec<Attribute> = vec![];
        let result = parse_serde_enum_representation(&attrs).unwrap();
        assert_eq!(result, EnumRepresentation::ExternallyTagged);
    }

    #[test]
    fn parse_serde_tag_only_returns_internally_tagged() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[serde(tag = "type")])];
        let result = parse_serde_enum_representation(&attrs).unwrap();
        assert_eq!(
            result,
            EnumRepresentation::InternallyTagged {
                tag: "type".to_string()
            }
        );
    }

    #[test]
    fn parse_serde_tag_and_content_returns_adjacently_tagged() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[serde(tag = "t", content = "c")])];
        let result = parse_serde_enum_representation(&attrs).unwrap();
        assert_eq!(
            result,
            EnumRepresentation::AdjacentlyTagged {
                tag: "t".to_string(),
                content: "c".to_string()
            }
        );
    }

    #[test]
    fn parse_serde_untagged() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[serde(untagged)])];
        let result = parse_serde_enum_representation(&attrs).unwrap();
        assert_eq!(result, EnumRepresentation::Untagged);
    }

    #[test]
    fn parse_serde_content_without_tag_errors() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[serde(content = "c")])];
        let result = parse_serde_enum_representation(&attrs);
        assert!(result.is_err());
    }

    #[test]
    fn parse_serde_ignores_non_serde_attrs() {
        let attrs: Vec<Attribute> = vec![
            parse_quote!(#[derive(Debug)]),
            parse_quote!(#[serde(tag = "kind")]),
        ];
        let result = parse_serde_enum_representation(&attrs).unwrap();
        assert_eq!(
            result,
            EnumRepresentation::InternallyTagged {
                tag: "kind".to_string()
            }
        );
    }
}
