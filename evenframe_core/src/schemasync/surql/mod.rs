pub mod access;
pub mod assert;
pub mod define;
pub mod execute;
pub mod insert;
pub mod remove;
pub mod upsert;
pub mod value;

use crate::{
    registry,
    schemasync::{edge::EdgeConfig, table::TableConfig},
    types::{FieldType, StructField},
};
use convert_case::{Case, Casing};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryType {
    Create,
    Update,
    Select,
}

/// Check if a field is nullable (wrapped in Option)
fn is_nullable_field(field: &StructField) -> bool {
    matches!(&field.field_type, FieldType::Option(_))
}

/// Check if a field is a nullable partial struct that needs conditional wrapping
fn is_nullable_partial_struct(field: &StructField, original_table: Option<&TableConfig>) -> bool {
    // Check if this is a struct field (partial update)
    if let FieldType::Struct(_) = &field.field_type
        && let Some(table) = original_table
        && let Some(original_field) = table
            .struct_config
            .fields
            .iter()
            .find(|f| f.field_name == field.field_name)
    {
        // Check if the original field type is Option<...>
        return is_nullable_field(original_field);
    }
    false
}

/// Check if any field needs null-preserving conditional logic
fn needs_null_preservation(field: &StructField, original_table: Option<&TableConfig>) -> bool {
    // Direct nullable check
    if is_nullable_field(field) {
        return true;
    }

    // Check for nullable partial structs
    if is_nullable_partial_struct(field, original_table) {
        return true;
    }

    false
}

/// Recursively unpacks Option and Vec types to find an inner RecordLink FieldType.
fn get_inner_record_link_type(field_type: &FieldType) -> Option<&FieldType> {
    match field_type {
        FieldType::Option(inner) => get_inner_record_link_type(inner),
        FieldType::Vec(inner) => get_inner_record_link_type(inner),
        ft @ FieldType::RecordLink(_) => Some(ft),
        _ => None,
    }
}

/// Generate edge update queries using SurrealDB's built-in array and record functions
/// This approach uses database-side diffing to efficiently handle edge operations
pub fn generate_edge_update_query<T: Serialize>(
    table_config: &TableConfig,
    object: &T,
    record_id: &str,
) -> String {
    let value = serde_json::to_value(object).expect("Failed to serialize object to JSON Value");

    // Collect all edge configurations from the table
    let edge_configs: Vec<&EdgeConfig> = table_config
        .struct_config
        .fields
        .iter()
        .filter_map(|field| field.edge_config.as_ref())
        .collect();

    if edge_configs.is_empty() {
        return String::new();
    }

    let formatted_record_id = value::to_surreal_string(
        &FieldType::EvenframeRecordId,
        &Value::String(record_id.to_string()),
    );

    let mut query_parts = Vec::new();
    let mut let_statements = Vec::new();
    let mut relation_statements = Vec::new();

    for field in &table_config.struct_config.fields {
        if let Some(edge_config) = &field.edge_config
            && let Some(field_value) = value.get(&field.field_name)
        {
            let edge_table = &edge_config.edge_name;

            // Generate incoming edge data array
            let incoming_edges = extract_incoming_edges_as_surql(ExtractEdgesParams {
                field_value,
                edge_config,
                record_id,
                field,
                let_statements: &mut let_statements,
                relation_statements: &mut relation_statements,
                depth: 0,
                current_table_name: &table_config.table_name,
            });

            if !incoming_edges.is_empty() {
                let edge_update_query = generate_single_edge_table_update(
                    edge_table,
                    &formatted_record_id,
                    &incoming_edges,
                    edge_config,
                    &table_config.table_name,
                );
                query_parts.push(edge_update_query);
            }
        }
    }

    query_parts.join(" ")
}

