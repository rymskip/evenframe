use core::fmt;
use quote::{ToTokens, quote};
use serde::{Deserialize, Serialize};
use syn::Type as SynType;

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FieldType {
    String,
    Char,
    Bool,
    #[default]
    Unit,
    F32,
    F64,
    I8,
    I16,
    I32,
    I64,
    I128,
    Isize,
    U8,
    U16,
    U32,
    U64,
    U128,
    Usize,
    /// `std::time::Duration`.
    Duration,
    Tuple(Vec<FieldType>),
    Struct(Vec<(String, FieldType)>),
    Option(Box<FieldType>),
    Vec(Box<FieldType>),
    HashMap(Box<FieldType>, Box<FieldType>),
    BTreeMap(Box<FieldType>, Box<FieldType>),
    RecordLink(Box<FieldType>),
    Other(String),
}

/// The paths that name `std::time::Duration` without any import.
pub const STD_DURATION_PATHS: [&str; 2] = ["std::time::Duration", "core::time::Duration"];

impl FieldType {
    /// The shape serde writes a `std::time::Duration` in: its whole seconds
    /// and the nanoseconds past them.
    pub fn serde_duration() -> FieldType {
        FieldType::Struct(vec![
            ("secs".to_string(), FieldType::U64),
            ("nanos".to_string(), FieldType::U32),
        ])
    }

    /// True when the field stores a number (float or integer), looking
    /// through `Option` layers.
    pub fn is_numeric(&self) -> bool {
        match self {
            FieldType::Option(inner) => inner.is_numeric(),
            FieldType::F32
            | FieldType::F64
            | FieldType::I8
            | FieldType::I16
            | FieldType::I32
            | FieldType::I64
            | FieldType::I128
            | FieldType::Isize
            | FieldType::U8
            | FieldType::U16
            | FieldType::U32
            | FieldType::U64
            | FieldType::U128
            | FieldType::Usize => true,
            _ => false,
        }
    }
}

