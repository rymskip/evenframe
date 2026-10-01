use crate::imports::generate_deserialize_imports;
use convert_case::{Case, Casing};
use evenframe_core::derive::validator_parser::FieldValidators;
use quote::quote;
use syn::{Data, DeriveInput, Fields, spanned::Spanned};

/// Generates a custom Deserialize implementation that applies each field's
/// validators while deserializing. `fields_validators` holds the validators of
/// every named field, in field order.
pub fn generate_custom_deserialize(
    input: &DeriveInput,
    fields_validators: &[FieldValidators],
) -> proc_macro2::TokenStream {
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
    let field_deserializations = fields
        .iter()
        .zip(fields_validators)
        .map(|(field, validators)| {
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
            let enum_variant =
                quote::format_ident!("{}", field_name.to_string().to_case(Case::Pascal));

            if !validators.is_empty() {
                let temp_var = quote::format_ident!("__temp_{}", field_name);
                let read =
                    match validators.read_tokens(&temp_var, field_type, &field_name.to_string()) {
                        Ok(read) => read,
                        Err(err) => return err.to_compile_error(),
                    };
                quote! {
                    Field::#enum_variant => {
                        if #field_name.is_some() {
                            return Err(de::Error::duplicate_field(stringify!(#field_name)));
                        }
                        #read
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

    let field_names: Vec<_> = fields
        .iter()
        .filter_map(|field| field.ident.as_ref())
        .collect();

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
