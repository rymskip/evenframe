use crate::imports::generate_deserialize_imports;
use convert_case::{Case, Casing};
use evenframe_core::derive::validator_parser::parse_field_validators_with_logic;
use quote::quote;
use syn::{Data, DeriveInput, Fields, spanned::Spanned};

/// Generates a custom Deserialize implementation that includes field validation
/// This is used when structs have validators that need to be applied during deserialization
pub fn generate_custom_deserialize(input: &DeriveInput) -> proc_macro2::TokenStream {
    let struct_name = &input.ident;

    // Extract fields from the struct
    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => &fields.named,
            Fields::Unnamed(_) => {
                return syn::Error::new(
                        input.span(),
                        "Custom deserialization is only supported for structs with named fields.\n\nExample:\nstruct MyStruct {\n    field1: String,\n    field2: i32,\n}"
                    ).to_compile_error();
            }
            Fields::Unit => {
                return syn::Error::new(
                        input.span(),
                        "Custom deserialization is not supported for unit structs.\n\nUnit structs have no fields to validate."
                    ).to_compile_error();
            }
        },
        Data::Enum(_) => {
            return syn::Error::new(
                input.span(),
                "Custom deserialization is currently only implemented for structs, not enums.\n\nEnums should use the standard Serde derive."
            ).to_compile_error();
        }
        Data::Union(_) => {
            return syn::Error::new(
                input.span(),
                "Custom deserialization is not supported for unions.\n\nUnions are not supported by Evenframe."
            ).to_compile_error();
        }
    };

    // Check if there are any fields to deserialize
    if fields.is_empty() {
        return syn::Error::new(
            input.span(),
            "Cannot generate custom deserialization for struct with no fields.\n\nEmpty structs should use the standard #[derive(Deserialize)]"
        ).to_compile_error();
    }

    // Generate field deserialization with validation
    let field_deserializations = fields.iter().map(|field| {
        let field_name = match field.ident.as_ref() {
            Some(ident) => ident,
            None => {
                return syn::Error::new(
                    field.span(),
                    "Internal error: Named field should have an identifier",
                )
                .to_compile_error();
            }
        };
        let field_type = &field.ty;
        let enum_variant = quote::format_ident!("{}", field_name.to_string().to_case(Case::Pascal));

        // Create a temporary variable name for validation
        let temp_var_name = format!("__temp_{}", field_name);

        // Parse validators and get both validator tokens and logic tokens
        let (_, validation_logic_tokens) =
            match parse_field_validators_with_logic(&field.attrs, &temp_var_name, Some(field_type)) {
                Ok(tokens) => tokens,
                Err(err) => {
                    return err.to_compile_error();
                }
            };

        if !validation_logic_tokens.is_empty() {
            let temp_var = quote::format_ident!("{}", temp_var_name);
            // Generate validation code with better error context
            quote! {
                Field::#enum_variant => {
                    if #field_name.is_some() {
                        return Err(de::Error::duplicate_field(stringify!(#field_name)));
                    }
                    let mut #temp_var: #field_type = map.next_value()?;
                    // Apply validators - any validation errors will be converted to deserialization errors
                    #(#validation_logic_tokens)*
                    #field_name = Some(#temp_var);
                }
            }
        } else {
            // Standard deserialization without validation
            quote! {
                Field::#enum_variant => {
                    if #field_name.is_some() {
                        return Err(de::Error::duplicate_field(stringify!(#field_name)));
                    }
                    #field_name = Some(map.next_value()?);
                }
            }
        }
    });

    let field_names: Vec<_> = fields.iter().filter_map(|f| f.ident.as_ref()).collect();

    // Validate that all fields have names (this should always be true after our earlier check)
    if field_names.len() != fields.len() {
        return syn::Error::new(
            input.span(),
            "Internal error: Some fields are missing identifiers after validation",
        )
        .to_compile_error();
    }
    let enum_variants: Vec<_> = field_names
        .iter()
        .map(|name| quote::format_ident!("{}", name.to_string().to_case(Case::Pascal)))
        .collect();

    let imports = generate_deserialize_imports();
    quote! {
        const _: () = {
            #imports

            // Custom deserialization implementation
            impl<'de> EvenframeDeserialize<'de> for #struct_name {
            fn evenframe_deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: ::serde::Deserializer<'de>,
            {
                use ::serde::de::{self, Visitor, MapAccess};
                use std::fmt;

                enum Field {
                    #(#enum_variants,)*
                }

                impl<'de> ::serde::Deserialize<'de> for Field {
                    fn deserialize<D>(deserializer: D) -> Result<Field, D::Error>
                    where
                        D: ::serde::Deserializer<'de>,
                    {
                        struct FieldVisitor;

                        impl<'de> Visitor<'de> for FieldVisitor {
                            type Value = Field;

                            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                                formatter.write_str("field identifier")
                            }

                            fn visit_str<E>(self, value: &str) -> Result<Field, E>
                            where
                                E: de::Error,
                            {
                                match value {
                                    #(stringify!(#field_names) => Ok(Field::#enum_variants),)*
                                    _ => Err(de::Error::unknown_field(value, &[#(stringify!(#field_names)),*])),
                                }
                            }
                        }

                        deserializer.deserialize_identifier(FieldVisitor)
                    }
                }

                struct StructVisitor;

                impl<'de> Visitor<'de> for StructVisitor {
                    type Value = #struct_name;

                    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                        formatter.write_str(concat!("struct ", stringify!(#struct_name)))
                    }

                    fn visit_map<V>(self, mut map: V) -> Result<#struct_name, V::Error>
                    where
                        V: MapAccess<'de>,
                    {
                        #(let mut #field_names = None;)*

                        while let Some(key) = map.next_key()? {
                            match key {
                                #(#field_deserializations)*
                            }
                        }

                        #(
                            let #field_names = #field_names.ok_or_else(|| de::Error::missing_field(stringify!(#field_names)))?;
                        )*

                        Ok(#struct_name {
                            #(#field_names,)*
                        })
                    }
                }

                const FIELDS: &'static [&'static str] = &[#(stringify!(#field_names)),*];
                deserializer.deserialize_struct(stringify!(#struct_name), FIELDS, StructVisitor)
            }
        }

        };

        // Default Deserialize implementation that delegates to custom trait
        impl<'de> ::serde::Deserialize<'de> for #struct_name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: ::serde::Deserializer<'de>,
            {
                #imports
                Self::evenframe_deserialize(deserializer)
            }
        }
    }
}
