//! The config functions and registry entries only a `metadata` build compiles.

use proc_macro2::TokenStream;
use quote::quote;
use syn::DeriveInput;

/// `body` behind the `metadata` feature. A registry entry names one concrete
/// type, so a metadata build refuses a type with generic parameters there,
/// while a build without metadata, which registers nothing, still compiles it.
pub(crate) fn gate(input: &DeriveInput, body: TokenStream) -> TokenStream {
    let gated = match input.generics.params.first() {
        Some(parameter) => syn::Error::new_spanned(
            parameter,
            format!(
                "a registry entry names one concrete type, so `{}` cannot have generic \
                 parameters in a build with evenframe's `metadata` feature",
                input.ident
            ),
        )
        .to_compile_error(),
        None => body,
    };
    quote! { ::evenframe::__metadata! { #gated } }
}
