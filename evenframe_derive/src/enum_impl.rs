use evenframe_core::types::FieldType;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields};

pub fn generate_enum_impl(input: DeriveInput) -> TokenStream {
    let ident = input.ident.clone();

    if let Data::Enum(ref data_enum) = input.data {
        let enum_name = ident.to_string();
        let mut variant_tokens = Vec::new();

        for variant in &data_enum.variants {
            let variant_name = variant.ident.to_string();

            let variant_data = match &variant.fields {
                Fields::Unit => {
                    quote! { None }
                }
                Fields::Unnamed(fields) => {
                    if fields.unnamed.len() == 1 {
                        let field_type = &fields.unnamed.first().unwrap().ty;
                        let field_type_parsed = FieldType::parse_syn_ty(field_type);
                        quote! {
                            Some(VariantData::DataStructureRef(#field_type_parsed))
                        }
                    } else {
                        // Multiple unnamed fields - create an inline struct
                        let struct_fields: Vec<_> = fields
                            .unnamed
                            .iter()
                            .enumerate()
                            .map(|(i, field)| {
                                let field_name = format!("field_{}", i);
                                let field_type = FieldType::parse_syn_ty(&field.ty);
                                quote! {
                                    StructField {
                                        field_name: #field_name.to_string(),
                                        field_type: #field_type,
                                        edge_config: None,
                                        define_config: None,
                                        format: None,
                                        validators: vec![],
                                        always_regenerate: false,
                                    }
                                }
                            })
                            .collect();

                        quote! {
                            Some(VariantData::InlineStruct(StructConfig {
                                struct_name: format!("{}_{}", #enum_name, #variant_name),
                                fields: vec![#(#struct_fields),*],
                                validators: vec![],
                            }))
                        }
                    }
                }
                Fields::Named(fields) => {
                    // Named fields - create an inline struct
                    let struct_fields: Vec<_> = fields
                        .named
                        .iter()
                        .map(|field| {
                            let field_name = field.ident.as_ref().unwrap().to_string();
                            let field_type = FieldType::parse_syn_ty(&field.ty);
                            quote! {
                                StructField {
                                    field_name: #field_name.to_string(),
                                    field_type: #field_type,
                                    edge_config: None,
                                    define_config: None,
                                    format: None,
                                    validators: vec![],
                                    always_regenerate: false,
                                }
                            }
                        })
                        .collect();

                    quote! {
                        Some(VariantData::InlineStruct(StructConfig {
                            struct_name: format!("{}_{}", #enum_name, #variant_name),
                            fields: vec![#(#struct_fields),*],
                            validators: vec![],
                        }))
                    }
                }
            };

            variant_tokens.push(quote! {
                Variant {
                    name: #variant_name.to_string(),
                    data: #variant_data,
                }
            });
        }

        // Generate registry submission for enum structs
        let registry_var_name = syn::Ident::new(
            &format!("{}_ENUM_REGISTRY_ENTRY", ident.to_string().to_uppercase()),
            ident.span(),
        );
        let registry_submission = quote! {
            #[linkme::distributed_slice(::evenframe::registry::ENUM_REGISTRY_ENTRIES)]
            static #registry_var_name: ::evenframe::registry::EnumRegistryEntry = ::evenframe::registry::EnumRegistryEntry {
                type_name: #enum_name,
                tagged_union_fn: || #ident::variants(),
            };
        };

        quote! {
            const _: () = {
                use ::evenframe::types::{TaggedUnion, Variant, VariantData, StructConfig, StructField, FieldType};
                use ::evenframe::traits::EvenframeTaggedUnion;

                impl EvenframeTaggedUnion for #ident {
                    fn variants() -> TaggedUnion {
                        TaggedUnion {
                            enum_name: #enum_name.to_string(),
                            variants: vec![#(#variant_tokens),*],
                        }
                    }
                }

                #registry_submission
            };
        }
    } else {
        syn::Error::new(
            ident.span(),
            format!("The Evenframe derive macro can only be applied to enums when using generate_enum_impl.\n\nYou tried to apply it to: {}\n\nExample of correct usage:\n#[derive(Evenframe)]\nenum MyEnum {{\n    Variant1,\n    Variant2(String),\n    Variant3 {{ field: i32 }}\n}}", ident),
        )
        .to_compile_error()
    }
}
