use proc_macro2::TokenStream;
use quote::quote;
use syn::{Attribute, Expr, ExprArray, ExprLit, Lit, Meta, spanned::Spanned};
use tracing::{debug, error, info, trace, warn};

/// Parse coordinate attributes from mock_data attribute
/// Returns Ok(None) if no coordinate attribute found, Ok(Some(vec)) if found, or Err on parse errors
pub fn parse_coordinate_attribute(
    attrs: &[Attribute],
) -> Result<Option<Vec<TokenStream>>, syn::Error> {
    info!(
        "Starting coordinate attribute parsing for {} attributes",
        attrs.len()
    );
    for (index, attr) in attrs.iter().enumerate() {
        trace!("Processing attribute {} of {}", index + 1, attrs.len());
        if attr.path().is_ident("mock_data") {
            debug!("Found mock_data attribute, parsing for coordinate parameter");
            let result: Result<syn::punctuated::Punctuated<Meta, syn::Token![,]>, _> =
                attr.parse_args_with(syn::punctuated::Punctuated::parse_terminated);

            match result {
                Ok(metas) => {
                    debug!(
                        "Successfully parsed {} meta arguments, searching for coordinate",
                        metas.len()
                    );
                    for (meta_index, meta) in metas.iter().enumerate() {
                        trace!(
                            "Processing meta {} of {}: coordinate parameter",
                            meta_index + 1,
                            metas.len()
                        );
                        match meta {
                            Meta::NameValue(nv) if nv.path.is_ident("coordinate") => {
                                debug!("Found coordinate parameter");
                                // coordinate = [...]
                                if let Expr::Array(ExprArray { elems, .. }) = &nv.value {
                                    debug!(
                                        "Coordinate parameter is an array with {} elements",
                                        elems.len()
                                    );
                                    let mut coordinates = Vec::new();

                                    for (elem_index, elem) in elems.iter().enumerate() {
                                        trace!(
                                            "Processing coordinate element {} of {}",
                                            elem_index + 1,
                                            elems.len()
                                        );
                                        match parse_coordinate_expr(elem) {
                                            Ok(coord_tokens) => {
                                                debug!(
                                                    "Successfully parsed coordinate element {}",
                                                    elem_index + 1
                                                );
                                                coordinates.push(coord_tokens);
                                            }
                                            Err(err) => {
                                                error!(
                                                    "Failed to parse coordinate element {}: {}",
                                                    elem_index + 1,
                                                    err
                                                );
                                                return Err(err);
                                            }
                                        }
                                    }

                                    info!(
                                        "Successfully parsed {} coordinate rules",
                                        coordinates.len()
                                    );
                                    return Ok(Some(coordinates));
                                } else {
                                    warn!("Coordinate parameter is not an array");
                                    return Err(syn::Error::new(
                                        nv.value.span(),
                                        "The 'coordinate' parameter must be an array of coordination rules.\n\nExample:\n#[mock_data(coordinate = [\n    InitializeEqual([\"field1\", \"field2\"]),\n    InitializeSum { fields: [\"price\", \"tax\"], total: 100.0 }\n])]",
                                    ));
                                }
                            }
                            _ => {
                                trace!("Skipping non-coordinate meta");
                            }
                        }
                    }
                }
                Err(err) => {
                    error!("Failed to parse mock_data attribute arguments: {}", err);
                    return Err(syn::Error::new(
                        attr.span(),
                        format!("Failed to parse mock_data attribute: {}", err),
                    ));
                }
            }
        }
    }
    debug!("No coordinate attribute found");
    Ok(None)
}

/// Parse individual coordinate expression
fn parse_coordinate_expr(expr: &Expr) -> Result<TokenStream, syn::Error> {
    trace!("Parsing coordinate expression");
    match expr {
        Expr::Call(call) => {
            let func_name = if let Expr::Path(path) = &*call.func {
                path.path
                    .segments
                    .last()
                    .ok_or_else(|| {
                        error!("Invalid coordination function path");
                        syn::Error::new(call.func.span(), "Invalid coordination function path")
                    })?
                    .ident
                    .to_string()
            } else {
                warn!("Coordination rule is not a function call");
                return Err(syn::Error::new(
                    call.func.span(),
                    "Coordination rule must be a function call like InitializeEqual(...)",
                ));
            };
            debug!("Found coordination function: {}", func_name);

            match func_name.as_str() {
                "InitializeEqual" => {
                    debug!("Processing InitializeEqual coordination rule");
                    parse_initialize_equal(call)
                }
                "InitializeSequential" => {
                    debug!("Processing InitializeSequential coordination rule");
                    parse_initialize_sequential(call)
                }
                "InitializeSum" => {
                    debug!("Processing InitializeSum coordination rule");
                    parse_initialize_sum(call)
                }
                "InitializeCoherent" => {
                    debug!("Processing InitializeCoherent coordination rule");
                    parse_initialize_coherent(call)
                }
                _ => {
                    warn!("Unknown coordination rule: {}", func_name);
                    Err(syn::Error::new(
                        call.func.span(),
                        format!(
                            "Unknown coordination rule '{}'. Valid rules are:\n- InitializeEqual\n- InitializeSequential\n- InitializeSum\n- InitializeCoherent",
                            func_name
                        ),
                    ))
                }
            }
        }
        _ => Err(syn::Error::new(
            expr.span(),
            "Coordination rule must be a function call.\n\nExample: InitializeEqual([\"field1\", \"field2\"])",
        )),
    }
}

