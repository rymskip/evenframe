//! The generated `Validate` impl, and the validator chains it shares with the
//! generated `Deserialize`.

use evenframe_core::derive::naming::{ItemWire, unraw};
use evenframe_core::derive::validator_parser::FieldValidators;
use evenframe_core::validator::RuntimeStep;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{DeriveInput, Fields};

/// A named field with its validators, at its path in serde's names.
pub(crate) struct CheckedField<'a> {
    pub field: &'a syn::Field,
    pub path: String,
    pub validators: &'a FieldValidators,
}

impl CheckedField<'_> {
    pub fn ident(&self) -> syn::Result<&syn::Ident> {
        self.field
            .ident
            .as_ref()
            .ok_or_else(|| syn::Error::new_spanned(self.field, "a named field has no identifier"))
    }

    pub fn is_optional(&self) -> bool {
        is_option_type(&self.field.ty)
    }

    /// Whether a validator rewrites the value, which needs it mutable.
    pub fn transforms(&self) -> syn::Result<bool> {
        Ok(self
            .runtime_steps(&quote! { value })?
            .iter()
            .any(|step| matches!(step, RuntimeStep::Transform(_))))
    }

    fn runtime_steps(&self, place: &TokenStream) -> syn::Result<Vec<RuntimeStep>> {
        self.validators
            .steps()
            .iter()
            .map(|validator| {
                validator
                    .runtime_step(place)
                    .map_err(|message| syn::Error::new_spanned(&self.field.ty, message))
            })
            .collect()
    }

    /// The field's steps on `place`, run in order until one fails, which is
    /// recorded under the field's path. Transforms run only with
    /// `with_transforms`; `validate` has an immutable value to check.
    pub fn chain(&self, place: &TokenStream, with_transforms: bool) -> syn::Result<TokenStream> {
        let path = &self.path;
        let steps: Vec<TokenStream> = self
            .runtime_steps(place)?
            .into_iter()
            .filter_map(|step| match step {
                RuntimeStep::Check(expression) => Some(expression),
                RuntimeStep::Transform(expression) if with_transforms => Some(expression),
                RuntimeStep::Transform(_) | RuntimeStep::Nothing => None,
            })
            .map(|expression| {
                quote! {
                    if let ::std::result::Result::Err(__rejection) = #expression {
                        __errors.push(#path, __rejection);
                    }
                }
            })
            .collect();
        Ok(steps
            .into_iter()
            .reduce(|chain, step| quote! { #chain else #step })
            .unwrap_or_default())
    }
}

/// Whether `ty` is an `Option<T>`.
pub(crate) fn is_option_type(ty: &syn::Type) -> bool {
    if let syn::Type::Path(type_path) = ty
        && let Some(segment) = type_path.path.segments.last()
    {
        return segment.ident == "Option";
    }
    false
}

/// Validates a value held at `path` that the derive cannot see into: its own
/// `Validate` impl, when its type has one.
fn nested(value: &TokenStream, path: &str) -> TokenStream {
    quote! {
        if let ::std::result::Result::Err(__nested) =
            (&::evenframe::validator::validate::__private::Probe(#value)).nested()
        {
            __errors.nest(#path, __nested);
        }
    }
}

/// The impl around `body`, which records failures in `__errors`. A type
/// with nothing to check is always valid.
fn validate_impl(input: &DeriveInput, body: TokenStream) -> TokenStream {
    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let checked = if body.is_empty() {
        quote! { ::std::result::Result::Ok(()) }
    } else {
        quote! {
            use ::evenframe::validator::validate::__private::Nested as _;
            let mut __errors = ::evenframe::validator::validate::ValidationErrors::new();
            #body
            __errors.into_result()
        }
    };
    quote! {
        impl #impl_generics ::evenframe::validator::validate::Validate for #ident #ty_generics #where_clause {
            fn validate(
                &self,
            ) -> ::std::result::Result<(), ::evenframe::validator::validate::ValidationErrors> {
                #checked
            }
        }
    }
}

/// `Validate` for a struct: every field's checks, then every nested value.
pub(crate) fn struct_validate(
    input: &DeriveInput,
    fields: &[CheckedField],
) -> syn::Result<TokenStream> {
    let mut body = TokenStream::new();
    for field in fields {
        let ident = field.ident()?;
        let checks = if field.is_optional() {
            let chain = field.chain(&quote! { (*__value) }, false)?;
            if chain.is_empty() {
                chain
            } else {
                quote! {
                    if let ::std::option::Option::Some(__value) = &self.#ident { #chain }
                }
            }
        } else {
            field.chain(&quote! { self.#ident }, false)?
        };
        body.extend(checks);
        body.extend(nested(&quote! { &self.#ident }, &field.path));
    }
    Ok(validate_impl(input, body))
}

/// `Validate` for an enum: the values each variant holds.
pub(crate) fn enum_validate(input: &DeriveInput, wire: &ItemWire) -> syn::Result<TokenStream> {
    let syn::Data::Enum(data) = &input.data else {
        return Err(syn::Error::new_spanned(&input.ident, "expected an enum"));
    };
    let arms = data
        .variants
        .iter()
        .zip(&wire.variants)
        .map(|(variant, variant_wire)| {
            let variant_ident = &variant.ident;
            let variant_name = variant_wire
                .wire
                .serde
                .clone()
                .unwrap_or_else(|| unraw(variant_ident));
            match &variant.fields {
                Fields::Unit => None,
                Fields::Unnamed(unnamed) => {
                    let bindings: Vec<_> = (0..unnamed.unnamed.len())
                        .map(|position| format_ident!("__field{position}"))
                        .collect();
                    let checks = bindings.iter().enumerate().map(|(position, binding)| {
                        nested(&quote! { #binding }, &format!("{variant_name}.{position}"))
                    });
                    Some(quote! { Self::#variant_ident(#(#bindings),*) => { #(#checks)* } })
                }
                Fields::Named(named) => {
                    let members: Vec<_> = named.named.iter().filter_map(|field| field.ident.as_ref()).collect();
                    let bindings: Vec<_> = (0..members.len())
                        .map(|position| format_ident!("__field{position}"))
                        .collect();
                    let checks = bindings.iter().zip(members.iter().zip(&variant_wire.fields)).map(
                        |(binding, (member, field_wire))| {
                            let field_name =
                                field_wire.serde.clone().unwrap_or_else(|| unraw(member));
                            nested(&quote! { #binding }, &format!("{variant_name}.{field_name}"))
                        },
                    );
                    Some(quote! { Self::#variant_ident { #(#members: #bindings),* } => { #(#checks)* } })
                }
            }
        })
        .collect::<Vec<Option<TokenStream>>>();
    // A unit variant holds nothing; with only those there is nothing to match.
    let body = if arms.iter().all(Option::is_none) {
        TokenStream::new()
    } else {
        let rest = arms.iter().any(Option::is_none).then(|| quote! { _ => {} });
        let arms = arms.into_iter().flatten();
        quote! { match self { #(#arms)* #rest } }
    };
    Ok(validate_impl(input, body))
}
