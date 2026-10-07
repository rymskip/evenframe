use crate::PipelineKind;
use crate::deserialization_impl::{CheckedVariant, generate_enum_deserialize};
use crate::surreal_value_impl::{Mode, enum_surreal_value};
use crate::validate_impl::{CheckedField, enum_validate};
use evenframe_core::{
    derive::{
        attributes::parse_rust_derives,
        naming,
        schemasync_attributes::{parse_validator_overrides, refuse_container_validators},
        typesync_attributes::{Position, TypesyncAttributes},
        validator_parser::{FieldValidators, parse_element_validators, parse_field_validators},
    },
    types::{FieldType, PathNames},
};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields};

pub fn generate_enum_impl(input: DeriveInput, pipeline: PipelineKind) -> TokenStream {
    let ident = input.ident.clone();

    if let Data::Enum(ref data_enum) = input.data {
        let enum_name = ident.to_string();

        if let Err(err) = refuse_container_validators(&input.attrs) {
            return err.to_compile_error();
        }
        let TypesyncAttributes {
            macroforge_derives,
            annotations: enum_annotations,
            ..
        } = match TypesyncAttributes::parse(&input.attrs, Position::Container) {
            Ok(typesync) => typesync,
            Err(err) => return err.to_compile_error(),
        };

        // Parse all Rust derives
        let rust_derives = parse_rust_derives(&input.attrs);

        let wire = match naming::resolve(&input) {
            Ok(wire) => wire,
            Err(err) => return err.to_compile_error(),
        };

        let representation_tokens = &wire.representation;

        let pipeline_tokens = pipeline.to_tokens();

        let mut variant_tokens = Vec::new();

        for (variant, variant_wire) in data_enum.variants.iter().zip(&wire.variants) {
            let variant_name = naming::unraw(&variant.ident);
            let variant_wire_tokens = &variant_wire.wire;

            let variant_position = match variant.fields {
                Fields::Named(_) => Position::StructVariant,
                Fields::Unnamed(_) | Fields::Unit => Position::Variant,
            };
            let TypesyncAttributes {
                macroforge_derives: variant_macroforge_derives,
                annotations: variant_annotations,
                ..
            } = match TypesyncAttributes::parse(&variant.attrs, variant_position) {
                Ok(typesync) => typesync,
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
                    let mut struct_fields = Vec::new();
                    for (field, field_wire) in fields.named.iter().zip(&variant_wire.fields) {
                        let Some(member) = field.ident.as_ref() else {
                            continue;
                        };
                        let field_name = naming::unraw(member);
                        let field_type = FieldType::parse_syn_ty(&field.ty);
                        let (morphs, validators) = match parse_field_validators(&field.attrs) {
                            Ok(validators) => {
                                (validators.morph_tokens(), validators.config_tokens())
                            }
                            Err(err) => return err.to_compile_error(),
                        };
                        let validator_overrides = match parse_validator_overrides(&field.attrs) {
                            Ok(overrides) => overrides,
                            Err(err) => return err.to_compile_error(),
                        };
                        struct_fields.push(quote! {
                            StructField {
                                field_name: #field_name.to_string(),
                                field_type: #field_type,
                                wire: #field_wire,
                                edge_config: None,
                                define_config: None,
                                format: None,
                                morphs: vec![#(#morphs),*],
                                validators: vec![#(#validators),*],
                                validator_overrides: #validator_overrides,
                                always_regenerate: false,
                                doccom: None,
                                annotations: vec![],
                                unique: false,
                                output_override: None,
                                raw_attributes: std::collections::BTreeMap::new(),
                            }
                        });
                    }

                    let pipeline_tokens_inner = pipeline.to_tokens();
                    quote! {
                        Some(VariantData::InlineStruct(StructConfig {
                            struct_name: format!("{}_{}", #enum_name, #variant_name),
                            fields: vec![#(#struct_fields),*],
                            doccom: None,
                            macroforge_derives: vec![#(#variant_macroforge_derives.to_string()),*],
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
            let ElementTokens {
                morphs: element_morphs_tokens,
                validators: element_validators_tokens,
                overrides: element_validator_overrides_tokens,
            } = match element_validator_tokens(&variant.fields) {
                Ok(tokens) => tokens,
                Err(err) => return err.to_compile_error(),
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
                    element_morphs: #element_morphs_tokens,
                    element_validators: #element_validators_tokens,
                    element_validator_overrides: #element_validator_overrides_tokens,
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

        // Each variant's fields' validators, in field order, for the validated
        // deserializer.
        let variant_validators = match data_enum
            .variants
            .iter()
            .map(|variant| {
                variant
                    .fields
                    .iter()
                    .map(|field| parse_field_validators(&field.attrs))
                    .collect::<syn::Result<Vec<FieldValidators>>>()
            })
            .collect::<syn::Result<Vec<_>>>()
        {
            Ok(validators) => validators,
            Err(err) => return err.to_compile_error(),
        };
        let deserialize_impl = if variant_validators
            .iter()
            .flatten()
            .any(|validators| !validators.is_empty())
        {
            let checked_variants: Vec<CheckedVariant> = data_enum
                .variants
                .iter()
                .zip(&wire.variants)
                .zip(&variant_validators)
                .map(|((variant, variant_wire), validators)| {
                    let variant_name = variant_wire
                        .wire
                        .serde
                        .clone()
                        .unwrap_or_else(|| naming::unraw(&variant.ident));
                    let fields = match &variant.fields {
                        Fields::Named(named) => named
                            .named
                            .iter()
                            .zip(&variant_wire.fields)
                            .zip(validators)
                            .enumerate()
                            .map(|(position, ((field, field_wire), validators))| {
                                let field_name = field_wire.serde.clone().unwrap_or_else(|| {
                                    field.ident.as_ref().map(naming::unraw).unwrap_or_default()
                                });
                                CheckedField::new(
                                    field,
                                    position,
                                    format!("{variant_name}.{field_name}"),
                                    validators,
                                )
                            })
                            .collect(),
                        Fields::Unnamed(unnamed) => unnamed
                            .unnamed
                            .iter()
                            .zip(validators)
                            .enumerate()
                            .map(|(position, (field, validators))| {
                                CheckedField::new(
                                    field,
                                    position,
                                    format!("{variant_name}.{position}"),
                                    validators,
                                )
                            })
                            .collect(),
                        Fields::Unit => Vec::new(),
                    };
                    CheckedVariant { variant, fields }
                })
                .collect();
            match generate_enum_deserialize(&input, &checked_variants) {
                Ok(tokens) => tokens,
                Err(err) => return err.to_compile_error(),
            }
        } else {
            TokenStream::new()
        };

        let surreal_value_impl = if pipeline.reaches_database() {
            match enum_surreal_value(&input, &wire, Mode::Stored) {
                Ok(tokens) => tokens,
                Err(err) => return err.to_compile_error(),
            }
        } else {
            TokenStream::new()
        };

        let metadata_gate = crate::metadata::gate(
            &input,
            quote! {
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
            },
        );
        quote! {
            #validate_impl

            #deserialize_impl

            #surreal_value_impl

            #metadata_gate
        }
    } else {
        syn::Error::new(
            ident.span(),
            format!("The Evenframe derive macro can only be applied to enums when using generate_enum_impl.\n\nYou tried to apply it to: {}\n\nExample of correct usage:\n#[derive(Evenframe)]\nenum MyEnum {{\n    Variant1,\n    Variant2(String),\n    Variant3 {{ field: i32 }}\n}}", ident),
        )
        .to_compile_error()
    }
}

/// A tuple payload's validators and their overrides, one entry per element,
/// or none when no element has any, as the scanner records them.
/// A tuple payload's per-element morphs, validators and overrides, as
/// tokens for its static config.
pub(crate) struct ElementTokens {
    pub morphs: TokenStream,
    pub validators: TokenStream,
    pub overrides: TokenStream,
}

impl ElementTokens {
    pub fn none() -> Self {
        Self {
            morphs: quote! { ::std::vec::Vec::new() },
            validators: quote! { ::std::vec::Vec::new() },
            overrides: quote! { ::std::vec::Vec::new() },
        }
    }
}

pub(crate) fn element_validator_tokens(fields: &Fields) -> syn::Result<ElementTokens> {
    let Fields::Unnamed(unnamed) = fields else {
        return Ok(ElementTokens::none());
    };
    let elements = parse_element_validators(&unnamed.unnamed)?;
    let morphs = elements
        .morphs
        .iter()
        .map(|element| quote! { vec![#(#element),*] });
    let validators = elements
        .validators
        .iter()
        .map(|element| quote! { vec![#(#element),*] });
    let overrides = &elements.overrides;
    Ok(ElementTokens {
        morphs: quote! { vec![#(#morphs),*] },
        validators: quote! { vec![#(#validators),*] },
        overrides: quote! { vec![#(#overrides),*] },
    })
}
