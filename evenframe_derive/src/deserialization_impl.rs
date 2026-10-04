//! The generated `Deserialize` for a struct with field validators. serde
//! reads the input into a private shadow of the struct carrying the same
//! `#[serde(...)]` attributes, so names, aliases, defaults and unknown keys
//! behave exactly as serde's own derive. Every field's parse morph and
//! validators then run, and every failing field is reported together.

use crate::validate_impl::CheckedField;
use evenframe_core::derive::naming::unraw;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::punctuated::Punctuated;
use syn::{Attribute, DeriveInput, Meta, Token};

/// The `#[serde(...)]` entries of `attrs`, each attribute's entries in order.
fn serde_entries(attrs: &[Attribute]) -> syn::Result<Vec<Meta>> {
    let mut entries = Vec::new();
    for attr in attrs.iter().filter(|attr| attr.path().is_ident("serde")) {
        entries.extend(attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?);
    }
    Ok(entries)
}

fn serde_attribute(entries: &[Meta]) -> TokenStream {
    if entries.is_empty() {
        TokenStream::new()
    } else {
        quote! { #[serde(#(#entries),*)] }
    }
}

/// How serde fills a struct missing from the input under a container
/// `#[serde(default)]`.
enum ContainerDefault {
    None,
    Trait,
    Function(syn::ExprPath),
}

/// The container entries the shadow keeps, and the container default it
/// rebuilds for the shadow's own type.
fn container_entries(input: &DeriveInput) -> syn::Result<(Vec<Meta>, ContainerDefault)> {
    let mut kept = Vec::new();
    let mut default = ContainerDefault::None;
    for entry in serde_entries(&input.attrs)? {
        let key = entry
            .path()
            .get_ident()
            .map(syn::Ident::to_string)
            .unwrap_or_default();
        match (key.as_str(), &entry) {
            ("from" | "try_from" | "remote", _) => {
                return Err(syn::Error::new_spanned(
                    &entry,
                    format!(
                        "#[serde({key} = \"...\")] is not supported with field validators: \
                         serde would build the struct without reading its fields"
                    ),
                ));
            }
            ("default", Meta::Path(_)) => default = ContainerDefault::Trait,
            ("default", Meta::NameValue(name_value)) => {
                let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(function),
                    ..
                }) = &name_value.value
                else {
                    return Err(syn::Error::new_spanned(
                        &name_value.value,
                        "#[serde(default = \"...\")] takes a function path as a string",
                    ));
                };
                default = ContainerDefault::Function(function.parse()?);
            }
            _ => kept.push(entry),
        }
    }
    Ok((kept, default))
}

