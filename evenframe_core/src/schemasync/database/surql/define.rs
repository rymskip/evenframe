use crate::{
    error::{EvenframeError, Result},
    schemasync::table::{TableConfig, surql_ident},
    types::{StructConfig, TaggedUnion},
};
use std::collections::BTreeMap;
use tracing::{debug, info, trace};

/// The DEFINE statements for `table_name`: the table, its fields, indexes
/// and events. Fails naming the field whose type has no definition.
pub fn generate_define_statements(
    table_name: &str,
    table_config: &TableConfig,
    query_details: &BTreeMap<String, TableConfig>,
    server_only: &BTreeMap<String, StructConfig>,
    enums: &BTreeMap<String, TaggedUnion>,
    registry: &crate::types::ForeignTypeRegistry,
    options: impl Into<crate::schemasync::config::SurqlOptions>,
) -> Result<String> {
    let options = options.into();
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

        // A record whose keys are partly known only from a value cannot list
        // them all, so its table takes any key.
        let schema = if table_config.struct_config.is_open() {
            "SCHEMALESS"
        } else {
            "SCHEMAFULL"
        };
        output.push_str(&format!(
            "DEFINE TABLE OVERWRITE {table_name} {schema} TYPE {table_type} CHANGEFEED 3d PERMISSIONS FOR select {select_permissions} FOR update {update_permissions} FOR create {create_permissions} FOR delete {delete_permissions};\n"
        ));
    }

    debug!(table_name = %table_name, field_count = table_config.struct_config.fields.len(), "Processing table fields");
    for table_field in &table_config.struct_config.fields {
        // An edge is not defined in the table itself, and a flattened
        // field's keys sit beside the record's own, under no key of its own.
        if table_field.edge_config.is_none()
            && !table_field.effective().wire.storage.flatten
            && !matches!(table_field.db_name(), "in" | "out" | "id")
        {
            if table_field.define_config.is_some() {
                let statement = table_field
                    .generate_define_statement(
                        enums,
                        server_only,
                        query_details,
                        &table_name.to_string(),
                        registry,
                        options,
                    )
                    .map_err(|error| {
                        EvenframeError::database(format!(
                            "Cannot define field '{}' on table '{table_name}': {error}",
                            table_field.field_name
                        ))
                    })?;
                output.push_str(&statement);
            } else {
                output.push_str(&format!(
                    "DEFINE FIELD OVERWRITE {} ON TABLE {} TYPE any PERMISSIONS FULL;\n",
                    surql_ident(table_field.db_name()),
                    table_name
                ))
            }
        }
    }

    // Generate DEFINE INDEX statements for field-level #[unique] and
    // struct-level #[indexes(...)] entries.
    for index in table_config.all_indexes(table_name, server_only) {
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
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::{BTreeMap, TableConfig, generate_define_statements};
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
        )
        .unwrap();

        assert!(statements.contains("DEFINE EVENT user_change ON TABLE user"));
        assert!(statements.trim().ends_with(';'));
    }

    #[test]
    fn generate_computed_field_statement() {
        dotenvy::dotenv().ok();
        let field = StructField {
            field_name: "upper_name".to_string(),
            wire: Default::default(),
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
            output_override: None,
            raw_attributes: BTreeMap::new(),
            validator_overrides: Default::default(),
        };

        let result = field
            .generate_define_statement(
                &BTreeMap::new(),
                &BTreeMap::new(),
                &BTreeMap::new(),
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
        dotenvy::dotenv().ok();
        let field = StructField {
            field_name: "upper_name".to_string(),
            wire: Default::default(),
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
            output_override: None,
            raw_attributes: BTreeMap::new(),
            validator_overrides: Default::default(),
        };

        let result = field
            .generate_define_statement(
                &BTreeMap::new(),
                &BTreeMap::new(),
                &BTreeMap::new(),
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
        dotenvy::dotenv().ok();
        let field = StructField {
            field_name: "email".to_string(),
            wire: Default::default(),
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
            output_override: None,
            raw_attributes: BTreeMap::new(),
            validator_overrides: Default::default(),
        };

        let result = field
            .generate_define_statement(
                &BTreeMap::new(),
                &BTreeMap::new(),
                &BTreeMap::new(),
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
        dotenvy::dotenv().ok();
        let table_config = TableConfig {
            table_name: "user".to_string(),
            struct_config: StructConfig {
                resolve_only: false,
                struct_name: "User".to_string(),
                fields: vec![
                    StructField {
                        field_name: "email".to_string(),
                        wire: Default::default(),
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
                        output_override: None,
                        raw_attributes: BTreeMap::new(),
                        validator_overrides: Default::default(),
                    },
                    StructField {
                        field_name: "name".to_string(),
                        wire: Default::default(),
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
                        output_override: None,
                        raw_attributes: BTreeMap::new(),
                        validator_overrides: Default::default(),
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
        )
        .unwrap();

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
        dotenvy::dotenv().ok();
        use crate::schemasync::{Bm25, IndexConfig, IndexKind};

        let make_field = |name: &str| StructField {
            field_name: name.to_string(),
            wire: Default::default(),
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
            output_override: None,
            raw_attributes: BTreeMap::new(),
            validator_overrides: Default::default(),
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
        )
        .unwrap();

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
            wire: Default::default(),
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
            output_override: None,
            raw_attributes: BTreeMap::new(),
            validator_overrides: Default::default(),
        };

        let reg = crate::types::ForeignTypeRegistry::default();
        let gen_stmt = |f: StructField, allow_scripting: bool| {
            f.generate_define_statement(
                &BTreeMap::new(),
                &BTreeMap::new(),
                &BTreeMap::new(),
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
            stmt.contains(
                "ASSERT string::matches($value, \"^[0-9A-Za-z_%+.-]+@[0-9.A-Za-z-]+\\\\.[A-Za-z]{2,}$\") AND string::len($value) <= 50"
            ),
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
            stmt.contains(
                "ASSERT ($value != NONE) AND (string::matches($value, \"^[0-9A-Za-z_%+.-]+@[0-9.A-Za-z-]+\\\\.[A-Za-z]{2,}$\"))"
            ),
            "merged manual+validator ASSERT missing; got: {stmt}"
        );

        // A scripting-only validator is emitted as embedded JS when allowed and
        // omitted entirely when scripting is disabled.
        let json = || {
            make(
                vec![Validator::StringValidator(StringValidator::Json)],
                None,
            )
        };
        assert!(
            gen_stmt(json(), true).contains("ASSERT function($value)"),
            "JS ASSERT missing when scripting enabled"
        );
        assert!(
            !gen_stmt(json(), false).contains("ASSERT"),
            "JS ASSERT must be omitted when scripting disabled"
        );
    }

    #[test]
    fn internally_tagged_named_payload_keeps_its_discriminator() {
        use crate::types::{EnumRepresentation, Variant, VariantData};
        let payload = StructConfig {
            struct_name: "Created".to_string(),
            fields: vec![StructField {
                field_name: "initial_data".to_string(),
                field_type: FieldType::Option(Box::new(FieldType::String)),
                ..Default::default()
            }],
            ..Default::default()
        };
        let structs = BTreeMap::from([("Created".to_string(), payload)]);
        let enums = BTreeMap::from([(
            "Action".to_string(),
            TaggedUnion {
                enum_name: "Action".to_string(),
                representation: EnumRepresentation::InternallyTagged {
                    tag: "variant".to_string(),
                },
                variants: vec![Variant {
                    name: "Created".to_string(),
                    data: Some(VariantData::DataStructureRef(FieldType::Other(
                        "Created".to_string(),
                    ))),
                    wire: Default::default(),
                    doccom: None,
                    annotations: Vec::new(),
                    output_override: None,
                    raw_attributes: BTreeMap::new(),
                    is_default: false,
                    element_validators: Vec::new(),
                    element_validator_overrides: Vec::new(),
                }],
                doccom: None,
                macroforge_derives: Vec::new(),
                annotations: Vec::new(),
                pipeline: Default::default(),
                rust_derives: Vec::new(),
                output_override: None,
                resolve_only: false,
                raw_attributes: BTreeMap::new(),
            },
        )]);
        let field = StructField {
            field_name: "action".to_string(),
            field_type: FieldType::Other("Action".to_string()),
            define_config: DefineConfig::parse(&syn::parse_quote! {
                #[define_field_statement()]
                action: Action
            })
            .unwrap(),
            ..Default::default()
        };
        let statement = field
            .generate_define_statement(
                &enums,
                &structs,
                &BTreeMap::new(),
                &"entry".to_string(),
                &crate::types::ForeignTypeRegistry::default(),
                false,
            )
            .unwrap();
        assert!(statement.contains("variant: \"Created\""), "{statement}");
        assert!(
            statement.contains("initial_data: option<string>"),
            "{statement}"
        );
        assert!(
            statement.contains("DEFAULT { variant: 'Created', initial_data: NONE }"),
            "{statement}"
        );
    }

    #[test]
    fn null_option_policy_controls_schema_defaults_and_assertions() {
        use crate::schemasync::config::{OptionNone, SurqlOptions};
        use crate::validator::{StringValidator, Validator};
        let field = StructField {
            field_name: "bio".to_string(),
            field_type: FieldType::Option(Box::new(FieldType::String)),
            define_config: DefineConfig::parse(&syn::parse_quote! {
                #[define_field_statement()]
                bio: Option<String>
            })
            .unwrap(),
            validators: vec![Validator::StringValidator(StringValidator::MaxLength(500))],
            ..Default::default()
        };
        let render = |field: &StructField| {
            field
                .generate_define_statement(
                    &BTreeMap::new(),
                    &BTreeMap::new(),
                    &BTreeMap::new(),
                    &"profile".to_string(),
                    &crate::types::ForeignTypeRegistry::default(),
                    SurqlOptions {
                        option_none: OptionNone::Null,
                        ..Default::default()
                    },
                )
                .unwrap()
        };
        let statement = render(&field);
        assert!(
            statement.contains("TYPE null | string DEFAULT NULL"),
            "{statement}"
        );
        assert!(statement.contains("$value = NULL OR"), "{statement}");
        let unannotated = render(&StructField {
            define_config: None,
            ..field
        });
        assert!(
            unannotated.contains("TYPE null | string DEFAULT NULL"),
            "{unannotated}"
        );
    }

    #[cfg(feature = "schemasync")]
    #[tokio::test]
    async fn optional_values_round_trip_under_both_database_policies() {
        use crate::schemasync::config::{OptionNone, SurqlOptions};
        use crate::schemasync::database::surql::execute::execute_and_validate;
        use surrealdb::Surreal;
        use surrealdb::engine::local::Mem;
        use surrealdb::types::{ToSql, Value};

        let database = Surreal::new::<Mem>(()).await.expect("create test database");
        database
            .use_ns("test")
            .use_db("test")
            .await
            .expect("select test database");
        for option_none in [OptionNone::None, OptionNone::Null] {
            let table_name = format!("samples_{}", option_none.literal().to_lowercase());
            let options = SurqlOptions {
                option_none,
                ..Default::default()
            };
            let mut statements = format!("DEFINE TABLE {table_name} SCHEMAFULL;\n");
            for field in [
                StructField {
                    field_name: "values".to_string(),
                    field_type: FieldType::Vec(Box::new(FieldType::Option(Box::new(
                        FieldType::I32,
                    )))),
                    ..Default::default()
                },
                StructField {
                    field_name: "note".to_string(),
                    field_type: FieldType::Option(Box::new(FieldType::String)),
                    ..Default::default()
                },
            ] {
                statements.push_str(
                    &field
                        .generate_define_statement(
                            &BTreeMap::new(),
                            &BTreeMap::new(),
                            &BTreeMap::new(),
                            &table_name,
                            &crate::types::ForeignTypeRegistry::default(),
                            options,
                        )
                        .expect("render policy-aware fields"),
                );
            }
            execute_and_validate(&database, &statements, "define test policy", &table_name)
                .await
                .expect("apply policy-aware schema");
            let expected = vec![Some(7_i32), None];
            execute_and_validate(
                &database,
                &format!(
                    "CREATE {table_name}:example SET values = {};",
                    option_none.into_value(expected.clone()).to_sql()
                ),
                "write configured absence",
                &table_name,
            )
            .await
            .expect("write configured absence with conflict replay");
            let mut response = database.query(format!(
                "RETURN (SELECT VALUE values FROM ONLY {table_name}:example); RETURN (SELECT VALUE note FROM ONLY {table_name}:example);"
            )).await.expect("read configured absence").check().expect("check every statement");
            let stored: Value = response.take(0).expect("read stored array");
            assert_eq!(stored, option_none.into_value(expected.clone()));
            assert_eq!(
                option_none
                    .read_value::<Vec<Option<i32>>>(stored)
                    .expect("decode configured absence"),
                expected
            );
            let note: Value = response.take(1).expect("read absent optional field");
            assert_eq!(note, option_none.into_value(None::<String>));
        }
    }

    #[test]
    fn optional_field_assert_is_none_guarded() {
        use crate::validator::{StringValidator, Validator};

        // Option<String> is emitted as `option<string>`; the validator assert
        // must skip NONE or inserts of unset values fail with
        // "string::len() ... found NONE".
        let field = StructField {
            field_name: "bio".to_string(),
            wire: Default::default(),
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
            output_override: None,
            raw_attributes: BTreeMap::new(),
            validator_overrides: Default::default(),
        };

        let merged = field
            .merged_assert(true)
            .expect("optional field with validators should produce an assert");
        assert_eq!(merged, "$value = NONE OR (string::len($value) <= 500)");

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
            wire: Default::default(),
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
            output_override: None,
            raw_attributes: BTreeMap::new(),
            validator_overrides: Default::default(),
        };
        let reg = crate::types::ForeignTypeRegistry::default();
        let gen_stmt = |f: StructField| {
            f.generate_define_statement(
                &BTreeMap::new(),
                &BTreeMap::new(),
                &BTreeMap::new(),
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
