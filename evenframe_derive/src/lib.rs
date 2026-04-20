use proc_macro::TokenStream;
use syn::{Data, DeriveInput, parse_macro_input};
mod deserialization_impl;
mod enum_impl;
mod imports;
mod struct_impl;
mod union_impl;

/// Which pipeline(s) the derived type participates in.
/// This is a local mirror used to generate the correct `::evenframe::types::Pipeline` tokens.
#[derive(Clone, Copy)]
pub(crate) enum PipelineKind {
    Both,
    Typesync,
    Schemasync,
}

impl PipelineKind {
    pub fn to_tokens(self) -> proc_macro2::TokenStream {
        use quote::quote;
        match self {
            PipelineKind::Both => quote! { ::evenframe::types::Pipeline::Both },
            PipelineKind::Typesync => quote! { ::evenframe::types::Pipeline::Typesync },
            PipelineKind::Schemasync => quote! { ::evenframe::types::Pipeline::Schemasync },
        }
    }
}

/// For structs it generates both:
/// - A `table_schema()` function returning a `helpers::TableSchema`
#[proc_macro_derive(
    Evenframe,
    attributes(
        edge,
        define_field_statement,
        format,
        permissions,
        mock_data,
        validators,
        relation,
        event,
        doccom,
        macroforge_derive,
        annotation,
        unique,
        index
    )
)]
pub fn evenframe_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match input.data {
        Data::Struct(_) => struct_impl::generate_struct_impl(input, PipelineKind::Both).into(),
        Data::Enum(_) => enum_impl::generate_enum_impl(input, PipelineKind::Both).into(),
        _ => syn::Error::new(
            input.ident.span(),
            "Evenframe can only be used on structs and enums",
        )
        .to_compile_error()
        .into(),
    }
}

/// Derive macro for unions of persistable structs
/// Each variant must contain exactly one persistable struct type
#[proc_macro_derive(EvenframeUnion, attributes(macroforge_derive, annotation))]
pub fn evenframe_union_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match input.data {
        Data::Enum(_) => union_impl::generate_union_impl(input).into(),
        _ => syn::Error::new(
            input.ident.span(),
            "EvenframeUnion can only be used on enums",
        )
        .to_compile_error()
        .into(),
    }
}

/// Derive macro for types that only participate in TypeScript type generation.
#[proc_macro_derive(
    Typesync,
    attributes(
        edge,
        define_field_statement,
        format,
        permissions,
        mock_data,
        validators,
        relation,
        event,
        doccom,
        macroforge_derive,
        annotation,
        unique,
        index
    )
)]
pub fn typesync_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match input.data {
        Data::Struct(_) => struct_impl::generate_struct_impl(input, PipelineKind::Typesync).into(),
        Data::Enum(_) => enum_impl::generate_enum_impl(input, PipelineKind::Typesync).into(),
        _ => syn::Error::new(
            input.ident.span(),
            "Typesync can only be used on structs and enums",
        )
        .to_compile_error()
        .into(),
    }
}

/// Derive macro for types that only participate in database schema synchronization.
#[proc_macro_derive(
    Schemasync,
    attributes(
        edge,
        define_field_statement,
        format,
        permissions,
        mock_data,
        validators,
        relation,
        event,
        doccom,
        macroforge_derive,
        annotation,
        unique,
        index
    )
)]
pub fn schemasync_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match input.data {
        Data::Struct(_) => {
            struct_impl::generate_struct_impl(input, PipelineKind::Schemasync).into()
        }
        Data::Enum(_) => enum_impl::generate_enum_impl(input, PipelineKind::Schemasync).into(),
        _ => syn::Error::new(
            input.ident.span(),
            "Schemasync can only be used on structs and enums",
        )
        .to_compile_error()
        .into(),
    }
}
