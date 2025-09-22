use quote::quote;
use syn::spanned::Spanned;
use syn::{GenericArgument, PathArguments, Type};
use tracing::{debug, trace, warn};

// Remove unused import - type_parser generates tokens that need fully qualified paths

/// Generate a helpful error message for unsupported types
fn unsupported_type_error(ty: &Type, type_str: &str, hint: &str) -> proc_macro2::TokenStream {
    syn::Error::new(
        ty.span(),
        format!(
            "Unsupported type: '{}'. {}\n\nSupported types include:\n\
            - Primitives: bool, char, String, i8-i128, u8-u128, f32, f64\n\
            - Special: Decimal, DateTime, EvenframeDuration, EvenframeRecordId\n\
            - Containers: Option<T>, Vec<T>, HashMap<K,V>, BTreeMap<K,V>\n\
            - Custom: RecordLink<T>, OrderedFloat<T>, or any custom struct/enum",
            type_str, hint
        ),
    )
    .to_compile_error()
}

/// Parse generic type arguments
fn parse_generic_args(
    ty: &Type,
    type_name: &str,
    args: &syn::punctuated::Punctuated<GenericArgument, syn::token::Comma>,
) -> proc_macro2::TokenStream {
    debug!(
        "Parsing generic arguments for type '{}' with {} arguments",
        type_name,
        args.len()
    );
    match (type_name, args.len()) {
        ("Option" | "Vec" | "RecordLink" | "OrderedFloat", 1) => {
            if let Some(GenericArgument::Type(inner_ty)) = args.first() {
                trace!("Processing inner type for {}", type_name);
                let inner_parsed = parse_data_type(inner_ty);
                match type_name {
                    "Option" => {
                        quote! { ::evenframe::types::FieldType::Option(Box::new(#inner_parsed)) }
                    }
                    "Vec" => {
                        quote! { ::evenframe::types::FieldType::Vec(Box::new(#inner_parsed)) }
                    }
                    "RecordLink" => {
                        quote! { ::evenframe::types::FieldType::RecordLink(Box::new(#inner_parsed)) }
                    }
                    "OrderedFloat" => {
                        quote! { ::evenframe::types::FieldType::OrderedFloat(Box::new(#inner_parsed)) }
                    }
                    _ => unreachable!(),
                }
            } else {
                syn::Error::new(
                    args.span(),
                    format!("{} type parameter must be a type", type_name),
                )
                .to_compile_error()
            }
        }
        ("HashMap" | "BTreeMap", 2) => {
            let mut args_iter = args.iter();
            match (args_iter.next(), args_iter.next()) {
                (Some(GenericArgument::Type(key_ty)), Some(GenericArgument::Type(value_ty))) => {
                    let key_parsed = parse_data_type(key_ty);
                    let value_parsed = parse_data_type(value_ty);
                    match type_name {
                        "HashMap" => {
                            quote! { ::evenframe::types::FieldType::HashMap(Box::new(#key_parsed), Box::new(#value_parsed)) }
                        }
                        "BTreeMap" => {
                            quote! { ::evenframe::types::FieldType::BTreeMap(Box::new(#key_parsed), Box::new(#value_parsed)) }
                        }
                        _ => unreachable!(),
                    }
                }
                _ => syn::Error::new(
                    args.span(),
                    format!("{} type parameters must be types", type_name),
                )
                .to_compile_error(),
            }
        }
        ("DateTime" | "EvenframeDuration", _) => {
            // These can have type params but we ignore them
            match type_name {
                "DateTime" => quote! { ::evenframe::types::FieldType::DateTime },
                "EvenframeDuration" => quote! { ::evenframe::types::FieldType::EvenframeDuration },
                _ => unreachable!(),
            }
        }
        (name, count) => {
            let expected = match name {
                "Option" | "Vec" | "RecordLink" | "OrderedFloat" => 1,
                "HashMap" | "BTreeMap" => 2,
                _ => {
                    return unsupported_type_error(
                        ty,
                        &format!("{}<...>", name),
                        "Unknown generic type",
                    );
                }
            };
            syn::Error::new(
                args.span(),
                format!(
                    "{} must have exactly {} type parameter{}, found {}",
                    name,
                    expected,
                    if expected == 1 { "" } else { "s" },
                    count
                ),
            )
            .to_compile_error()
        }
    }
}

/// Parse simple type by name
fn parse_simple_type(name: &str) -> Option<proc_macro2::TokenStream> {
    trace!("Checking if '{}' is a simple type", name);
    match name {
        "String" => Some(quote! { ::evenframe::types::FieldType::String }),
        "char" => Some(quote! { ::evenframe::types::FieldType::Char }),
        "bool" => Some(quote! { ::evenframe::types::FieldType::Bool }),
        "f32" => Some(quote! { ::evenframe::types::FieldType::F32 }),
        "f64" => Some(quote! { ::evenframe::types::FieldType::F64 }),
        "i8" => Some(quote! { ::evenframe::types::FieldType::I8 }),
        "i16" => Some(quote! { ::evenframe::types::FieldType::I16 }),
        "i32" => Some(quote! { ::evenframe::types::FieldType::I32 }),
        "i64" => Some(quote! { ::evenframe::types::FieldType::I64 }),
        "i128" => Some(quote! { ::evenframe::types::FieldType::I128 }),
        "isize" => Some(quote! { ::evenframe::types::FieldType::Isize }),
        "u8" => Some(quote! { ::evenframe::types::FieldType::U8 }),
        "u16" => Some(quote! { ::evenframe::types::FieldType::U16 }),
        "u32" => Some(quote! { ::evenframe::types::FieldType::U32 }),
        "u64" => Some(quote! { ::evenframe::types::FieldType::U64 }),
        "u128" => Some(quote! { ::evenframe::types::FieldType::U128 }),
        "usize" => Some(quote! { ::evenframe::types::FieldType::Usize }),
        "EvenframeRecordId" => Some(quote! { ::evenframe::types::FieldType::EvenframeRecordId }),
        "Decimal" => Some(quote! { ::evenframe::types::FieldType::Decimal }),
        "DateTime" => Some(quote! { ::evenframe::types::FieldType::DateTime }),
        "EvenframeDuration" => Some(quote! { ::evenframe::types::FieldType::EvenframeDuration }),
        "Tz" => Some(quote! { ::evenframe::types::FieldType::Timezone }),
        "()" => Some(quote! { ::evenframe::types::FieldType::Unit }),
        _ => {
            trace!("'{}' is not a simple type", name);
            None
        }
    }
}

/// Check for common type mistakes
fn check_common_mistakes(ident: &syn::Ident) -> Option<proc_macro2::TokenStream> {
    match ident.to_string().as_str() {
        "str" => Some(syn::Error::new(
            ident.span(),
            "Use 'String' instead of 'str' for owned string types"
        ).to_compile_error()),
        "int" | "float" | "double" => Some(syn::Error::new(
            ident.span(),
            format!("'{}' is not a Rust type. Use i32/i64 for integers or f32/f64 for floating-point numbers", ident)
        ).to_compile_error()),
        _ => None,
    }
}

/// Parse a Rust type into its corresponding FieldType representation
pub fn parse_data_type(ty: &Type) -> proc_macro2::TokenStream {
    let type_str = quote! { #ty }.to_string();
    trace!("Parsing data type: {}", type_str);
    match ty {
        // Handle reference types
        Type::Reference(type_ref) => {
            warn!("Unsupported reference type: {}", type_str);
            syn::Error::new(
                type_ref.span(),
                "Reference types (&T or &mut T) are not supported. \
                Use owned types instead (e.g., String instead of &str)",
            )
            .to_compile_error()
        }

        // Handle pointer types
        Type::Ptr(_) => {
            warn!("Unsupported pointer type: {}", type_str);
            syn::Error::new(
                ty.span(),
                "Raw pointer types are not supported in Evenframe schemas",
            )
            .to_compile_error()
        }

        // Handle array types
        Type::Array(arr) => {
            warn!("Unsupported array type: {}", type_str);
            syn::Error::new(
                arr.span(),
                "Fixed-size arrays are not supported. Use Vec<T> for dynamic arrays instead",
            )
            .to_compile_error()
        }

        // Handle slice types
        Type::Slice(slice) => {
            warn!("Unsupported slice type: {}", type_str);
            syn::Error::new(
                slice.span(),
                "Slice types are not supported. Use Vec<T> instead",
            )
            .to_compile_error()
        }

        // Handle tuple types
        Type::Tuple(tuple) => {
            debug!("Parsing tuple type with {} elements", tuple.elems.len());
            let elems = tuple.elems.iter().enumerate().map(|(index, elem)| {
                trace!(
                    "Processing tuple element {} of {}",
                    index + 1,
                    tuple.elems.len()
                );
                parse_data_type(elem)
            });
            debug!("Successfully parsed tuple type");
            quote! { ::evenframe::types::FieldType::Tuple(vec![ #(#elems),* ]) }
        }

        // Handle path types (the most common case)
        Type::Path(type_path) => {
            trace!("Processing path type: {}", type_str);
            parse_path_type(ty, type_path)
        }

        // Fallback for any other type
        _ => {
            warn!("Unknown/unsupported type pattern: {}", type_str);
            unsupported_type_error(ty, &type_str, "This type pattern is not supported")
        }
    }
}

/// Parse path types (e.g., String, Vec<T>, std::collections::HashMap<K, V>)
fn parse_path_type(ty: &Type, type_path: &syn::TypePath) -> proc_macro2::TokenStream {
    let type_str = quote! { #ty }.to_string();
    debug!("Parsing path type: {}", type_str);

    // Get the last segment of the path (the actual type name)
    if let Some(last_segment) = type_path.path.segments.last() {
        let ident = &last_segment.ident;
        let ident_str = ident.to_string();
        trace!("Extracted type identifier: {}", ident_str);

        // Check for common mistakes
        if let Some(error) = check_common_mistakes(ident) {
            warn!("Common type mistake detected: {}", ident_str);
            return error;
        }

        // Check if it's a known simple type
        if let Some(field_type) = parse_simple_type(&ident_str) {
            debug!("Found simple type: {}", ident_str);
            return field_type;
        }

        // Check if it has generic arguments
        if let PathArguments::AngleBracketed(angle_args) = &last_segment.arguments {
            debug!("Type has generic arguments: {}<...>", ident_str);
            return parse_generic_args(ty, &ident_str, &angle_args.args);
        }

        // Handle known types without generic args that might be namespaced
        match ident_str.as_str() {
            "DateTime" => return quote! { ::evenframe::types::FieldType::DateTime },
            "EvenframeDuration" => {
                return quote! { ::evenframe::types::FieldType::EvenframeDuration };
            }
            "Decimal" => return quote! { ::evenframe::types::FieldType::Decimal },
            "Tz" => return quote! { ::evenframe::types::FieldType::Timezone },
            _ => {}
        }

        // Check for common namespaced types
        if type_str.ends_with("::String") {
            return quote! { ::evenframe::types::FieldType::String };
        }
        if type_str.ends_with("::HashMap") || type_str.ends_with("::BTreeMap") {
            return syn::Error::new(
                ty.span(),
                format!("'{}' requires type parameters. Use {}::<K, V> where K and V are your key and value types", type_str, type_str)
            ).to_compile_error();
        }
        if type_str.ends_with("::Vec") || type_str.ends_with("::Option") {
            return syn::Error::new(
                ty.span(),
                format!(
                    "'{}' requires a type parameter. Use {}::<T> where T is your inner type",
                    type_str, type_str
                ),
            )
            .to_compile_error();
        }
    }

    // If we get here, it's a custom type (struct/enum)
    debug!("Treating as custom type: {}", type_str);
    let lit = syn::LitStr::new(&type_str, ty.span());
    quote! { ::evenframe::types::FieldType::Other(#lit.to_string()) }
}