/// Parse InitializeEqual coordination rule
fn parse_initialize_equal(call: &syn::ExprCall) -> Result<TokenStream, syn::Error> {
    trace!("Parsing InitializeEqual arguments");
    if let Some(Expr::Array(arr)) = call.args.first() {
        debug!("Found array argument with {} elements", arr.elems.len());
        let fields = parse_string_array(arr)?;
        if fields.is_empty() {
            error!("InitializeEqual requires at least one field");
            return Err(syn::Error::new(
                arr.span(),
                "InitializeEqual requires at least one field",
            ));
        }
        debug!(
            "Successfully parsed {} fields for InitializeEqual: {:?}",
            fields.len(),
            fields
        );
        Ok(quote! {
            evenframe::coordinate::Coordination::InitializeEqual(vec![#(#fields.to_string()),*])
        })
    } else {
        Err(syn::Error::new(
            call.args.span(),
            "InitializeEqual requires an array of field names.\n\nExample: InitializeEqual([\"field1\", \"field2\"])",
        ))
    }
}

/// Parse InitializeSequential coordination rule
fn parse_initialize_sequential(call: &syn::ExprCall) -> Result<TokenStream, syn::Error> {
    if let Some(Expr::Struct(s)) = call.args.first() {
        let fields = extract_field_value(&s.fields, "fields").ok_or_else(|| {
            syn::Error::new(
                s.span(),
                "InitializeSequential requires a 'fields' parameter",
            )
        })?;

        let fields_array = if let Expr::Array(arr) = fields {
            arr
        } else {
            return Err(syn::Error::new(
                fields.span(),
                "The 'fields' parameter must be an array of field names",
            ));
        };

        let field_names = parse_string_array(fields_array)?;
        if field_names.is_empty() {
            return Err(syn::Error::new(
                fields_array.span(),
                "InitializeSequential requires at least one field",
            ));
        }

        let increment = extract_field_value(&s.fields, "increment").ok_or_else(|| {
            syn::Error::new(
                s.span(),
                "InitializeSequential requires an 'increment' parameter",
            )
        })?;

        let increment_tokens = parse_increment(increment)?;

        Ok(quote! {
            evenframe::coordinate::Coordination::InitializeSequential {
                fields: vec![#(#field_names.to_string()),*],
                increment: #increment_tokens,
            }
        })
    } else {
        Err(syn::Error::new(
            call.args.span(),
            "InitializeSequential requires a struct literal.\n\nExample:\nInitializeSequential {\n    fields: [\"date1\", \"date2\"],\n    increment: Days(7)\n}",
        ))
    }
}

/// Parse InitializeSum coordination rule
fn parse_initialize_sum(call: &syn::ExprCall) -> Result<TokenStream, syn::Error> {
    if let Some(Expr::Struct(s)) = call.args.first() {
        let fields = extract_field_value(&s.fields, "fields").ok_or_else(|| {
            syn::Error::new(s.span(), "InitializeSum requires a 'fields' parameter")
        })?;

        let fields_array = if let Expr::Array(arr) = fields {
            arr
        } else {
            return Err(syn::Error::new(
                fields.span(),
                "The 'fields' parameter must be an array of field names",
            ));
        };

        let field_names = parse_string_array(fields_array)?;
        if field_names.len() < 2 {
            return Err(syn::Error::new(
                fields_array.span(),
                "InitializeSum requires at least two fields to sum",
            ));
        }

        let total_expr = extract_field_value(&s.fields, "total").ok_or_else(|| {
            syn::Error::new(s.span(), "InitializeSum requires a 'total' parameter")
        })?;

        let total = if let Expr::Lit(ExprLit {
            lit: Lit::Float(f), ..
        }) = total_expr
        {
            f.base10_parse::<f64>().map_err(|_| {
                syn::Error::new(
                    f.span(),
                    format!("Invalid float value: {}", f.base10_digits()),
                )
            })?
        } else if let Expr::Lit(ExprLit {
            lit: Lit::Int(i), ..
        }) = total_expr
        {
            i.base10_parse::<f64>().map_err(|_| {
                syn::Error::new(
                    i.span(),
                    format!("Invalid numeric value: {}", i.base10_digits()),
                )
            })?
        } else {
            return Err(syn::Error::new(
                total_expr.span(),
                "The 'total' parameter must be a numeric literal",
            ));
        };

        Ok(quote! {
            evenframe::coordinate::Coordination::InitializeSum {
                fields: vec![#(#field_names.to_string()),*],
                total: #total,
            }
        })
    } else {
        Err(syn::Error::new(
            call.args.span(),
            "InitializeSum requires a struct literal.\n\nExample:\nInitializeSum {\n    fields: [\"price\", \"tax\", \"shipping\"],\n    total: 100.0\n}",
        ))
    }
}

/// Parse InitializeCoherent coordination rule
fn parse_initialize_coherent(call: &syn::ExprCall) -> Result<TokenStream, syn::Error> {
    if let Some(Expr::Struct(s)) = call.args.first() {
        let fields = extract_field_value(&s.fields, "fields").ok_or_else(|| {
            syn::Error::new(s.span(), "InitializeCoherent requires a 'fields' parameter")
        })?;

        let fields_array = if let Expr::Array(arr) = fields {
            arr
        } else {
            return Err(syn::Error::new(
                fields.span(),
                "The 'fields' parameter must be an array of field names",
            ));
        };

        let field_names = parse_string_array(fields_array)?;
        if field_names.is_empty() {
            return Err(syn::Error::new(
                fields_array.span(),
                "InitializeCoherent requires at least one field",
            ));
        }

        let dataset = extract_field_value(&s.fields, "dataset").ok_or_else(|| {
            syn::Error::new(
                s.span(),
                "InitializeCoherent requires a 'dataset' parameter",
            )
        })?;

        let dataset_tokens = parse_coherent_dataset(dataset)?;

        Ok(quote! {
            evenframe::coordinate::Coordination::InitializeCoherent {
                fields: vec![#(#field_names.to_string()),*],
                dataset: #dataset_tokens,
            }
        })
    } else {
        Err(syn::Error::new(
            call.args.span(),
            "InitializeCoherent requires a struct literal.\n\nExample:\nInitializeCoherent {\n    fields: [\"street\", \"city\", \"state\"],\n    dataset: Address\n}",
        ))
    }
}

/// Parse an array of string literals
fn parse_string_array(arr: &ExprArray) -> Result<Vec<String>, syn::Error> {
    trace!("Parsing string array with {} elements", arr.elems.len());
    let mut strings = Vec::new();
    for (index, elem) in arr.elems.iter().enumerate() {
        trace!(
            "Processing array element {} of {}",
            index + 1,
            arr.elems.len()
        );
        if let Expr::Lit(ExprLit {
            lit: Lit::Str(s), ..
        }) = elem
        {
            let string_value = s.value();
            trace!("Found string literal: {}", string_value);
            strings.push(string_value);
        } else {
            error!("Array element {} is not a string literal", index + 1);
            return Err(syn::Error::new(
                elem.span(),
                "Array elements must be string literals.\n\nExample: [\"field1\", \"field2\"]",
            ));
        }
    }
    debug!("Successfully parsed {} string literals", strings.len());
    Ok(strings)
}

/// Extract a field value from struct fields by name
fn extract_field_value<'a>(
    fields: &'a syn::punctuated::Punctuated<syn::FieldValue, syn::Token![,]>,
    name: &str,
) -> Option<&'a Expr> {
    for field in fields {
        if let syn::Member::Named(ident) = &field.member
            && ident == name
        {
            return Some(&field.expr);
        }
    }

    None
}

/// Parse increment value for sequential initialization
fn parse_increment(expr: &Expr) -> Result<TokenStream, syn::Error> {
    if let Expr::Call(call) = expr {
        let func_name = if let Expr::Path(path) = &*call.func {
            path.path
                .segments
                .last()
                .ok_or_else(|| {
                    syn::Error::new(call.func.span(), "Invalid increment function path")
                })?
                .ident
                .to_string()
        } else {
            return Err(syn::Error::new(
                call.func.span(),
                "Increment must be a function call like Days(7) or Hours(24)",
            ));
        };

        let first_arg = call.args.first().ok_or_else(|| {
            syn::Error::new(
                call.args.span(),
                format!("{} requires a numeric argument", func_name),
            )
        })?;

        match func_name.as_str() {
            "Days" | "Hours" | "Minutes" => {
                if let Expr::Lit(ExprLit {
                    lit: Lit::Int(n), ..
                }) = first_arg
                {
                    let value: i32 = n.base10_parse().map_err(|_| {
                        syn::Error::new(
                            n.span(),
                            format!(
                                "Invalid integer value: {}. Must be a valid i32.",
                                n.base10_digits()
                            ),
                        )
                    })?;

                    match func_name.as_str() {
                        "Days" => Ok(
                            quote! { helpers::schemasync::coordinate::CoordinateIncrement::Days(#value) },
                        ),
                        "Hours" => Ok(
                            quote! { helpers::schemasync::coordinate::CoordinateIncrement::Hours(#value) },
                        ),
                        "Minutes" => Ok(
                            quote! { helpers::schemasync::coordinate::CoordinateIncrement::Minutes(#value) },
                        ),
                        _ => unreachable!(),
                    }
                } else {
                    Err(syn::Error::new(
                        first_arg.span(),
                        format!(
                            "{} requires an integer argument.\n\nExample: {}(7)",
                            func_name, func_name
                        ),
                    ))
                }
            }
            "Numeric" => {
                let value = if let Expr::Lit(ExprLit {
                    lit: Lit::Float(f), ..
                }) = first_arg
                {
                    f.base10_parse::<f64>().map_err(|_| {
                        syn::Error::new(
                            f.span(),
                            format!("Invalid float value: {}", f.base10_digits()),
                        )
                    })?
                } else if let Expr::Lit(ExprLit {
                    lit: Lit::Int(i), ..
                }) = first_arg
                {
                    i.base10_parse::<f64>().map_err(|_| {
                        syn::Error::new(
                            i.span(),
                            format!("Invalid numeric value: {}", i.base10_digits()),
                        )
                    })?
                } else {
                    return Err(syn::Error::new(
                        first_arg.span(),
                        "Numeric requires a numeric argument.\n\nExample: Numeric(1.5)",
                    ));
                };
                Ok(quote! { helpers::schemasync::coordinate::CoordinateIncrement::Numeric(#value) })
            }
            _ => Err(syn::Error::new(
                call.func.span(),
                format!(
                    "Unknown increment type '{}'. Valid types are:\n- Days(n)\n- Hours(n)\n- Minutes(n)\n- Numeric(n)",
                    func_name
                ),
            )),
        }
    } else {
        Err(syn::Error::new(
            expr.span(),
            "Increment must be a function call.\n\nExamples: Days(7), Hours(24), Minutes(30), Numeric(1.5)",
        ))
    }
}

/// Parse coherent dataset type
fn parse_coherent_dataset(expr: &Expr) -> Result<TokenStream, syn::Error> {
    if let Expr::Path(path) = expr {
        let dataset_name = path
            .path
            .segments
            .last()
            .ok_or_else(|| syn::Error::new(path.span(), "Invalid dataset path"))?
            .ident
            .to_string();

        match dataset_name.as_str() {
            "Address" => Ok(quote! { helpers::schemasync::coordinate::CoherentDataset::Address }),
            "PersonName" => {
                Ok(quote! { helpers::schemasync::coordinate::CoherentDataset::PersonName })
            }
            "GeoLocation" => {
                Ok(quote! { helpers::schemasync::coordinate::CoherentDataset::GeoLocation })
            }
            "DateRange" => {
                Ok(quote! { helpers::schemasync::coordinate::CoherentDataset::DateRange })
            }
            "Financial" => {
                Ok(quote! { helpers::schemasync::coordinate::CoherentDataset::Financial })
            }
            "GeoRadius" => {
                Ok(quote! { helpers::schemasync::coordinate::CoherentDataset::GeoRadius })
            }
            _ => Err(syn::Error::new(
                path.span(),
                format!(
                    "Unknown dataset type '{}'. Valid datasets are:\n- Address\n- PersonName\n- GeoLocation\n- GeoRadius\n- DateRange\n- Financial",
                    dataset_name
                ),
            )),
        }
    } else {
        Err(syn::Error::new(
            expr.span(),
            "Dataset must be a simple identifier.\n\nExample: dataset: Address",
        ))
    }
}
