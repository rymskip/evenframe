//! The generated `Deserialize` for a struct or enum with field validators.
//! serde reads the input into a private shadow of the type carrying the same
//! `#[serde(...)]` attributes, so names, tags, aliases, defaults and unknown
//! keys behave exactly as serde's own derive. Every field's parse morph and
//! validators then run, and every failing field is reported together.

use crate::validate_impl::CheckedField;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::punctuated::Punctuated;
use syn::{Attribute, DeriveInput, Fields, Meta, Token};

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

/// Where a struct's validated read takes its fields from.
enum Source {
    /// Its own fields, read by a shadow of it.
    Fields,
    /// A value of another type converted into it, by `From` or, when
    /// `fallible`, `TryFrom`, as serde's `from` and `try_from` read it.
    Converted { source: syn::Type, fallible: bool },
    /// serde's `remote`: the fields of the type `remote`, which the read builds
    /// and returns, through `From<Self>` when a field has a `getter`.
    Remote { remote: syn::Path, getters: bool },
}

/// A string literal's type or path, as serde's container keys write them.
fn quoted<T: syn::parse::Parse>(value: &syn::Expr) -> syn::Result<T> {
    match value {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(literal),
            ..
        }) => literal.parse(),
        other => Err(syn::Error::new_spanned(other, "expected a string")),
    }
}

