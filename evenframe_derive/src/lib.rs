use proc_macro::TokenStream;
use syn::{Data, DeriveInput, parse_macro_input};
mod deserialization_impl;
mod enum_impl;
mod imports;
mod metadata;
mod newtype_impl;
mod struct_impl;
mod surreal_value_impl;
mod union_impl;
mod validate_impl;

/// Which pipeline(s) the derived type participates in.
/// This is a local mirror used to generate the correct `::evenframe::types::Pipeline` tokens.
#[derive(Clone, Copy)]
pub(crate) enum PipelineKind {
    Both,
    Typesync,
    Schemasync,
}

impl PipelineKind {
    /// Whether the type is stored, and so converts to the database's value.
    pub fn reaches_database(self) -> bool {
        matches!(self, PipelineKind::Both | PipelineKind::Schemasync)
    }

    /// The part of the pipeline that is typesync, if any.
    pub fn typesync_part(self) -> Option<PipelineKind> {
        match self {
            PipelineKind::Both | PipelineKind::Typesync => Some(PipelineKind::Typesync),
            PipelineKind::Schemasync => None,
        }
    }

    /// The part of the pipeline that is schemasync, if any.
    pub fn schemasync_part(self) -> Option<PipelineKind> {
        match self {
            PipelineKind::Both | PipelineKind::Schemasync => Some(PipelineKind::Schemasync),
            PipelineKind::Typesync => None,
        }
    }

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
        evenframe,
        edge,
        define_field_statement,
        format,
        permissions,
        mock_data,
        morphs,
        validators,
        relation,
        event,
        doccom,
        unique,
        index,
        indexes,
        fulltext,
        hnsw,
        diskann,
        surreal,
        schemasync,
        typesync
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
#[proc_macro_derive(EvenframeUnion, attributes(typesync))]
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

/// `SurrealValue` alone, in the shape evenframe stores a type: for a value
/// read from or written to the database that no output describes, such as a
/// query's row projection. A key is its `#[surreal]` name, else its serde
/// name, as serde read it from the database's JSON; defaults and
/// representation come from its `#[serde(...)]` attributes.
#[proc_macro_derive(SurrealValue, attributes(serde, surreal))]
pub fn surreal_value_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let mode = surreal_value_impl::Mode::Row;
    let tokens =
        evenframe_core::derive::naming::resolve(&input).and_then(|wire| match input.data {
            Data::Struct(_) => surreal_value_impl::struct_surreal_value(&input, &wire, mode, &[]),
            Data::Enum(_) => surreal_value_impl::enum_surreal_value(&input, &wire, mode),
            Data::Union(_) => Err(syn::Error::new(
                input.ident.span(),
                "SurrealValue can only be derived for structs and enums",
            )),
        });
    tokens.unwrap_or_else(syn::Error::into_compile_error).into()
}

/// Derive macro for types that only participate in TypeScript type generation.
#[proc_macro_derive(
    Typesync,
    attributes(
        evenframe,
        edge,
        define_field_statement,
        format,
        permissions,
        mock_data,
        morphs,
        validators,
        relation,
        event,
        doccom,
        unique,
        index,
        indexes,
        fulltext,
        hnsw,
        diskann,
        surreal,
        schemasync,
        typesync
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
        evenframe,
        edge,
        define_field_statement,
        format,
        permissions,
        mock_data,
        morphs,
        validators,
        relation,
        event,
        doccom,
        unique,
        index,
        indexes,
        fulltext,
        hnsw,
        diskann,
        surreal,
        schemasync,
        typesync
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
