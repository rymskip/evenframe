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
    EvenframeRecordId,
    DateTime,
    EvenframeDuration,
    Timezone,
    Decimal,
    OrderedFloat(Box<FieldType>), // Wraps F32 or F64
    Tuple(Vec<FieldType>),
    Struct(Vec<(String, FieldType)>),
    Option(Box<FieldType>),
    Vec(Box<FieldType>),
    HashMap(Box<FieldType>, Box<FieldType>),
    BTreeMap(Box<FieldType>, Box<FieldType>),
    RecordLink(Box<FieldType>),
    Other(String),
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
            FieldType::EvenframeRecordId => tokens.extend(quote! { FieldType::EvenframeRecordId }),
            FieldType::DateTime => tokens.extend(quote! { FieldType::DateTime }),
            FieldType::EvenframeDuration => tokens.extend(quote! { FieldType::EvenframeDuration }),
            FieldType::Timezone => tokens.extend(quote! { FieldType::Timezone }),
            FieldType::Decimal => tokens.extend(quote! { FieldType::Decimal }),
            FieldType::Unit => tokens.extend(quote! { FieldType::Unit }),
            FieldType::OrderedFloat(inner) => {
                tokens.extend(quote! {
                    FieldType::OrderedFloat(Box::new(#inner))
                });
            }
            FieldType::Other(s) => {
                // Wrap the string in a LitStr so it becomes a literal token.
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

impl FieldType {
    pub fn parse_syn_ty(ty: &SynType) -> FieldType {
        use quote::ToTokens;
        tracing::trace!("Parsing syn type: {}", ty.to_token_stream());

        let result = match ty {
            // Path types (most common - String, Vec<T>, custom types, etc.)
            SynType::Path(tp) => Self::handle_type_path(tp),

            // Tuple types - (T1, T2, ...) or unit ()
            SynType::Tuple(t) => Self::handle_tuple(t),

            // [T] -> treat as Vec<T>
            SynType::Slice(s) => FieldType::Vec(Box::new(Self::parse_syn_ty(&s.elem))),

            // [T; N] -> treat as Vec<T> (ignore size)
            SynType::Array(arr) => FieldType::Vec(Box::new(Self::parse_syn_ty(&arr.elem))),

            // &T or &mut T -> parse inner type
            SynType::Reference(r) => Self::parse_syn_ty(&r.elem),

            // *const T or *mut T -> parse inner type
            SynType::Ptr(p) => Self::parse_syn_ty(&p.elem),

            // (T) -> unwrap parentheses
            SynType::Paren(p) => Self::parse_syn_ty(&p.elem),

            // Group tokens -> unwrap
            SynType::Group(g) => Self::parse_syn_ty(&g.elem),

            // impl Trait -> fallback to string
            SynType::ImplTrait(it) => {
                tracing::debug!(
                    "impl Trait not directly supported: {}",
                    it.to_token_stream()
                );
                FieldType::Other(it.to_token_stream().to_string())
            }

            // dyn Trait -> fallback to string
            SynType::TraitObject(to) => {
                tracing::debug!(
                    "Trait object not directly supported: {}",
                    to.to_token_stream()
                );
                FieldType::Other(to.to_token_stream().to_string())
            }

            // fn(...) -> ... -> fallback to string
            SynType::BareFn(f) => {
                tracing::debug!(
                    "Function pointer not directly supported: {}",
                    f.to_token_stream()
                );
                FieldType::Other(f.to_token_stream().to_string())
            }

            // _ (infer) -> fallback to string
            SynType::Infer(i) => FieldType::Other(i.to_token_stream().to_string()),

            // ! (never) -> fallback to string
            SynType::Never(n) => FieldType::Other(n.to_token_stream().to_string()),

            // type_macro!(...) -> fallback to string
            SynType::Macro(m) => {
                tracing::debug!("Type macro not supported: {}", m.to_token_stream());
                FieldType::Other(m.to_token_stream().to_string())
            }

            // Verbatim tokens from macro expansion -> fallback to string
            SynType::Verbatim(ts) => FieldType::Other(ts.to_string()),

            // Future-proofing for new syn variants
            _ => {
                tracing::warn!("Unknown type variant: {}", ty.to_token_stream());
                FieldType::Other(ty.to_token_stream().to_string())
            }
        };

        tracing::trace!("Parsed type as: {:?}", result);
        result
    }

    fn handle_tuple(t: &syn::TypeTuple) -> FieldType {
        if t.elems.is_empty() {
            // Unit type ()
            FieldType::Unit
        } else {
            let elems = t.elems.iter().map(Self::parse_syn_ty).collect();
            FieldType::Tuple(elems)
        }
    }

    fn handle_type_path(tp: &syn::TypePath) -> FieldType {
        use quote::ToTokens;

        // If qualified self type (e.g., <T as Trait>::Assoc), fallback
        if tp.qself.is_some() {
            return FieldType::Other(tp.to_token_stream().to_string());
        }

        // Get the last segment (works for both `String` and `std::string::String`)
        let last = match tp.path.segments.last() {
            Some(s) => s,
            None => return FieldType::Other(tp.to_token_stream().to_string()),
        };

        let ident = last.ident.to_string();

        // Handle generic types with angle brackets
        if let syn::PathArguments::AngleBracketed(args) = &last.arguments {
            // Extract only type arguments (ignore lifetimes and const generics)
            let type_args: Vec<_> = args
                .args
                .iter()
                .filter_map(|ga| match ga {
                    syn::GenericArgument::Type(t) => Some(t),
                    _ => None,
                })
                .collect();

            match ident.as_str() {
                "Option" if type_args.len() == 1 => {
                    return FieldType::Option(Box::new(Self::parse_syn_ty(type_args[0])));
                }
                "Vec" if type_args.len() == 1 => {
                    return FieldType::Vec(Box::new(Self::parse_syn_ty(type_args[0])));
                }
                "Box" if type_args.len() == 1 => {
                    return Self::parse_syn_ty(type_args[0]);
                }
                "HashMap" if type_args.len() == 2 => {
                    return FieldType::HashMap(
                        Box::new(Self::parse_syn_ty(type_args[0])),
                        Box::new(Self::parse_syn_ty(type_args[1])),
                    );
                }
                "BTreeMap" if type_args.len() == 2 => {
                    return FieldType::BTreeMap(
                        Box::new(Self::parse_syn_ty(type_args[0])),
                        Box::new(Self::parse_syn_ty(type_args[1])),
                    );
                }
                "RecordLink" if type_args.len() == 1 => {
                    return FieldType::RecordLink(Box::new(Self::parse_syn_ty(type_args[0])));
                }
                "OrderedFloat" if type_args.len() == 1 => {
                    tracing::debug!("Found OrderedFloat with inner type");
                    return FieldType::OrderedFloat(Box::new(Self::parse_syn_ty(type_args[0])));
                }
                "DateTime" => {
                    // DateTime<Utc>, DateTime<Local>, etc. all become DateTime
                    return FieldType::DateTime;
                }
                _ => {
                    // Unknown generic type, fall through to check if it's a known non-generic
                }
            }
        }

        // Match known types without generics
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
            "EvenframeRecordId" => FieldType::EvenframeRecordId,
            "DateTime" => FieldType::DateTime,
            "EvenframeDuration" => FieldType::EvenframeDuration,
            "Tz" | "Timezone" => FieldType::Timezone,
            "Decimal" => FieldType::Decimal,
            _ => {
                // Unknown type - store as Other
                let type_str = tp.to_token_stream().to_string();
                tracing::trace!("Unknown type '{}', storing as Other", type_str);
                FieldType::Other(type_str)
            }
        }
    }

    pub fn parse_type_str(type_str: &str) -> FieldType {
        // Remove whitespace for consistent parsing
        let clean_str = type_str
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>();

        // Check for simple primitive types first
        match clean_str.as_str() {
            "String" => FieldType::String,
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
            "EvenframeRecordId" => FieldType::EvenframeRecordId,
            "DateTime" => FieldType::DateTime,
            "EvenframeDuration" => FieldType::EvenframeDuration,
            "Tz" | "Timezone" => FieldType::Timezone,
            "Decimal" => FieldType::Decimal,
            "()" => FieldType::Unit,
            _ => {
                // Check for generic types like Option<T> or Vec<T>
                if let Some(start) = clean_str.find('<') {
                    if let Some(end) = clean_str.rfind('>') {
                        let outer = &clean_str[..start];
                        let inner = &clean_str[start + 1..end];

                        match outer {
                            "Option" => {
                                let inner_type = Self::parse_type_str(inner);
                                FieldType::Option(Box::new(inner_type))
                            }
                            "Vec" => {
                                let inner_type = Self::parse_type_str(inner);
                                FieldType::Vec(Box::new(inner_type))
                            }
                            "Box" => Self::parse_type_str(inner),
                            "DateTime" => FieldType::DateTime,
                            "EvenframeDuration" => FieldType::EvenframeDuration,
                            _ => FieldType::Other(clean_str),
                        }
                    } else {
                        // Malformed generic type (missing closing '>')
                        FieldType::Other(clean_str)
                    }
                } else if clean_str.starts_with('(') && clean_str.ends_with(')') {
                    // Handle tuple types
                    // Strip the outer parentheses
                    let inner = &clean_str[1..clean_str.len() - 1];

                    // Split by commas, but handle nested generics carefully
                    let mut elements = Vec::new();
                    let mut current = String::new();
                    let mut depth = 0;

                    for c in inner.chars() {
                        match c {
                            '<' => {
                                depth += 1;
                                current.push(c);
                            }
                            '>' => {
                                depth -= 1;
                                current.push(c);
                            }
                            ',' if depth == 0 => {
                                if !current.is_empty() {
                                    elements.push(Self::parse_type_str(&current));
                                    current.clear();
                                }
                            }
                            _ => current.push(c),
                        }
                    }

                    // Don't forget the last element
                    if !current.is_empty() {
                        elements.push(Self::parse_type_str(&current));
                    }

                    FieldType::Tuple(elements)
                } else {
                    // Unknown or complex type
                    FieldType::Other(clean_str)
                }
            }
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
            FieldType::EvenframeRecordId => write!(f, "EvenframeRecordId"),
            FieldType::DateTime => write!(f, "DateTime"),
            FieldType::EvenframeDuration => write!(f, "EvenframeDuration"),
            FieldType::Timezone => write!(f, "Timezone"),
            FieldType::Decimal => write!(f, "Decimal"),
            FieldType::OrderedFloat(inner) => write!(f, "OrderedFloat<{}>", inner),
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