impl ToTokens for FieldType {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        match self {
            FieldType::String => tokens.extend(quote! { FieldType::String }),
            FieldType::Char => tokens.extend(quote! { FieldType::Char }),
            FieldType::Bool => tokens.extend(quote! { FieldType::Bool }),
            FieldType::F32 => tokens.extend(quote! { FieldType::F32 }),
            FieldType::F64 => tokens.extend(quote! { FieldType::F64 }),
            FieldType::I8 => tokens.extend(quote! { FieldType::I8 }),
            FieldType::I16 => tokens.extend(quote! { FieldType::I16 }),
            FieldType::I32 => tokens.extend(quote! { FieldType::I32 }),
            FieldType::I64 => tokens.extend(quote! { FieldType::I64 }),
            FieldType::I128 => tokens.extend(quote! { FieldType::I128 }),
            FieldType::Isize => tokens.extend(quote! { FieldType::Isize }),
            FieldType::U8 => tokens.extend(quote! { FieldType::U8 }),
            FieldType::U16 => tokens.extend(quote! { FieldType::U16 }),
            FieldType::U32 => tokens.extend(quote! { FieldType::U32 }),
            FieldType::U64 => tokens.extend(quote! { FieldType::U64 }),
            FieldType::U128 => tokens.extend(quote! { FieldType::U128 }),
            FieldType::Usize => tokens.extend(quote! { FieldType::Usize }),
            FieldType::Duration => tokens.extend(quote! { FieldType::Duration }),
            FieldType::Unit => tokens.extend(quote! { FieldType::Unit }),
            FieldType::Other(s) => {
                let lit = syn::LitStr::new(s, proc_macro2::Span::call_site());
                tokens.extend(quote! { FieldType::Other(#lit.to_string()) });
            }
            FieldType::Option(inner) => {
                tokens.extend(quote! {
                    FieldType::Option(Box::new(#inner))
                });
            }
            FieldType::Vec(inner) => {
                tokens.extend(quote! {
                    FieldType::Vec(Box::new(#inner))
                });
            }
            FieldType::Tuple(types) => {
                tokens.extend(quote! {
                    FieldType::Tuple(vec![#(#types),*])
                });
            }
            FieldType::Struct(fields) => {
                let field_tokens = fields.iter().map(|(fname, fty)| {
                    let lit = syn::LitStr::new(fname, proc_macro2::Span::call_site());
                    quote! { (#lit.to_string(), #fty) }
                });
                tokens.extend(quote! {
                    FieldType::Struct(vec![#(#field_tokens),*])
                });
            }
            FieldType::HashMap(key, value) => tokens.extend(quote! {
            FieldType::HashMap(Box::new(#key),Box::new(#value) ) }),
            FieldType::BTreeMap(key, value) => tokens.extend(quote! {
            FieldType::BTreeMap(Box::new(#key),Box::new(#value) ) }),
            FieldType::RecordLink(inner) => tokens.extend(quote! {
            FieldType::RecordLink(Box::new(#inner)) }),
        }
    }
}

/// How a parsed field type names a type it does not know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathNames {
    /// By the path's last segment, which is how foreign types and scanned
    /// types are looked up once resolved.
    Last,
    /// By the whole path as written, for the scanner to resolve against the
    /// defining module's imports.
    Written,
}

impl FieldType {
    /// What serde writes for a tuple variant: a newtype variant writes its one
    /// field, and any other count, none included, writes an array.
    pub fn parse_tuple_variant(fields: &syn::FieldsUnnamed, names: PathNames) -> FieldType {
        let items: Vec<FieldType> = fields
            .unnamed
            .iter()
            .map(|field| FieldType::parse(&field.ty, names))
            .collect();
        match <[FieldType; 1]>::try_from(items) {
            Ok([only]) => only,
            Err(items) => FieldType::Tuple(items),
        }
    }

    /// `ty` as a field type, naming unknown types by their last segment.
    pub fn parse_syn_ty(ty: &SynType) -> FieldType {
        Self::parse(ty, PathNames::Last)
    }

    /// `ty` as a field type, naming unknown types as `names` says.
    pub fn parse(ty: &SynType, names: PathNames) -> FieldType {
        use quote::ToTokens;
        tracing::trace!("Parsing syn type: {}", ty.to_token_stream());

        let result = match ty {
            SynType::Path(path) => Self::handle_type_path(path, names),
            SynType::Tuple(tuple) => Self::handle_tuple(tuple, names),
            SynType::Slice(slice) => FieldType::Vec(Box::new(Self::parse(&slice.elem, names))),
            SynType::Array(array) => FieldType::Vec(Box::new(Self::parse(&array.elem, names))),
            SynType::Reference(reference) => Self::parse(&reference.elem, names),
            SynType::Ptr(pointer) => Self::parse(&pointer.elem, names),
            SynType::Paren(paren) => Self::parse(&paren.elem, names),
            SynType::Group(group) => Self::parse(&group.elem, names),
            SynType::ImplTrait(impl_trait) => {
                tracing::debug!(
                    "impl Trait not directly supported: {}",
                    impl_trait.to_token_stream()
                );
                FieldType::Other(impl_trait.to_token_stream().to_string())
            }
            SynType::TraitObject(trait_object) => {
                tracing::debug!(
                    "Trait object not directly supported: {}",
                    trait_object.to_token_stream()
                );
                FieldType::Other(trait_object.to_token_stream().to_string())
            }
            SynType::FnPtr(function) => {
                tracing::debug!(
                    "Function pointer not directly supported: {}",
                    function.to_token_stream()
                );
                FieldType::Other(function.to_token_stream().to_string())
            }
            SynType::Infer(infer) => FieldType::Other(infer.to_token_stream().to_string()),
            SynType::Never(never) => FieldType::Other(never.to_token_stream().to_string()),
            SynType::Macro(type_macro) => {
                tracing::debug!("Type macro not supported: {}", type_macro.to_token_stream());
                FieldType::Other(type_macro.to_token_stream().to_string())
            }
            SynType::Verbatim(tokens) => FieldType::Other(tokens.to_string()),
            _ => {
                tracing::warn!("Unknown type variant: {}", ty.to_token_stream());
                FieldType::Other(ty.to_token_stream().to_string())
            }
        };

        tracing::trace!("Parsed type as: {:?}", result);
        result
    }

    fn handle_tuple(tuple: &syn::TypeTuple, names: PathNames) -> FieldType {
        if tuple.elems.is_empty() {
            FieldType::Unit
        } else {
            let elems = tuple
                .elems
                .iter()
                .map(|elem| Self::parse(elem, names))
                .collect();
            FieldType::Tuple(elems)
        }
    }

    fn handle_type_path(type_path: &syn::TypePath, names: PathNames) -> FieldType {
        use quote::ToTokens;

        if type_path.qself.is_some() {
            return FieldType::Other(type_path.to_token_stream().to_string());
        }

        let last = match type_path.path.segments.last() {
            Some(segment) => segment,
            None => return FieldType::Other(type_path.to_token_stream().to_string()),
        };

        let ident = last.ident.to_string();
        // Resolution decides what a written path names; by its last segment,
        // only the standard library's own paths name its `Duration`.
        if names == PathNames::Last {
            let path = path_text(&type_path.path);
            let path = path.trim_start_matches("::");
            if path == "Duration" || STD_DURATION_PATHS.contains(&path) {
                return FieldType::Duration;
            }
        }
        let unknown = || match names {
            PathNames::Last => FieldType::Other(ident.clone()),
            PathNames::Written => FieldType::Other(path_text(&type_path.path)),
        };

        // Handle generic types with angle brackets
        if let syn::PathArguments::AngleBracketed(args) = &last.arguments {
            let type_args: Vec<_> = args
                .args
                .iter()
                .filter_map(|argument| match argument {
                    syn::GenericArgument::Type(argument_type) => Some(argument_type),
                    _ => None,
                })
                .collect();
            let parse = |position: usize| Box::new(Self::parse(type_args[position], names));

            return match ident.as_str() {
                "Option" if type_args.len() == 1 => FieldType::Option(parse(0)),
                "Vec" if type_args.len() == 1 => FieldType::Vec(parse(0)),
                "Box" if type_args.len() == 1 => *parse(0),
                "HashMap" if type_args.len() == 2 => FieldType::HashMap(parse(0), parse(1)),
                "BTreeMap" if type_args.len() == 2 => FieldType::BTreeMap(parse(0), parse(1)),
                "RecordLink" if type_args.len() == 1 => FieldType::RecordLink(parse(0)),
                // Any other generic type (e.g. `DateTime<Utc>`) is named
                // without its arguments, so foreign type config can match it.
                _ => unknown(),
            };
        }

        // Match known built-in types without generics
        match ident.as_str() {
            "String" | "str" => FieldType::String,
            "char" => FieldType::Char,
            "bool" => FieldType::Bool,
            "f32" => FieldType::F32,
            "f64" => FieldType::F64,
            "i8" => FieldType::I8,
            "i16" => FieldType::I16,
            "i32" => FieldType::I32,
            "i64" => FieldType::I64,
            "i128" => FieldType::I128,
            "isize" => FieldType::Isize,
            "u8" => FieldType::U8,
            "u16" => FieldType::U16,
            "u32" => FieldType::U32,
            "u64" => FieldType::U64,
            "u128" => FieldType::U128,
            "usize" => FieldType::Usize,
            _ => {
                tracing::trace!("Unknown type '{}', storing as Other", ident);
                unknown()
            }
        }
    }
}

/// The text of a type path as written, without generic arguments.
fn path_text(path: &syn::Path) -> String {
    let segments = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::");
    if path.leading_colon.is_some() {
        format!("::{segments}")
    } else {
        segments
    }
}

impl FieldType {
    /// Returns a human-readable canonical name using Rust-like syntax.
    ///
    /// Examples: `"String"`, `"Decimal"`, `"Option<DateTime>"`, `"Vec<i32>"`, `"HashMap<String, i64>"`
    pub fn canonical_name(&self) -> String {
        match self {
            FieldType::String => "String".to_string(),
            FieldType::Char => "char".to_string(),
            FieldType::Bool => "bool".to_string(),
            FieldType::Unit => "()".to_string(),
            FieldType::F32 => "f32".to_string(),
            FieldType::F64 => "f64".to_string(),
            FieldType::I8 => "i8".to_string(),
            FieldType::I16 => "i16".to_string(),
            FieldType::I32 => "i32".to_string(),
            FieldType::I64 => "i64".to_string(),
            FieldType::I128 => "i128".to_string(),
            FieldType::Isize => "isize".to_string(),
            FieldType::U8 => "u8".to_string(),
            FieldType::U16 => "u16".to_string(),
            FieldType::U32 => "u32".to_string(),
            FieldType::U64 => "u64".to_string(),
            FieldType::U128 => "u128".to_string(),
            FieldType::Usize => "usize".to_string(),
            FieldType::Duration => "Duration".to_string(),
            FieldType::Tuple(types) => {
                let inner: Vec<String> = types.iter().map(|t| t.canonical_name()).collect();
                format!("({})", inner.join(", "))
            }
            FieldType::Struct(fields) => {
                let inner: Vec<String> = fields
                    .iter()
                    .map(|(name, ft)| format!("{}: {}", name, ft.canonical_name()))
                    .collect();
                format!("{{ {} }}", inner.join(", "))
            }
            FieldType::Option(inner) => format!("Option<{}>", inner.canonical_name()),
            FieldType::Vec(inner) => format!("Vec<{}>", inner.canonical_name()),
            FieldType::HashMap(k, v) => {
                format!("HashMap<{}, {}>", k.canonical_name(), v.canonical_name())
            }
            FieldType::BTreeMap(k, v) => {
                format!("BTreeMap<{}, {}>", k.canonical_name(), v.canonical_name())
            }
            FieldType::RecordLink(inner) => format!("RecordLink<{}>", inner.canonical_name()),
            FieldType::Other(name) => name.clone(),
        }
    }
}

impl fmt::Display for FieldType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FieldType::String => write!(f, "String"),
            FieldType::Char => write!(f, "Char"),
            FieldType::Bool => write!(f, "Bool"),
            FieldType::Unit => write!(f, "Unit"),
            FieldType::F32 => write!(f, "F32"),
            FieldType::F64 => write!(f, "F64"),
            FieldType::I8 => write!(f, "I8"),
            FieldType::I16 => write!(f, "I16"),
            FieldType::I32 => write!(f, "I32"),
            FieldType::I64 => write!(f, "I64"),
            FieldType::I128 => write!(f, "I128"),
            FieldType::Isize => write!(f, "Isize"),
            FieldType::U8 => write!(f, "U8"),
            FieldType::U16 => write!(f, "U16"),
            FieldType::U32 => write!(f, "U32"),
            FieldType::U64 => write!(f, "U64"),
            FieldType::U128 => write!(f, "U128"),
            FieldType::Usize => write!(f, "Usize"),
            FieldType::Duration => write!(f, "Duration"),
            FieldType::Tuple(types) => {
                write!(f, "Tuple(")?;
                let mut first = true;
                for field_type in types {
                    if !first {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", field_type)?;
                    first = false;
                }
                write!(f, ")")
            }
            FieldType::Struct(fields) => {
                write!(f, "Struct(")?;
                let mut first = true;
                for (name, field_type) in fields {
                    if !first {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", name, field_type)?;
                    first = false;
                }
                write!(f, ")")
            }
            FieldType::Option(inner) => write!(f, "Option({})", inner),
            FieldType::Vec(inner) => write!(f, "Vec({})", inner),
            FieldType::HashMap(key, value) => write!(f, "HashMap({}, {})", key, value),
            FieldType::BTreeMap(key, value) => write!(f, "BTreeMap({}, {})", key, value),
            FieldType::RecordLink(inner) => write!(f, "RecordLink({})", inner),
            FieldType::Other(name) => write!(f, "{}", name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FieldType, PathNames};

    fn variant_types(item: syn::ItemEnum) -> Vec<FieldType> {
        item.variants
            .iter()
            .filter_map(|variant| match &variant.fields {
                syn::Fields::Unnamed(fields) => {
                    Some(FieldType::parse_tuple_variant(fields, PathNames::Last))
                }
                syn::Fields::Named(_) | syn::Fields::Unit => None,
            })
            .collect()
    }

    #[test]
    fn only_the_standard_library_duration_is_native() {
        let parse = |ty: syn::Type| FieldType::parse_syn_ty(&ty);
        assert_eq!(parse(syn::parse_quote!(Duration)), FieldType::Duration);
        assert_eq!(
            parse(syn::parse_quote!(std::time::Duration)),
            FieldType::Duration
        );
        assert_eq!(
            parse(syn::parse_quote!(::core::time::Duration)),
            FieldType::Duration
        );
        assert_eq!(
            parse(syn::parse_quote!(chrono::Duration)),
            FieldType::Other("Duration".to_string())
        );
        assert_eq!(
            FieldType::parse(&syn::parse_quote!(Duration), PathNames::Written),
            FieldType::Other("Duration".to_string())
        );
    }

    #[test]
    fn tuple_variants_are_typed_as_serde_writes_them() {
        let types = variant_types(syn::parse_quote! {
            enum Event {
                Cleared(),
                Scored(u32),
                Pinned(f64, String),
            }
        });
        assert_eq!(
            types,
            vec![
                FieldType::Tuple(Vec::new()),
                FieldType::U32,
                FieldType::Tuple(vec![FieldType::F64, FieldType::String]),
            ]
        );
    }
}
