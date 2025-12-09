use crate::{
    schemasync::table::TableConfig,
    types::{StructConfig, TaggedUnion},
};
use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, quote};
use std::collections::HashMap;
use syn::{LitStr, parenthesized};
use tracing::{debug, error, info, trace};

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct DefineConfig {
    pub select_permissions: Option<String>,
    pub update_permissions: Option<String>,
    pub create_permissions: Option<String>,
    pub data_type: Option<String>,
    pub should_skip: bool,
    pub default: Option<String>,
    pub default_always: Option<String>,
    pub value: Option<String>,
    pub assert: Option<String>,
    pub readonly: Option<bool>,
    pub flexible: Option<bool>,
}

impl ToTokens for DefineConfig {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        // Helper closure to convert Option<String> to tokens.
        let opt_lit = |s: &Option<String>| -> TokenStream {
            if let Some(text) = s {
                let lit = LitStr::new(text, Span::call_site());
                // Wrap the literal in String::from to produce a String.
                quote! { Some(String::from(#lit)) }
            } else {
                quote! { None }
            }
        };

        let select_permissions = opt_lit(&self.select_permissions);
        let update_permissions = opt_lit(&self.update_permissions);
        let create_permissions = opt_lit(&self.create_permissions);
        let data_type = opt_lit(&self.data_type);
        let default = opt_lit(&self.default);
        let default_always = opt_lit(&self.default_always);
        let value = opt_lit(&self.value);
        let assert_field = opt_lit(&self.assert);
        let readonly = if let Some(b) = self.readonly {
            quote! { Some(#b) }
        } else {
            quote! { None }
        };
        let flexible = if let Some(f) = self.flexible {
            quote! { Some(#f) }
        } else {
            quote! { None }
        };

        let should_skip = self.should_skip;

        tokens.extend(quote! {
            ::evenframe::schemasync::DefineConfig {
                select_permissions: #select_permissions,
                update_permissions: #update_permissions,
                create_permissions: #create_permissions,
                data_type: #data_type,
                should_skip: #should_skip,
                default: #default,
                default_always: #default_always,
                value: #value,
                assert: #assert_field,
                readonly: #readonly,
                flexible: #flexible
            }
        });
    }
}

impl DefineConfig {
    pub fn parse(field: &syn::Field) -> syn::Result<Option<DefineConfig>> {
        let mut select_permissions: Option<String> = None;
        let mut update_permissions: Option<String> = None;
        let mut create_permissions: Option<String> = None;
        let mut data_type: Option<String> = None;
        let mut should_skip: Option<bool> = None;
        let mut default: Option<String> = None;
        let mut default_always: Option<String> = None;
        let mut value: Option<String> = None;
        let mut assert: Option<String> = None;
        let mut readonly: Option<bool> = None;
        let mut flexible: Option<bool> = None;

        for attr in &field.attrs {
            if attr.path().is_ident("define_field_statement") {
                attr.parse_nested_meta(|meta| {
                    // Helper closure for optional string fields that works directly on the ParseBuffer.
                    let parse_opt_string =
                        |content: &mut syn::parse::ParseBuffer| -> syn::Result<Option<String>> {
                            if content.peek(syn::Ident) {
                                let ident: syn::Ident = content.parse()?;
                                if ident == "None" {
                                    Ok(None)
                                } else {
                                    Err(syn::Error::new(
                                        ident.span(),
                                        "expected `None` or a string literal",
                                    ))
                                }
                            } else {
                                let lit: syn::LitStr = content.parse()?;
                                if lit.value() == "None" {
                                    Ok(None)
                                } else {
                                    Ok(Some(lit.value()))
                                }
                            }
                        };
                    if meta.path.is_ident("flexible") {
                        let content;
                        parenthesized!(content in meta.input);
                        if flexible.is_some() {
                            return Err(meta.error("duplicate flexible attribute"));
                        }
                        flexible = Some(content.parse::<syn::LitBool>()?.value);
                        return Ok(());
                    }
                    if meta.path.is_ident("select_permissions") {
                        let mut content;
                        parenthesized!(content in meta.input);
                        if select_permissions.is_some() {
                            return Err(meta.error("duplicate select_permissions attribute"));
                        }
                        select_permissions = parse_opt_string(&mut content)?;
                        return Ok(());
                    }
                    if meta.path.is_ident("update_permissions") {
                        let mut content;
                        parenthesized!(content in meta.input);
                        if update_permissions.is_some() {
                            return Err(meta.error("duplicate update_permissions attribute"));
                        }
                        update_permissions = parse_opt_string(&mut content)?;
                        return Ok(());
                    }
                    if meta.path.is_ident("create_permissions") {
                        let mut content;
                        parenthesized!(content in meta.input);
                        if create_permissions.is_some() {
                            return Err(meta.error("duplicate create_permissions attribute"));
                        }
                        create_permissions = parse_opt_string(&mut content)?;
                        return Ok(());
                    }
                    if meta.path.is_ident("data_type") {
                        let mut content;
                        parenthesized!(content in meta.input);
                        if data_type.is_some() {
                            return Err(meta.error("duplicate data_type attribute"));
                        }
                        data_type = parse_opt_string(&mut content)?;
                        return Ok(());
                    }
                    if meta.path.is_ident("should_skip") {
                        let content;
                        parenthesized!(content in meta.input);
                        if should_skip.is_some() {
                            return Err(meta.error("duplicate should_skip attribute"));
                        }
                        should_skip = Some(content.parse::<syn::LitBool>()?.value);
                        return Ok(());
                    }
                    if meta.path.is_ident("default") {
                        let mut content;
                        parenthesized!(content in meta.input);
                        if default.is_some() {
                            return Err(meta.error("duplicate default attribute"));
                        }
                        default = parse_opt_string(&mut content)?;
                        return Ok(());
                    }
                    if meta.path.is_ident("default_always") {
                        let mut content;
                        parenthesized!(content in meta.input);
                        if default_always.is_some() {
                            return Err(meta.error("duplicate default_always attribute"));
                        }
                        default_always = parse_opt_string(&mut content)?;
                        return Ok(());
                    }
                    if meta.path.is_ident("value") {
                        let mut content;
                        parenthesized!(content in meta.input);
                        if value.is_some() {
                            return Err(meta.error("duplicate value attribute"));
                        }
                        value = parse_opt_string(&mut content)?;
                        return Ok(());
                    }
                    if meta.path.is_ident("assert") {
                        let mut content;
                        parenthesized!(content in meta.input);
                        if assert.is_some() {
                            return Err(meta.error("duplicate assert attribute"));
                        }
                        assert = parse_opt_string(&mut content)?;
                        return Ok(());
                    }
                    if meta.path.is_ident("readonly") {
                        let content;
                        parenthesized!(content in meta.input);
                        if readonly.is_some() {
                            return Err(meta.error("duplicate readonly attribute"));
                        }
                        readonly = Some(content.parse::<syn::LitBool>()?.value);
                        return Ok(());
                    }

                    Err(meta.error("unrecognized define detail"))
                })?;

                let should_skip = should_skip.unwrap_or(false);
                return Ok(Some(DefineConfig {
                    select_permissions,
                    update_permissions,
                    create_permissions,
                    data_type,
                    should_skip,
                    default,
                    default_always,
                    value,
                    assert,
                    readonly,
                    flexible,
                }));
            }
        }

        Ok(Some(DefineConfig {
            select_permissions: Some("FULL".to_string()),
            update_permissions: Some("FULL".to_string()),
            create_permissions: Some("FULL".to_string()),
            data_type: None,
            should_skip: false,
            default: None,
            default_always: None,
            value: None,
            assert: None,
            readonly: None,
            flexible: Some(false),
        }))
    }
}

pub fn generate_define_statements(
    table_name: &str,
    table_config: &TableConfig,
    query_details: &HashMap<String, TableConfig>,
    server_only: &HashMap<String, StructConfig>,
    enums: &HashMap<String, TaggedUnion>,
    full_refresh_mode: bool,
) -> String {
    info!(
        "Generating define statements for table {table_name}, full_refresh_mode: {full_refresh_mode}"
    );
    debug!(
        query_details_count = query_details.len(),
        server_only_count = server_only.len(),
        enum_count = enums.len(),
        "Context sizes"
    );
    trace!("Table config: {:?}", table_config);
    let table_type = if let Some(relation) = &table_config.relation {
        debug!(
            table = %table_name,
            from = ?relation.from,
            to = ?relation.to,
            "Table is a relation."
        );
        let from_clause = relation.from.join(" | ");
        let to_clause = relation.to.join(" | ");
        format!("RELATION FROM {} TO {}", from_clause, to_clause)
    } else {
        debug!(table_name = %table_name, "Table is normal type");
        "NORMAL".to_string()
    };
    let select_permissions = table_config
        .permissions
        .as_ref()
        .and_then(|p| p.select_permissions.as_deref())
        .unwrap_or("FULL");
    let create_permissions = table_config
        .permissions
        .as_ref()
        .and_then(|p| p.create_permissions.as_deref())
        .unwrap_or("FULL");
    let update_permissions = table_config
        .permissions
        .as_ref()
        .and_then(|p| p.update_permissions.as_deref())
        .unwrap_or("FULL");
    let delete_permissions = table_config
        .permissions
        .as_ref()
        .and_then(|p| p.delete_permissions.as_deref())
        .unwrap_or("FULL");

    let mut output = "".to_owned();
    debug!(table_name = %table_name, "Starting statement generation");

    output.push_str(&format!(
        "DEFINE TABLE OVERWRITE {table_name} SCHEMAFULL TYPE {table_type} CHANGEFEED 3d PERMISSIONS FOR select {select_permissions} FOR update {update_permissions} FOR create {create_permissions} FOR delete {delete_permissions};\n"
    ));

    debug!(table_name = %table_name, field_count = table_config.struct_config.fields.len(), "Processing table fields");
    for table_field in &table_config.struct_config.fields {
        // if struct field is an edge it should not be defined in the table itself
        if table_field.edge_config.is_none()
            && (table_field.field_name != "in"
                && table_field.field_name != "out"
                && table_field.field_name != "id")
        {
            if table_field.define_config.is_some() {
                match table_field.generate_define_statement(
                    enums.clone(),
                    server_only.clone(),
                    query_details.clone(),
                    &table_name.to_string(),
                ) {
                    Ok(statement) => output.push_str(&statement),
                    Err(e) => {
                        error!(
                            table_name = %table_name,
                            field_name = %table_field.field_name,
                            error = %e,
                            "Failed to generate define statement for field"
                        );
                        // Continue with a fallback definition
                        output.push_str(&format!(
                            "DEFINE FIELD OVERWRITE {} ON TABLE {} TYPE any PERMISSIONS FULL;\n",
                            table_field.field_name, table_name
                        ));
                    }
                }
            } else {
                output.push_str(&format!(
                    "DEFINE FIELD OVERWRITE {} ON TABLE {} TYPE any PERMISSIONS FULL;\n",
                    table_field.field_name, table_name
                ))
            }
        }
    }

    if !table_config.events.is_empty() {
        trace!(
            table_name = %table_name,
            event_count = table_config.events.len(),
            "Appending event statements"
        );
    }

    for event in &table_config.events {
        let statement = event.statement.trim();
        trace!(table_name = %table_name, "Adding event statement: {}", statement);
        output.push_str(statement);
        if !statement.ends_with(';') {
            output.push(';');
        }
        output.push('\n');
    }

    info!(table_name = %table_name, output_length = output.len(), "Completed define statements generation");
    trace!(table_name = %table_name, "Generated output: {}", output);
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schemasync::EventConfig;
    use crate::types::{StructConfig, TaggedUnion};

    #[test]
    fn generate_define_statements_appends_events() {
        let table_config = TableConfig {
            table_name: "user".to_string(),
            struct_config: StructConfig {
                struct_name: "User".to_string(),
                fields: Vec::new(),
                validators: Vec::new(),
            },
            relation: None,
            permissions: None,
            mock_generation_config: None,
            events: vec![EventConfig {
                statement: "DEFINE EVENT user_change ON TABLE user WHEN true THEN { RETURN true };"
                    .to_string(),
            }],
        };

        let query_details: HashMap<String, TableConfig> = HashMap::new();
        let server_only: HashMap<String, StructConfig> = HashMap::new();
        let enums: HashMap<String, TaggedUnion> = HashMap::new();

        let statements = generate_define_statements(
            "user",
            &table_config,
            &query_details,
            &server_only,
            &enums,
            false,
        );

        assert!(statements.contains("DEFINE EVENT user_change ON TABLE user"));
        assert!(statements.trim().ends_with(';'));
    }
}