fn handle_record_link(
    field: &StructField,
    field_value: &Value,
    inner_type: &FieldType,
    let_statements: &mut Vec<String>,
    relation_statements: &mut Vec<String>,
    depth: u32,
) -> String {
    if field_value.is_string() {
        let record_id_str = field_value.as_str().unwrap();
        if let Err(e) = validate_record_id_table(record_id_str, &field.field_type) {
            panic!("Record ID validation failed: {}", e);
        }
        record_id_str.to_string()
    } else if field_value.is_object() {
        let mut create_new_object = true;
        let mut object_id = String::new();

        if let Some(id_val) = field_value.get("id")
            && let Some(id_str) = id_val.as_str()
            && !id_str.is_empty()
            && validate_record_id_table(id_str, &field.field_type).is_ok()
        {
            create_new_object = false;
            object_id = id_str.to_string();
        }

        if create_new_object {
            let struct_name = match inner_type {
                FieldType::Other(name) => name,
                _ => panic!("RecordLink inner type must be a named struct (FieldType::Other)"),
            };

            if let Some(nested_table_config) = registry::get_table_config(struct_name) {
                let (nested_query_body, _) = generate_recursive(
                    QueryType::Create,
                    &nested_table_config,
                    field_value,
                    None,
                    let_statements,
                    relation_statements,
                    depth + 1,
                );

                let var_name = format!(
                    "nested_{}_{}",
                    nested_table_config.table_name.to_case(Case::Snake),
                    depth
                );
                let let_stmt = format!(
                    "LET ${} = ({});",
                    var_name,
                    nested_query_body.trim_end_matches(';')
                );
                let_statements.push(let_stmt);

                format!("array::first(${}.id)", var_name)
            } else {
                value::to_surreal_string(&field.field_type, field_value)
            }
        } else {
            object_id
        }
    } else {
        value::to_surreal_string(&field.field_type, field_value)
    }
}

/// Parameters for extracting incoming edges as SurrealQL
struct ExtractEdgesParams<'a> {
    field_value: &'a Value,
    edge_config: &'a EdgeConfig,
    record_id: &'a str,
    field: &'a StructField,
    let_statements: &'a mut Vec<String>,
    relation_statements: &'a mut Vec<String>,
    depth: u32,
    current_table_name: &'a str,
}

