use crate::{
    schemasync::table::TableConfig,
    types::{StructConfig, TaggedUnion},
};
use std::collections::BTreeMap;
use tracing::{debug, error, info, trace};

pub fn generate_define_statements(
    table_name: &str,
    table_config: &TableConfig,
    query_details: &BTreeMap<String, TableConfig>,
    server_only: &BTreeMap<String, StructConfig>,
    enums: &BTreeMap<String, TaggedUnion>,
    registry: &crate::types::ForeignTypeRegistry,
    allow_scripting: bool,
) -> String {
    info!("Generating define statements for table {table_name}");
    debug!(
        query_details_count = query_details.len(),
        server_only_count = server_only.len(),
        enum_count = enums.len(),
        "Context sizes"
    );
    trace!("Table config: {:?}", table_config);

    let mut output = String::new();
    debug!(table_name = %table_name, "Starting statement generation");

    {
        let table_type = if let Some(relation) = &table_config.relation {
            debug!(
                table = %table_name,
                from = ?relation.from,
                to = ?relation.to,
                "Table is a relation."
            );
            if relation.from.is_empty() && relation.to.is_empty() {
                // Unconstrained relation edge: SurrealDB accepts `TYPE RELATION`
                // with no `FROM … TO …` clause, letting the edge connect any
                // tables. Used by audit edges (e.g. `did`) that link every
                // entity across every product and cannot enumerate them.
                "RELATION".to_string()
            } else {
                let from_clause = relation.from.join(" | ");
                let to_clause = relation.to.join(" | ");
                format!("RELATION FROM {} TO {}", from_clause, to_clause)
            }
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

        output.push_str(&format!(
            "DEFINE TABLE OVERWRITE {table_name} SCHEMAFULL TYPE {table_type} CHANGEFEED 3d PERMISSIONS FOR select {select_permissions} FOR update {update_permissions} FOR create {create_permissions} FOR delete {delete_permissions};\n"
        ));
    }

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
                    registry,
                    allow_scripting,
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

    // Generate DEFINE INDEX statements for field-level #[unique] and
    // struct-level #[index(...)] attributes.
    for index in table_config.all_indexes(table_name) {
        debug!(
            table_name = %table_name,
            fields = ?index.fields,
            kind = ?index.kind,
            "Generating index"
        );
        output.push_str(&index.define_statement(table_name));
        output.push('\n');
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
                resolve_only: false,
                struct_name: "User".to_string(),
                fields: Vec::new(),
                validators: Vec::new(),
                doccom: None,
                macroforge_derives: vec![],
                annotations: vec![],
                pipeline: crate::types::Pipeline::default(),
                rust_derives: vec![],
                output_override: None,
                raw_attributes: BTreeMap::new(),
            },
            relation: None,
            permissions: None,
            mock_generation_config: None,
            events: vec![EventConfig {
                statement: "DEFINE EVENT user_change ON TABLE user WHEN true THEN { RETURN true };"
                    .to_string(),
            }],
            indexes: vec![],
            output_override: None,
        };

        let query_details: BTreeMap<String, TableConfig> = BTreeMap::new();
        let server_only: BTreeMap<String, StructConfig> = BTreeMap::new();
        let enums: BTreeMap<String, TaggedUnion> = BTreeMap::new();

        let statements = generate_define_statements(
            "user",
            &table_config,
            &query_details,
            &server_only,
            &enums,
            &crate::types::ForeignTypeRegistry::default(),
            true,
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
            output_override: None,
            raw_attributes: BTreeMap::new(),
        };

        let result = field
            .generate_define_statement(
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::new(),
                &"user".to_string(),
                &crate::types::ForeignTypeRegistry::default(),
                true,
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
            output_override: None,
            raw_attributes: BTreeMap::new(),
        };

        let result = field
            .generate_define_statement(
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::new(),
                &"user".to_string(),
                &crate::types::ForeignTypeRegistry::default(),
                true,
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
            output_override: None,
            raw_attributes: BTreeMap::new(),
        };

        let result = field
            .generate_define_statement(
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::new(),
                &"user".to_string(),
                &crate::types::ForeignTypeRegistry::default(),
                true,
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
                resolve_only: false,
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
                        output_override: None,
                        raw_attributes: BTreeMap::new(),
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
                        output_override: None,
                        raw_attributes: BTreeMap::new(),
                    },
                ],
                validators: Vec::new(),
                doccom: None,
                macroforge_derives: vec![],
                annotations: vec![],
                pipeline: crate::types::Pipeline::default(),
                rust_derives: vec![],
                output_override: None,
                raw_attributes: BTreeMap::new(),
            },
            relation: None,
            permissions: None,
            mock_generation_config: None,
            events: vec![],
            indexes: vec![],
            output_override: None,
        };

        let query_details: BTreeMap<String, TableConfig> = BTreeMap::new();
        let server_only: BTreeMap<String, StructConfig> = BTreeMap::new();
        let enums: BTreeMap<String, TaggedUnion> = BTreeMap::new();

        let statements = generate_define_statements(
            "user",
            &table_config,
            &query_details,
            &server_only,
            &enums,
            &crate::types::ForeignTypeRegistry::default(),
            true,
        );

        // Should contain unique index for email but not for name
        assert!(
            statements.contains(
                "DEFINE INDEX OVERWRITE idx_user_email ON TABLE user FIELDS email UNIQUE;"
            )
        );
        assert!(!statements.contains("idx_user_name"));
    }

    #[test]
    fn generate_define_statements_includes_composite_index() {
        dotenv::dotenv().ok();
        use crate::schemasync::{Bm25, IndexConfig, IndexKind};

        let make_field = |name: &str| StructField {
            field_name: name.to_string(),
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
            output_override: None,
            raw_attributes: BTreeMap::new(),
        };

        let table_config = TableConfig {
            table_name: "reaction".to_string(),
            struct_config: StructConfig {
                resolve_only: false,
                struct_name: "Reaction".to_string(),
                fields: vec![
                    make_field("user"),
                    make_field("message"),
                    make_field("created_at"),
                ],
                validators: Vec::new(),
                doccom: None,
                macroforge_derives: vec![],
                annotations: vec![],
                pipeline: crate::types::Pipeline::default(),
                rust_derives: vec![],
                output_override: None,
                raw_attributes: BTreeMap::new(),
            },
            relation: None,
            permissions: None,
            mock_generation_config: None,
            events: vec![],
            indexes: vec![
                IndexConfig {
                    fields: vec!["user".to_string(), "message".to_string()],
                    name: None,
                    kind: IndexKind::Unique,
                    comment: None,
                    concurrently: false,
                },
                IndexConfig {
                    fields: vec!["created_at".to_string()],
                    name: None,
                    kind: IndexKind::Standard,
                    comment: None,
                    concurrently: false,
                },
                IndexConfig {
                    fields: vec!["message".to_string()],
                    name: Some("reaction_search".to_string()),
                    kind: IndexKind::FullText {
                        analyzer: Some("en".to_string()),
                        bm25: Some(Bm25::Default),
                        highlights: true,
                    },
                    comment: None,
                    concurrently: true,
                },
            ],
            output_override: None,
        };

        let query_details: BTreeMap<String, TableConfig> = BTreeMap::new();
        let server_only: BTreeMap<String, StructConfig> = BTreeMap::new();
        let enums: BTreeMap<String, TaggedUnion> = BTreeMap::new();

        let statements = generate_define_statements(
            "reaction",
            &table_config,
            &query_details,
            &server_only,
            &enums,
            &crate::types::ForeignTypeRegistry::default(),
            true,
        );

        assert!(
            statements.contains(
                "DEFINE INDEX OVERWRITE idx_reaction_user_message ON TABLE reaction FIELDS user, message UNIQUE;"
            ),
            "missing composite UNIQUE index line; output was:\n{}",
            statements
        );
        assert!(
            statements.contains(
                "DEFINE INDEX OVERWRITE idx_reaction_created_at ON TABLE reaction FIELDS created_at;"
            ),
            "missing single-column non-unique index line; output was:\n{}",
            statements
        );
        assert!(
            statements.contains(
                "DEFINE INDEX OVERWRITE reaction_search ON TABLE reaction FIELDS message FULLTEXT ANALYZER en BM25 HIGHLIGHTS CONCURRENTLY;"
            ),
            "missing fulltext index line; output was:\n{}",
            statements
        );
    }

    #[test]
    fn validator_asserts_are_emitted_in_define_field() {
        use crate::validator::{StringValidator, Validator};

        let make = |validators: Vec<Validator>, assert: Option<String>| StructField {
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
                assert,
                readonly: None,
                flexible: Some(false),
                computed: None,
                comment: None,
            }),
            format: None,
            validators,
            always_regenerate: false,
            doccom: None,
            annotations: vec![],
            unique: false,
            mock_plugin: None,
            output_override: None,
            raw_attributes: BTreeMap::new(),
        };

        let reg = crate::types::ForeignTypeRegistry::default();
        let gen_stmt = |f: StructField, allow_scripting: bool| {
            f.generate_define_statement(
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::new(),
                &"user".to_string(),
                &reg,
                allow_scripting,
            )
            .unwrap()
        };

        // Validators alone -> combined native ASSERT lands in the DEFINE FIELD.
        let stmt = gen_stmt(
            make(
                vec![
                    Validator::StringValidator(StringValidator::Email),
                    Validator::StringValidator(StringValidator::MaxLength(50)),
                ],
                None,
            ),
            true,
        );
        assert!(
            stmt.contains("ASSERT string::is_email($value) AND string::len($value) <= 50"),
            "validator ASSERT missing; got: {stmt}"
        );

        // Manual assert + validators -> each side parenthesized and AND-joined.
        let stmt = gen_stmt(
            make(
                vec![Validator::StringValidator(StringValidator::Email)],
                Some("$value != NONE".to_string()),
            ),
            true,
        );
        assert!(
            stmt.contains("ASSERT ($value != NONE) AND (string::is_email($value))"),
            "merged manual+validator ASSERT missing; got: {stmt}"
        );

        // A scripting-only validator is emitted as embedded JS when allowed and
        // omitted entirely when scripting is disabled.
        let cc = || {
            make(
                vec![Validator::StringValidator(StringValidator::CreditCard)],
                None,
            )
        };
        assert!(
            gen_stmt(cc(), true).contains("ASSERT function($value)"),
            "JS ASSERT missing when scripting enabled"
        );
        assert!(
            !gen_stmt(cc(), false).contains("ASSERT"),
            "JS ASSERT must be omitted when scripting disabled"
        );
    }

    #[test]
    fn optional_field_assert_is_null_guarded() {
        use crate::validator::{StringValidator, Validator};

        // Option<String> is emitted as `null | string DEFAULT NULL`; the
        // validator assert must skip NULL or inserts of unset values fail with
        // "string::len() ... found NULL".
        let field = StructField {
            field_name: "bio".to_string(),
            field_type: FieldType::Option(Box::new(FieldType::String)),
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
            validators: vec![Validator::StringValidator(StringValidator::MaxLength(500))],
            always_regenerate: false,
            doccom: None,
            annotations: vec![],
            unique: false,
            mock_plugin: None,
            output_override: None,
            raw_attributes: BTreeMap::new(),
        };

        let merged = field
            .merged_assert(true)
            .expect("optional field with validators should produce an assert");
        assert_eq!(merged, "$value = NULL OR (string::len($value) <= 500)");

        // Non-optional fields are not guarded.
        let mut required = field.clone();
        required.field_type = FieldType::String;
        assert_eq!(
            required.merged_assert(true).unwrap(),
            "string::len($value) <= 500"
        );
    }

    #[test]
    fn default_violating_validators_is_omitted() {
        use crate::validator::{StringValidator, Validator};

        let make = |validators: Vec<Validator>| StructField {
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
            validators,
            always_regenerate: false,
            doccom: None,
            annotations: vec![],
            unique: false,
            mock_plugin: None,
            output_override: None,
            raw_attributes: BTreeMap::new(),
        };
        let reg = crate::types::ForeignTypeRegistry::default();
        let gen_stmt = |f: StructField| {
            f.generate_define_statement(
                BTreeMap::new(),
                BTreeMap::new(),
                BTreeMap::new(),
                &"t".to_string(),
                &reg,
                true,
            )
            .unwrap()
        };

        // NonEmpty rejects the `''` fallback default -> no DEFAULT (field required).
        let s = gen_stmt(make(vec![Validator::StringValidator(
            StringValidator::NonEmpty,
        )]));
        assert!(!s.contains("DEFAULT"), "default should be omitted: {s}");
        assert!(s.contains("ASSERT string::len($value) > 0"));

        // MaxLength accepts `''` -> fallback default is kept.
        let s = gen_stmt(make(vec![Validator::StringValidator(
            StringValidator::MaxLength(50),
        )]));
        assert!(s.contains("DEFAULT ''"), "default should be kept: {s}");
    }
}
