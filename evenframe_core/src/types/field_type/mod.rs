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

impl FieldType {
    pub fn parse_syn_ty(ty: &SynType) -> FieldType {
        use quote::ToTokens;
        tracing::trace!("Parsing syn type: {}", ty.to_token_stream());

        let result = match ty {
            SynType::Path(tp) => Self::handle_type_path(tp),
            SynType::Tuple(t) => Self::handle_tuple(t),
            SynType::Slice(s) => FieldType::Vec(Box::new(Self::parse_syn_ty(&s.elem))),
            SynType::Array(arr) => FieldType::Vec(Box::new(Self::parse_syn_ty(&arr.elem))),
            SynType::Reference(r) => Self::parse_syn_ty(&r.elem),
            SynType::Ptr(p) => Self::parse_syn_ty(&p.elem),
            SynType::Paren(p) => Self::parse_syn_ty(&p.elem),
            SynType::Group(g) => Self::parse_syn_ty(&g.elem),
            SynType::ImplTrait(it) => {
                tracing::debug!(
                    "impl Trait not directly supported: {}",
                    it.to_token_stream()
                );
                FieldType::Other(it.to_token_stream().to_string())
            }
            SynType::TraitObject(to) => {
                tracing::debug!(
                    "Trait object not directly supported: {}",
                    to.to_token_stream()
                );
                FieldType::Other(to.to_token_stream().to_string())
            }
            SynType::BareFn(f) => {
                tracing::debug!(
                    "Function pointer not directly supported: {}",
                    f.to_token_stream()
                );
                FieldType::Other(f.to_token_stream().to_string())
            }
            SynType::Infer(i) => FieldType::Other(i.to_token_stream().to_string()),
            SynType::Never(n) => FieldType::Other(n.to_token_stream().to_string()),
            SynType::Macro(m) => {
                tracing::debug!("Type macro not supported: {}", m.to_token_stream());
                FieldType::Other(m.to_token_stream().to_string())
            }
            SynType::Verbatim(ts) => FieldType::Other(ts.to_string()),
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
            FieldType::Unit
        } else {
            let elems = t.elems.iter().map(Self::parse_syn_ty).collect();
            FieldType::Tuple(elems)
        }
    }

    fn handle_type_path(tp: &syn::TypePath) -> FieldType {
        use quote::ToTokens;

        if tp.qself.is_some() {
            return FieldType::Other(tp.to_token_stream().to_string());
        }

        let last = match tp.path.segments.last() {
            Some(s) => s,
            None => return FieldType::Other(tp.to_token_stream().to_string()),
        };

        let ident = last.ident.to_string();

        // Handle generic types with angle brackets
        if let syn::PathArguments::AngleBracketed(args) = &last.arguments {
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
                _ => {
                    // For any unknown generic type (e.g., DateTime<Utc>),
                    // store just the base name as Other so foreign type config can match it.
                    return FieldType::Other(ident);
                }
            }
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
                // Unknown type - store as Other (use only the last segment identifier,
                // not the full path, so `crate::module::Foo` becomes just `Foo`)
                let type_str = ident.clone();
                tracing::trace!("Unknown type '{}', storing as Other", type_str);
                FieldType::Other(type_str)
            }
        }
    }

    pub fn parse_type_str(type_str: &str) -> FieldType {
        let clean_str = type_str
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>();

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
                            // For any generic type (e.g., DateTime<Utc>), store just the base name
                            _ => FieldType::Other(outer.to_string()),
                        }
                    } else {
                        FieldType::Other(clean_str)
                    }
                } else if clean_str.starts_with('(') && clean_str.ends_with(')') {
                    let inner = &clean_str[1..clean_str.len() - 1];

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

                    if !current.is_empty() {
                        elements.push(Self::parse_type_str(&current));
                    }

                    FieldType::Tuple(elements)
                } else {
                    // Unknown or complex type — could be a foreign type or custom struct/enum
                    FieldType::Other(clean_str)
                }
            }
        }
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