/// The container entries the shadow keeps, the container default it rebuilds
/// for the shadow's own type, and where the read takes its fields from.
fn container_entries(input: &DeriveInput) -> syn::Result<(Vec<Meta>, ContainerDefault, Source)> {
    let mut kept = Vec::new();
    let mut default = ContainerDefault::None;
    let mut source = Source::Fields;
    for entry in serde_entries(&input.attrs)? {
        let key = entry
            .path()
            .get_ident()
            .map(syn::Ident::to_string)
            .unwrap_or_default();
        match (key.as_str(), &entry) {
            ("from" | "try_from", Meta::NameValue(name_value)) => {
                source = Source::Converted {
                    source: quoted(&name_value.value)?,
                    fallible: key == "try_from",
                };
            }
            ("remote", Meta::NameValue(name_value)) => {
                let getters = match &input.data {
                    syn::Data::Struct(data) => {
                        data.fields.iter().try_fold(false, |found, field| {
                            Ok::<_, syn::Error>(
                                found
                                    || serde_entries(&field.attrs)?
                                        .iter()
                                        .any(|entry| entry.path().is_ident("getter")),
                            )
                        })?
                    }
                    syn::Data::Enum(_) | syn::Data::Union(_) => false,
                };
                source = Source::Remote {
                    remote: quoted(&name_value.value)?,
                    getters,
                };
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
    Ok((kept, default, source))
}

/// One record's fields as the shadow reads them: the shadow's members, the
/// reads taking each value off the shadow, the morph and validator pipelines
/// and the assignments that build the record.
struct FieldPlan {
    wire_fields: Vec<TokenStream>,
    reads: Vec<TokenStream>,
    pipelines: Vec<TokenStream>,
    assignments: Vec<TokenStream>,
    checks_anything: bool,
}

/// Plans `fields`, whose values the shadow holds at `source(field)`.
fn plan_fields(
    fields: &[CheckedField],
    source: impl Fn(&CheckedField) -> TokenStream,
) -> syn::Result<FieldPlan> {
    let mut plan = FieldPlan {
        wire_fields: Vec::new(),
        reads: Vec::new(),
        pipelines: Vec::new(),
        assignments: Vec::new(),
        checks_anything: false,
    };
    for field in fields {
        let member = &field.member;
        let value = source(field);
        let local = format_ident!("__field_{}", field.binding_name());
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

        plan.reads.push(quote! { let #mutability #local = #value; });
        plan.assignments.push(quote! { #member: #local });

        let chain = field.chain(&quote! { (*__value) }, true)?;
        let direct_chain = field.chain(&quote! { #local }, true)?;
        if !chain.is_empty() {
            plan.checks_anything = true;
            plan.pipelines.push(if optional {
                quote! { if let ::std::option::Option::Some(__value) = #borrow #local { #chain } }
            } else {
                direct_chain
            });
        }
        if transforms {
            plan.pipelines
                .push(recheck(&quote! { &#local }, &quote! { #field_type }, path));
        }

        let field_attribute = serde_attribute(&entries);
        plan.wire_fields.push(match member {
            syn::Member::Named(_) => quote! { #field_attribute #member: #field_type },
            syn::Member::Unnamed(_) => quote! { #field_attribute #field_type },
        });
    }
    Ok(plan)
}

/// The value's own `Validate` again, after a morph rewrote it without going
/// through its own deserializer. The value's type is named for the probe to
/// pick its impl by.
fn recheck(value: &TokenStream, value_type: &TokenStream, path: &str) -> TokenStream {
    quote! {
        {
            use ::evenframe::validator::validate::__private::Nested as _;
            if let ::std::result::Result::Err(__nested) =
                (&::evenframe::validator::validate::__private::Probe::<#value_type>(#value)).nested()
            {
                __errors.nest(#path, __nested);
            }
        }
    }
}

/// Runs `plan`'s reads and pipelines, then builds the record at `path`, or
/// fails with every rejection the pipelines recorded.
fn build(plan: &FieldPlan, path: &TokenStream) -> TokenStream {
    let FieldPlan {
        reads,
        pipelines,
        assignments,
        checks_anything,
        ..
    } = plan;
    let finish = if *checks_anything {
        quote! {
            if __errors.is_empty() {
                ::std::result::Result::Ok(#path { #(#assignments),* })
            } else {
                ::std::result::Result::Err(::serde::de::Error::custom(__errors))
            }
        }
    } else {
        quote! { ::std::result::Result::Ok(#path { #(#assignments),* }) }
    };
    let errors = checks_anything.then(|| {
        quote! { let mut __errors = ::evenframe::validator::validate::ValidationErrors::new(); }
    });
    quote! {
        #errors
        #(#reads)*
        #(#pipelines)*
        #finish
    }
}

/// A validated type borrows nothing from its input: its fields are rewritten
/// after reading, so they must own their data.
fn refuse_borrowing(input: &DeriveInput) -> syn::Result<()> {
    match input.generics.lifetimes().next() {
        Some(lifetime) => Err(syn::Error::new_spanned(
            lifetime,
            "a type with field validators cannot borrow from its input: its fields are \
             rewritten after reading, so they must own their data",
        )),
        None => Ok(()),
    }
}

/// The `impl Deserialize` reading `__EvenframeWire` and then running `body`
/// on `__wire`.
fn deserialize_impl(input: &DeriveInput, body: TokenStream) -> TokenStream {
    let ident = &input.ident;
    let (_, ty_generics, _) = input.generics.split_for_impl();
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
    quote! {
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
                #body
            }
        }
    }
}

pub(crate) fn generate_custom_deserialize(
    input: &DeriveInput,
    fields: &[CheckedField],
) -> syn::Result<TokenStream> {
    refuse_borrowing(input)?;
    let ident = &input.ident;
    let (container, default, source) = container_entries(input)?;
    if let Source::Converted { source, fallible } = source {
        return converted_deserialize(input, fields, &source, fallible);
    }
    let plan = plan_fields(fields, |field| {
        let member = &field.member;
        quote! { __wire.#member }
    })?;

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
            let members: Vec<_> = fields.iter().map(|field| &field.member).collect();
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
    let generics = &input.generics;
    let wire_fields = &plan.wire_fields;
    let deserialize = match source {
        Source::Remote { remote, getters } => {
            let body = if getters {
                let read = build(&plan, &quote! { Self });
                quote! {
                    let __read: ::std::result::Result<Self, __D::Error> = { #read };
                    __read.map(<#remote as ::std::convert::From<Self>>::from)
                }
            } else {
                build(&plan, &quote! { #remote })
            };
            remote_handoff(input, &remote, body)
        }
        Source::Fields | Source::Converted { .. } => {
            deserialize_impl(input, build(&plan, &quote! { Self }))
        }
    };
    // A tuple struct's shadow is one too, so serde reads it as an array.
    let shadow = match fields.first().map(|field| &field.member) {
        Some(syn::Member::Unnamed(_)) => quote! {
            struct __EvenframeWire #generics (#(#wire_fields),*) #where_clause;
        },
        _ => quote! {
            struct __EvenframeWire #generics #where_clause {
                #(#wire_fields),*
            }
        },
    };

    Ok(quote! {
        const _: () = {
            #[derive(::serde::Deserialize)]
            #container_attribute
            #default_attribute
            #shadow

            #default_function

            #deserialize
        };
    })
}

/// serde's `remote` handoff, `Self::deserialize`, returning the remote type
/// that `body` builds from `__wire`, the shadow serde read.
fn remote_handoff(input: &DeriveInput, remote: &syn::Path, body: TokenStream) -> TokenStream {
    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    quote! {
        impl #impl_generics #ident #ty_generics #where_clause {
            pub fn deserialize<'__de, __D>(__deserializer: __D) -> ::std::result::Result<#remote, __D::Error>
            where
                __D: ::serde::Deserializer<'__de>,
                __EvenframeWire #ty_generics: ::serde::Deserialize<'__de>,
            {
                let __wire = <__EvenframeWire #ty_generics as ::serde::Deserialize<'__de>>::deserialize(
                    __deserializer,
                )?;
                #body
            }
        }
    }
}

/// serde's `from` or `try_from`: the source type read and converted, then
/// each field's morphs and validators run on the result.
fn converted_deserialize(
    input: &DeriveInput,
    fields: &[CheckedField],
    source: &syn::Type,
    fallible: bool,
) -> syn::Result<TokenStream> {
    let ident = &input.ident;
    let plan = plan_fields(fields, |field| {
        let binding = format_ident!("__converted_{}", field.binding_name());
        quote! { #binding }
    })?;
    let members: Vec<_> = fields.iter().map(|field| &field.member).collect();
    let bindings: Vec<_> = fields
        .iter()
        .map(|field| format_ident!("__converted_{}", field.binding_name()))
        .collect();
    let convert = if fallible {
        quote! {
            <Self as ::std::convert::TryFrom<#source>>::try_from(__source)
                .map_err(::serde::de::Error::custom)?
        }
    } else {
        quote! { <Self as ::std::convert::From<#source>>::from(__source) }
    };
    let body = build(&plan, &quote! { Self });
    let (_, ty_generics, _) = input.generics.split_for_impl();
    let mut generics = input.generics.clone();
    generics.params.insert(0, syn::parse_quote! { '__de });
    generics
        .make_where_clause()
        .predicates
        .push(syn::parse_quote! { #source: ::serde::Deserialize<'__de> });
    let (impl_generics, _, where_clause) = generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics ::serde::Deserialize<'__de> for #ident #ty_generics #where_clause {
            fn deserialize<__D>(__deserializer: __D) -> ::std::result::Result<Self, __D::Error>
            where
                __D: ::serde::Deserializer<'__de>,
            {
                let __source = <#source as ::serde::Deserialize<'__de>>::deserialize(__deserializer)?;
                let __converted: Self = #convert;
                let Self { #(#members: #bindings),* } = __converted;
                #body
            }
        }
    })
}

/// One variant of an enum with field validators: its fields as checked, by
/// name or position.
pub(crate) struct CheckedVariant<'a> {
    pub variant: &'a syn::Variant,
    pub fields: Vec<CheckedField<'a>>,
}

/// The generated `Deserialize` for an enum with variant field validators. The
/// shadow enum keeps every container, variant and field `#[serde(...)]`
/// attribute, so tagging and naming read exactly as serde's own derive.
pub(crate) fn generate_enum_deserialize(
    input: &DeriveInput,
    variants: &[CheckedVariant],
) -> syn::Result<TokenStream> {
    refuse_borrowing(input)?;
    let (container, default, source) = container_entries(input)?;
    if let Source::Converted { source, fallible } = &source {
        return converted_enum_deserialize(input, variants, source, *fallible);
    }
    // A remote enum's read builds the remote type's variants.
    let built = |variant: &syn::Ident| match &source {
        Source::Remote { remote, .. } => quote! { #remote::#variant },
        Source::Fields | Source::Converted { .. } => quote! { Self::#variant },
    };
    if !matches!(default, ContainerDefault::None) {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "#[serde(default)] does not apply to an enum",
        ));
    }

    let mut wire_variants = Vec::new();
    let mut arms = Vec::new();
    for CheckedVariant { variant, fields } in variants {
        let variant_ident = &variant.ident;
        let variant_attribute = serde_attribute(&serde_entries(&variant.attrs)?);
        match &variant.fields {
            Fields::Unit => {
                wire_variants.push(quote! { #variant_attribute #variant_ident });
                let variant = built(variant_ident);
                arms.push(quote! {
                    __EvenframeWire::#variant_ident => ::std::result::Result::Ok(#variant),
                });
            }
            Fields::Unnamed(_) if fields.iter().any(|field| !field.validators.is_empty()) => {
                let plan = plan_fields(fields, |field| {
                    let binding = format_ident!("__wire_{}", field.binding_name());
                    quote! { #binding }
                })?;
                let bindings: Vec<_> = fields
                    .iter()
                    .map(|field| format_ident!("__wire_{}", field.binding_name()))
                    .collect();
                let wire_fields = &plan.wire_fields;
                wire_variants.push(quote! { #variant_attribute #variant_ident(#(#wire_fields),*) });
                let body = build(&plan, &built(variant_ident));
                arms.push(quote! {
                    __EvenframeWire::#variant_ident(#(#bindings),*) => { #body }
                });
            }
            Fields::Unnamed(unnamed) => {
                let mut members = Vec::new();
                let mut bindings = Vec::new();
                for (position, field) in unnamed.unnamed.iter().enumerate() {
                    let field_attribute = serde_attribute(&serde_entries(&field.attrs)?);
                    let ty = &field.ty;
                    members.push(quote! { #field_attribute #ty });
                    bindings.push(format_ident!("__field{position}"));
                }
                wire_variants.push(quote! { #variant_attribute #variant_ident(#(#members),*) });
                let variant = built(variant_ident);
                arms.push(quote! {
                    __EvenframeWire::#variant_ident(#(#bindings),*) =>
                        ::std::result::Result::Ok(#variant(#(#bindings),*)),
                });
            }
            Fields::Named(_) => {
                let plan = plan_fields(fields, |field| {
                    let binding = format_ident!("__wire_{}", field.binding_name());
                    quote! { #binding }
                })?;
                let members: Vec<_> = fields.iter().map(|field| &field.member).collect();
                let bindings = fields
                    .iter()
                    .map(|field| format_ident!("__wire_{}", field.binding_name()));
                let wire_fields = &plan.wire_fields;
                wire_variants
                    .push(quote! { #variant_attribute #variant_ident { #(#wire_fields),* } });
                let body = build(&plan, &built(variant_ident));
                arms.push(quote! {
                    __EvenframeWire::#variant_ident { #(#members: #bindings),* } => { #body }
                });
            }
        }
    }

    let container_attribute = serde_attribute(&container);
    let generics = &input.generics;
    let (_, _, where_clause) = input.generics.split_for_impl();
    let read = quote! { match __wire { #(#arms)* } };
    let deserialize = match &source {
        Source::Remote { remote, .. } => remote_handoff(input, remote, read),
        Source::Fields | Source::Converted { .. } => deserialize_impl(input, read),
    };
    Ok(quote! {
        const _: () = {
            #[derive(::serde::Deserialize)]
            #container_attribute
            enum __EvenframeWire #generics #where_clause {
                #(#wire_variants),*
            }

            #deserialize
        };
    })
}

/// The generated `Deserialize` for a newtype with validators: the value serde
/// writes for it is read, through the parse morph when there is one, every
/// validator runs on it, and the newtype is built around it. `others` are a
/// transparent struct's skipped fields, which take their defaults as serde's
/// own derive gives them.
pub(crate) fn generate_newtype_deserialize(
    input: &DeriveInput,
    checked: &CheckedField,
    others: &[&syn::Member],
) -> syn::Result<TokenStream> {
    refuse_borrowing(input)?;
    let ident = &input.ident;
    let member = &checked.member;
    let field_type = &checked.field.ty;
    let mutability = checked.transforms()?.then(|| quote! { mut });
    let rejected = quote! {
        ::std::result::Result::Err(::serde::de::Error::custom(__errors))
    };
    let read = quote! {
        let #mutability __value =
            <#field_type as ::serde::Deserialize<'__de>>::deserialize(__deserializer)?;
    };
    let chain = newtype_chain(checked)?;
    let checks = !chain.is_empty();
    let recheck = mutability
        .is_some()
        .then(|| recheck(&quote! { &__value }, &quote! { #field_type }, ""));

    let (errors, verdict) = if checks {
        (
            quote! { let mut __errors = ::evenframe::validator::validate::ValidationErrors::new(); },
            quote! {
                if !__errors.is_empty() {
                    return #rejected;
                }
            },
        )
    } else {
        (TokenStream::new(), TokenStream::new())
    };

    let (_, ty_generics, _) = input.generics.split_for_impl();
    let mut generics = input.generics.clone();
    generics.params.insert(0, syn::parse_quote! { '__de });
    generics
        .make_where_clause()
        .predicates
        .push(syn::parse_quote! { #field_type: ::serde::Deserialize<'__de> });
    let (impl_generics, _, where_clause) = generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics ::serde::Deserialize<'__de> for #ident #ty_generics #where_clause {
            fn deserialize<__D>(__deserializer: __D) -> ::std::result::Result<Self, __D::Error>
            where
                __D: ::serde::Deserializer<'__de>,
            {
                #errors
                #read
                #chain
                #recheck
                #verdict
                ::std::result::Result::Ok(Self {
                    #member: __value,
                    #(#others: ::std::default::Default::default(),)*
                })
            }
        }
    })
}

/// A newtype's validators run in order on `__value`, rewriting it, each
/// rejection recorded in `__errors`; an absent optional value is not checked.
fn newtype_chain(checked: &CheckedField) -> syn::Result<TokenStream> {
    let optional = checked.is_optional();
    let place = if optional {
        quote! { (*__present) }
    } else {
        quote! { __value }
    };
    let steps = checked.chain(&place, true)?;
    if !optional || steps.is_empty() {
        return Ok(steps);
    }
    let borrow = if checked.transforms()? {
        quote! { &mut }
    } else {
        quote! { & }
    };
    Ok(quote! {
        if let ::std::option::Option::Some(__present) = #borrow __value { #steps }
    })
}

/// A validated newtype built from a value the program holds: `TryFrom` its
/// value runs the validators as reading it does, then the value's own
/// `Validate`, since nothing has checked it yet. A value of `String` is also
/// built from a `&str` and read back with `as_str`. A value naming one of the
/// newtype's type parameters gets none of these, since core's blanket
/// `TryFrom<U> for T` already covers that impl.
pub(crate) fn generate_newtype_constructors(
    input: &DeriveInput,
    checked: &CheckedField,
    others: &[&syn::Member],
) -> syn::Result<TokenStream> {
    let field_type = &checked.field.ty;
    if names_type_parameter(quote! { #field_type }, &input.generics) {
        return Ok(TokenStream::new());
    }
    let ident = &input.ident;
    let member = &checked.member;
    let mutability = checked.transforms()?.then(|| quote! { mut });
    let chain = newtype_chain(checked)?;
    let recheck = recheck(&quote! { &__value }, &quote! { #field_type }, "");
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let text = is_string_type(field_type).then(|| {
        quote! {
            impl #impl_generics ::std::convert::TryFrom<&str> for #ident #ty_generics #where_clause {
                type Error = ::evenframe::validator::validate::ValidationErrors;

                fn try_from(text: &str) -> ::std::result::Result<Self, Self::Error> {
                    Self::try_from(text.to_owned())
                }
            }

            impl #impl_generics #ident #ty_generics #where_clause {
                pub fn as_str(&self) -> &str {
                    &self.#member
                }
            }
        }
    });
    Ok(quote! {
        impl #impl_generics ::std::convert::TryFrom<#field_type> for #ident #ty_generics #where_clause {
            type Error = ::evenframe::validator::validate::ValidationErrors;

            fn try_from(#mutability __value: #field_type) -> ::std::result::Result<Self, Self::Error> {
                let mut __errors = ::evenframe::validator::validate::ValidationErrors::new();
                #chain
                #recheck
                __errors.into_result()?;
                ::std::result::Result::Ok(Self {
                    #member: __value,
                    #(#others: ::std::default::Default::default(),)*
                })
            }
        }

        #text
    })
}

/// Whether `tokens` name one of `generics`' type parameters anywhere.
fn names_type_parameter(tokens: TokenStream, generics: &syn::Generics) -> bool {
    let parameters: Vec<&syn::Ident> = generics
        .type_params()
        .map(|parameter| &parameter.ident)
        .collect();
    tokens.into_iter().any(|tree| match tree {
        proc_macro2::TokenTree::Ident(name) => parameters.contains(&&name),
        proc_macro2::TokenTree::Group(group) => names_type_parameter(group.stream(), generics),
        proc_macro2::TokenTree::Punct(_) | proc_macro2::TokenTree::Literal(_) => false,
    })
}

/// Whether `ty` is `String`, written bare or by its `std`/`alloc` path.
fn is_string_type(ty: &syn::Type) -> bool {
    let syn::Type::Path(type_path) = ty else {
        return false;
    };
    let segments: Vec<String> = type_path
        .path
        .segments
        .iter()
        .map(|segment| match segment.arguments {
            syn::PathArguments::None => segment.ident.to_string(),
            syn::PathArguments::AngleBracketed(_) | syn::PathArguments::Parenthesized(_) => {
                String::new()
            }
        })
        .collect();
    type_path.qself.is_none()
        && matches!(
            segments
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .as_slice(),
            ["String"] | ["std" | "alloc", "string", "String"]
        )
}

/// An enum serde reads by `from` or `try_from`: the source read and converted,
/// then each variant field's validators run, in order and rewriting it, on the
/// converted value, and each field's own `Validate` after a rewrite.
fn converted_enum_deserialize(
    input: &DeriveInput,
    variants: &[CheckedVariant],
    source: &syn::Type,
    fallible: bool,
) -> syn::Result<TokenStream> {
    let ident = &input.ident;
    let mut arms = Vec::new();
    for CheckedVariant { variant, fields } in variants {
        if fields.iter().all(|field| field.validators.is_empty()) {
            continue;
        }
        let variant_ident = &variant.ident;
        let bindings: Vec<_> = fields
            .iter()
            .map(|field| format_ident!("__converted_{}", field.binding_name()))
            .collect();
        let mut checks = Vec::new();
        for (field, binding) in fields.iter().zip(&bindings) {
            let path = &field.path;
            let field_type = &field.field.ty;
            let chain = if field.is_optional() {
                let chain = field.chain(&quote! { (*__value) }, true)?;
                quote! { if let ::std::option::Option::Some(__value) = #binding { #chain } }
            } else {
                field.chain(&quote! { (*#binding) }, true)?
            };
            checks.push(quote! {
                #chain
                if let ::std::result::Result::Err(__nested) =
                    (&::evenframe::validator::validate::__private::Probe::<#field_type>(&*#binding)).nested()
                {
                    __errors.nest(#path, __nested);
                }
            });
        }
        let pattern = match &variant.fields {
            Fields::Named(_) => {
                let members = fields.iter().map(|field| &field.member);
                quote! { Self::#variant_ident { #(#members: #bindings),* } }
            }
            Fields::Unnamed(_) => quote! { Self::#variant_ident(#(#bindings),*) },
            Fields::Unit => continue,
        };
        arms.push(quote! { #pattern => { #(#checks)* } });
    }
    let convert = if fallible {
        quote! {
            <Self as ::std::convert::TryFrom<#source>>::try_from(__source)
                .map_err(::serde::de::Error::custom)?
        }
    } else {
        quote! { <Self as ::std::convert::From<#source>>::from(__source) }
    };
    // Variants with nothing to check need an arm only when others have one.
    let rest = (arms.len() < variants.len()).then(|| quote! { _ => {} });
    let body = if arms.is_empty() {
        quote! { ::std::result::Result::Ok(#convert) }
    } else {
        quote! {
            use ::evenframe::validator::validate::__private::Nested as _;
            let mut __converted: Self = #convert;
            let mut __errors = ::evenframe::validator::validate::ValidationErrors::new();
            match &mut __converted {
                #(#arms)*
                #rest
            }
            if __errors.is_empty() {
                ::std::result::Result::Ok(__converted)
            } else {
                ::std::result::Result::Err(::serde::de::Error::custom(__errors))
            }
        }
    };
    let (_, ty_generics, _) = input.generics.split_for_impl();
    let mut generics = input.generics.clone();
    generics.params.insert(0, syn::parse_quote! { '__de });
    generics
        .make_where_clause()
        .predicates
        .push(syn::parse_quote! { #source: ::serde::Deserialize<'__de> });
    let (impl_generics, _, where_clause) = generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics ::serde::Deserialize<'__de> for #ident #ty_generics #where_clause {
            fn deserialize<__D>(__deserializer: __D) -> ::std::result::Result<Self, __D::Error>
            where
                __D: ::serde::Deserializer<'__de>,
            {
                let __source = <#source as ::serde::Deserialize<'__de>>::deserialize(__deserializer)?;
                #body
            }
        }
    })
}
