use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Type, TypePath, spanned::Spanned};

/// Extract the innermost type name from a potentially wrapped type (e.g., Box<Account> -> Account)
fn extract_inner_type_name(ty: &Type) -> String {
    match ty {
        Type::Path(TypePath { path, .. }) => {
            if let Some(segment) = path.segments.last() {
                let ident = &segment.ident;

                // Check if this is a generic type like Box<T>, Option<T>, etc.
                if !segment.arguments.is_empty()
                    && let syn::PathArguments::AngleBracketed(args) = &segment.arguments
                    && let Some(syn::GenericArgument::Type(inner_type)) = args.args.first()
                {
                    // Recursively extract from the inner type
                    return extract_inner_type_name(inner_type);
                }

                // Return the current segment name if no generics or can't extract
                ident.to_string()
            } else {
                // Fallback to the quoted representation
                quote! { #ty }.to_string()
            }
        }
        _ => {
            // For non-path types, fall back to quoted representation
            quote! { #ty }.to_string()
        }
    }
}

pub fn generate_union_impl(input: DeriveInput) -> TokenStream {
    let ident = input.ident.clone();

    if let Data::Enum(ref data_enum) = input.data {
        let mut table_config_arms = Vec::new();
        let mut table_names = Vec::new();

        for variant in &data_enum.variants {
            let variant_ident = &variant.ident;

            match &variant.fields {
                Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                    let field_type = &fields.unnamed.first().unwrap().ty;
                    let type_name = extract_inner_type_name(field_type);
                    table_names.push(type_name);

                    table_config_arms.push(quote! {
                        #ident::#variant_ident(inner) => inner.table_config()
                    });
                }
                Fields::Named(fields) if fields.named.len() == 1 => {
                    let field_type = &fields.named.first().unwrap().ty;
                    let type_name = extract_inner_type_name(field_type);
                    table_names.push(type_name);

                    let field_name = fields.named.first().unwrap().ident.as_ref().unwrap();
                    table_config_arms.push(quote! {
                        #ident::#variant_ident { #field_name } => #field_name.table_config()
                    });
                }
                Fields::Unit => {
                    return syn::Error::new(
                        variant.span(),
                        format!("EvenframeUnion variant '{}' cannot be a unit variant. Each variant must contain exactly one persistable struct.", variant_ident)
                    ).to_compile_error();
                }
                _ => {
                    return syn::Error::new(
                        variant.span(),
                        format!("EvenframeUnion variant '{}' must contain exactly one field that is a persistable struct.", variant_ident)
                    ).to_compile_error();
                }
            }
        }

        let union_name = ident.to_string();
        let table_names_static: Vec<_> = table_names.iter().map(|name| quote! { #name }).collect();

        // Generate registry submission for union of tables
        let registry_var_name = syn::Ident::new(
            &format!(
                "{}_UNION_OF_TABLES_REGISTRY_ENTRY",
                ident.to_string().to_uppercase()
            ),
            ident.span(),
        );
        let registry_submission = quote! {
            #[linkme::distributed_slice(::evenframe::registry::UNION_OF_TABLES_REGISTRY_ENTRIES)]
            static #registry_var_name: ::evenframe::registry::UnionOfTablesRegistryEntry = ::evenframe::registry::UnionOfTablesRegistryEntry {
                type_name: #union_name,
                table_names: &[#(#table_names_static),*],
            };
        };

        quote! {
            const _: () = {
                impl ::evenframe::traits::EvenframePersistableStruct for #ident {
                    fn static_table_config() -> ::evenframe::schemasync::TableConfig {
                        panic!("EvenframeUnion types do not support static_table_config() because the configuration depends on which variant is present. Use the instance method table_config(&self) instead.")
                    }

                    fn table_config(&self) -> ::evenframe::schemasync::TableConfig {
                        match self {
                            #(#table_config_arms),*
                        }
                    }
                }

                #registry_submission
            };
        }
    } else {
        syn::Error::new(
            ident.span(),
            format!("The EvenframeUnion derive macro can only be applied to enums.\n\nYou tried to apply it to: {}\n\nExample of correct usage:\n#[derive(EvenframeUnion)]\nenum MyUnion {{\n    User(User),\n    Admin(Admin),\n}}", ident),
        )
        .to_compile_error()
    }
}
