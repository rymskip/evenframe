//! The generated `SurrealValue`: a value in the shape evenframe's schema
//! defines for its type. Fields sit under their database names, enums keep
//! serde's representation, and a field missing from the record takes serde's
//! default. A read is validated as serde's read is.

use crate::validate_impl::{CheckedField, is_option_type};
use evenframe_core::derive::naming::{FieldDefault, FieldRead, ItemWire, unraw};
use evenframe_core::types::{EnumRepresentation, Wire};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::{DeriveInput, Fields, GenericArgument, PathArguments, Type};

fn reject_custom_deserializer(field: &syn::Field) -> syn::Result<()> {
    for attribute in field
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("serde"))
    {
        let entries = attribute.parse_args_with(
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
        )?;
        if let Some(entry) = entries.iter().find(|entry| {
            entry.path().is_ident("with") || entry.path().is_ident("deserialize_with")
        }) {
            return Err(syn::Error::new_spanned(
                entry,
                "a custom serde deserializer has no SurrealValue equivalent; implement SurrealValue for a field wrapper instead",
            ));
        }
    }
    Ok(())
}

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

/// One named field as the impl writes and reads it.
struct NamedField<'a> {
    member: &'a syn::Ident,
    binding: TokenStream,
    ty: &'a Type,
    key: String,
    skipped: bool,
    default: FieldDefault,
    /// The field's own fields sit beside its siblings in the object.
    flatten: bool,
    /// A row's text field, which reads any value as its JSON text.
    reads_text: bool,
    /// The other names the field is read by.
    aliases: Vec<String>,
}

impl<'a> NamedField<'a> {
    fn is_required(&self) -> bool {
        !self.skipped
            && matches!(self.default, FieldDefault::None)
            && option_inner(self.ty).is_none()
    }

    fn new(
        field: &'a syn::Field,
        wire: &Wire,
        read: &FieldRead,
        binding: TokenStream,
        mode: Mode,
    ) -> syn::Result<Self> {
        reject_custom_deserializer(field)?;
        let member = field
            .ident
            .as_ref()
            .ok_or_else(|| syn::Error::new_spanned(field, "a named field has no identifier"))?;
        Ok(Self {
            member,
            binding,
            ty: &field.ty,
            key: mode.key(wire, member),
            skipped: wire.serde_skipped,
            default: read.default.clone(),
            flatten: read.flatten,
            reads_text: matches!(mode, Mode::Row) && text_reader(&field.ty).is_some(),
            aliases: mode.aliases(wire, &read.aliases),
        })
    }
}

