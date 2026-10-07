//! The derive for a struct serde writes as another type: a single-field tuple
//! struct or a `#[serde(transparent)]` struct, written as its one field's
//! value, and a multi-field tuple or unit struct, written as an array or null.

use crate::{
    PipelineKind,
    deserialization_impl::{
        generate_custom_deserialize, generate_newtype_constructors, generate_newtype_deserialize,
    },
    enum_impl::{ElementTokens, element_validator_tokens},
    surreal_value_impl::{Mode, struct_surreal_value},
    validate_impl::{CheckedField, struct_validate},
};
use evenframe_core::{
    derive::{
        attributes::parse_rust_derives,
        naming::{ItemShape, ItemWire, UnitValue},
        schemasync_attributes::{
            parse_container_validator_overrides, parse_validator_overrides,
            refuse_container_validators,
        },
        typesync_attributes::{Position, TypesyncAttributes},
        validator_parser::{FieldValidators, parse_field_validators},
    },
    types::{FieldType, PathNames},
    validator::ValidatorOverrides,
};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Attribute, DeriveInput};

pub fn generate_newtype_impl(
    input: &DeriveInput,
    pipeline: PipelineKind,
    wire: &ItemWire,
) -> TokenStream {
    newtype_impl(input, pipeline, wire).unwrap_or_else(syn::Error::into_compile_error)
}

