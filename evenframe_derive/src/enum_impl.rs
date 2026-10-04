use crate::PipelineKind;
use crate::validate_impl::enum_validate;
use evenframe_core::{
    derive::{
        attributes::{
            parse_annotation_attributes, parse_macroforge_derive_attribute, parse_rust_derives,
        },
        naming,
    },
    types::{EnumRepresentation, FieldType, PathNames},
};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields};

pub fn generate_enum_impl(input: DeriveInput, pipeline: PipelineKind) -> TokenStream {
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

        // Parse all Rust derives
        let rust_derives = parse_rust_derives(&input.attrs);

        let wire = match naming::resolve(&input) {
            Ok(wire) => wire,
            Err(err) => return err.to_compile_error(),
        };

        let representation_tokens = match &wire.representation {
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

        let pipeline_tokens = pipeline.to_tokens();

        let mut variant_tokens = Vec::new();

        for (variant, variant_wire) in data_enum.variants.iter().zip(&wire.variants) {
            let variant_name = naming::unraw(&variant.ident);
            let variant_wire_tokens = &variant_wire.wire;

            // Parse variant-level annotation attributes
            let variant_annotations = match parse_annotation_attributes(&variant.attrs) {
                Ok(annotations) => annotations,
                Err(err) => return err.to_compile_error(),
            };

            // Presence of `#[default]` (the stdlib attribute used by
            // `#[derive(Default)]` on enums) marks this as the default variant.
            let is_default_variant = variant.attrs.iter().any(|a| a.path().is_ident("default"));

            let variant_data = match &variant.fields {
                Fields::Unit => {
                    quote! { None }
                }
                Fields::Unnamed(fields) => {
                    let field_type = FieldType::parse_tuple_variant(fields, PathNames::Last);
                    quote! {
                        Some(VariantData::DataStructureRef(#field_type))
                    }
                }
                Fields::Named(fields) => {
                    // Named fields - create an inline struct
                    let struct_fields: Vec<_> = fields
                        .named
                        .iter()
                        .zip(&variant_wire.fields)
                        .filter_map(|(field, field_wire)| {
                            let field_name = naming::unraw(field.ident.as_ref()?);
                            let field_type = FieldType::parse_syn_ty(&field.ty);
                            Some(quote! {
                                StructField {
                                    field_name: #field_name.to_string(),
                                    field_type: #field_type,
                                    wire: #field_wire,
                                    edge_config: None,
                                    define_config: None,
                                    format: None,
                                    validators: vec![],
                                    always_regenerate: false,
                                    doccom: None,
                                    annotations: vec![],
                                    unique: false,
                                    output_override: None,
                                    raw_attributes: std::collections::BTreeMap::new(),
                                }
                            })
                        })
                        .collect();

                    let pipeline_tokens_inner = pipeline.to_tokens();
                    quote! {
                        Some(VariantData::InlineStruct(StructConfig {
                            struct_name: format!("{}_{}", #enum_name, #variant_name),
                            fields: vec![#(#struct_fields),*],
                            validators: vec![],
                            doccom: None,
                            macroforge_derives: vec![],
                            annotations: vec![],
                            pipeline: #pipeline_tokens_inner,
                            rust_derives: vec![],
                            output_override: None,
                            raw_attributes: std::collections::BTreeMap::new(),
                            resolve_only: false,
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
                    wire: #variant_wire_tokens,
                    doccom: None,
                    annotations: #variant_annotations_tokens,
                    output_override: None,
                    raw_attributes: std::collections::BTreeMap::new(),
                    is_default: #is_default_variant,
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
            #[linkme(crate = ::evenframe::linkme)]
            static #registry_var_name: ::evenframe::registry::EnumRegistryEntry = ::evenframe::registry::EnumRegistryEntry {
                type_name: #enum_name,
                tagged_union_fn: || #ident::variants(),
                pipeline: #pipeline_tokens,
            };
        };

        let validate_impl = match enum_validate(&input, &wire) {
            Ok(tokens) => tokens,
            Err(err) => return err.to_compile_error(),
        };

        quote! {
            #validate_impl

            ::evenframe::__metadata! {
                const _: () = {
                    use ::evenframe::types::{TaggedUnion, Variant, VariantData, StructConfig, StructField, FieldType, EnumRepresentation, Pipeline};
                    use ::evenframe::traits::EvenframeTaggedUnion;

                    impl EvenframeTaggedUnion for #ident {
                        fn variants() -> TaggedUnion {
                            let macroforge_derives_val: Vec<String> = vec![#(#macroforge_derives.to_string()),*];
                            let enum_annotations_val: Vec<String> = vec![#(#enum_annotations.to_string()),*];
                            let rust_derives_val: Vec<String> = vec![#(#rust_derives.to_string()),*];
                            TaggedUnion {
                                enum_name: #enum_name.to_string(),
                                variants: vec![#(#variant_tokens),*],
                                representation: #representation_tokens,
                                doccom: None,
                                macroforge_derives: macroforge_derives_val,
                                annotations: enum_annotations_val,
                                pipeline: #pipeline_tokens,
                                rust_derives: rust_derives_val,
                                output_override: None,
                                resolve_only: false,
                                raw_attributes: std::collections::BTreeMap::new(),
                            }
                        }
                    }

                    #registry_submission
                };
            }
        }
    } else {
        syn::Error::new(
            ident.span(),
            format!("The Evenframe derive macro can only be applied to enums when using generate_enum_impl.\n\nYou tried to apply it to: {}\n\nExample of correct usage:\n#[derive(Evenframe)]\nenum MyEnum {{\n    Variant1,\n    Variant2(String),\n    Variant3 {{ field: i32 }}\n}}", ident),
        )
        .to_compile_error()
    }
}
