//! The generated `SurrealValue`: a value in the shape evenframe's schema
//! defines for its type. Fields sit under their database names, enums keep
//! serde's representation, and a field missing from the record takes serde's
//! default. A read is validated as serde's read is.

use crate::validate_impl::{CheckedField, is_option_type};
use convert_case::{Case, Casing};
use evenframe_core::derive::naming::{
    FieldDefault, FieldHandling, ItemShape, ItemWire, UnitValue, unraw,
};
use evenframe_core::types::{ContentStorage, EnumRepresentation, Wire};
use proc_macro2::TokenStream;
use quote::{format_ident, quote, quote_spanned};
use syn::{DeriveInput, Fields, GenericArgument, PathArguments, Type, spanned::Spanned};

/// The path the generated code reaches the conversions through.
fn private() -> TokenStream {
    quote! { ::evenframe::surreal_value::__private }
}

/// What the impl is for.
#[derive(Clone, Copy)]
pub(crate) enum Mode {
    /// A type evenframe describes: keys are its database names, and a read is
    /// validated as serde's read is.
    Stored,
    /// A query's row: keys are its `#[surreal]` names, else its serde names,
    /// as serde read it from the database's JSON; it has no validators.
    Row,
}

impl Mode {
    fn finish(self, type_name: &str) -> TokenStream {
        let private = private();
        match self {
            Mode::Stored => quote! { #private::validated(__read, #type_name) },
            Mode::Row => quote! { ::std::result::Result::Ok(__read) },
        }
    }

    fn key(self, wire: &Wire, ident: &syn::Ident) -> String {
        let named = match self {
            Mode::Stored => wire.surreal.clone(),
            Mode::Row => wire.surreal.clone().or_else(|| wire.serde.clone()),
        };
        named.unwrap_or_else(|| unraw(ident))
    }

    /// The other names a key is read by: serde's aliases, where a row reads
    /// by serde's name.
    fn aliases(self, wire: &Wire, aliases: &[String]) -> Vec<String> {
        match self {
            Mode::Row if wire.surreal.is_none() => aliases.to_vec(),
            Mode::Row | Mode::Stored => Vec::new(),
        }
    }
}

/// `T` of an `Option<T>`.
fn option_inner(ty: &Type) -> Option<&Type> {
    if !is_option_type(ty) {
        return None;
    }
    let Type::Path(type_path) = ty else {
        return None;
    };
    let PathArguments::AngleBracketed(arguments) = &type_path.path.segments.last()?.arguments
    else {
        return None;
    };
    match arguments.args.first()? {
        GenericArgument::Type(inner) => Some(inner),
        _ => None,
    }
}

/// The reader for a text field of a row: `String` and `Vec<String>` read
/// any value as its JSON text, as serde read the row. An `Option` around
/// them is unwrapped by the caller.
fn text_reader(ty: &Type) -> Option<syn::Ident> {
    let inner = option_inner(ty).unwrap_or(ty);
    let Type::Path(path) = inner else {
        return None;
    };
    let last = path.path.segments.last()?;
    let reader = match last.ident.to_string().as_str() {
        "String" => "text",
        "Vec" => match &last.arguments {
            PathArguments::AngleBracketed(arguments) => match arguments.args.first() {
                Some(GenericArgument::Type(Type::Path(item))) if item.path.is_ident("String") => {
                    "texts"
                }
                _ => return None,
            },
            _ => return None,
        },
        _ => return None,
    };
    Some(syn::Ident::new(reader, proc_macro2::Span::call_site()))
}

/// How a value converts to the database's.
#[derive(Clone)]
enum Conversion {
    /// Its type's own `SurrealValue`, or serde where it has none.
    Own,
    /// Through serde, by `#[surreal(wrap)]`.
    Serde,
    /// Through serde with the field's own functions, `with`,
    /// `serialize_with` or `deserialize_with`, by the private type named.
    Shim(syn::Ident),
}

impl Conversion {
    /// `value` of type `ty` as the database's value.
    fn write(&self, ty: &Type, value: TokenStream) -> TokenStream {
        let private = private();
        match self {
            Conversion::Own => quote! { (&#private::Field::<#ty>::new()).write(#value) },
            Conversion::Serde => {
                quote! { #private::SurrealValue::into_value(#private::SerdeWrapper(#value)) }
            }
            Conversion::Shim(shim) => quote! {
                #private::SurrealValue::into_value(#private::SerdeWrapper(#shim(
                    #value,
                    ::std::marker::PhantomData,
                )))
            },
        }
    }

    /// The database's `value` read as `ty`.
    fn read(&self, ty: &Type, value: TokenStream) -> TokenStream {
        let private = private();
        match self {
            Conversion::Own => quote! { (&#private::Field::<#ty>::new()).read(#value) },
            Conversion::Serde => quote! {
                <#private::SerdeWrapper<#ty> as #private::SurrealValue>::from_value(#value)
                    .map(|__wrapped| __wrapped.0)
            },
            Conversion::Shim(shim) => quote! {
                <#private::SerdeWrapper<#shim> as #private::SurrealValue>::from_value(#value)
                    .map(|__wrapped| __wrapped.0.0)
            },
        }
    }

    /// Whether the value converts whole, an `Option` included, as serde's
    /// functions see it.
    fn whole(&self) -> bool {
        !matches!(self, Conversion::Own)
    }
}

/// The private type a field with its own serde functions is stored through,
/// carrying those functions, generic over the container's type parameters.
fn serde_shim(
    input: &DeriveInput,
    field: &syn::Field,
    name: &syn::Ident,
) -> syn::Result<TokenStream> {
    let functions: Vec<syn::Meta> = field
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("serde"))
        .map(|attribute| {
            attribute.parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            )
        })
        .collect::<syn::Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .filter(|entry| {
            ["with", "serialize_with", "deserialize_with"]
                .iter()
                .any(|key| entry.path().is_ident(key))
        })
        .collect();
    let ty = &field.ty;
    let parameters: Vec<&syn::Ident> = input
        .generics
        .type_params()
        .map(|parameter| &parameter.ident)
        .collect();
    Ok(quote! {
        #[derive(::serde::Serialize, ::serde::Deserialize)]
        #[serde(transparent)]
        struct #name<#(#parameters),*>(
            #[serde(#(#functions),*)] #ty,
            #[serde(skip)] ::std::marker::PhantomData<(#(#parameters,)*)>,
        );
    })
}

/// One named field as the impl writes and reads it.
struct NamedField<'a> {
    member: &'a syn::Ident,
    binding: TokenStream,
    ty: &'a Type,
    key: String,
    /// Whether the database holds the field.
    written: bool,
    /// Whether a stored value is read rather than the field's default.
    read: bool,
    default: FieldDefault,
    /// The field's own fields sit beside its siblings in the object.
    flatten: bool,
    /// A row's text field, which reads any value as its JSON text.
    reads_text: bool,
    /// The other names the field is read by.
    aliases: Vec<String>,
    conversion: Conversion,
    /// The private type a `Shim` conversion goes through.
    shim: Option<TokenStream>,
}

impl<'a> NamedField<'a> {
    fn is_required(&self) -> bool {
        self.read && matches!(self.default, FieldDefault::None) && option_inner(self.ty).is_none()
    }

