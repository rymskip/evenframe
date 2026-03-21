use evenframe_core::{
    derive::attributes::{
        parse_annotation_attributes, parse_macroforge_derive_attribute,
        parse_serde_enum_representation,
    },
    types::{EnumRepresentation, FieldType},
};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields};

pub fn generate_enum_impl(input: DeriveInput) -> TokenStream {
    let ident = input.ident.clone();

    if let Data::Enum(ref data_enum) = input.data {
        let enum_name = ident.to_string();

        // Parse enum-level macroforge_derive attribute
        let macroforge_derives = match parse_macroforge_derive_attribute(&input.attrs) {
            Ok(derives) => derives,
            Err(err) => return err.to_compile_error(),
        };

        // Parse enum-level annotation attributes
        let enum_annotations = match parse_annotation_attributes(&input.attrs) {
            Ok(annotations) => annotations,
            Err(err) => return err.to_compile_error(),
        };

        // Parse serde enum representation
        let representation = match parse_serde_enum_representation(&input.attrs) {
            Ok(repr) => repr,
            Err(err) => return err.to_compile_error(),
        };

        let representation_tokens = match &representation {
            EnumRepresentation::ExternallyTagged => {
                quote! { EnumRepresentation::ExternallyTagged }
            }
            EnumRepresentation::InternallyTagged { tag } => {
                quote! { EnumRepresentation::InternallyTagged { tag: #tag.to_string() } }
            }
            EnumRepresentation::AdjacentlyTagged { tag, content } => {
                quote! { EnumRepresentation::AdjacentlyTagged { tag: #tag.to_string(), content: #content.to_string() } }
            }
            EnumRepresentation::Untagged => {
                quote! { EnumRepresentation::Untagged }
            }
        };

        let mut variant_tokens = Vec::new();

        for variant in &data_enum.variants {
            let variant_name = variant.ident.to_string();

            // Parse variant-level annotation attributes
            let variant_annotations = match parse_annotation_attributes(&variant.attrs) {
                Ok(annotations) => annotations,
                Err(err) => return err.to_compile_error(),
            };

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
                                        doccom: None,
                                        annotations: vec![],
                                        unique: false,
                                        mock_plugin: None,
                                    }
                                }
                            })
                            .collect();

                        quote! {
                            Some(VariantData::InlineStruct(StructConfig {
                                struct_name: format!("{}_{}", #enum_name, #variant_name),
                                fields: vec![#(#struct_fields),*],
                                validators: vec![],
                                doccom: None,
                                macroforge_derives: vec![],
                                annotations: vec![],
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
                                    doccom: None,
                                    annotations: vec![],
                                    unique: false,
                                    mock_plugin: None,
                                }
                            }
                        })
                        .collect();

                    quote! {
                        Some(VariantData::InlineStruct(StructConfig {
                            struct_name: format!("{}_{}", #enum_name, #variant_name),
                            fields: vec![#(#struct_fields),*],
                            validators: vec![],
                            doccom: None,
                            macroforge_derives: vec![],
                            annotations: vec![],
                        }))
                    }
                }
            };

            let variant_annotations_tokens = if variant_annotations.is_empty() {
                quote! { vec![] }
            } else {
                quote! { vec![#(#variant_annotations.to_string()),*] }
            };

            variant_tokens.push(quote! {
                Variant {
                    name: #variant_name.to_string(),
                    data: #variant_data,
                    doccom: None,
                    annotations: #variant_annotations_tokens,
                }
            });
        }

        // Generate registry submission for enum structs
        let registry_var_name = syn::Ident::new(
            &format!("{}_ENUM_REGISTRY_ENTRY", ident.to_string().to_uppercase()),
            ident.span(),
        );
        let registry_submission = quote! {
            #[::evenframe::linkme::distributed_slice(::evenframe::registry::ENUM_REGISTRY_ENTRIES)]
            static #registry_var_name: ::evenframe::registry::EnumRegistryEntry = ::evenframe::registry::EnumRegistryEntry {
                type_name: #enum_name,
                tagged_union_fn: || #ident::variants(),
            };
        };

        quote! {
            const _: () = {
                use ::evenframe::types::{TaggedUnion, Variant, VariantData, StructConfig, StructField, FieldType, EnumRepresentation};
                use ::evenframe::traits::EvenframeTaggedUnion;

                impl EvenframeTaggedUnion for #ident {
                    fn variants() -> TaggedUnion {
                        let macroforge_derives_val: Vec<String> = vec![#(#macroforge_derives.to_string()),*];
                        let enum_annotations_val: Vec<String> = vec![#(#enum_annotations.to_string()),*];
                        TaggedUnion {
                            enum_name: #enum_name.to_string(),
                            variants: vec![#(#variant_tokens),*],
                            representation: #representation_tokens,
                            doccom: None,
                            macroforge_derives: macroforge_derives_val,
                            annotations: enum_annotations_val,
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
