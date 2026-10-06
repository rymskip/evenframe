use crate::surreal_value_impl::{UnionTable, union_surreal_value};
use crate::validate_impl::enum_validate;
use convert_case::{Case, Casing};
use evenframe_core::derive::{
    naming,
    typesync_attributes::{Position, TypesyncAttributes},
};
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
        // The scan reads these for the outputs; the derive checks them.
        let typesync = std::iter::once((&input.attrs, Position::Container)).chain(
            data_enum
                .variants
                .iter()
                .map(|variant| (&variant.attrs, Position::Variant)),
        );
        for (attrs, position) in typesync {
            if let Err(err) = TypesyncAttributes::parse(attrs, position) {
                return err.to_compile_error();
            }
        }

        let mut table_config_arms = Vec::new();
        let mut table_names = Vec::new();
        let mut tables = Vec::new();

        for variant in &data_enum.variants {
            let variant_ident = &variant.ident;
            let mut fields = variant.fields.iter();
            let field = match (&variant.fields, fields.next(), fields.next()) {
                (Fields::Unit, _, _) => {
                    return syn::Error::new(
                        variant.span(),
                        format!("EvenframeUnion variant '{}' cannot be a unit variant. Each variant must contain exactly one persistable struct.", variant_ident)
                    ).to_compile_error();
                }
                (_, Some(field), None) => field,
                _ => {
                    return syn::Error::new(
                        variant.span(),
                        format!("EvenframeUnion variant '{}' must contain exactly one field that is a persistable struct.", variant_ident)
                    ).to_compile_error();
                }
            };
            let type_name = extract_inner_type_name(&field.ty);
            match &field.ident {
                Some(field_name) => table_config_arms.push(quote! {
                    #ident::#variant_ident { #field_name } => #field_name.table_config()
                }),
                None => table_config_arms.push(quote! {
                    #ident::#variant_ident(inner) => inner.table_config()
                }),
            }
            tables.push(UnionTable {
                variant: variant_ident,
                member: field.ident.as_ref(),
                ty: &field.ty,
                table: type_name.to_case(Case::Snake),
            });
            table_names.push(type_name);
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
            #[::evenframe::linkme::distributed_slice(::evenframe::registry::UNION_OF_TABLES_REGISTRY_ENTRIES)]
            #[linkme(crate = ::evenframe::linkme)]
            static #registry_var_name: ::evenframe::registry::UnionOfTablesRegistryEntry = ::evenframe::registry::UnionOfTablesRegistryEntry {
                type_name: #union_name,
                table_names: &[#(#table_names_static),*],
                pipeline: ::evenframe::types::Pipeline::Both,
            };
        };

        let validate_impl =
            match naming::resolve(&input).and_then(|wire| enum_validate(&input, &wire)) {
                Ok(tokens) => tokens,
                Err(err) => return err.to_compile_error(),
            };

        let surreal_value_impl = union_surreal_value(&input, &tables);

        let metadata_gate = crate::metadata::gate(
            &input,
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
            },
        );
        quote! {
            impl ::evenframe::traits::EvenframeTable for #ident {}

            #validate_impl

            #surreal_value_impl

            #metadata_gate
        }
    } else {
        syn::Error::new(
            ident.span(),
            format!("The EvenframeUnion derive macro can only be applied to enums.\n\nYou tried to apply it to: {}\n\nExample of correct usage:\n#[derive(EvenframeUnion)]\nenum MyUnion {{\n    User(User),\n    Admin(Admin),\n}}", ident),
        )
        .to_compile_error()
    }
}