pub(crate) fn generate_custom_deserialize(
    input: &DeriveInput,
    fields: &[CheckedField],
) -> syn::Result<TokenStream> {
    if let Some(lifetime) = input.generics.lifetimes().next() {
        return Err(syn::Error::new_spanned(
            lifetime,
            "a struct with field validators cannot borrow from its input: its fields are \
             rewritten after reading, so they must own their data",
        ));
    }
    let ident = &input.ident;
    let (container, default) = container_entries(input)?;
    let has_parse = fields.iter().any(|field| field.validators.parse.is_some());
    if has_parse && !matches!(default, ContainerDefault::None) {
        return Err(syn::Error::new_spanned(
            ident,
            "#[serde(default)] on a struct cannot fill a field read through a parse morph, \
             which reads text rather than the field's type",
        ));
    }

    let mut wire_fields = Vec::new();
    let mut reads = Vec::new();
    let mut pipelines = Vec::new();
    let mut parsed = Vec::new();
    let mut assignments = Vec::new();
    let mut checks_anything = false;
    for field in fields {
        let member = field.ident()?;
        let local = format_ident!("__field_{}", unraw(member));
        let entries = serde_entries(&field.field.attrs)?;
        let path = &field.path;
        let optional = field.is_optional();
        let transforms = field.transforms()?;
        let mutability = transforms.then(|| quote! { mut });
        let borrow = if transforms {
            quote! { &mut }
        } else {
            quote! { & }
        };
        let field_type = &field.field.ty;

        let wire_type = match field.validators.parse {
            Some(parse) => {
                if let Some(entry) = entries.iter().find(|entry| {
                    ["default", "deserialize_with", "with"]
                        .iter()
                        .any(|key| entry.path().is_ident(key))
                }) {
                    return Err(syn::Error::new_spanned(
                        entry,
                        "this field is read through a parse morph, which reads the input as \
                         text, so serde cannot also default or deserialize it another way",
                    ));
                }
                checks_anything = true;
                let function = format_ident!("{}", parse.runtime_function());
                let parse_call = if optional {
                    quote! {
                        __wire.#member
                            .as_deref()
                            .map(::evenframe::validator::runtime::#function)
                            .transpose()
                    }
                } else {
                    quote! { ::evenframe::validator::runtime::#function(&__wire.#member) }
                };
                reads.push(quote! {
                    let #mutability #local = match #parse_call {
                        ::std::result::Result::Ok(__parsed) => ::std::option::Option::Some(__parsed),
                        ::std::result::Result::Err(__rejection) => {
                            __errors.push(#path, __rejection);
                            ::std::option::Option::None
                        }
                    };
                });
                let parsed_local = format_ident!("__parsed_{}", unraw(member));
                parsed.push((local.clone(), parsed_local.clone()));
                assignments.push(quote! { #member: #parsed_local });
                if optional {
                    quote! { ::std::option::Option<::std::string::String> }
                } else {
                    quote! { ::std::string::String }
                }
            }
            None => {
                reads.push(quote! { let #mutability #local = __wire.#member; });
                assignments.push(quote! { #member: #local });
                quote! { #field_type }
            }
        };

        let chain = field.chain(&quote! { (*__value) }, true)?;
        let direct_chain = field.chain(&quote! { #local }, true)?;
        if !chain.is_empty() {
            checks_anything = true;
            let present = match (field.validators.parse.is_some(), optional) {
                (true, true) => Some(quote! {
                    ::std::option::Option::Some(::std::option::Option::Some(__value))
                }),
                (true, false) | (false, true) => {
                    Some(quote! { ::std::option::Option::Some(__value) })
                }
                (false, false) => None,
            };
            pipelines.push(match present {
                Some(pattern) => quote! { if let #pattern = #borrow #local { #chain } },
                None => direct_chain,
            });
        }

        let field_attribute = serde_attribute(&entries);
        wire_fields.push(quote! { #field_attribute #member: #wire_type });
    }

    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let wire_default = match default {
        ContainerDefault::None => None,
        ContainerDefault::Trait => {
            Some(quote! { <#ident #ty_generics as ::std::default::Default>::default() })
        }
        ContainerDefault::Function(function) => Some(quote! { #function() }),
    };
    let (default_attribute, default_function) = match wire_default {
        None => (TokenStream::new(), TokenStream::new()),
        Some(value) => {
            let members: Vec<_> = fields
                .iter()
                .map(CheckedField::ident)
                .collect::<syn::Result<_>>()?;
            (
                quote! { #[serde(default = "__evenframe_wire_default")] },
                quote! {
                    fn __evenframe_wire_default #impl_generics () -> __EvenframeWire #ty_generics #where_clause {
                        let __value: #ident #ty_generics = #value;
                        __EvenframeWire { #(#members: __value.#members),* }
                    }
                },
            )
        }
    };

    let container_attribute = serde_attribute(&container);
    let errors = checks_anything.then(|| {
        quote! { let mut __errors = ::evenframe::validator::validate::ValidationErrors::new(); }
    });
    let finish = if !checks_anything {
        quote! { ::std::result::Result::Ok(Self { #(#assignments),* }) }
    } else if parsed.is_empty() {
        quote! {
            if __errors.is_empty() {
                ::std::result::Result::Ok(Self { #(#assignments),* })
            } else {
                ::std::result::Result::Err(::serde::de::Error::custom(__errors))
            }
        }
    } else {
        let locals = parsed.iter().map(|(local, _)| local);
        let patterns = parsed
            .iter()
            .map(|(_, parsed_local)| quote! { ::std::option::Option::Some(#parsed_local) });
        quote! {
            match (#(#locals,)*) {
                (#(#patterns,)*) if __errors.is_empty() => {
                    ::std::result::Result::Ok(Self { #(#assignments),* })
                }
                _ => ::std::result::Result::Err(::serde::de::Error::custom(__errors)),
            }
        }
    };

    let mut deserialize_generics = input.generics.clone();
    deserialize_generics
        .params
        .insert(0, syn::parse_quote! { '__de });
    deserialize_generics
        .make_where_clause()
        .predicates
        .push(syn::parse_quote! { __EvenframeWire #ty_generics: ::serde::Deserialize<'__de> });
    let (deserialize_impl_generics, _, deserialize_where_clause) =
        deserialize_generics.split_for_impl();
    let generics = &input.generics;

    Ok(quote! {
        const _: () = {
            #[derive(::serde::Deserialize)]
            #container_attribute
            #default_attribute
            struct __EvenframeWire #generics #where_clause {
                #(#wire_fields),*
            }

            #default_function

            impl #deserialize_impl_generics ::serde::Deserialize<'__de> for #ident #ty_generics
            #deserialize_where_clause
            {
                fn deserialize<__D>(__deserializer: __D) -> ::std::result::Result<Self, __D::Error>
                where
                    __D: ::serde::Deserializer<'__de>,
                {
                    let __wire = <__EvenframeWire #ty_generics as ::serde::Deserialize<'__de>>::deserialize(
                        __deserializer,
                    )?;
                    #errors
                    #(#reads)*
                    #(#pipelines)*
                    #finish
                }
            }
        };
    })
}