/// Extract incoming edges as SurrealQL array literal
fn extract_incoming_edges_as_surql(params: ExtractEdgesParams) -> String {
    let ExtractEdgesParams {
        field_value,
        edge_config,
        record_id,
        field,
        let_statements,
        relation_statements,
        depth,
        current_table_name,
    } = params;
    // Handle null/missing values - return empty array
    if field_value.is_null() {
        return "[]".to_string();
    }

    let field_values = if let Some(arr) = field_value.as_array() {
        arr.to_vec()
    } else {
        vec![field_value.clone()]
    };

    let mut edge_objects = Vec::new();

    // Get edge table config from registry
    let edge_table_config =
        registry::get_table_config(&edge_config.edge_name.to_case(Case::Pascal));

    for value in field_values {
        if let Some(field_value_obj) = value.as_object() {
            let mut edge_obj_parts = Vec::new();
            let mut target_record_id = None;

            // Determine the direction and extract the target record ID
            let effective_direction = edge_config.resolve_direction_for_table(current_table_name);
            let target_field = match effective_direction {
                crate::schemasync::edge::Direction::From => "out",
                crate::schemasync::edge::Direction::To => "in",
                crate::schemasync::edge::Direction::Both => "out",
            };

            if let Some(id_val) = field_value_obj.get(target_field) {
                let id_str = if let Some(str_val) = id_val.as_str() {
                    // Direct string ID
                    str_val.to_string()
                } else if let Some(obj_val) = id_val.as_object() {
                    // Object with id field
                    if let Some(nested_id) = obj_val.get("id") {
                        if let Some(nested_id_str) = nested_id.as_str() {
                            nested_id_str.to_string()
                        } else {
                            continue; // Skip if id is not a string
                        }
                    } else {
                        continue; // Skip if no id field
                    }
                } else {
                    continue; // Skip if neither string nor object
                };

                // ID Validation for edge objects
                if let Some(config) = &edge_table_config
                    && let Some(edge_target_field) = config
                        .struct_config
                        .fields
                        .iter()
                        .find(|f| f.field_name == target_field)
                    && let Err(e) = validate_record_id_table(&id_str, &edge_target_field.field_type)
                {
                    panic!(
                        "Edge record ID validation failed for field '{}': {}",
                        target_field, e
                    );
                }
                target_record_id = Some(id_str);
            }

            if let Some(target_id) = target_record_id {
                let (in_record, out_record) = match effective_direction {
                    crate::schemasync::edge::Direction::From => (record_id.to_string(), target_id),
                    crate::schemasync::edge::Direction::To => (target_id, record_id.to_string()),
                    crate::schemasync::edge::Direction::Both => (record_id.to_string(), target_id),
                };

                let formatted_in = value::to_surreal_string(
                    &FieldType::EvenframeRecordId,
                    &Value::String(in_record),
                );
                let formatted_out = value::to_surreal_string(
                    &FieldType::EvenframeRecordId,
                    &Value::String(out_record),
                );

                edge_obj_parts.push(format!("in: {}", formatted_in));
                edge_obj_parts.push(format!("out: {}", formatted_out));

                // Loop through table_config.struct_config.fields like the main loop
                if let Some(config) = &edge_table_config {
                    for edge_field in &config.struct_config.fields {
                        let key = &edge_field.field_name;
                        if key == "in" || key == "out" || key == "id" {
                            continue;
                        }

                        if let Some(edge_prop_value) = field_value_obj.get(key) {
                            let surreal_string = if let FieldType::RecordLink(inner_type) =
                                &edge_field.field_type
                            {
                                handle_record_link(
                                    edge_field,
                                    edge_prop_value,
                                    inner_type,
                                    let_statements,
                                    relation_statements,
                                    depth,
                                )
                            } else {
                                value::to_surreal_string(&edge_field.field_type, edge_prop_value)
                            };
                            edge_obj_parts.push(format!("{}: {}", key, surreal_string));
                        }
                    }
                }

                edge_objects.push(format!("{{ {} }}", edge_obj_parts.join(", ")));
            }
        } else if let Some(id_str) = value.as_str() {
            // ID Validation for string IDs
            if let Some(record_link_type) = get_inner_record_link_type(&field.field_type)
                && let Err(e) = validate_record_id_table(id_str, record_link_type)
            {
                panic!("Edge record ID validation failed: {}", e);
            }

            // Handle case where the value is just an ID string
            let effective_direction = edge_config.resolve_direction_for_table(current_table_name);
            let (in_record, out_record) = match effective_direction {
                crate::schemasync::edge::Direction::From => {
                    (record_id.to_string(), id_str.to_string())
                }
                crate::schemasync::edge::Direction::To => {
                    (id_str.to_string(), record_id.to_string())
                }
                crate::schemasync::edge::Direction::Both => {
                    (record_id.to_string(), id_str.to_string())
                }
            };

            let formatted_in =
                value::to_surreal_string(&FieldType::EvenframeRecordId, &Value::String(in_record));
            let formatted_out =
                value::to_surreal_string(&FieldType::EvenframeRecordId, &Value::String(out_record));

            edge_objects.push(format!(
                "{{ in: {}, out: {} }}",
                formatted_in, formatted_out
            ));
        }
    }

    format!("[{}]", edge_objects.join(", "))
}

/// Generate update query for a single edge table using SurrealDB array functions
fn generate_single_edge_table_update(
    edge_table: &str,
    record_id: &str,
    incoming_edges: &str,
    edge_config: &EdgeConfig,
    current_table_name: &str,
) -> String {
    // Determine which side of the edge to query based on direction
    let edge_field = match edge_config.resolve_direction_for_table(current_table_name) {
        crate::schemasync::edge::Direction::From => "in",
        crate::schemasync::edge::Direction::To => "out",
        crate::schemasync::edge::Direction::Both => "in", // Default to 'in' for Both
    };

    // If incoming edges is empty array, just delete all existing edges for this record
    if incoming_edges.trim() == "[]" {
        return format!(
            r#"DELETE {table} WHERE {edge_field} = {record_id};"#,
            table = edge_table,
            edge_field = edge_field,
            record_id = record_id
        );
    }

    format!(
        r#"
        LET $existing_{table} = (SELECT id, in, out FROM {table} WHERE {edge_field} = {record_id});
        LET $incoming_{table} = {incoming_edges};

        -- Delete edges that don't exist in incoming
        FOR $existing IN $existing_{table} {{
            IF !array::any($incoming_{table}, |$incoming| $incoming.in = $existing.in AND $incoming.out = $existing.out) {{
                DELETE $existing.id;
            }};
        }};

        -- Insert edges that don't already exist
        FOR $incoming IN $incoming_{table} {{
            IF !array::any($existing_{table}, |$existing| $existing.in = $incoming.in AND $existing.out = $incoming.out) {{
                INSERT RELATION INTO {table} $incoming;
            }};
        }};
        "#,
        table = edge_table,
        edge_field = edge_field,
        record_id = record_id,
        incoming_edges = incoming_edges
    )
}