    fn new(
        input: &DeriveInput,
        field: &'a syn::Field,
        wire: &Wire,
        handling: &FieldHandling,
        binding: TokenStream,
        mode: Mode,
    ) -> syn::Result<Self> {
        let member = field
            .ident
            .as_ref()
            .ok_or_else(|| syn::Error::new_spanned(field, "a named field has no identifier"))?;
        let (conversion, shim) = if handling.custom_serde {
            let name = format_ident!("__EvenframeStored{}", unraw(member).to_case(Case::Pascal));
            let shim = serde_shim(input, field, &name)?;
            (Conversion::Shim(name), Some(shim))
        } else if wire.storage.opaque {
            (Conversion::Serde, None)
        } else {
            (Conversion::Own, None)
        };
        Ok(Self {
            member,
            binding,
            ty: &field.ty,
            key: mode.key(wire, member),
            written: !wire.storage.skipped,
            read: handling.storage_reads,
            default: handling.stored_default.clone(),
            flatten: wire.storage.flatten,
            reads_text: matches!(mode, Mode::Row) && text_reader(&field.ty).is_some(),
            aliases: mode.aliases(wire, &handling.aliases),
            conversion,
            shim,
        })
    }
}

/// A flattened field read from the keys its siblings left in `__fields`,
/// taking a struct's own keys from the later ones, as serde does. serde reads
/// a flattened `Option` as `None` when what it holds cannot be read from
/// those keys, so that failure means absence rather than an error.
fn read_flattened(field: &NamedField, type_name: &str) -> TokenStream {
    let private = private();
    let key = &field.key;
    let conversion = &field.conversion;
    match option_inner(field.ty).filter(|_| !conversion.whole()) {
        Some(inner) => {
            let read = conversion.read(inner, quote! { #private::rest(&__fields) });
            quote! {
                match #read {
                    ::std::result::Result::Ok(__held) => {
                        #private::take(&mut __fields, (&#private::Field::<#inner>::new()).taken());
                        ::std::option::Option::Some(__held)
                    }
                    ::std::result::Result::Err(_) => ::std::option::Option::None,
                }
            }
        }
        None => {
            let ty = field.ty;
            let read = conversion.read(ty, quote! { #private::rest(&__fields) });
            quote! {{
                let __held = #private::field(#read, #type_name, #key)?;
                #private::take(&mut __fields, (&#private::Field::<#ty>::new()).taken());
                __held
            }}
        }
    }
}

/// Statements inserting each field's value into the map `__fields`.
fn write_fields(fields: &[NamedField]) -> TokenStream {
    let private = private();
    fields
        .iter()
        .filter(|field| field.written)
        .map(|field| {
            let key = &field.key;
            let binding = &field.binding;
            let conversion = &field.conversion;
            if field.flatten {
                let written = conversion.write(field.ty, quote! { #binding });
                return quote! { #private::merge(&mut __fields, #key, #written); };
            }
            match option_inner(field.ty).filter(|_| !conversion.whole()) {
                Some(inner) => {
                    let written = conversion.write(inner, quote! { __inner });
                    quote! {
                        __fields.insert(
                            #key.to_owned(),
                            match #binding {
                                ::std::option::Option::Some(__inner) => #written,
                                ::std::option::Option::None => #private::Value::None,
                            },
                        );
                    }
                }
                None => {
                    let written = conversion.write(field.ty, quote! { #binding });
                    quote! { __fields.insert(#key.to_owned(), #written); }
                }
            }
        })
        .collect()
}

/// The value of a field missing from the record, or the error serde gives.
fn missing_value(field: &NamedField, type_name: &str, container_default: bool) -> TokenStream {
    let private = private();
    let member = field.member;
    match &field.default {
        FieldDefault::Function(function) => quote! { #function() },
        FieldDefault::Trait => quote! { ::std::default::Default::default() },
        FieldDefault::None if container_default => quote! { __default.#member },
        FieldDefault::None if !field.read || option_inner(field.ty).is_some() => {
            quote! { ::std::default::Default::default() }
        }
        FieldDefault::None => {
            let key = &field.key;
            quote! { return ::std::result::Result::Err(#private::missing(#type_name, #key)) }
        }
    }
}

/// `member: value` for each field, read from the map `__fields`. A
/// flattened field comes last, to read the keys its siblings left.
fn read_fields(
    fields: &[NamedField],
    type_name: &str,
    container_default: bool,
) -> Vec<TokenStream> {
    let private = private();
    let (flattened, own): (Vec<&NamedField>, Vec<&NamedField>) =
        fields.iter().partition(|field| field.flatten && field.read);
    own.into_iter()
        .chain(flattened)
        .map(|field| {
            let member = field.member;
            let missing = missing_value(field, type_name, container_default);
            if !field.read {
                return quote! { #member: #missing };
            }
            let key = &field.key;
            if field.flatten {
                let read = read_flattened(field, type_name);
                return quote! { #member: #read };
            }
            let read = |ty: &Type| {
                let converted = match text_reader(ty).filter(|_| field.reads_text) {
                    Some(reader) => quote! { #private::#reader(__present) },
                    None => field.conversion.read(ty, quote! { __present }),
                };
                quote! { #private::field(#converted, #type_name, #key)? }
            };
            let present = match option_inner(field.ty).filter(|_| !field.conversion.whole()) {
                Some(inner) => {
                    let read = read(inner);
                    quote! { ::std::option::Option::Some(#read) }
                }
                None => read(field.ty),
            };
            let aliases = &field.aliases;
            let optional = option_inner(field.ty).is_some();
            quote! {
                #member: match #private::present(&mut __fields, &[#key #(, #aliases)*], #optional) {
                    ::std::option::Option::Some(__present) => #present,
                    ::std::option::Option::None => #missing,
                }
            }
        })
        .collect()
}

fn deny_unknown(enabled: bool, type_name: &str) -> TokenStream {
    let private = private();
    if enabled {
        quote! { #private::deny_unknown(&__fields, #type_name)?; }
    } else {
        TokenStream::new()
    }
}

/// The impl around the three bodies, each type parameter bound to
/// `SurrealValue` so its fields convert with the parameter's own impl, beside
/// the private `support` types the bodies use.
fn surreal_value_impl(
    input: &DeriveInput,
    kind: TokenStream,
    into_value: TokenStream,
    from_value: TokenStream,
    support: TokenStream,
) -> TokenStream {
    let private = private();
    let ident = &input.ident;
    let mut generics = input.generics.clone();
    let parameters: Vec<_> = generics
        .type_params()
        .map(|parameter| parameter.ident.clone())
        .collect();
    let where_clause = generics.make_where_clause();
    for parameter in parameters {
        where_clause
            .predicates
            .push(syn::parse_quote! { #parameter: #private::SurrealValue });
    }
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    quote! {
        ::evenframe::__surreal_value! {
            const _: () = {
                use #private::Convert as _;
                use #private::Taken as _;

                #support

                impl #impl_generics #private::SurrealValue for #ident #ty_generics #where_clause {
                    fn kind_of() -> #private::Kind {
                        #kind
                    }

                    fn into_value(self) -> #private::Value {
                        #into_value
                    }

                    fn from_value(
                        __value: #private::Value,
                    ) -> ::std::result::Result<Self, #private::Error> {
                        #from_value
                    }
                }
            };
        }
    }
}

pub(crate) fn struct_surreal_value(
    input: &DeriveInput,
    wire: &ItemWire,
    mode: Mode,
    checked_fields: &[CheckedField],
) -> syn::Result<TokenStream> {
    let private = private();
    let syn::Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(&input.ident, "expected a struct"));
    };
    let type_name = input.ident.to_string();
    if let Fields::Unnamed(unnamed) = &data.fields {
        return Ok(tuple_struct_surreal_value(
            input, wire, unnamed, &type_name, mode,
        ));
    }
    if let ItemShape::Newtype {
        member: syn::Member::Named(member),
    } = &wire.shape
    {
        return transparent_surreal_value(input, data, member, &type_name, mode);
    }
    if let Fields::Unit = &data.fields {
        return Ok(unit_struct_surreal_value(input, wire, &type_name, mode));
    }
    let fields = data
        .fields
        .iter()
        .zip(wire.fields.iter().zip(&wire.handling))
        .map(|(field, (field_wire, handling))| {
            let member = field.ident.as_ref();
            NamedField::new(
                input,
                field,
                field_wire,
                handling,
                quote! { self.#member },
                mode,
            )
        })
        .collect::<syn::Result<Vec<_>>>()?;

    let writes = write_fields(&fields);
    let into_value = quote! {
        let mut __fields = ::std::collections::BTreeMap::new();
        #writes
        #private::object_value(__fields)
    };
    let stored_keys = stored_keys_impl(input, &fields);

    let (container_default, default_binding) = match &wire.stored_container_default {
        FieldDefault::None => (false, TokenStream::new()),
        FieldDefault::Trait => (
            true,
            quote! { let __default: Self = ::std::default::Default::default(); },
        ),
        FieldDefault::Function(function) => (true, quote! { let __default: Self = #function(); }),
    };
    let reads = read_fields(&fields, &type_name, container_default);
    let deny = deny_unknown(wire.deny_unknown_fields, &type_name);
    let finish = mode.finish(&type_name);
    let from_value = if matches!(mode, Mode::Stored) && !checked_fields.is_empty() {
        checked_struct_read(
            &fields,
            checked_fields,
            &type_name,
            container_default,
            default_binding,
            deny,
            finish,
        )?
    } else {
        quote! {
            let mut __fields = #private::object(__value, #type_name)?;
            #default_binding
            let __read = Self { #(#reads),* };
            #deny
            #finish
        }
    };
    let shims = fields.iter().filter_map(|field| field.shim.as_ref());
    let surreal_value = surreal_value_impl(
        input,
        quote! { #private::Kind::Object },
        into_value,
        from_value,
        quote! { #(#shims)* },
    );
    Ok(quote! { #surreal_value #stored_keys })
}

/// The keys a struct reads, for a struct that flattens it to take.
fn stored_keys_impl(input: &DeriveInput, fields: &[NamedField]) -> TokenStream {
    let private = private();
    let ident = &input.ident;
    let keys = fields
        .iter()
        .filter(|field| field.read && !field.flatten)
        .flat_map(|field| std::iter::once(&field.key).chain(&field.aliases));
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    quote! {
        ::evenframe::__surreal_value! {
            impl #impl_generics #private::StoredKeys for #ident #ty_generics #where_clause {
                fn stored_keys() -> &'static [&'static str] {
                    &[#(#keys),*]
                }
            }
        }
    }
}

fn checked_struct_read(
    fields: &[NamedField],
    checked_fields: &[CheckedField],
    type_name: &str,
    container_default: bool,
    default_binding: TokenStream,
    deny: TokenStream,
    finish: TokenStream,
) -> syn::Result<TokenStream> {
    if fields.len() != checked_fields.len() {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "SurrealValue field validation metadata is incomplete",
        ));
    }
    let private = private();
    let mut bindings = Vec::new();
    let mut pipelines = Vec::new();
    let mut assignments = Vec::new();
    let mut checks_anything = false;

    for (field, checked) in fields.iter().zip(checked_fields) {
        let member = field.member;
        let local = format_ident!("__field_{}", unraw(member));
        let missing = missing_value(field, type_name, container_default);
        let key = &field.key;
        let aliases = &field.aliases;
        let optional = option_inner(field.ty).is_some();
        let transforms = checked.transforms()?;
        let mutability = transforms.then(|| quote! { mut });

        let read = |ty: &Type| {
            let converted = field.conversion.read(ty, quote! { __present });
            quote! { #private::field(#converted, #type_name, #key)? }
        };
        let present = match option_inner(field.ty).filter(|_| !field.conversion.whole()) {
            Some(inner) => {
                let read = read(inner);
                quote! { ::std::option::Option::Some(#read) }
            }
            None => read(field.ty),
        };
        let raw = if !field.read {
            missing
        } else if field.flatten {
            read_flattened(field, type_name)
        } else {
            quote! {
                match #private::present(&mut __fields, &[#key #(, #aliases)*], #optional) {
                    ::std::option::Option::Some(__present) => #present,
                    ::std::option::Option::None => #missing,
                }
            }
        };

        bindings.push(quote! { let #mutability #local = #raw; });
        assignments.push(quote! { #member: #local });

        let chain = checked.chain(&quote! { (*__value) }, true)?;
        let direct_chain = checked.chain(&quote! { #local }, true)?;
        if !chain.is_empty() {
            checks_anything = true;
            let borrow = if transforms {
                quote! { &mut }
            } else {
                quote! { & }
            };
            pipelines.push(if optional {
                quote! { if let ::std::option::Option::Some(__value) = #borrow #local { #chain } }
            } else {
                direct_chain
            });
        }
    }

    let errors = checks_anything.then(|| {
        quote! {
            let mut __errors =
                ::evenframe::validator::validate::ValidationErrors::new();
        }
    });
    let build = quote! { Self { #(#assignments),* } };
    let reject = checks_anything.then(|| {
        quote! {
            if !__errors.is_empty() {
                return ::std::result::Result::Err(#private::error(
                    ::std::format!("{}: {}", #type_name, __errors),
                ));
            }
        }
    });
    Ok(quote! {
        let mut __fields = #private::object(__value, #type_name)?;
        #default_binding
        #errors
        #(#bindings)*
        #(#pipelines)*
        #deny
        #reject
        let __read = { #build };
        #finish
    })
}

/// A `#[serde(transparent)]` struct as serde writes it: the value of its one
/// field, the others left at their defaults.
fn transparent_surreal_value(
    input: &DeriveInput,
    data: &syn::DataStruct,
    member: &syn::Ident,
    type_name: &str,
    mode: Mode,
) -> syn::Result<TokenStream> {
    let private = private();
    let field = data
        .fields
        .iter()
        .find(|field| field.ident.as_ref() == Some(member))
        .ok_or_else(|| syn::Error::new_spanned(member, "the transparent field is missing"))?;
    let ty = &field.ty;
    let key = unraw(member);
    let others = data
        .fields
        .iter()
        .filter_map(|field| field.ident.as_ref())
        .filter(|ident| *ident != member);
    let finish = mode.finish(type_name);
    let kind = quote! { (&#private::Field::<#ty>::new()).kind() };
    let into_value = quote! { (&#private::Field::<#ty>::new()).write(self.#member) };
    let from_value = quote! {
        let __read: Self = Self {
            #member: #private::field(
                (&#private::Field::<#ty>::new()).read(__value),
                #type_name,
                #key,
            )?,
            #(#others: ::std::default::Default::default(),)*
        };
        #finish
    };
    Ok(surreal_value_impl(
        input,
        kind,
        into_value,
        from_value,
        TokenStream::new(),
    ))
}

/// A unit struct as serde writes it, null, or as its `#[surreal(value)]`.
fn unit_struct_surreal_value(
    input: &DeriveInput,
    wire: &ItemWire,
    type_name: &str,
    mode: Mode,
) -> TokenStream {
    let private = private();
    let literal = wire.unit_value.clone().unwrap_or(UnitValue::Null);
    let value = literal_value(&literal);
    let kind = literal_kind(&literal);
    let stored = literal.surql();
    let finish = mode.finish(type_name);
    // serde reads a unit struct from null, and the database gives NONE for
    // an absent one.
    let accepted = match literal {
        UnitValue::Null => quote! { #private::Value::Null | #private::Value::None },
        _ => quote! { __literal if __literal == #value },
    };
    let from_value = quote! {
        let __read: Self = match __value {
            #accepted => Self,
            __other => {
                return ::std::result::Result::Err(#private::error(::std::format!(
                    "{} is stored as {}, got {}",
                    #type_name,
                    #stored,
                    __other.kind(),
                )))
            }
        };
        #finish
    };
    surreal_value_impl(input, kind, value, from_value, TokenStream::new())
}

/// A tuple struct as serde writes it: a newtype as its value, more fields as
/// an array.
fn tuple_struct_surreal_value(
    input: &DeriveInput,
    wire: &ItemWire,
    unnamed: &syn::FieldsUnnamed,
    type_name: &str,
    mode: Mode,
) -> TokenStream {
    let private = private();
    let types: Vec<&Type> = unnamed.unnamed.iter().map(|field| &field.ty).collect();
    let conversions: Vec<Conversion> = (0..types.len())
        .map(|position| match wire.opaque_elements.get(position) {
            Some(true) => Conversion::Serde,
            _ => Conversion::Own,
        })
        .collect();
    let finish = mode.finish(type_name);
    let (kind, into_value, read) = match (types.as_slice(), wire.stored_tuple) {
        ([ty], false) => (
            match conversions[0] {
                Conversion::Own => quote! { (&#private::Field::<#ty>::new()).kind() },
                _ => quote! { #private::Kind::Any },
            },
            conversions[0].write(ty, quote! { self.0 }),
            {
                let read = conversions[0].read(ty, quote! { __value });
                quote! { Self(#private::field(#read, #type_name, "0")?) }
            },
        ),
        _ => {
            let length = types.len();
            let writes =
                types
                    .iter()
                    .zip(&conversions)
                    .enumerate()
                    .map(|(position, (ty, conversion))| {
                        let member = syn::Index::from(position);
                        conversion.write(ty, quote! { self.#member })
                    });
            let reads =
                types
                    .iter()
                    .zip(&conversions)
                    .enumerate()
                    .map(|(position, (ty, conversion))| {
                        let key = position.to_string();
                        let read = conversion.read(
                    ty,
                    quote! { __items.next().ok_or_else(|| #private::missing(#type_name, #key))? },
                );
                        quote! { #private::field(#read, #type_name, #key)? }
                    });
            (
                quote! { #private::Kind::Array(::std::boxed::Box::new(#private::Kind::Any), ::std::option::Option::None) },
                quote! { #private::array_value(::std::vec![#(#writes),*]) },
                quote! {{
                    let mut __items = #private::array(__value, #type_name, #length)?.into_iter();
                    Self(#(#reads),*)
                }},
            )
        }
    };
    let from_value = quote! {
        let __read: Self = #read;
        #finish
    };
    surreal_value_impl(input, kind, into_value, from_value, TokenStream::new())
}

/// One variant's payload: how it is written and read.
enum Payload<'a> {
    /// A unit variant, and the literal it is stored as when untagged.
    Unit {
        literal: Option<UnitValue>,
    },
    /// A tuple variant's elements, each with its conversion, written as an
    /// array when there are several or `#[surreal(tuple)]` says so.
    Unnamed {
        types: Vec<&'a Type>,
        conversions: Vec<Conversion>,
        array: bool,
    },
    Named(Vec<NamedField<'a>>),
}

struct EnumVariant<'a> {
    ident: &'a syn::Ident,
    name: String,
    /// The other names the variant is read by.
    aliases: Vec<String>,
    payload: Payload<'a>,
    /// Written bare, and read after every tagged variant.
    untagged: bool,
    /// Never stored: serde skips it, so it is written as NONE.
    skipped: bool,
    /// Read for any value no other variant reads, by `#[surreal(other)]`.
    other: bool,
    /// When an adjacently tagged variant's content key is written.
    content: ContentStorage,
    /// The predicate `#[surreal(skip_content_if)]` names.
    skip_content_if: Option<syn::Path>,
}

/// How a set of variants is written and read under one representation.
struct Represented {
    kinds: Vec<TokenStream>,
    /// One `match self` arm per variant.
    writes: Vec<TokenStream>,
    /// Reads `__value` into `Self`, returning any failure from the function
    /// around it.
    read: TokenStream,
}

/// A unit variant's stored literal as the database's value.
fn literal_value(literal: &UnitValue) -> TokenStream {
    let private = private();
    match literal {
        UnitValue::None => quote! { #private::Value::None },
        UnitValue::Null => quote! { #private::Value::Null },
        UnitValue::Bool(value) => quote! { #private::Value::Bool(#value) },
        UnitValue::String(value) => quote! { #private::Value::String(#value.to_owned()) },
        UnitValue::Int(value) => quote! { #private::Value::Number(#private::Number::Int(#value)) },
        UnitValue::Float(value) => {
            quote! { #private::Value::Number(#private::Number::Float(#value)) }
        }
    }
}

/// A unit variant's stored literal as a kind.
fn literal_kind(literal: &UnitValue) -> TokenStream {
    let private = private();
    match literal {
        UnitValue::None => quote! { #private::Kind::None },
        UnitValue::Null => quote! { #private::Kind::Null },
        UnitValue::Bool(value) => {
            quote! { #private::Kind::Literal(#private::KindLiteral::Bool(#value)) }
        }
        UnitValue::String(value) => {
            quote! { #private::Kind::Literal(#private::KindLiteral::String(#value.to_owned())) }
        }
        UnitValue::Int(value) => {
            quote! { #private::Kind::Literal(#private::KindLiteral::Integer(#value)) }
        }
        UnitValue::Float(value) => {
            quote! { #private::Kind::Literal(#private::KindLiteral::Float(#value)) }
        }
    }
}

fn payload_kind(payload: &Payload) -> TokenStream {
    let private = private();
    match payload {
        Payload::Unit {
            literal: Some(literal),
        } => literal_kind(literal),
        Payload::Unit { literal: None } => quote! { #private::Kind::None },
        Payload::Unnamed {
            types,
            conversions,
            array: false,
        } => match conversions.first() {
            Some(Conversion::Own) => {
                let ty = types[0];
                quote! { (&#private::Field::<#ty>::new()).kind() }
            }
            _ => quote! { #private::Kind::Any },
        },
        Payload::Unnamed { array: true, .. } => quote! {
            #private::Kind::Array(
                ::std::boxed::Box::new(#private::Kind::Any),
                ::std::option::Option::None,
            )
        },
        Payload::Named(_) => quote! { #private::Kind::Object },
    }
}

/// Whether one object's serialized keys could satisfy another variant's
/// reader. Field types may reject more values, so this is conservative.
fn object_can_read(
    reader: &[NamedField],
    writer: &[NamedField],
    deny_unknown_fields: bool,
) -> bool {
    if reader.iter().any(|field| field.read && field.flatten)
        || writer.iter().any(|field| field.written && field.flatten)
    {
        return true;
    }
    let required_present = reader
        .iter()
        .filter(|field| field.is_required())
        .all(|field| {
            writer
                .iter()
                .filter(|written| written.written)
                .any(|written| field.key == written.key || field.aliases.contains(&written.key))
        });
    // A key the writer always writes must be one the reader knows.
    let unknown_accepted = !deny_unknown_fields
        || writer
            .iter()
            .filter(|field| field.written && option_inner(field.ty).is_none())
            .all(|written| {
                reader
                    .iter()
                    .filter(|field| field.read)
                    .any(|field| field.key == written.key || field.aliases.contains(&written.key))
            });
    required_present && unknown_accepted
}

fn objects_are_distinguishable(left: &Payload, right: &Payload, deny_unknown_fields: bool) -> bool {
    match (left, right) {
        (Payload::Named(left), Payload::Named(right)) => {
            !object_can_read(left, right, deny_unknown_fields)
                && !object_can_read(right, left, deny_unknown_fields)
        }
        _ => false,
    }
}

/// The kind of value an untagged payload is stored as, which two untagged
/// variants cannot share unless their objects tell them apart.
fn untagged_kind_key(payload: &Payload) -> String {
    let (types, conversions) = match payload {
        Payload::Unit {
            literal: Some(literal),
        } => return format!("{} literal", literal.surql()),
        Payload::Unit { literal: None } => return "none".to_owned(),
        Payload::Named(_) => return "object".to_owned(),
        Payload::Unnamed { array: true, .. } => return "array".to_owned(),
        Payload::Unnamed {
            types, conversions, ..
        } => (types, conversions),
    };
    if !matches!(conversions.first(), Some(Conversion::Own)) {
        return "opaque".to_owned();
    }
    let Some(Type::Path(path)) = types.first() else {
        return "opaque".to_owned();
    };
    let Some(segment) = path.path.segments.last() else {
        return "opaque".to_owned();
    };
    match segment.ident.to_string().as_str() {
        "String" | "str" => "string".to_owned(),
        "bool" => "bool".to_owned(),
        "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64" | "u128"
        | "usize" => "int".to_owned(),
        "f32" | "f64" => "float".to_owned(),
        "Vec" | "Array" => "array".to_owned(),
        "RecordId" => "record".to_owned(),
        "Value" => "any".to_owned(),
        _ => "opaque".to_owned(),
    }
}

impl EnumVariant<'_> {
    /// The pattern matching every name the variant is read by.
    fn names(&self) -> TokenStream {
        let name = &self.name;
        let aliases = &self.aliases;
        quote! { #name #(| #aliases)* }
    }

    /// The pattern binding the variant's values.
    fn pattern(&self) -> TokenStream {
        let ident = self.ident;
        match &self.payload {
            Payload::Unit { .. } => quote! { Self::#ident },
            Payload::Unnamed { types, .. } => {
                let bindings = (0..types.len()).map(|position| format_ident!("__field{position}"));
                quote! { Self::#ident(#(#bindings),*) }
            }
            Payload::Named(fields) => {
                let members = fields.iter().map(|field| field.member);
                let bindings = fields.iter().map(|field| &field.binding);
                quote! { Self::#ident { #(#members: #bindings),* } }
            }
        }
    }

    /// The pattern matching the variant without binding its values.
    fn wildcard(&self) -> TokenStream {
        let ident = self.ident;
        match &self.payload {
            Payload::Unit { .. } => quote! { Self::#ident },
            Payload::Unnamed { .. } => quote! { Self::#ident(..) },
            Payload::Named(_) => quote! { Self::#ident { .. } },
        }
    }

    /// The payload's value from the pattern's bindings.
    fn payload_value(&self) -> TokenStream {
        let private = private();
        match &self.payload {
            Payload::Unit {
                literal: Some(literal),
            } => literal_value(literal),
            Payload::Unit { literal: None } => quote! { #private::Value::None },
            Payload::Unnamed {
                types,
                conversions,
                array,
            } => {
                let items: Vec<TokenStream> = types
                    .iter()
                    .zip(conversions)
                    .enumerate()
                    .map(|(position, (ty, conversion))| {
                        let binding = format_ident!("__field{position}");
                        conversion.write(ty, quote! { #binding })
                    })
                    .collect();
                if *array {
                    quote! { #private::array_value(::std::vec![#(#items),*]) }
                } else {
                    quote! { #(#items)* }
                }
            }
            Payload::Named(fields) => {
                let writes = write_fields(fields);
                quote! {{
                    let mut __fields = ::std::collections::BTreeMap::new();
                    #writes
                    #private::object_value(__fields)
                }}
            }
        }
    }

    /// The variant built from its fields' defaults, for content never stored.
    fn defaulted(&self) -> TokenStream {
        let ident = self.ident;
        match &self.payload {
            Payload::Unit { .. } => quote! { Self::#ident },
            Payload::Unnamed { types, .. } => {
                let defaults = types
                    .iter()
                    .map(|_| quote! { ::std::default::Default::default() });
                quote! { Self::#ident(#(#defaults),*) }
            }
            Payload::Named(fields) => {
                let members = fields.iter().map(|field| field.member);
                quote! { Self::#ident { #(#members: ::std::default::Default::default()),* } }
            }
        }
    }

    /// The variant read from the value in `__payload`.
    fn read_payload(&self, type_name: &str, deny: bool) -> TokenStream {
        let private = private();
        let ident = self.ident;
        let variant_name = format!("{type_name}::{}", self.name);
        match &self.payload {
            Payload::Unit {
                literal: Some(literal),
            } => {
                let value = literal_value(literal);
                let stored = literal.surql();
                quote! {
                    if __payload == #value {
                        Self::#ident
                    } else {
                        return ::std::result::Result::Err(#private::error(::std::format!(
                            "{} is stored as {}, got {}",
                            #variant_name,
                            #stored,
                            __payload.kind(),
                        )));
                    }
                }
            }
            Payload::Unit { literal: None } => quote! { Self::#ident },
            Payload::Unnamed {
                types,
                conversions,
                array: false,
            } => {
                let read = conversions[0].read(types[0], quote! { __payload });
                quote! { Self::#ident(#private::field(#read, #type_name, #variant_name)?) }
            }
            Payload::Unnamed {
                types,
                conversions,
                array: true,
            } => {
                let length = types.len();
                let items = types.iter().zip(conversions).enumerate().map(
                    |(position, (ty, conversion))| {
                        let key = position.to_string();
                        let read = conversion.read(
                            ty,
                            quote! {
                                __items
                                    .next()
                                    .ok_or_else(|| #private::missing(#variant_name, #key))?
                            },
                        );
                        quote! { #private::field(#read, #variant_name, #key)? }
                    },
                );
                quote! {{
                    let mut __items = #private::array(__payload, #variant_name, #length)?.into_iter();
                    Self::#ident(#(#items),*)
                }}
            }
            Payload::Named(fields) => {
                let reads = read_fields(fields, &variant_name, false);
                let deny = deny_unknown(deny, &variant_name);
                quote! {{
                    let mut __fields = #private::object(__payload, #variant_name)?;
                    let __read = Self::#ident { #(#reads),* };
                    #deny
                    __read
                }}
            }
        }
    }
}

pub(crate) fn enum_surreal_value(
    input: &DeriveInput,
    wire: &ItemWire,
    mode: Mode,
) -> syn::Result<TokenStream> {
    let syn::Data::Enum(data) = &input.data else {
        return Err(syn::Error::new_spanned(&input.ident, "expected an enum"));
    };
    let type_name = input.ident.to_string();
    // The enum's `#[surreal]` representation stores every variant, in place
    // of serde's.
    let stored = wire
        .variants
        .iter()
        .find_map(|variant| variant.wire.storage.representation.clone())
        .unwrap_or_else(|| wire.representation.clone());
    let variants = data
        .variants
        .iter()
        .zip(&wire.variants)
        .map(|(variant, variant_wire)| {
            let storage = &variant_wire.wire.storage;
            let conversion = |position: usize| match storage.opaque_elements.get(position) {
                Some(true) => Conversion::Serde,
                _ => Conversion::Own,
            };
            let payload = match &variant.fields {
                Fields::Unit => Payload::Unit {
                    literal: variant_wire.unit_value.clone().or_else(|| {
                        (storage.value.as_deref() == Some("NULL")).then_some(UnitValue::Null)
                    }),
                },
                Fields::Unnamed(unnamed) => Payload::Unnamed {
                    types: unnamed.unnamed.iter().map(|field| &field.ty).collect(),
                    conversions: (0..unnamed.unnamed.len()).map(conversion).collect(),
                    array: unnamed.unnamed.len() != 1 || storage.tuple,
                },
                Fields::Named(named) => Payload::Named(
                    named
                        .named
                        .iter()
                        .zip(variant_wire.fields.iter().zip(&variant_wire.handling))
                        .enumerate()
                        .map(|(position, (field, (field_wire, handling)))| {
                            let binding = format_ident!("__field{position}");
                            NamedField::new(
                                input,
                                field,
                                field_wire,
                                handling,
                                quote! { #binding },
                                mode,
                            )
                        })
                        .collect::<syn::Result<_>>()?,
                ),
            };
            let untagged = match &storage.representation {
                Some(representation) => matches!(representation, EnumRepresentation::Untagged),
                None => {
                    variant_wire.wire.serde_untagged
                        || matches!(wire.representation, EnumRepresentation::Untagged)
                }
            };
            Ok(EnumVariant {
                ident: &variant.ident,
                name: mode.key(&variant_wire.wire, &variant.ident),
                aliases: mode.aliases(&variant_wire.wire, &variant_wire.aliases),
                payload,
                untagged,
                skipped: storage.skipped,
                other: storage.other,
                content: storage.content,
                skip_content_if: variant_wire.skip_content_if.clone(),
            })
        })
        .collect::<syn::Result<Vec<_>>>()?;

    let stored_variants = || variants.iter().filter(|variant| !variant.skipped);
    let (untagged_variants, tagged_variants): (Vec<&EnumVariant>, Vec<&EnumVariant>) =
        stored_variants()
            .filter(|variant| !variant.other)
            .partition(|variant| variant.untagged);
    let fallback = stored_variants()
        .find(|variant| variant.other)
        .map(|variant| {
            let ident = variant.ident;
            quote! { Self::#ident }
        });
    for variant in &tagged_variants {
        if let (EnumRepresentation::InternallyTagged { .. }, Payload::Unnamed { array: true, .. }) =
            (&stored, &variant.payload)
        {
            return Err(syn::Error::new_spanned(
                variant.ident,
                "an internally tagged enum's variant of several elements cannot be stored: the \
                 tag sits in the payload's object, which an array is not, as serde refuses too",
            ));
        }
    }
    for (position, variant) in untagged_variants.iter().enumerate() {
        let key = untagged_kind_key(&variant.payload);
        if let Some(first) = untagged_variants[..position].iter().find(|first| {
            untagged_kind_key(&first.payload) == key
                && !objects_are_distinguishable(
                    &first.payload,
                    &variant.payload,
                    wire.deny_unknown_fields,
                )
        }) {
            return Err(syn::Error::new_spanned(
                variant.ident,
                format!(
                    "untagged variants `{}` and `{}` both read the `{key}` value kind; their database representation is ambiguous",
                    first.name, variant.name
                ),
            ));
        }
    }

    let deny = wire.deny_unknown_fields;
    // Only the last read falls back to the `other` variant.
    let tagged_fallback = untagged_variants
        .is_empty()
        .then_some(fallback.as_ref())
        .flatten();
    let tagged = match (&stored, tagged_variants.is_empty()) {
        (_, true) | (EnumRepresentation::Untagged, false) => None,
        (EnumRepresentation::ExternallyTagged, false) => Some(external(
            &tagged_variants,
            &type_name,
            deny,
            tagged_fallback,
        )),
        (EnumRepresentation::InternallyTagged { tag }, false) => Some(internal(
            &tagged_variants,
            &type_name,
            tag,
            deny,
            tagged_fallback,
        )),
        (EnumRepresentation::AdjacentlyTagged { tag, content }, false) => Some(adjacent(
            &tagged_variants,
            &type_name,
            tag,
            content,
            deny,
            tagged_fallback,
        )),
    };
    let untagged = (!untagged_variants.is_empty()).then(|| {
        untagged(
            &untagged_variants,
            &type_name,
            deny,
            tagged.is_some(),
            fallback.as_ref(),
        )
    });
    let private = private();
    let read = match (&tagged, &untagged) {
        (Some(tagged), None) => tagged.read.clone(),
        (None, Some(untagged)) => untagged.read.clone(),
        // serde reads the tagged variants first, then the untagged ones.
        (Some(tagged), Some(untagged)) => {
            let tagged_read = &tagged.read;
            let untagged_read = &untagged.read;
            quote! {
                let __tagged = |__value: #private::Value| -> ::std::result::Result<Self, #private::Error> {
                    ::std::result::Result::Ok({ #tagged_read })
                };
                match __tagged(__value.clone()) {
                    ::std::result::Result::Ok(__read) => __read,
                    ::std::result::Result::Err(__tagged_failure) => { #untagged_read }
                }
            }
        }
        (None, None) => match &fallback {
            Some(fallback) => quote! { #fallback },
            None => quote! {
                return ::std::result::Result::Err(#private::error(::std::format!(
                    "{} has no variants to read",
                    #type_name,
                )))
            },
        },
    };
    let (mut kinds, mut writes): (Vec<TokenStream>, Vec<TokenStream>) = tagged
        .into_iter()
        .chain(untagged)
        .map(|represented| (represented.kinds, represented.writes))
        .fold(
            (Vec::new(), Vec::new()),
            |(mut kinds, mut writes), (more_kinds, more_writes)| {
                kinds.extend(more_kinds);
                writes.extend(more_writes);
                (kinds, writes)
            },
        );
    // The fallback variant is written as its own name, as the enum's
    // representation writes a unit variant.
    if let Some(variant) = stored_variants().find(|variant| variant.other) {
        let wildcard = variant.wildcard();
        let name = &variant.name;
        let written = match &stored {
            EnumRepresentation::InternallyTagged { tag }
            | EnumRepresentation::AdjacentlyTagged { tag, .. } => quote! {
                #private::object_value(::std::collections::BTreeMap::from([
                    (#tag.to_owned(), #private::Value::String(#name.to_owned())),
                ]))
            },
            EnumRepresentation::ExternallyTagged | EnumRepresentation::Untagged => {
                quote! { #private::Value::String(#name.to_owned()) }
            }
        };
        writes.push(quote! { #wildcard => #written });
    }
    // A variant serde skips is stored as NONE, which reads as nothing.
    let skipped: Vec<&EnumVariant> = variants.iter().filter(|variant| variant.skipped).collect();
    for variant in &skipped {
        let wildcard = variant.wildcard();
        writes.push(quote! { #wildcard => #private::Value::None });
    }
    let never_read = (!skipped.is_empty()).then(|| {
        kinds.push(quote! { #private::Kind::None });
        quote! {
            if let #private::Value::None = __value {
                return ::std::result::Result::Err(#private::error(::std::format!(
                    "{} is stored as NONE, as a variant serde skips is written, and no variant \
                     reads it",
                    #type_name,
                )));
            }
        }
    });
    let kind = quote! { #private::Kind::Either(::std::vec![#(#kinds),*]) };
    let into_value = quote! { match self { #(#writes),* } };
    let finish = mode.finish(&type_name);
    let from_value = quote! {
        #never_read
        let __read: Self = { #read };
        #finish
    };
    let shims = variants.iter().flat_map(|variant| match &variant.payload {
        Payload::Named(fields) => fields
            .iter()
            .filter_map(|field| field.shim.clone())
            .collect::<Vec<_>>(),
        Payload::Unit { .. } | Payload::Unnamed { .. } => Vec::new(),
    });
    Ok(surreal_value_impl(
        input,
        kind,
        into_value,
        from_value,
        quote! { #(#shims)* },
    ))
}

/// The arm reading a name no variant has: the `other` variant, else an error.
fn unknown_variant_arm(type_name: &str, fallback: Option<&TokenStream>) -> TokenStream {
    let private = private();
    match fallback {
        Some(fallback) => quote! { _ => #fallback },
        None => quote! {
            __other => {
                return ::std::result::Result::Err(#private::unknown_variant(#type_name, __other))
            }
        },
    }
}

/// `"Unit"`, `{ Variant: payload }`.
fn external(
    variants: &[&EnumVariant],
    type_name: &str,
    deny: bool,
    fallback: Option<&TokenStream>,
) -> Represented {
    let private = private();
    let mut kinds = Vec::new();
    let mut writes = Vec::new();
    let mut unit_reads = Vec::new();
    let mut payload_reads = Vec::new();
    for variant in variants {
        let name = &variant.name;
        let pattern = variant.pattern();
        let read = variant.read_payload(type_name, deny);
        if let Payload::Unit { .. } = variant.payload {
            kinds.push(quote! {
                #private::Kind::Literal(#private::KindLiteral::String(#name.to_owned()))
            });
            writes.push(quote! { #pattern => #private::Value::String(#name.to_owned()) });
            let names = variant.names();
            unit_reads.push(quote! { #names => #read });
        } else {
            let payload = variant.payload_value();
            writes.push(quote! {
                #pattern => #private::object_value(::std::collections::BTreeMap::from([
                    (#name.to_owned(), #payload),
                ]))
            });
            let names = variant.names();
            payload_reads.push(quote! { #names => #read });
        }
    }
    if !payload_reads.is_empty() {
        kinds.push(quote! { #private::Kind::Object });
    }
    let unknown = unknown_variant_arm(type_name, fallback);
    let other_value = match fallback {
        Some(fallback) => quote! { _ => #fallback },
        None => quote! {
            __other => {
                return ::std::result::Result::Err(#private::error(::std::format!(
                    "{} must be a variant name or an object with one variant key, got {}",
                    #type_name,
                    __other.kind(),
                )))
            }
        },
    };
    let read = quote! {
        match __value {
            #private::Value::String(__name) => match __name.as_str() {
                #(#unit_reads,)*
                #unknown
            },
            #private::Value::Object(__object) => {
                let (__name, __payload) = #private::external(__object.into_inner(), #type_name)?;
                match __name.as_str() {
                    #(#payload_reads,)*
                    #unknown
                }
            }
            #other_value
        }
    };
    Represented {
        kinds,
        writes,
        read,
    }
}

/// `{ tag: "Variant", ...fields }`, a newtype variant's tag beside the fields
/// of the struct it holds.
fn internal(
    variants: &[&EnumVariant],
    type_name: &str,
    tag: &str,
    deny: bool,
    fallback: Option<&TokenStream>,
) -> Represented {
    let private = private();
    let mut writes = Vec::new();
    let mut reads = Vec::new();
    let mut kinds = Vec::new();
    for variant in variants {
        let name = &variant.name;
        let pattern = variant.pattern();
        let ident = variant.ident;
        let variant_name = format!("{type_name}::{name}");
        let (write_payload, construct, field_kinds) = match &variant.payload {
            Payload::Named(fields) => {
                let field_writes = write_fields(fields);
                let field_reads = read_fields(fields, &variant_name, false);
                let field_kinds = fields
                    .iter()
                    .filter(|field| field.written)
                    .map(|field| {
                        let key = &field.key;
                        let ty = field.ty;
                        quote! { (#key.to_owned(), (&#private::Field::<#ty>::new()).kind()) }
                    })
                    .collect::<Vec<_>>();
                (
                    field_writes,
                    quote! { Self::#ident { #(#field_reads),* } },
                    quote! { #private::Kind::Literal(#private::KindLiteral::Object(::std::collections::BTreeMap::from([
                        (#tag.to_owned(), #private::Kind::Literal(#private::KindLiteral::String(#name.to_owned()))),
                        #(#field_kinds,)*
                    ]))) },
                )
            }
            Payload::Unnamed {
                types, conversions, ..
            } => {
                let payload = types[0];
                let written = conversions[0].write(payload, quote! { __field0 });
                let read = conversions[0].read(payload, quote! { #private::rest(&__fields) });
                let stored_object = quote_spanned! {payload.span()=>
                    #private::stored_object::<#payload>();
                };
                (
                    quote! {
                        #stored_object
                        #private::merge(&mut __fields, #name, #written);
                    },
                    quote! { Self::#ident(#private::field(#read, #type_name, #variant_name)?) },
                    quote! { #private::Kind::Object },
                )
            }
            Payload::Unit { .. } => (
                TokenStream::new(),
                quote! { Self::#ident },
                quote! { #private::Kind::Literal(#private::KindLiteral::Object(::std::collections::BTreeMap::from([
                    (#tag.to_owned(), #private::Kind::Literal(#private::KindLiteral::String(#name.to_owned()))),
                ]))) },
            ),
        };
        kinds.push(field_kinds);
        writes.push(quote! {
            #pattern => {
                let mut __fields = ::std::collections::BTreeMap::new();
                __fields.insert(#tag.to_owned(), #private::Value::String(#name.to_owned()));
                #write_payload
                #private::object_value(__fields)
            }
        });
        // A newtype variant's struct reads every key beside the tag.
        let deny = match variant.payload {
            Payload::Unnamed { .. } => TokenStream::new(),
            Payload::Named(_) | Payload::Unit { .. } => deny_unknown(deny, &variant_name),
        };
        let names = variant.names();
        reads.push(quote! {
            #names => {
                let __read = #construct;
                #deny
                __read
            }
        });
    }
    let unknown = unknown_variant_arm(type_name, fallback);
    let read = quote! {
        let mut __fields = #private::object(__value, #type_name)?;
        let __name = #private::tag(&mut __fields, #type_name, #tag)?;
        match __name.as_str() {
            #(#reads,)*
            #unknown
        }
    };
    Represented {
        kinds,
        writes,
        read,
    }
}

/// `{ tag: "Variant", content: payload }`, the content left out where
/// `#[surreal(skip_content)]` says so.
fn adjacent(
    variants: &[&EnumVariant],
    type_name: &str,
    tag: &str,
    content: &str,
    deny: bool,
    fallback: Option<&TokenStream>,
) -> Represented {
    let private = private();
    let mut writes = Vec::new();
    let mut reads = Vec::new();
    for variant in variants {
        let name = &variant.name;
        let pattern = variant.pattern();
        let content_write = match (&variant.payload, variant.content) {
            (Payload::Unit { .. }, _) | (_, ContentStorage::Never) => TokenStream::new(),
            (_, ContentStorage::Sometimes) => {
                let payload = variant.payload_value();
                let predicate = &variant.skip_content_if;
                quote! {
                    let __content_value = #payload;
                    if !#predicate(&__content_value) {
                        __fields.insert(#content.to_owned(), __content_value);
                    }
                }
            }
            (_, ContentStorage::Always) => {
                let payload = variant.payload_value();
                quote! { __fields.insert(#content.to_owned(), #payload); }
            }
        };
        // A variant whose content is never written binds nothing.
        let pattern = match (&variant.payload, variant.content) {
            (_, ContentStorage::Never) => variant.wildcard(),
            _ => pattern,
        };
        writes.push(quote! {
            #pattern => {
                let mut __fields = ::std::collections::BTreeMap::new();
                __fields.insert(#tag.to_owned(), #private::Value::String(#name.to_owned()));
                #content_write
                #private::object_value(__fields)
            }
        });
        let read = variant.read_payload(type_name, deny);
        let variant_read = match (&variant.payload, variant.content) {
            (Payload::Unit { .. }, _) => read,
            (_, ContentStorage::Never) => variant.defaulted(),
            (_, ContentStorage::Sometimes) => {
                let defaulted = variant.defaulted();
                quote! {
                    match __content {
                        ::std::option::Option::Some(__payload) => #read,
                        ::std::option::Option::None => #defaulted,
                    }
                }
            }
            (_, ContentStorage::Always) => quote! {{
                let __payload = __content.ok_or_else(|| #private::missing(#type_name, #content))?;
                #read
            }},
        };
        let names = variant.names();
        reads.push(quote! { #names => #variant_read });
    }
    let unknown = unknown_variant_arm(type_name, fallback);
    let deny_rest = deny_unknown(deny, type_name);
    let kinds = variants
        .iter()
        .map(|variant| {
            let name = &variant.name;
            let content_kind = match (&variant.payload, variant.content) {
                (Payload::Unit { .. }, _) | (_, ContentStorage::Never) => TokenStream::new(),
                (_, ContentStorage::Sometimes) => {
                    let payload_kind = payload_kind(&variant.payload);
                    quote! {
                        (#content.to_owned(), #private::Kind::option(#payload_kind)),
                    }
                }
                (_, ContentStorage::Always) => {
                    let payload_kind = payload_kind(&variant.payload);
                    quote! {
                        (#content.to_owned(), #payload_kind),
                    }
                }
            };
            quote! {
                #private::Kind::Literal(#private::KindLiteral::Object(::std::collections::BTreeMap::from([
                    (#tag.to_owned(), #private::Kind::Literal(#private::KindLiteral::String(#name.to_owned()))),
                    #content_kind
                ])))
            }
        })
        .collect();
    let read = quote! {
        let mut __fields = #private::object(__value, #type_name)?;
        let __name = #private::tag(&mut __fields, #type_name, #tag)?;
        let __content = __fields.remove(#content);
        #deny_rest
        match __name.as_str() {
            #(#reads,)*
            #unknown
        }
    };
    Represented {
        kinds,
        writes,
        read,
    }
}

/// The payload alone; a read takes the first variant that reads it, as
/// serde's does, then the `other` variant. After tagged variants, the tagged
/// read's failure, in `__tagged_failure`, is reported with the others.
fn untagged(
    variants: &[&EnumVariant],
    type_name: &str,
    deny: bool,
    after_tagged: bool,
    fallback: Option<&TokenStream>,
) -> Represented {
    let private = private();
    let writes = variants
        .iter()
        .map(|variant| {
            let pattern = variant.pattern();
            let payload = variant.payload_value();
            quote! { #pattern => #payload }
        })
        .collect();
    let attempts = variants.iter().map(|variant| {
        let read = variant.read_payload(type_name, deny);
        quote! {
            |__payload: #private::Value| -> ::std::result::Result<Self, #private::Error> {
                ::std::result::Result::Ok(#read)
            }
        }
    });
    let count = variants.len();
    let kinds = variants
        .iter()
        .map(|variant| payload_kind(&variant.payload))
        .collect();
    let tagged_failure =
        after_tagged.then(|| quote! { __failures.push(__tagged_failure.to_string()); });
    let none_read = match fallback {
        Some(fallback) => quote! { #fallback },
        None => quote! {
            return ::std::result::Result::Err(#private::error(::std::format!(
                "{} matches none of its variants: {}",
                #type_name,
                __failures.join("; "),
            )))
        },
    };
    let read = quote! {
        let __attempts: [fn(#private::Value) -> ::std::result::Result<Self, #private::Error>; #count] =
            [#(#attempts),*];
        let mut __failures = ::std::vec::Vec::new();
        #tagged_failure
        let mut __found = ::std::option::Option::None;
        for __attempt in __attempts {
            match __attempt(__value.clone()) {
                ::std::result::Result::Ok(__read) => {
                    __found = ::std::option::Option::Some(__read);
                    break;
                }
                ::std::result::Result::Err(__failure) => __failures.push(__failure.to_string()),
            }
        }
        match __found {
            ::std::option::Option::Some(__read) => __read,
            ::std::option::Option::None => #none_read,
        }
    };
    Represented {
        kinds,
        writes,
        read,
    }
}

/// One table of an `EvenframeUnion`: its variant, how the variant holds the
/// record, the record's type and its table.
pub(crate) struct UnionTable<'a> {
    pub variant: &'a syn::Ident,
    pub member: Option<&'a syn::Ident>,
    pub ty: &'a Type,
    pub table: String,
}

/// A union of tables stores the record of one of them, so a read picks the
/// variant by the table its `id` names.
pub(crate) fn union_surreal_value(input: &DeriveInput, tables: &[UnionTable]) -> TokenStream {
    let private = private();
    let type_name = input.ident.to_string();
    let writes = tables.iter().map(|table| {
        let variant = table.variant;
        let ty = table.ty;
        let pattern = match table.member {
            Some(member) => quote! { Self::#variant { #member: __record } },
            None => quote! { Self::#variant(__record) },
        };
        quote! { #pattern => (&#private::Field::<#ty>::new()).write(__record) }
    });
    let reads = tables.iter().map(|table| {
        let variant = table.variant;
        let ty = table.ty;
        let name = &table.table;
        let record = quote! {
            #private::field(
                (&#private::Field::<#ty>::new()).read(__value),
                #type_name,
                #name,
            )?
        };
        let construct = match table.member {
            Some(member) => quote! { Self::#variant { #member: #record } },
            None => quote! { Self::#variant(#record) },
        };
        quote! { #name => #construct }
    });
    let kinds = tables.iter().map(|table| {
        let name = &table.table;
        quote! { #private::Table::new(#name) }
    });
    let unknown = unknown_variant_arm(&type_name, None);
    let finish = Mode::Stored.finish(&type_name);
    let into_value = quote! { match self { #(#writes),* } };
    let from_value = quote! {
        let __table = #private::record_table(&__value, #type_name)?;
        let __read: Self = match __table.as_str() {
            #(#reads,)*
            #unknown
        };
        #finish
    };
    surreal_value_impl(
        input,
        quote! { #private::Kind::Record(::std::vec![#(#kinds),*]) },
        into_value,
        from_value,
        TokenStream::new(),
    )
}
