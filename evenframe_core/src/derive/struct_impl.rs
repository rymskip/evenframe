use crate::{
    derive::{
        attributes::{parse_format_attribute, parse_mock_data_attribute, parse_relation_attribute},
        deserialization_impl::generate_custom_deserialize,
        imports::generate_struct_imports,
        validator_parser::parse_field_validators,
    },
    schemasync::{DefineConfig, EdgeConfig, PermissionsConfig},
    types::FieldType,
};
use proc_macro2::TokenStream;
use quote::quote;
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Fields};
use tracing::{debug, error, info, trace};

pub fn generate_struct_impl(input: DeriveInput) -> TokenStream {
    let ident = input.ident.clone();
    info!("Generating struct implementation for: {}", ident);

    // Get centralized imports for struct implementations
    debug!("Generating struct imports");
    let imports = generate_struct_imports();

    if let Data::Struct(ref data_struct) = input.data {
        debug!("Processing struct data");
        // Ensure the struct has named fields.
        let fields_named = if let Fields::Named(ref fields_named) = data_struct.fields {
            debug!("Found {} named fields", fields_named.named.len());
            fields_named
        } else {
            error!("Struct does not have named fields");
            return syn::Error::new(
                ident.span(),
                format!("Evenframe derive macro only supports structs with named fields.\n\nExample of a valid struct:\n\nstruct {} {{\n    id: String,\n    name: String,\n}}", ident),
            )
            .to_compile_error();
        };

        // Parse struct-level attributes
        debug!("Parsing struct-level attributes");
        let permissions_config = match PermissionsConfig::parse(&input.attrs) {
            Ok(config) => config,
            Err(err) => {
                return syn::Error::new(
                        input.span(),
                        format!("Failed to parse permissions configuration: {}\n\nExample usage:\n#[permissions(\n    select = \"true\",\n    create = \"auth.role == 'admin'\",\n    update = \"$auth.id == id\",\n    delete = \"false\"\n)]\nstruct MyStruct {{ ... }}", err)
                    )
                    .to_compile_error();
            }
        };

        // Parse mock_data attribute
        let mock_data_config = match parse_mock_data_attribute(&input.attrs) {
            Ok(config) => config,
            Err(err) => return err.to_compile_error(),
        };

        // Parse table-level validators using the same parser as field validators
        let table_validators = match parse_field_validators(&input.attrs) {
            Ok(validators) => validators,
            Err(err) => {
                return syn::Error::new(
                    input.span(),
                    format!("Failed to parse table validators: {}\n\nExample usage:\n#[validators(StringValidator::MinLength(5))]\nstruct MyStruct {{ ... }}", err)
                )
                .to_compile_error();
            }
        };

        // Parse relation attribute
        let relation_config = match parse_relation_attribute(&input.attrs) {
            Ok(config) => config,
            Err(err) => return err.to_compile_error(),
        };

        // Check if an "id" field exists.
        // Structs with an "id" field are treated as persistable entities (database tables).
        // Structs without an "id" field are treated as application-level data structures.
        debug!("Checking for 'id' field to determine struct type");
        let has_id = fields_named.named.iter().any(|field| {
            // Check if field name is "id" - unwrap_or(false) handles unnamed fields gracefully
            field.ident.as_ref().map(|id| id == "id").unwrap_or(false)
        });
        if has_id {
            debug!("Found 'id' field - treating as persistable entity");
        } else {
            debug!("No 'id' field found - treating as application-level data structure");
        }

        // Single pass over all fields.
        debug!("Processing {} fields", fields_named.named.len());
        let mut table_field_tokens = Vec::new();
        let mut json_assignments = Vec::new();

        for (field_index, field) in fields_named.named.iter().enumerate() {
            trace!(
                "Processing field {} of {}",
                field_index + 1,
                fields_named.named.len()
            );
            let field_ident = match field.ident.as_ref() {
                Some(ident) => ident,
                None => {
                    return syn::Error::new(
                        field.span(),
                        "Internal error: Field identifier is missing. This should not happen with named fields."
                    )
                    .to_compile_error();
                }
            };
            let field_name = field_ident.to_string();
            // Remove the r# prefix from raw identifiers (e.g., r#type -> type)
            let field_name_trim = field_name.trim_start_matches("r#");

            // Build the field type token.
            let ty = &field.ty;
            trace!("Parsing data type: {}", quote! {#ty}.to_string());
            let field_type = FieldType::parse_syn_ty(ty);

            // Parse any edge attribute.
            let edge_config = match EdgeConfig::parse(field) {
                Ok(details) => details,
                Err(err) => {
                    return syn::Error::new(
                        field.span(),
                        format!("Failed to parse edge configuration for field '{}': {}\n\nExample usage:\n#[edge(name = \"has_user\", direction = \"from\", to = \"User\")]\npub user: RecordLink<User>", field_name, err)
                    )
                    .to_compile_error();
                }
            };

            // Parse any define details.
            let define_config = match DefineConfig::parse(field) {
                Ok(details) => details,
                Err(err) => {
                    return syn::Error::new(
                        field.span(),
                        format!("Failed to parse define configuration for field '{}': {}\n\nExample usage:\n#[define(default = \"0\", readonly = true)]\npub count: u32", field_name, err)
                    )
                    .to_compile_error();
                }
            };

            // Parse any format attribute.
            let format = match parse_format_attribute(&field.attrs) {
                Ok(fmt) => fmt,
                Err(err) => {
                    return syn::Error::new(
                        field.span(),
                        format!(
                            "Failed to parse format attribute for field '{}': {}",
                            field_name, err
                        ),
                    )
                    .to_compile_error();
                }
            };

            // Parse field-level validators
            let field_validators = match parse_field_validators(&field.attrs) {
                Ok(v) => v,
                Err(err) => {
                    return syn::Error::new(
                        field.span(),
                        format!("Failed to parse validators for field '{}': {}\n\nExample usage:\n#[validate(min_length = 3, max_length = 50)]\npub name: String\n\n#[validate(email)]\npub email: String", field_name, err)
                    )
                    .to_compile_error();
                }
            };

            // Build the schema token for this field.
            let edge_config_tokens = if let Some(ref details) = edge_config {
                quote! {
                    Some(#details)
                }
            } else {
                quote! { None }
            };

            // Build the schema token for this field.
            let define_config_tokens = if let Some(ref define) = define_config {
                quote! {
                    Some(#define)
                }
            } else {
                quote! { None }
            };

            // Build the schema token for this field.
            let format_tokens = if let Some(ref fmt) = format {
                quote! { Some(#fmt) }
            } else {
                quote! { None }
            };

            // Build validators token for this field
            let validators_tokens = if field_validators.is_empty() {
                quote! { vec![] }
            } else {
                quote! { vec![#(#field_validators),*] }
            };

            table_field_tokens.push(quote! {
                StructField {
                    field_name: #field_name_trim.to_string(),
                    field_type: #field_type,
                    edge_config: #edge_config_tokens,
                    define_config: #define_config_tokens,
                    format: #format_tokens,
                    validators: #validators_tokens,
                    always_regenerate: false
                }
            });

            // For the JSON payload, skip the "id" field and any field with an edge attribute.
            if field_name != "id" && edge_config.is_none() {
                json_assignments.push(quote! {
                    #field_name: payload.#field_ident,
                });
            }
        }

        // Build the JSON payload block.
        // let json_payload = quote! { { #(#json_assignments)* } };

        // Generate tokens for parsed attributes (shared between implementations)
        let struct_name = ident.to_string();

        let table_name = ident.to_string();

        let permissions_config_tokens = if let Some(ref config) = permissions_config {
            quote! { Some(#config) }
        } else {
            quote! { None }
        };

        let table_validators_tokens = if !table_validators.is_empty() {
            // The validators are already TokenStreams from parse_field_validators
            quote! { vec![#(#table_validators),*] }
        } else {
            quote! { vec![] }
        };

        let mock_data_tokens = if let Some(config) = mock_data_config {
            quote! { Some(#config) }
        } else {
            quote! { None }
        };

        let relation_tokens = if let Some(ref rel) = relation_config {
            quote! { Some(#rel) }
        } else {
            quote! { None }
        };

        let evenframe_persistable_struct_impl = {
            quote! {
                impl EvenframePersistableStruct for #ident {
                    fn table_config() -> Option<TableConfig> {
                        Some(TableConfig {
                            table_name: #table_name.to_case(Case::Snake),
                            struct_config: ::evenframe::types::StructConfig {
                                struct_name: #struct_name.to_owned(),
                                fields: vec![ #(#table_field_tokens),* ],
                                validators: #table_validators_tokens,
                            },
                            relation: #relation_tokens,
                            permissions: #permissions_config_tokens,
                            mock_generation_config: #mock_data_tokens,
                        })
                    }
                }
            }
        };

        // No trait implementation needed for app structs - the derive macro itself is the marker

        // Check if any field has validators
        // We check this to determine if we need to generate custom deserialization
        let has_field_validators = fields_named.named.iter().any(|field| {
            match parse_field_validators(&field.attrs) {
                Ok(validators) => !validators.is_empty(),
                Err(_) => true, // If parsing fails, assume validators exist to be safe
            }
        });

        // Generate custom deserialization if there are field validators
        let deserialize_impl = if has_field_validators || !table_validators.is_empty() {
            generate_custom_deserialize(&input)
        } else {
            quote! {}
        };

        if has_id {
            info!(
                "Successfully generated persistable struct implementation for: {}",
                ident
            );
            quote! {
                const _: () = {
                    #imports

                    #evenframe_persistable_struct_impl
                };

                #deserialize_impl
            }
        } else {
            // For app structs, we only generate deserialization if needed
            // The derive macro itself serves as the marker
            if has_field_validators {
                info!(
                    "Successfully generated app struct implementation with validation for: {}",
                    ident
                );
                deserialize_impl
            } else {
                info!(
                    "Successfully generated app struct implementation (no validation) for: {}",
                    ident
                );
                quote! {}
            }
        }
    } else {
        error!(
            "Attempted to use derive macro on non-struct type: {}",
            ident
        );
        syn::Error::new(
            ident.span(),
            format!("The Evenframe derive macro can only be applied to structs.\n\nYou tried to apply it to: {}\n\nExample of correct usage:\n#[derive(Evenframe)]\nstruct MyStruct {{\n    id: String,\n    // ... other fields\n}}", ident)
        )
        .to_compile_error()
    }
}
