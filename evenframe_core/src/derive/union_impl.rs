use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, spanned::Spanned};

pub fn generate_union_impl(input: DeriveInput) -> TokenStream {
    let ident = input.ident.clone();

    if let Data::Enum(ref data_enum) = input.data {
        let mut table_config_arms = Vec::new();

        for variant in &data_enum.variants {
            let variant_ident = &variant.ident;

            match &variant.fields {
                Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
                    table_config_arms.push(quote! {
                        #ident::#variant_ident(inner) => inner.table_config()
                    });
                }
                Fields::Named(fields) if fields.named.len() == 1 => {
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