fn newtype_impl(
    input: &DeriveInput,
    pipeline: PipelineKind,
    wire: &ItemWire,
) -> syn::Result<TokenStream> {
    let syn::Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(&input.ident, "expected a struct"));
    };
    let ident = &input.ident;
    let name = ident.to_string();
    let fields: Vec<&syn::Field> = data.fields.iter().collect();

    // The field serde writes, and its position, for a struct written as one value.
    let value = match &wire.shape {
        ItemShape::Newtype { member } => Some(
            fields
                .iter()
                .enumerate()
                .find(|(position, field)| match (member, &field.ident) {
                    (syn::Member::Named(name), Some(ident)) => name == ident,
                    (syn::Member::Unnamed(index), None) => index.index as usize == *position,
                    _ => false,
                })
                .map(|(position, field)| (position, *field))
                .ok_or_else(|| syn::Error::new_spanned(ident, "the newtype's field is missing"))?,
        ),
        ItemShape::Tuple(_) | ItemShape::Unit => None,
        ItemShape::Named => {
            return Err(syn::Error::new_spanned(
                ident,
                "a struct of named fields is not a newtype",
            ));
        }
    };

    let (inner, kind, validators, validator_overrides) = match value {
        Some((_, field)) => {
            // The container's validators check the value, and so do the
            // field's own, after them.
            let attributes: Vec<Attribute> =
                input.attrs.iter().chain(&field.attrs).cloned().collect();
            (
                FieldType::parse(&field.ty, PathNames::Last),
                quote! { ::evenframe::types::NewtypeKind::Branded },
                parse_field_validators(&attributes)?,
                parse_container_validator_overrides(&input.attrs)?
                    .followed_by(parse_validator_overrides(&field.attrs)?),
            )
        }
        None => {
            if let Some(attribute) = input.attrs.iter().find(|attribute| {
                attribute.path().is_ident("validators") || attribute.path().is_ident("morphs")
            }) {
                return Err(syn::Error::new_spanned(
                    attribute,
                    "serde writes a tuple struct of several fields as an array and a unit struct \
                     as null, so there is no one value for `#[validators]` or `#[morphs]`: put \
                     each on the value it applies to",
                ));
            }
            let inner = match &wire.shape {
                ItemShape::Unit => FieldType::Unit,
                _ => FieldType::Tuple(
                    fields
                        .iter()
                        .map(|field| FieldType::parse(&field.ty, PathNames::Last))
                        .collect(),
                ),
            };
            refuse_container_validators(&input.attrs)?;
            (
                inner,
                quote! { ::evenframe::types::NewtypeKind::Alias },
                FieldValidators::default(),
                ValidatorOverrides::default(),
            )
        }
    };

    // A branded newtype checks the value it is written as. A tuple struct
    // checks each element with its own validators, then its own `Validate`.
    let element_validators: Vec<FieldValidators> = match value {
        Some(_) => Vec::new(),
        None => fields
            .iter()
            .map(|field| parse_field_validators(&field.attrs))
            .collect::<syn::Result<_>>()?,
    };
    let checked: Vec<CheckedField> = match value {
        Some((position, field)) => {
            vec![CheckedField::new(
                field,
                position,
                String::new(),
                &validators,
            )]
        }
        None => fields
            .iter()
            .zip(&element_validators)
            .enumerate()
            .map(|(position, (field, validators))| {
                CheckedField::new(field, position, position.to_string(), validators)
            })
            .collect(),
    };
    let validate_impl = struct_validate(input, &checked)?;

    // The fields serde skips beside a transparent struct's value, built from
    // their defaults as serde builds them.
    let others: Vec<syn::Member> = match value {
        Some((position, _)) => fields
            .iter()
            .enumerate()
            .filter(|(other, _)| *other != position)
            .map(|(other, field)| member_at(field, other))
            .collect(),
        None => Vec::new(),
    };
    let others: Vec<&syn::Member> = others.iter().collect();
    let deserialize_impl = match checked.first() {
        Some(written) if value.is_some() && !validators.is_empty() => {
            let deserialize = generate_newtype_deserialize(input, written, &others)?;
            let constructors = generate_newtype_constructors(input, written, &others)?;
            quote! { #deserialize #constructors }
        }
        _ if element_validators
            .iter()
            .any(|validators| !validators.is_empty()) =>
        {
            generate_custom_deserialize(input, &checked)?
        }
        _ => TokenStream::new(),
    };
    let newtype_impl = match (value, checked.first()) {
        (Some((_, field)), Some(written)) => {
            newtype_trait_impls(input, field, &written.member, &others)
        }
        _ => TokenStream::new(),
    };

    let surreal_value_impl = if pipeline.reaches_database() {
        struct_surreal_value(input, wire, Mode::Stored, &[])?
    } else {
        TokenStream::new()
    };

    let morph_tokens = validators.morph_tokens();
    let validator_tokens = validators.config_tokens();
    let storage = evenframe_core::types::Storage {
        tuple: wire.stored_tuple,
        opaque_elements: wire.opaque_elements.clone(),
        value: wire.unit_value.as_ref().map(UnitValue::surql),
        ..evenframe_core::types::Storage::default()
    };
    let ElementTokens {
        morphs: element_morphs,
        validators: element_validators_tokens,
        overrides: element_validator_overrides,
    } = match value {
        Some(_) => ElementTokens::none(),
        None => element_validator_tokens(&data.fields)?,
    };
    let TypesyncAttributes {
        macroforge_derives,
        annotations,
        ..
    } = TypesyncAttributes::parse(&input.attrs, Position::Container)?;
    let rust_derives = parse_rust_derives(&input.attrs);
    let pipeline_tokens = pipeline.to_tokens();
    let registry_entry = syn::Ident::new(
        &format!("{}_NEWTYPE_REGISTRY_ENTRY", name.to_uppercase()),
        ident.span(),
    );

    let metadata_gate = crate::metadata::gate(
        input,
        quote! {
            const _: () = {
                use ::evenframe::types::FieldType;

                impl #ident {
                    pub fn static_newtype_config() -> ::evenframe::types::NewtypeConfig {
                        ::evenframe::types::NewtypeConfig {
                            name: #name.to_owned(),
                            inner: #inner,
                            kind: #kind,
                            morphs: vec![#(#morph_tokens),*],
                            validators: vec![#(#validator_tokens),*],
                            validator_overrides: #validator_overrides,
                            element_morphs: #element_morphs,
                            element_validators: #element_validators_tokens,
                            element_validator_overrides: #element_validator_overrides,
                            storage: #storage,
                            doccom: None,
                            annotations: vec![#(#annotations.to_string()),*],
                            macroforge_derives: vec![#(#macroforge_derives.to_string()),*],
                            rust_derives: vec![#(#rust_derives.to_string()),*],
                            pipeline: #pipeline_tokens,
                            resolve_only: false,
                            raw_attributes: ::std::collections::BTreeMap::new(),
                            output_override: None,
                        }
                    }
                }

                #[::evenframe::linkme::distributed_slice(::evenframe::registry::NEWTYPE_REGISTRY_ENTRIES)]
                #[linkme(crate = ::evenframe::linkme)]
                static #registry_entry: ::evenframe::registry::NewtypeRegistryEntry =
                    ::evenframe::registry::NewtypeRegistryEntry {
                        type_name: #name,
                        newtype_config_fn: || #ident::static_newtype_config(),
                        pipeline: #pipeline_tokens,
                    };
            };
        },
    );
    Ok(quote! {
        #metadata_gate

        #validate_impl

        #newtype_impl

        #deserialize_impl

        #surreal_value_impl
    })
}

/// The metadata of a struct `#[serde(into = "...")]` writes as `written`: a
/// newtype aliasing that type, which the typesync outputs describe in place of
/// the struct's fields.
pub(crate) fn written_as_metadata(
    input: &DeriveInput,
    written: &syn::Type,
    pipeline: PipelineKind,
) -> TokenStream {
    let ident = &input.ident;
    let name = ident.to_string();
    let inner = FieldType::parse(written, PathNames::Last);
    let pipeline_tokens = pipeline.to_tokens();
    let registry_entry = syn::Ident::new(
        &format!("{}_WRITTEN_AS_REGISTRY_ENTRY", name.to_uppercase()),
        ident.span(),
    );
    crate::metadata::gate(
        input,
        quote! {
            const _: () = {
                use ::evenframe::types::FieldType;

                #[::evenframe::linkme::distributed_slice(::evenframe::registry::NEWTYPE_REGISTRY_ENTRIES)]
                #[linkme(crate = ::evenframe::linkme)]
                static #registry_entry: ::evenframe::registry::NewtypeRegistryEntry =
                    ::evenframe::registry::NewtypeRegistryEntry {
                        type_name: #name,
                        newtype_config_fn: || ::evenframe::types::NewtypeConfig {
                            name: #name.to_owned(),
                            inner: #inner,
                            kind: ::evenframe::types::NewtypeKind::Alias,
                            pipeline: #pipeline_tokens,
                            ..::std::default::Default::default()
                        },
                        pipeline: #pipeline_tokens,
                    };
            };
        },
    )
}

/// The runtime's view of a branded newtype as the value it holds, which lets
/// a validator on a field holding the newtype check that value.
fn newtype_trait_impls(
    input: &DeriveInput,
    field: &syn::Field,
    member: &syn::Member,
    others: &[&syn::Member],
) -> TokenStream {
    let ident = &input.ident;
    let inner = &field.ty;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    quote! {
        impl #impl_generics ::evenframe::validator::runtime::Newtype for #ident #ty_generics #where_clause {
            type Inner = #inner;
            fn inner(&self) -> &#inner {
                &self.#member
            }
        }

        impl #impl_generics ::evenframe::validator::runtime::NewtypeParts for #ident #ty_generics #where_clause {
            fn inner_mut(&mut self) -> &mut #inner {
                &mut self.#member
            }
            fn from_inner(inner: #inner) -> Self {
                Self {
                    #member: inner,
                    #(#others: ::std::default::Default::default(),)*
                }
            }
        }
    }
}

/// How the field at `position` is reached.
fn member_at(field: &syn::Field, position: usize) -> syn::Member {
    match &field.ident {
        Some(ident) => syn::Member::Named(ident.clone()),
        None => syn::Member::Unnamed(syn::Index::from(position)),
    }
}