/// Statements inserting each field's value into the map `__fields`.
fn write_fields(fields: &[NamedField]) -> TokenStream {
    let private = private();
    fields
        .iter()
        .filter(|field| !field.skipped)
        .map(|field| {
            let key = &field.key;
            let binding = &field.binding;
            if field.flatten {
                let ty = field.ty;
                return quote! {
                    #private::merge(
                        &mut __fields,
                        #key,
                        (&#private::Field::<#ty>::new()).write(#binding),
                    );
                };
            }
            match option_inner(field.ty) {
                Some(inner) => quote! {
                    __fields.insert(
                        #key.to_owned(),
                        match #binding {
                            ::std::option::Option::Some(__inner) =>
                                (&#private::Field::<#inner>::new()).write(__inner),
                            ::std::option::Option::None => #private::Value::None,
                        },
                    );
                },
                None => {
                    let ty = field.ty;
                    quote! {
                        __fields.insert(
                            #key.to_owned(),
                            (&#private::Field::<#ty>::new()).write(#binding),
                        );
                    }
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
        FieldDefault::None if field.skipped || option_inner(field.ty).is_some() => {
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
    let (flattened, own): (Vec<&NamedField>, Vec<&NamedField>) = fields
        .iter()
        .partition(|field| field.flatten && !field.skipped);
    own.into_iter()
        .chain(flattened)
        .map(|field| {
            let member = field.member;
            let missing = missing_value(field, type_name, container_default);
            if field.skipped {
                return quote! { #member: #missing };
            }
            let key = &field.key;
            if field.flatten {
                let ty = field.ty;
                return quote! {
                    #member: #private::field(
                        (&#private::Field::<#ty>::new()).read(#private::rest(&__fields)),
                        #type_name,
                        #key,
                    )?
                };
            }
            let read = |ty: &Type| {
                let converted = match text_reader(ty).filter(|_| field.reads_text) {
                    Some(reader) => quote! { #private::#reader(__present) },
                    None => quote! { (&#private::Field::<#ty>::new()).read(__present) },
                };
                quote! { #private::field(#converted, #type_name, #key)? }
            };
            let present = match option_inner(field.ty) {
                Some(inner) => {
                    let read = read(inner);
                    quote! { ::std::option::Option::Some(#read) }
                }
                None => read(field.ty),
            };
            let aliases = &field.aliases;
            quote! {
                #member: match #private::present(&mut __fields, &[#key #(, #aliases)*]) {
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
/// `SurrealValue` so its fields convert with the parameter's own impl.
fn surreal_value_impl(
    input: &DeriveInput,
    kind: TokenStream,
    into_value: TokenStream,
    from_value: TokenStream,
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
        return Ok(tuple_struct_surreal_value(input, unnamed, &type_name, mode));
    }
    if let Fields::Unit = &data.fields {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "SurrealValue cannot be derived for a unit struct, which holds no value",
        ));
    }
    let fields = data
        .fields
        .iter()
        .zip(wire.fields.iter().zip(&wire.reads))
        .map(|(field, (field_wire, read))| {
            let member = field.ident.as_ref();
            NamedField::new(field, field_wire, read, quote! { self.#member }, mode)
        })
        .collect::<syn::Result<Vec<_>>>()?;

    let writes = write_fields(&fields);
    let into_value = quote! {
        let mut __fields = ::std::collections::BTreeMap::new();
        #writes
        #private::object_value(__fields)
    };

    let (container_default, default_binding) = match &wire.container_default {
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
    Ok(surreal_value_impl(
        input,
        quote! { #private::Kind::Object },
        into_value,
        from_value,
    ))
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
    let mut parsed = Vec::new();
    let mut checks_anything = false;

    for (field, checked) in fields.iter().zip(checked_fields) {
        let member = field.member;
        let local = format_ident!("__field_{}", unraw(member));
        let missing = missing_value(field, type_name, container_default);
        let key = &field.key;
        let aliases = &field.aliases;
        let optional = option_inner(field.ty).is_some();
        let parse = checked.validators.parse;
        let transforms = checked.transforms()?;
        let mutability = transforms.then(|| quote! { mut });

        let present = if parse.is_some() {
            let parsed_text =
                quote! { #private::field(#private::parse_text(__present), #type_name, #key)? };
            if optional {
                quote! { ::std::option::Option::Some(#parsed_text) }
            } else {
                parsed_text
            }
        } else {
            let read = |ty: &Type| {
                let converted = quote! { (&#private::Field::<#ty>::new()).read(__present) };
                quote! { #private::field(#converted, #type_name, #key)? }
            };
            match option_inner(field.ty) {
                Some(inner) => {
                    let read = read(inner);
                    quote! { ::std::option::Option::Some(#read) }
                }
                None => read(field.ty),
            }
        };
        let raw = if field.skipped {
            missing
        } else if field.flatten {
            let ty = field.ty;
            quote! {
                #private::field(
                    (&#private::Field::<#ty>::new()).read(#private::rest(&__fields)),
                    #type_name,
                    #key,
                )?
            }
        } else {
            quote! {
                match #private::present(&mut __fields, &[#key #(, #aliases)*]) {
                    ::std::option::Option::Some(__present) => #present,
                    ::std::option::Option::None => #missing,
                }
            }
        };

        if let Some(parse) = parse {
            checks_anything = true;
            let path = &checked.path;
            let function = format_ident!("{}", parse.runtime_function());
            let parse_call = if optional {
                quote! {
                    (#raw).as_deref()
                        .map(::evenframe::validator::runtime::#function)
                        .transpose()
                }
            } else {
                quote! { ::evenframe::validator::runtime::#function(&(#raw)) }
            };
            bindings.push(quote! {
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
        } else {
            bindings.push(quote! { let #mutability #local = #raw; });
            assignments.push(quote! { #member: #local });
        }

        let chain = checked.chain(&quote! { (*__value) }, true)?;
        let direct_chain = checked.chain(&quote! { #local }, true)?;
        if !chain.is_empty() {
            checks_anything = true;
            let pattern = match (parse.is_some(), optional) {
                (true, true) => Some(quote! {
                    ::std::option::Option::Some(::std::option::Option::Some(__value))
                }),
                (true, false) | (false, true) => {
                    Some(quote! { ::std::option::Option::Some(__value) })
                }
                (false, false) => None,
            };
            let borrow = if transforms {
                quote! { &mut }
            } else {
                quote! { & }
            };
            pipelines.push(match pattern {
                Some(pattern) => quote! { if let #pattern = #borrow #local { #chain } },
                None => direct_chain,
            });
        }
    }

    let errors = checks_anything.then(|| {
        quote! {
            let mut __errors =
                ::evenframe::validator::validate::ValidationErrors::new();
        }
    });
    let build = if parsed.is_empty() {
        quote! { Self { #(#assignments),* } }
    } else {
        let locals = parsed.iter().map(|(local, _)| local);
        let patterns = parsed
            .iter()
            .map(|(_, value)| quote! { ::std::option::Option::Some(#value) });
        quote! {
            match (#(#locals,)*) {
                (#(#patterns,)*) => Self { #(#assignments),* },
                _ => return ::std::result::Result::Err(#private::error(
                    ::std::format!("{} did not parse every field", #type_name),
                )),
            }
        }
    };
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

/// A tuple struct as serde writes it: a newtype as its value, more fields as
/// an array.
fn tuple_struct_surreal_value(
    input: &DeriveInput,
    unnamed: &syn::FieldsUnnamed,
    type_name: &str,
    mode: Mode,
) -> TokenStream {
    let private = private();
    let types: Vec<&Type> = unnamed.unnamed.iter().map(|field| &field.ty).collect();
    let finish = mode.finish(type_name);
    let (kind, into_value, read) = match types.as_slice() {
        [ty] => (
            quote! { (&#private::Field::<#ty>::new()).kind() },
            quote! { (&#private::Field::<#ty>::new()).write(self.0) },
            quote! {
                Self(#private::field(
                    (&#private::Field::<#ty>::new()).read(__value),
                    #type_name,
                    "0",
                )?)
            },
        ),
        _ => {
            let length = types.len();
            let writes = types.iter().enumerate().map(|(position, ty)| {
                let member = syn::Index::from(position);
                quote! { (&#private::Field::<#ty>::new()).write(self.#member) }
            });
            let reads = types.iter().enumerate().map(|(position, ty)| {
                let key = position.to_string();
                quote! {
                    #private::field(
                        (&#private::Field::<#ty>::new()).read(
                            __items.next().ok_or_else(|| #private::missing(#type_name, #key))?,
                        ),
                        #type_name,
                        #key,
                    )?
                }
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
    surreal_value_impl(input, kind, into_value, from_value)
}

/// One variant's payload: how it is written and read.
enum Payload<'a> {
    Unit,
    Unnamed(Vec<&'a Type>),
    Named(Vec<NamedField<'a>>),
}

struct EnumVariant<'a> {
    ident: &'a syn::Ident,
    name: String,
    /// The other names the variant is read by.
    aliases: Vec<String>,
    payload: Payload<'a>,
}

fn payload_kind(payload: &Payload) -> TokenStream {
    let private = private();
    match payload {
        Payload::Unit => quote! { #private::Kind::None },
        Payload::Unnamed(types) if types.len() == 1 => {
            let ty = types[0];
            quote! { (&#private::Field::<#ty>::new()).kind() }
        }
        Payload::Unnamed(_) => quote! {
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
    if reader
        .iter()
        .chain(writer)
        .any(|field| !field.skipped && field.flatten)
    {
        return true;
    }
    let required_present = reader
        .iter()
        .filter(|field| field.is_required())
        .all(|field| {
            writer
                .iter()
                .filter(|written| !written.skipped)
                .any(|written| field.key == written.key || field.aliases.contains(&written.key))
        });
    let unknown_accepted = !deny_unknown_fields
        || writer
            .iter()
            .filter(|field| field.is_required())
            .all(|written| {
                reader
                    .iter()
                    .filter(|field| !field.skipped)
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

fn untagged_kind_key(payload: &Payload) -> String {
    let Payload::Unnamed(types) = payload else {
        return match payload {
            Payload::Unit => "none",
            Payload::Named(_) => "object",
            Payload::Unnamed(_) => unreachable!(),
        }
        .to_owned();
    };
    if types.len() != 1 {
        return "array".to_owned();
    }
    let Type::Path(path) = types[0] else {
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
            Payload::Unit => quote! { Self::#ident },
            Payload::Unnamed(types) => {
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

    /// The payload's value from the pattern's bindings.
    fn payload_value(&self) -> TokenStream {
        let private = private();
        match &self.payload {
            Payload::Unit => quote! { #private::Value::None },
            Payload::Unnamed(types) if types.len() == 1 => {
                let ty = types[0];
                quote! { (&#private::Field::<#ty>::new()).write(__field0) }
            }
            Payload::Unnamed(types) => {
                let items = types.iter().enumerate().map(|(position, ty)| {
                    let binding = format_ident!("__field{position}");
                    quote! { (&#private::Field::<#ty>::new()).write(#binding) }
                });
                quote! { #private::array_value(::std::vec![#(#items),*]) }
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

    /// The variant read from the value in `__payload`.
    fn read_payload(&self, type_name: &str, deny: bool) -> TokenStream {
        let private = private();
        let ident = self.ident;
        let variant_name = format!("{type_name}::{}", self.name);
        match &self.payload {
            Payload::Unit => quote! { Self::#ident },
            Payload::Unnamed(types) if types.len() == 1 => {
                let ty = types[0];
                quote! {
                    Self::#ident(#private::field(
                        (&#private::Field::<#ty>::new()).read(__payload),
                        #type_name,
                        #variant_name,
                    )?)
                }
            }
            Payload::Unnamed(types) => {
                let length = types.len();
                let items = types.iter().enumerate().map(|(position, ty)| {
                    let key = position.to_string();
                    quote! {
                        #private::field(
                            (&#private::Field::<#ty>::new()).read(
                                __items
                                    .next()
                                    .ok_or_else(|| #private::missing(#variant_name, #key))?,
                            ),
                            #variant_name,
                            #key,
                        )?
                    }
                });
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
    let variants = data
        .variants
        .iter()
        .zip(&wire.variants)
        .map(|(variant, variant_wire)| {
            if variant_wire.wire.serde_skipped {
                return Err(syn::Error::new_spanned(
                    variant,
                    "#[serde(skip)] on a variant is not supported: SurrealValue writes every \
                     variant",
                ));
            }
            let payload = match &variant.fields {
                Fields::Unit => Payload::Unit,
                Fields::Unnamed(unnamed) => {
                    Payload::Unnamed(unnamed.unnamed.iter().map(|field| &field.ty).collect())
                }
                Fields::Named(named) => Payload::Named(
                    named
                        .named
                        .iter()
                        .zip(variant_wire.fields.iter().zip(&variant_wire.reads))
                        .enumerate()
                        .map(|(position, (field, (field_wire, read)))| {
                            let binding = format_ident!("__field{position}");
                            NamedField::new(field, field_wire, read, quote! { #binding }, mode)
                        })
                        .collect::<syn::Result<_>>()?,
                ),
            };
            Ok(EnumVariant {
                ident: &variant.ident,
                name: mode.key(&variant_wire.wire, &variant.ident),
                aliases: mode.aliases(&variant_wire.wire, &variant_wire.aliases),
                payload,
            })
        })
        .collect::<syn::Result<Vec<_>>>()?;

    for variant in &variants {
        let rejection = match (&wire.representation, &variant.payload) {
            (EnumRepresentation::InternallyTagged { .. }, Payload::Unnamed(_)) => Some(
                "an internally tagged enum's tuple variant is not supported: serde merges its \
                 value into the tag's object, which evenframe's schema does not describe",
            ),
            (EnumRepresentation::Untagged, Payload::Unit) => Some(
                "an untagged enum's unit variant is not supported: serde writes it as null, \
                 which evenframe's schema stores as the variant's name",
            ),
            _ => None,
        };
        if let Some(message) = rejection {
            return Err(syn::Error::new_spanned(variant.ident, message));
        }
    }

    if matches!(wire.representation, EnumRepresentation::Untagged) {
        for (position, variant) in variants.iter().enumerate() {
            let key = untagged_kind_key(&variant.payload);
            if let Some(first) = variants[..position].iter().find(|first| {
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
    }

    let (kind, into_value, from_value) = match &wire.representation {
        EnumRepresentation::ExternallyTagged => {
            external(&variants, &type_name, wire.deny_unknown_fields)
        }
        EnumRepresentation::InternallyTagged { tag } => {
            internal(&variants, &type_name, tag, wire.deny_unknown_fields)
        }
        EnumRepresentation::AdjacentlyTagged { tag, content } => adjacent(
            &variants,
            &type_name,
            tag,
            content,
            wire.deny_unknown_fields,
        ),
        EnumRepresentation::Untagged => untagged(&variants, &type_name, wire.deny_unknown_fields),
    };
    let finish = mode.finish(&type_name);
    let from_value = quote! {
        let __read: Self = { #from_value };
        #finish
    };
    Ok(surreal_value_impl(input, kind, into_value, from_value))
}

fn unknown_variant_arm(type_name: &str) -> TokenStream {
    let private = private();
    quote! {
        __other => {
            return ::std::result::Result::Err(#private::unknown_variant(#type_name, __other))
        }
    }
}

/// `"Unit"`, `{ Variant: payload }`.
fn external(
    variants: &[EnumVariant],
    type_name: &str,
    deny: bool,
) -> (TokenStream, TokenStream, TokenStream) {
    let private = private();
    let mut kinds = Vec::new();
    let mut writes = Vec::new();
    let mut unit_reads = Vec::new();
    let mut payload_reads = Vec::new();
    for variant in variants {
        let name = &variant.name;
        let pattern = variant.pattern();
        let read = variant.read_payload(type_name, deny);
        if let Payload::Unit = variant.payload {
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
    let unknown = unknown_variant_arm(type_name);
    let kind = quote! { #private::Kind::Either(::std::vec![#(#kinds),*]) };
    let into_value = quote! { match self { #(#writes),* } };
    let from_value = quote! {
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
            __other => {
                return ::std::result::Result::Err(#private::error(::std::format!(
                    "{} must be a variant name or an object with one variant key, got {}",
                    #type_name,
                    __other.kind(),
                )))
            }
        }
    };
    (kind, into_value, from_value)
}

/// `{ tag: "Variant", ...fields }`.
fn internal(
    variants: &[EnumVariant],
    type_name: &str,
    tag: &str,
    deny: bool,
) -> (TokenStream, TokenStream, TokenStream) {
    let private = private();
    let mut writes = Vec::new();
    let mut reads = Vec::new();
    for variant in variants {
        let name = &variant.name;
        let pattern = variant.pattern();
        let fields = match &variant.payload {
            Payload::Named(fields) => fields.as_slice(),
            Payload::Unit | Payload::Unnamed(_) => &[],
        };
        let field_writes = write_fields(fields);
        writes.push(quote! {
            #pattern => {
                let mut __fields = ::std::collections::BTreeMap::new();
                __fields.insert(#tag.to_owned(), #private::Value::String(#name.to_owned()));
                #field_writes
                #private::object_value(__fields)
            }
        });
        let ident = variant.ident;
        let variant_name = format!("{type_name}::{name}");
        let field_reads = read_fields(fields, &variant_name, false);
        let deny = deny_unknown(deny, &variant_name);
        let construct = match variant.payload {
            Payload::Unit => quote! { Self::#ident },
            _ => quote! { Self::#ident { #(#field_reads),* } },
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
    let unknown = unknown_variant_arm(type_name);
    let kinds = variants.iter().map(|variant| {
        let name = &variant.name;
        let field_kinds = match &variant.payload {
            Payload::Named(fields) => fields
                .iter()
                .filter(|field| !field.skipped)
                .map(|field| {
                    let key = &field.key;
                    let ty = field.ty;
                    quote! { (#key.to_owned(), (&#private::Field::<#ty>::new()).kind()) }
                })
                .collect::<Vec<_>>(),
            Payload::Unit | Payload::Unnamed(_) => Vec::new(),
        };
        quote! {
            #private::Kind::Literal(#private::KindLiteral::Object(::std::collections::BTreeMap::from([
                (#tag.to_owned(), #private::Kind::Literal(#private::KindLiteral::String(#name.to_owned()))),
                #(#field_kinds,)*
            ])))
        }
    });
    let into_value = quote! { match self { #(#writes),* } };
    let from_value = quote! {
        let mut __fields = #private::object(__value, #type_name)?;
        let __name = #private::tag(&mut __fields, #type_name, #tag)?;
        match __name.as_str() {
            #(#reads,)*
            #unknown
        }
    };
    (
        quote! { #private::Kind::Either(::std::vec![#(#kinds),*]) },
        into_value,
        from_value,
    )
}

/// `{ tag: "Variant", content: payload }`.
fn adjacent(
    variants: &[EnumVariant],
    type_name: &str,
    tag: &str,
    content: &str,
    deny: bool,
) -> (TokenStream, TokenStream, TokenStream) {
    let private = private();
    let mut writes = Vec::new();
    let mut reads = Vec::new();
    for variant in variants {
        let name = &variant.name;
        let pattern = variant.pattern();
        let content_write = match variant.payload {
            Payload::Unit => TokenStream::new(),
            _ => {
                let payload = variant.payload_value();
                quote! { __fields.insert(#content.to_owned(), #payload); }
            }
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
        let variant_read = match variant.payload {
            Payload::Unit => read,
            _ => quote! {{
                let __payload = __content.ok_or_else(|| #private::missing(#type_name, #content))?;
                #read
            }},
        };
        let names = variant.names();
        reads.push(quote! { #names => #variant_read });
    }
    let unknown = unknown_variant_arm(type_name);
    let deny_rest = deny_unknown(deny, type_name);
    let kinds = variants.iter().map(|variant| {
        let name = &variant.name;
        let content_kind = match variant.payload {
            Payload::Unit => TokenStream::new(),
            _ => {
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
    });
    let into_value = quote! { match self { #(#writes),* } };
    let from_value = quote! {
        let mut __fields = #private::object(__value, #type_name)?;
        let __name = #private::tag(&mut __fields, #type_name, #tag)?;
        let __content = __fields.remove(#content);
        #deny_rest
        match __name.as_str() {
            #(#reads,)*
            #unknown
        }
    };
    (
        quote! { #private::Kind::Either(::std::vec![#(#kinds),*]) },
        into_value,
        from_value,
    )
}

/// The payload alone; a read takes the first variant that reads it, as
/// serde's does.
fn untagged(
    variants: &[EnumVariant],
    type_name: &str,
    deny: bool,
) -> (TokenStream, TokenStream, TokenStream) {
    let private = private();
    let writes = variants.iter().map(|variant| {
        let pattern = variant.pattern();
        let payload = variant.payload_value();
        quote! { #pattern => #payload }
    });
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
        .map(|variant| payload_kind(&variant.payload));
    let into_value = quote! { match self { #(#writes),* } };
    let from_value = quote! {
        let __attempts: [fn(#private::Value) -> ::std::result::Result<Self, #private::Error>; #count] =
            [#(#attempts),*];
        let mut __failures = ::std::vec::Vec::new();
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
            ::std::option::Option::None => {
                return ::std::result::Result::Err(#private::error(::std::format!(
                    "{} matches none of its variants: {}",
                    #type_name,
                    __failures.join("; "),
                )))
            }
        }
    };
    (
        quote! { #private::Kind::Either(::std::vec![#(#kinds),*]) },
        into_value,
        from_value,
    )
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
    let unknown = unknown_variant_arm(&type_name);
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
    )
}
