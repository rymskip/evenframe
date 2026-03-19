use crate::{
    schemasync::table::TableConfig,
    types::{StructConfig, TaggedUnion},
};
use std::collections::HashMap;
use tracing::{debug, error, info, trace};

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

    // Generate DEFINE INDEX statements for unique fields
    for table_field in &table_config.struct_config.fields {
        if table_field.unique {
            debug!(
                table_name = %table_name,
                field_name = %table_field.field_name,
                "Generating unique index for field"
            );
            output.push_str(&format!(
                "DEFINE INDEX OVERWRITE idx_{}_{} ON TABLE {} FIELDS {} UNIQUE;\n",
                table_name, table_field.field_name, table_name, table_field.field_name
            ));
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
    use crate::schemasync::{DefineConfig, EventConfig};
    use crate::types::{FieldType, StructConfig, StructField, TaggedUnion};

    #[test]
    fn generate_define_statements_appends_events() {
        let table_config = TableConfig {
            table_name: "user".to_string(),
            struct_config: StructConfig {
                struct_name: "User".to_string(),
                fields: Vec::new(),
                validators: Vec::new(),
                doccom: None,
                macroforge_derives: vec![],
                annotations: vec![],
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

    #[test]
    fn generate_computed_field_statement() {
        dotenv::dotenv().ok();
        let field = StructField {
            field_name: "upper_name".to_string(),
            field_type: FieldType::String,
            edge_config: None,
            define_config: Some(DefineConfig {
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
                computed: Some("string::uppercase($value.name)".to_string()),
                comment: None,
            }),
            format: None,
            validators: Vec::new(),
            always_regenerate: false,
            doccom: None,
            annotations: vec![],
            unique: false,
            mock_plugin: None,
        };

        let result = field
            .generate_define_statement(
                HashMap::new(),
                HashMap::new(),
                HashMap::new(),
                &"user".to_string(),
            )
            .unwrap();

        assert!(result.contains("COMPUTED string::uppercase($value.name)"));
        assert!(result.contains("TYPE string"));
        assert!(!result.contains("DEFAULT"));
        assert!(!result.contains("VALUE"));
        assert!(!result.contains("ASSERT"));
        assert!(!result.contains("READONLY"));
    }

    #[test]
    fn generate_computed_field_with_comment() {
        dotenv::dotenv().ok();
        let field = StructField {
            field_name: "upper_name".to_string(),
            field_type: FieldType::String,
            edge_config: None,
            define_config: Some(DefineConfig {
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
                computed: Some("string::uppercase($value.name)".to_string()),
                comment: Some("Auto-uppercased name".to_string()),
            }),
            format: None,
            validators: Vec::new(),
            always_regenerate: false,
            doccom: None,
            annotations: vec![],
            unique: false,
            mock_plugin: None,
        };

        let result = field
            .generate_define_statement(
                HashMap::new(),
                HashMap::new(),
                HashMap::new(),
                &"user".to_string(),
            )
            .unwrap();

        assert!(result.contains("COMPUTED string::uppercase($value.name)"));
        assert!(result.contains("COMMENT 'Auto-uppercased name'"));
    }

    #[test]
    fn generate_regular_field_with_comment() {
        dotenv::dotenv().ok();
        let field = StructField {
            field_name: "email".to_string(),
            field_type: FieldType::String,
            edge_config: None,
            define_config: Some(DefineConfig {
                select_permissions: Some("FULL".to_string()),
                update_permissions: Some("FULL".to_string()),
                create_permissions: Some("FULL".to_string()),
                data_type: None,
                should_skip: false,
                default: Some("''".to_string()),
                default_always: None,
                value: None,
                assert: None,
                readonly: None,
                flexible: Some(false),
                computed: None,
                comment: Some("User email address".to_string()),
            }),
            format: None,
            validators: Vec::new(),
            always_regenerate: false,
            doccom: None,
            annotations: vec![],
            unique: false,
            mock_plugin: None,
        };

        let result = field
            .generate_define_statement(
                HashMap::new(),
                HashMap::new(),
                HashMap::new(),
                &"user".to_string(),
            )
            .unwrap();

        assert!(result.contains("TYPE string"));
        assert!(result.contains("DEFAULT ''"));
        assert!(result.contains("COMMENT 'User email address'"));
        assert!(!result.contains("COMPUTED"));
    }

    #[test]
    fn generate_define_statements_includes_unique_index() {
        dotenv::dotenv().ok();
        let table_config = TableConfig {
            table_name: "user".to_string(),
            struct_config: StructConfig {
                struct_name: "User".to_string(),
                fields: vec![
                    StructField {
                        field_name: "email".to_string(),
                        field_type: FieldType::String,
                        edge_config: None,
                        define_config: Some(DefineConfig {
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
                            computed: None,
                            comment: None,
                        }),
                        format: None,
                        validators: Vec::new(),
                        always_regenerate: false,
                        doccom: None,
                        annotations: vec![],
                        unique: true,
                        mock_plugin: None,
                    },
                    StructField {
                        field_name: "name".to_string(),
                        field_type: FieldType::String,
                        edge_config: None,
                        define_config: Some(DefineConfig {
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
                            computed: None,
                            comment: None,
                        }),
                        format: None,
                        validators: Vec::new(),
                        always_regenerate: false,
                        doccom: None,
                        annotations: vec![],
                        unique: false,
                        mock_plugin: None,
                    },
                ],
                validators: Vec::new(),
                doccom: None,
                macroforge_derives: vec![],
                annotations: vec![],
            },
            relation: None,
            permissions: None,
            mock_generation_config: None,
            events: vec![],
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

        // Should contain unique index for email but not for name
        assert!(
            statements.contains(
                "DEFINE INDEX OVERWRITE idx_user_email ON TABLE user FIELDS email UNIQUE;"
            )
        );
        assert!(!statements.contains("idx_user_name"));
    }
}