/// Generate a complete update query with edge handling using database-side operations
pub fn generate_update_query_with_edges<T: Serialize>(
    table_config: &TableConfig,
    object: &T,
    explicit_id: Option<String>,
    select_config: Option<SelectConfig>,
) -> (String, String) {
    let (main_query, record_id) = generate_query(
        QueryType::Update,
        table_config,
        object,
        explicit_id,
        select_config.clone(),
    );

    if let Some(ref config) = select_config
        && !config.filters.is_empty()
    {
        return (main_query, record_id);
    }

    let edge_query = generate_edge_update_query(table_config, object, &record_id);

    if edge_query.is_empty() {
        (main_query, record_id)
    } else {
        (
            format!("{} {}", main_query.trim_end_matches(';'), edge_query),
            record_id,
        )
    }
}

pub(crate) fn generate_recursive(
    query_type: QueryType,
    table_config: &TableConfig,
    value: &Value,
    explicit_id: Option<String>,
    let_statements: &mut Vec<String>,
    relation_statements: &mut Vec<String>,
    depth: u32,
) -> (String, String) {
    let generated_id = match query_type {
        QueryType::Create => {
            if let Some(id) = explicit_id.clone() {
                id
            } else {
                format!(
                    "{}:{}",
                    table_config.table_name,
                    uuid::Uuid::new_v4().to_string().replace('-', "")
                )
            }
        }
        QueryType::Update => {
            if let Some(id) = explicit_id.clone() {
                id
            } else if let Some(id_val) = value.get("id") {
                if let Some(id_str) = id_val.as_str() {
                    id_str.to_string()
                } else {
                    value::to_surreal_string(&FieldType::EvenframeRecordId, id_val)
                }
            } else {
                format!(
                    "{}:{}",
                    table_config.table_name,
                    uuid::Uuid::new_v4().to_string().replace('-', "")
                )
            }
        }
        QueryType::Select => unreachable!("Select should not be handled in generate_recursive"),
    };

    let record_id = if depth > 0 {
        match query_type {
            QueryType::Create => {
                if let Some(id) = explicit_id {
                    id
                } else {
                    table_config.table_name.clone()
                }
            }
            QueryType::Update => {
                if let Some(id) = explicit_id {
                    id
                } else if let Some(id_val) = value.get("id") {
                    value::to_surreal_string(&FieldType::EvenframeRecordId, id_val)
                } else {
                    format!("{}:rand()", table_config.table_name)
                }
            }
            QueryType::Select => unreachable!("Select should not be handled in generate_recursive"),
        }
    } else {
        generated_id.clone()
    };

    let mut content_parts = Vec::new();

    for field in &table_config.struct_config.fields {
        if &field.field_name == "id" {
            continue;
        }

        if let Some(edge_config) = &field.edge_config {
            // Handle edge processing differently for updates vs creates
            if matches!(query_type, QueryType::Update) {
                // For updates, use the database-side edge diffing approach
                let field_value = value.get(&field.field_name).unwrap_or(&Value::Null);
                let incoming_edges = extract_incoming_edges_as_surql(ExtractEdgesParams {
                    field_value,
                    edge_config,
                    record_id: &record_id,
                    field,
                    let_statements,
                    relation_statements,
                    depth,
                    current_table_name: &table_config.table_name,
                });

                let formatted_record_id = value::to_surreal_string(
                    &FieldType::EvenframeRecordId,
                    &Value::String(record_id.clone()),
                );
                let edge_update_stmt = generate_single_edge_table_update(
                    &edge_config.edge_name,
                    &formatted_record_id,
                    &incoming_edges,
                    edge_config,
                    &table_config.table_name,
                );
                relation_statements.push(edge_update_stmt);
                continue;
            }
        }

        if let Some(field_value) = value.get(&field.field_name) {
            if let Some(edge_config) = &field.edge_config {
                let from_record_id = &record_id;
                let relation_table = &edge_config.edge_name;
                let effective_direction =
                    edge_config.resolve_direction_for_table(&table_config.table_name);

                // Get the relation table config once upfront
                let relation_table_config =
                    registry::get_table_config(&relation_table.to_case(Case::Pascal));

                let field_values = if let Some(arr) = field_value.as_array() {
                    arr.to_vec()
                } else {
                    vec![field_value.clone()]
                };

                for value in field_values {
                    if let Some(field_value_obj) = value.as_object() {
                        let mut relation_data_parts = Vec::new();
                        let mut to_record_id = None;

                        // Determine the direction and extract the target record ID
                        let target_field = match effective_direction {
                            crate::schemasync::edge::Direction::From => "out",
                            crate::schemasync::edge::Direction::To => "in",
                            crate::schemasync::edge::Direction::Both => "out", // Defaulting to 'out' for Both
                        };

                        if let Some(id_val) = field_value_obj.get(target_field)
                            && let Some(id_str) = id_val.as_str()
                        {
                            to_record_id = Some(id_str.to_string());
                        }

                        // Construct data parts and update assignments from the rest of the fields
                        if let Some(config) = &relation_table_config {
                            for edge_field in &config.struct_config.fields {
                                let key = &edge_field.field_name;
                                if key == "in" || key == "out" || key == "id" {
                                    continue;
                                }

                                if let Some(edge_prop_value) = field_value_obj.get(key) {
                                    let surreal_string = if let FieldType::RecordLink(inner_type) =
                                        &edge_field.field_type
                                    {
                                        handle_record_link(
                                            edge_field,
                                            edge_prop_value,
                                            inner_type,
                                            let_statements,
                                            relation_statements,
                                            depth,
                                        )
                                    } else {
                                        value::to_surreal_string(
                                            &edge_field.field_type,
                                            edge_prop_value,
                                        )
                                    };
                                    relation_data_parts
                                        .push(format!("{}: {}", key, surreal_string));
                                }
                            }
                        }

                        if let Some(to_id) = to_record_id {
                            // Format record IDs properly
                            let formatted_to_id = value::to_surreal_string(
                                &FieldType::EvenframeRecordId,
                                &serde_json::Value::String(to_id.clone()),
                            );
                            let formatted_from_id = value::to_surreal_string(
                                &FieldType::EvenframeRecordId,
                                &serde_json::Value::String(from_record_id.clone()),
                            );

                            let (in_id, out_id) = match effective_direction {
                                crate::schemasync::edge::Direction::From => {
                                    (formatted_from_id, formatted_to_id)
                                }
                                crate::schemasync::edge::Direction::To => {
                                    (formatted_to_id, formatted_from_id)
                                }
                                crate::schemasync::edge::Direction::Both => {
                                    (formatted_from_id, formatted_to_id)
                                }
                            };

                            let relation_id = format!(
                                "{}:{}",
                                relation_table,
                                uuid::Uuid::new_v4().to_string().replace('-', "")
                            );
                            let formatted_relation_id = value::to_surreal_string(
                                &FieldType::EvenframeRecordId,
                                &serde_json::Value::String(relation_id),
                            );

                            let relation_data_str = if relation_data_parts.is_empty() {
                                format!("id: {}", formatted_relation_id)
                            } else {
                                format!(
                                    "id: {}, {}",
                                    formatted_relation_id,
                                    relation_data_parts.join(", ")
                                )
                            };

                            let insert_relation_stmt = format!(
                                "RELATE {}->{}->{} CONTENT {{ {} }};",
                                in_id, relation_table, out_id, relation_data_str
                            );
                            relation_statements.push(insert_relation_stmt);
                        }
                    } else {
                        // Handle case where the value is just an ID string
                        let to_record_id_raw = value::to_surreal_string(&field.field_type, &value);
                        let formatted_to_id = value::to_surreal_string(
                            &FieldType::EvenframeRecordId,
                            &serde_json::Value::String(to_record_id_raw),
                        );
                        let formatted_from_id = value::to_surreal_string(
                            &FieldType::EvenframeRecordId,
                            &serde_json::Value::String(from_record_id.clone()),
                        );

                        let (in_id, out_id) = match effective_direction {
                            crate::schemasync::edge::Direction::From => {
                                (formatted_from_id, formatted_to_id)
                            }
                            crate::schemasync::edge::Direction::To => {
                                (formatted_to_id, formatted_from_id)
                            }
                            crate::schemasync::edge::Direction::Both => {
                                (formatted_from_id, formatted_to_id)
                            }
                        };

                        let insert_relation_stmt =
                            format!("RELATE {}->{}->{};", in_id, relation_table, out_id);
                        relation_statements.push(insert_relation_stmt);
                    }
                }
                continue;
            }

            let surreal_string = if let FieldType::RecordLink(inner_type) = &field.field_type {
                handle_record_link(
                    field,
                    field_value,
                    inner_type,
                    let_statements,
                    relation_statements,
                    depth,
                )
            } else {
                value::to_surreal_string(&field.field_type, field_value)
            };
            content_parts.push(format!("{}: {}", field.field_name, surreal_string));
        }
    }

    let content_body = content_parts.join(", ");
    let main_query = match query_type {
        QueryType::Create => format!("CREATE {} CONTENT {{ {} }};", record_id, content_body),
        QueryType::Update => format!("UPDATE {} MERGE {{ {} }};", record_id, content_body),
        QueryType::Select => unreachable!("Select should not be handled in generate_recursive"),
    };

    (main_query, generated_id)
}

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SortDefinition {
    key: String,
    label: String,
    sort_type: FilterPrimitive,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SortValue {
    field_key: String,
    sort_type: FilterPrimitive,
    direction: SortDirection,
    #[serde(skip_serializing_if = "Option::is_none")]
    _tag: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SelectConfig {
    #[serde(default)]
    pub filters: Vec<FilterValue>,
    #[serde(default)]
    pub sorts: Vec<SortValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterPrimitive {
    String,
    Number,
    Boolean,
    Date,
    Select,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterOperator {
    Contains,
    Equals,
    StartsWith,
    EndsWith,
    GreaterThan,
    LessThan,
    Is,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterDefinition {
    pub key: String,
    pub label: String,
    pub filter_type: FilterPrimitive,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterValue {
    pub field_key: String,
    pub filter_type: FilterPrimitive,
    pub operator: FilterOperator,
    pub value: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub _tag: Option<String>,
}

fn map_filter_primitive_to_field_type(primitive: &FilterPrimitive) -> FieldType {
    match primitive {
        FilterPrimitive::String => FieldType::String,
        FilterPrimitive::Number => FieldType::Decimal,
        FilterPrimitive::Boolean => FieldType::Bool,
        FilterPrimitive::Date => FieldType::DateTime,
        FilterPrimitive::Select => FieldType::String, // Assuming select options are strings
    }
}

pub fn generate_where_clause(filters: &[FilterValue]) -> String {
    if filters.is_empty() {
        return String::new();
    }

    let conditions: Vec<String> = filters
        .iter()
        .map(|filter| {
            let field_type = map_filter_primitive_to_field_type(&filter.filter_type);
            let surreal_value = value::to_surreal_string(&field_type, &filter.value);

            match filter.operator {
                FilterOperator::Contains => {
                    format!(
                        "{} CONTAINS {}",
                        filter.field_key.to_case(Case::Snake),
                        surreal_value
                    )
                }
                FilterOperator::Equals => format!(
                    "{} = {}",
                    filter.field_key.to_case(Case::Snake),
                    surreal_value
                ),
                FilterOperator::StartsWith => {
                    format!(
                        "string::starts_with({}, {})",
                        filter.field_key.to_case(Case::Snake),
                        surreal_value
                    )
                }
                FilterOperator::EndsWith => {
                    format!(
                        "string::ends_with({}, {})",
                        filter.field_key.to_case(Case::Snake),
                        surreal_value
                    )
                }
                FilterOperator::GreaterThan => format!(
                    "{} > {}",
                    filter.field_key.to_case(Case::Snake),
                    surreal_value
                ),
                FilterOperator::LessThan => format!(
                    "{} < {}",
                    filter.field_key.to_case(Case::Snake),
                    surreal_value
                ),
                FilterOperator::Is => format!(
                    "{} IS {}",
                    filter.field_key.to_case(Case::Snake),
                    surreal_value
                ),
            }
        })
        .collect();

    format!("WHERE {}", conditions.join(" AND "))
}

pub fn generate_sort_clause(sorts: &[SortValue]) -> String {
    if sorts.is_empty() {
        return String::new();
    }

    let orderings: Vec<String> = sorts
        .iter()
        .map(|sort| {
            let field = sort.field_key.to_case(Case::Snake);
            let direction = match sort.direction {
                SortDirection::Asc => "ASC",
                SortDirection::Desc => "DESC",
            };

            format!("{} {}", field, direction)
        })
        .collect();

    format!("ORDER BY {}", orderings.join(", "))
}

pub fn generate_query<T: Serialize>(
    query_type: QueryType,
    table_config: &TableConfig,
    object: &T,
    explicit_id: Option<String>,
    select_config: Option<SelectConfig>,
) -> (String, String) {
    if query_type == QueryType::Select {
        let target = if let Some(id) = explicit_id {
            id
        } else {
            table_config.table_name.clone()
        };

        let mut query = format!("SELECT * FROM {}", target);
        if let Some(ref config) = select_config {
            let where_clause = generate_where_clause(&config.filters);
            if !where_clause.is_empty() {
                query.push(' ');
                query.push_str(&where_clause);
            }
            let sort_clause = generate_sort_clause(&config.sorts);
            if !sort_clause.is_empty() {
                query.push(' ');
                query.push_str(&sort_clause);
            }
        }
        query.push(';');
        return (query, String::new());
    }

    let value = serde_json::to_value(object).expect("Failed to serialize object to JSON Value");
    let mut all_let_statements = Vec::new();
    let mut all_relation_statements = Vec::new();

    let (mut main_query, generated_id) = generate_recursive(
        query_type,
        table_config,
        &value,
        explicit_id,
        &mut all_let_statements,
        &mut all_relation_statements,
        0,
    );

    let where_clause = if let Some(ref config) = select_config {
        let clause = generate_where_clause(&config.filters);
        if !clause.is_empty() {
            if let QueryType::Update = query_type {
                let parts: Vec<&str> = main_query.splitn(3, ' ').collect();
                if parts.len() == 3 && parts[0] == "UPDATE" {
                    main_query = format!("UPDATE {} {}", table_config.table_name, parts[2]);
                }
            }
            format!(" {}", clause)
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let let_prefix = if all_let_statements.is_empty() {
        String::new()
    } else {
        format!("{} ", all_let_statements.join(" "))
    };

    let relation_suffix = if all_relation_statements.is_empty() {
        String::new()
    } else {
        format!(" {}", all_relation_statements.join(" "))
    };

    // Inject WHERE clause before the semicolon of the main query
    let main_query_with_where = if let Some(pos) = main_query.rfind(';') {
        let (query_part, _) = main_query.split_at(pos);
        format!("{}{};", query_part, where_clause)
    } else {
        // Fallback if no semicolon is found
        format!(
            "{}{}{}",
            main_query,
            where_clause,
            if where_clause.is_empty() { "" } else { ";" }
        )
    };

    let full_query = format!(
        "{}{}{}",
        let_prefix,
        main_query_with_where.trim(),
        relation_suffix
    );

    (full_query, generated_id)
}

/// Validates if the table name part of a record ID matches the expected table for the given FieldType.
pub fn validate_record_id_table(record_id: &str, field_type: &FieldType) -> Result<(), String> {
    match field_type {
        FieldType::RecordLink(inner_type) => {
            let expected_struct_name = match &**inner_type {
                FieldType::Other(name) => name.clone(),
                _ => {
                    return Err(format!(
                        "Invalid type inside RecordLink: {:?}. A RecordLink must point to a struct type (FieldType::Other).",
                        inner_type
                    ));
                }
            };

            // Parse the table name from the provided record_id.
            let id_parts: Vec<&str> = record_id.split(':').collect();
            if id_parts.len() < 2 || id_parts[0].is_empty() {
                return Err(format!(
                    "Invalid record ID format for RecordLink: '{}'. Expected 'table:id'.",
                    record_id
                ));
            }
            let table_name_from_id = id_parts[0];

            // First try to get table config directly
            if let Some(expected_table_config) = registry::get_table_config(&expected_struct_name) {
                let expected_table_name = &expected_table_config.table_name;

                // Compare the table name from the ID with the expected table name.
                if table_name_from_id != expected_table_name {
                    return Err(format!(
                        "Mismatched table for record ID '{}'. Expected table for struct '{}' is '{}', but ID has table '{}'.",
                        record_id, expected_struct_name, expected_table_name, table_name_from_id
                    ));
                }
                return Ok(());
            }

            // Fallback: Check if it's a union of tables
            if let Some(union_table_names) = registry::get_union_of_tables(&expected_struct_name) {
                // Convert table name from ID to Pascal case for comparison
                let table_name_pascal = table_name_from_id.to_case(Case::Pascal);

                // Check if the table from the ID matches any table in the union
                for union_table_name in union_table_names {
                    if table_name_pascal == *union_table_name {
                        return Ok(());
                    }
                }

                return Err(format!(
                    "Mismatched table for record ID '{}'. Expected one of the tables from union '{}' ({}), but ID has table '{}' (converted to Pascal: '{}').",
                    record_id,
                    expected_struct_name,
                    union_table_names.join(", "),
                    table_name_from_id,
                    table_name_pascal
                ));
            }

            // Neither table nor union found
            Err(format!(
                "Validation failed: No table config or union of tables found for struct '{}' specified in RecordLink.",
                expected_struct_name
            ))
        }
        FieldType::Option(inner_type) => {
            // Recurse for optional types
            validate_record_id_table(record_id, inner_type)
        }
        // The caller should handle iterating over a Vec. This function validates a single ID.
        // Other types are not record links, so validation passes.
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::StructConfig;
    use serde_json::json;

    fn sample_table_config() -> TableConfig {
        TableConfig {
            table_name: "person".to_string(),
            struct_config: StructConfig {
                struct_name: "Person".to_string(),
                fields: Vec::new(),
                validators: Vec::new(),
            },
            relation: None,
            permissions: None,
            mock_generation_config: None,
            events: Vec::new(),
        }
    }

    #[test]
    fn generate_sort_clause_orders_multiple_fields() {
        let sorts = vec![
            SortValue {
                field_key: "Name".to_string(),
                sort_type: FilterPrimitive::String,
                direction: SortDirection::Asc,
                _tag: None,
            },
            SortValue {
                field_key: "createdAt".to_string(),
                sort_type: FilterPrimitive::Date,
                direction: SortDirection::Desc,
                _tag: None,
            },
        ];

        let clause = generate_sort_clause(&sorts);

        assert_eq!(clause, "ORDER BY name ASC, created_at DESC");
    }

    #[test]
    fn generate_query_select_includes_where_and_order_by() {
        let table_config = sample_table_config();
        let filters = vec![FilterValue {
            field_key: "name".to_string(),
            filter_type: FilterPrimitive::String,
            operator: FilterOperator::Equals,
            value: json!("Alice"),
            _tag: None,
        }];
        let sorts = vec![SortValue {
            field_key: "createdAt".to_string(),
            sort_type: FilterPrimitive::Date,
            direction: SortDirection::Desc,
            _tag: None,
        }];
        let select_config = SelectConfig { filters, sorts };

        let (query, temp_id) = generate_query(
            QueryType::Select,
            &table_config,
            &(),
            None,
            Some(select_config),
        );

        assert_eq!(temp_id, "");
        assert_eq!(
            query,
            "SELECT * FROM person WHERE name = 'Alice' ORDER BY created_at DESC;"
        );
    }
}
