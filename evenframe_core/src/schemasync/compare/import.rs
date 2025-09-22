use crate::{
    EvenframeError, Result,
    schemasync::{TableConfig, config::AccessType},
};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt::{self, Display, Formatter},
};
use surrealdb::Surreal;
use surrealdb::engine::remote::http::Client;
use tracing;

/// Represents a complex object type definition in SurrealDB
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ObjectType {
    /// Simple type like string, int, bool, etc.
    Simple(String),
    /// Object with nested fields
    Object(HashMap<String, ObjectType>),
    /// Array of a type
    Array(Box<ObjectType>),
    /// Union of multiple types (e.g., string | int)
    Union(Vec<ObjectType>),
    /// Nullable type (e.g., null | string)
    Nullable(Box<ObjectType>),
}

impl Display for ObjectType {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            ObjectType::Simple(s) => write!(f, "{}", s),
            ObjectType::Object(fields) => {
                let field_strs: Vec<String> = fields
                    .iter()
                    .map(|(name, field_type)| format!("{}: {}", name, field_type))
                    .collect();
                write!(f, "{{ {} }}", field_strs.join(", "))
            }
            ObjectType::Array(inner) => write!(f, "array<{}>", inner),
            ObjectType::Union(types) => {
                let type_strs: Vec<String> = types.iter().map(|t| t.to_string()).collect();
                write!(f, "({})", type_strs.join(" | "))
            }
            ObjectType::Nullable(inner) => write!(f, "null | {}", inner),
        }
    }
}

/// Represents a field definition in SurrealDB
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FieldDefinition {
    pub name: String,
    pub field_type: ObjectType,
    pub required: bool,
    pub default_value: Option<String>,
    pub assertions: Vec<String>,
    /// For array wildcard fields (e.g., phones[*]), this stores the parent field name
    pub parent_array_field: Option<String>,
}

/// Represents a table definition in SurrealDB
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TableDefinition {
    pub name: String,
    pub schema_type: SchemaType,
    pub fields: HashMap<String, FieldDefinition>,
    /// Array wildcard fields (e.g., phones[*]) are stored separately
    /// Key is the parent field name (e.g., "phones"), value is the wildcard field definition
    pub array_wildcard_fields: HashMap<String, FieldDefinition>,
    pub permissions: Option<PermissionSet>,
    pub indexes: Vec<IndexDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SchemaType {
    Schemafull,
    Schemaless,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PermissionSet {
    pub select: String,
    pub create: String,
    pub update: String,
    pub delete: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct IndexDefinition {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
}

/// Represents an access definition in SurrealDB
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccessDefinition {
    pub name: String,
    pub access_type: AccessType,
    pub database_level: bool, // true for DATABASE, false for NAMESPACE
    pub signup_query: Option<String>,
    pub signin_query: Option<String>,
    pub jwt_algorithm: Option<String>,
    pub jwt_key: Option<String>,
    pub jwt_url: Option<String>,
    pub issuer_key: Option<String>,
    pub authenticate: Option<String>,
    pub duration_for_token: Option<String>,
    pub duration_for_session: Option<String>,
    pub bearer_for: Option<String>, // "USER" or "RECORD"
}

/// Complete schema definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaDefinition {
    pub tables: HashMap<String, TableDefinition>,
    pub edges: HashMap<String, TableDefinition>,
    pub accesses: Vec<AccessDefinition>,
}

impl SchemaDefinition {
    /// Create from TableConfig HashMap (for code-based schema generation)
    pub fn from_table_configs(tables: &HashMap<String, TableConfig>) -> Result<Self> {
        tracing::debug!(
            table_count = tables.len(),
            "Creating SchemaDefinition from TableConfigs"
        );
        let mut schema_tables = HashMap::new();
        let mut schema_edges = HashMap::new();

        for (name, config) in tables {
            let table_def = TableDefinition {
                name: name.clone(),
                schema_type: SchemaType::Schemafull,
                fields: Self::extract_fields_from_config(config)?,
                array_wildcard_fields: HashMap::new(), // TODO: Extract wildcard fields from config if available
                permissions: Self::extract_permissions_from_config(config),
                indexes: Vec::new(), // TODO: Extract indexes from config if available
            };

            if config.relation.is_some() {
                schema_edges.insert(name.clone(), table_def);
            } else {
                schema_tables.insert(name.clone(), table_def);
            }
        }

        let definition = Self {
            tables: schema_tables.clone(),
            edges: schema_edges.clone(),
            accesses: Vec::new(), // TODO: Extract accesses from config if available
        };

        tracing::debug!(
            tables = definition.tables.len(),
            edges = definition.edges.len(),
            "SchemaDefinition created from configs"
        );

        Ok(definition)
    }

    fn extract_fields_from_config(
        config: &TableConfig,
    ) -> Result<HashMap<String, FieldDefinition>> {
        let mut fields = HashMap::new();

        for field in &config.struct_config.fields {
            // Check if field has a default value
            let default_value = field
                .define_config
                .as_ref()
                .and_then(|dc| dc.default.clone().or(dc.default_always.clone()));

            // Field is required if it doesn't have a default value and isn't skipped
            let is_required = default_value.is_none()
                && !field
                    .define_config
                    .as_ref()
                    .map(|dc| dc.should_skip)
                    .unwrap_or(false);

            let field_def = FieldDefinition {
                name: field.field_name.clone(),
                field_type: ObjectType::Simple(field.field_type.to_string()),
                required: is_required,
                default_value,
                assertions: field
                    .define_config
                    .as_ref()
                    .and_then(|dc| dc.assert.clone())
                    .map(|a| vec![a])
                    .unwrap_or_default(),
                parent_array_field: None,
            };
            fields.insert(field.field_name.clone(), field_def);
        }

        Ok(fields)
    }

    fn extract_permissions_from_config(config: &TableConfig) -> Option<PermissionSet> {
        tracing::trace!("Extracting permissions from table config");
        config.permissions.as_ref().map(|perms| PermissionSet {
            select: perms
                .all_permissions
                .clone()
                .or(perms.select_permissions.clone())
                .unwrap_or_else(|| "FULL".to_string()),
            create: perms
                .all_permissions
                .clone()
                .or(perms.create_permissions.clone())
                .unwrap_or_else(|| "FULL".to_string()),
            update: perms
                .all_permissions
                .clone()
                .or(perms.update_permissions.clone())
                .unwrap_or_else(|| "FULL".to_string()),
            delete: perms
                .all_permissions
                .clone()
                .or(perms.delete_permissions.clone())
                .unwrap_or_else(|| "FULL".to_string()),
        })
    }
}

/// Imports schema definitions from a SurrealDB instance
pub struct SchemaImporter<'a> {
    client: &'a Surreal<Client>,
}

impl<'a> SchemaImporter<'a> {
    pub fn new(client: &'a Surreal<Client>) -> Self {
        Self { client }
    }

    /// Import schema-only (no data) from the database
    pub async fn import_schema_only(&self) -> Result<SchemaDefinition> {
        // Export schema only (no records)
        let mut export_stream = self
            .client
            .export(())
            .with_config()
            .records(false) // Schema only, no data
            .await
            .map_err(|e| {
                EvenframeError::comparison(format!("Failed to export schema from database: {e}"))
            })?;

        let mut schema_statements = Vec::new();
        let mut statement_count = 0;

        // Collect all export statements
        while let Some(result) = export_stream.next().await {
            match result {
                Ok(bytes) => {
                    statement_count += 1;
                    let statement = String::from_utf8(bytes).map_err(|e| {
                        EvenframeError::comparison(format!(
                            "Failed to parse export data at statement {statement_count}: {e}",
                        ))
                    })?;

                    // Skip empty statements
                    if !statement.trim().is_empty() {
                        schema_statements.push(statement);
                    }
                }
                Err(e) => {
                    return Err(EvenframeError::comparison(format!(
                        "Error reading export stream at statement {statement_count}: {e}",
                    )));
                }
            }
        }

        // Check if we got any statements
        if schema_statements.is_empty() {
            return Err(EvenframeError::comparison(
                "No schema statements found in database export".to_string(),
            ));
        }

        // Parse the exported statements into our schema structure
        self.parse_schema_statements(schema_statements)
    }

    /// Export schema only as raw DEFINE statements
    pub async fn export_schema_only(&self) -> Result<String> {
        // Export schema only (no records)
        let mut export_stream = self
            .client
            .export(())
            .await
            .map_err(|e| EvenframeError::comparison(format!("Failed to export schema: {e}")))?;

        let mut schema_statements = Vec::new();

        while let Some(Ok(bytes)) = export_stream.next().await {
            let statement = String::from_utf8(bytes).map_err(|e| {
                EvenframeError::comparison(format!("Failed to parse export data: {e}"))
            })?;

            // Only keep schema-related statements (DEFINE)
            let trimmed = statement.trim();
            if trimmed.starts_with("DEFINE ") {
                schema_statements.push(statement);
            }
        }

        Ok(schema_statements.join("\n"))
    }

    /// Parse schema from raw export string
    pub fn parse_schema_from_export(&self, export_data: &str) -> Result<SchemaDefinition> {
        let statements: Vec<String> = export_data
            .lines()
            .map(|s| s.to_string())
            .filter(|s| !s.trim().is_empty())
            .collect();

        self.parse_schema_statements(statements)
    }

    /// Parse SurrealDB export statements into structured schema
    fn parse_schema_statements(&self, statements: Vec<String>) -> Result<SchemaDefinition> {
        let mut tables = HashMap::new();
        let edges = HashMap::new();
        let mut accesses = Vec::new();
        let mut current_table: Option<String> = None;
        let mut current_table_statement: Option<String> = None;
        let mut current_fields: HashMap<String, FieldDefinition> = HashMap::new();
        let mut current_wildcard_fields: HashMap<String, FieldDefinition> = HashMap::new();

        for statement in statements {
            let trimmed = statement.trim();

            // Parse DEFINE TABLE statements
            if trimmed.starts_with("DEFINE TABLE") {
                // Save previous table if exists
                if let Some(table_name) = current_table.take() {
                    let schema_type = if let Some(stmt) = &current_table_statement {
                        Self::extract_schema_type(stmt)
                    } else {
                        SchemaType::Schemaless
                    };

                    let table_def = TableDefinition {
                        name: table_name.clone(),
                        schema_type,
                        fields: current_fields.clone(),
                        array_wildcard_fields: current_wildcard_fields.clone(),
                        permissions: None,   // TODO: Parse permissions
                        indexes: Vec::new(), // TODO: Parse indexes
                    };
                    tables.insert(table_name, table_def);
                    current_fields.clear();
                    current_wildcard_fields.clear();
                }

                // Extract table name and store statement
                if let Some(name) = Self::extract_table_name(trimmed) {
                    current_table = Some(name);
                    current_table_statement = Some(trimmed.to_string());
                }
            }
            // Parse DEFINE ACCESS statements
            else if trimmed.starts_with("DEFINE ACCESS") {
                if let Some(access_def) = Self::parse_access_definition(trimmed) {
                    accesses.push(access_def);
                }
            }
            // Parse DEFINE FIELD statements
            else if trimmed.starts_with("DEFINE FIELD") && current_table.is_some() {
                if let Some(field_def) = Self::parse_field_definition(trimmed) {
                    // Check if this is an array wildcard field
                    if let Some(parent_field) = &field_def.parent_array_field {
                        current_wildcard_fields.insert(parent_field.clone(), field_def);
                    } else {
                        current_fields.insert(field_def.name.clone(), field_def);
                    }
                }
            }
            // Parse DEFINE INDEX statements
            else if trimmed.starts_with("DEFINE INDEX") {
                // TODO: Parse index definitions
            }
        }

        // Save last table if exists
        if let Some(table_name) = current_table {
            let schema_type = if let Some(stmt) = &current_table_statement {
                Self::extract_schema_type(stmt)
            } else {
                SchemaType::Schemaless
            };

            let table_def = TableDefinition {
                name: table_name.clone(),
                schema_type,
                fields: current_fields,
                array_wildcard_fields: current_wildcard_fields,
                permissions: None,
                indexes: Vec::new(),
            };
            tables.insert(table_name, table_def);
        }

        Ok(SchemaDefinition {
            tables,
            edges,
            accesses,
        })
    }

    /// Extract schema type from DEFINE TABLE statement
    fn extract_schema_type(statement: &str) -> SchemaType {
        let statement_upper = statement.to_uppercase();
        if statement_upper.contains("SCHEMAFULL") {
            SchemaType::Schemafull
        } else if statement_upper.contains("SCHEMALESS") {
            SchemaType::Schemaless
        } else {
            // Default to schemaless if not explicitly specified
            SchemaType::Schemaless
        }
    }

    /// Extract table name from DEFINE TABLE statement
    fn extract_table_name(statement: &str) -> Option<String> {
        // Example: "DEFINE TABLE person TYPE ANY SCHEMAFULL PERMISSIONS NONE;"
        let parts: Vec<&str> = statement.split_whitespace().collect();
        if parts.len() >= 3 && parts[0] == "DEFINE" && parts[1] == "TABLE" {
            // Handle quoted table names and remove any trailing characters
            let table_name = parts[2]
                .trim_start_matches('`')
                .trim_end_matches('`')
                .trim_end_matches(';');

            if table_name.is_empty() {
                None
            } else {
                Some(table_name.to_string())
            }
        } else {
            None
        }
    }

    /// Split union types properly, respecting nested structures
    fn split_union_types(type_str: &str) -> Vec<&str> {
        let mut parts = Vec::new();
        let mut current_start = 0;
        let mut brace_count = 0;
        let mut bracket_count = 0;
        let mut in_quotes = false;
        let mut quote_char = ' ';
        let chars: Vec<char> = type_str.chars().collect();

        let mut i = 0;
        while i < chars.len() {
            let ch = chars[i];

            // Handle quotes
            if !in_quotes && (ch == '\'' || ch == '"') {
                in_quotes = true;
                quote_char = ch;
            } else if in_quotes && ch == quote_char {
                in_quotes = false;
            }

            if !in_quotes {
                match ch {
                    '{' => brace_count += 1,
                    '}' => brace_count -= 1,
                    '<' => bracket_count += 1,
                    '>' => bracket_count -= 1,
                    '|' if brace_count == 0 && bracket_count == 0 => {
                        // Check if this is part of " | "
                        if i > 0
                            && i < chars.len() - 1
                            && chars[i - 1] == ' '
                            && chars[i + 1] == ' '
                        {
                            // Add the part before this union separator
                            let part = &type_str[current_start..i - 1];
                            if !part.trim().is_empty() {
                                parts.push(part.trim());
                            }
                            current_start = i + 2; // Skip past " | "
                            i += 1; // Extra increment to skip the space after |
                        }
                    }
                    _ => {}
                }
            }

            i += 1;
        }

        // Add the last part
        if current_start < type_str.len() {
            let part = &type_str[current_start..];
            if !part.trim().is_empty() {
                parts.push(part.trim());
            }
        }

        parts
    }

    /// Parse a type string into an ObjectType using an iterative work stack
    fn parse_type_string(type_str: &str) -> ObjectType {
        let mut work_stack: Vec<WorkItem> = Vec::new();
        let mut value_stack: Vec<ObjectType> = Vec::new();

        #[derive(Clone)]
        enum WorkItem {
            Parse(String),
            WrapArray,
            BuildUnion { count: usize },
        }

        // Quick bound to avoid pathological inputs
        let trimmed = type_str.trim();
        if trimmed.len() > 100_000 {
            return ObjectType::Simple(trimmed.to_string());
        }

        work_stack.push(WorkItem::Parse(trimmed.to_string()));

        while let Some(item) = work_stack.pop() {
            match item {
                WorkItem::Parse(s) => {
                    let t = s.trim();

                    // Object literal not part of a union: { ... }
                    if t.starts_with('{') && t.ends_with('}') {
                        // Ensure the outer braces are a single object and not a union at top level
                        let mut brace_count = 0;
                        let mut union_at_top = false;
                        let chars: Vec<char> = t.chars().collect();
                        for i in 0..chars.len() {
                            match chars[i] {
                                '{' => brace_count += 1,
                                '}' => brace_count -= 1,
                                '|' if brace_count == 0
                                    && i > 0
                                    && i < chars.len() - 1
                                    && chars[i - 1] == ' '
                                    && chars[i + 1] == ' ' =>
                                {
                                    union_at_top = true;
                                    break;
                                }
                                _ => {}
                            }
                        }

                        if !union_at_top {
                            let inner = &t[1..t.len() - 1];
                            value_stack.push(Self::parse_object_fields(inner));
                            continue;
                        }
                    }

                    // array<...>
                    if t.starts_with("array<") && t.ends_with('>') {
                        // Find matching > for the first < after "array<"
                        let inner = &t[6..t.len() - 1];
                        work_stack.push(WorkItem::WrapArray);
                        work_stack.push(WorkItem::Parse(inner.to_string()));
                        continue;
                    }

                    // Top-level union: split into parts without recursion
                    if t.contains(" | ") {
                        let parts = Self::split_union_types(t);
                        if parts.len() > 1 {
                            work_stack.push(WorkItem::BuildUnion { count: parts.len() });
                            // Parse in reverse so first part ends on top of value_stack
                            for part in parts.into_iter().rev() {
                                work_stack.push(WorkItem::Parse(part.to_string()));
                            }
                            continue;
                        }
                    }

                    // Fallback: simple type
                    value_stack.push(ObjectType::Simple(t.to_string()));
                }
                WorkItem::WrapArray => {
                    if let Some(inner) = value_stack.pop() {
                        value_stack.push(ObjectType::Array(Box::new(inner)));
                    } else {
                        value_stack.push(ObjectType::Simple("array<unknown>".to_string()));
                    }
                }
                WorkItem::BuildUnion { count } => {
                    // Pop 'count' values, reverse to restore original order
                    let mut items = Vec::with_capacity(count);
                    for _ in 0..count {
                        if let Some(v) = value_stack.pop() {
                            items.push(v);
                        }
                    }
                    items.reverse();

                    // Nullable special-case
                    if items.len() == 2
                        && items
                            .iter()
                            .any(|t| matches!(t, ObjectType::Simple(s) if s == "null"))
                    {
                        if let Some(non_null) = items
                            .into_iter()
                            .find(|t| !matches!(t, ObjectType::Simple(s) if s == "null"))
                        {
                            value_stack.push(ObjectType::Nullable(Box::new(non_null)));
                        } else {
                            value_stack.push(ObjectType::Union(vec![
                                ObjectType::Simple("null".to_string()),
                                ObjectType::Simple("null".to_string()),
                            ]));
                        }
                    } else {
                        value_stack.push(ObjectType::Union(items));
                    }
                }
            }
        }

        // Result
        value_stack.pop().unwrap_or_else(|| ObjectType::Simple("unknown".to_string()))
    }

    /// Parse object field definitions
    fn parse_object_fields(fields_str: &str) -> ObjectType {
        let mut fields = HashMap::new();

        let mut current_pos = 0;
        let chars: Vec<char> = fields_str.chars().collect();

        while current_pos < chars.len() {
            // Skip whitespace
            while current_pos < chars.len() && chars[current_pos].is_whitespace() {
                current_pos += 1;
            }

            if current_pos >= chars.len() {
                break;
            }

            // Find field name (up to ':')
            let name_start = current_pos;
            while current_pos < chars.len() && chars[current_pos] != ':' {
                current_pos += 1;
            }

            if current_pos >= chars.len() {
                break;
            }

            let field_name = chars[name_start..current_pos]
                .iter()
                .collect::<String>()
                .trim()
                .to_string();
            current_pos += 1; // Skip ':'

            // Skip whitespace after ':'
            while current_pos < chars.len() && chars[current_pos].is_whitespace() {
                current_pos += 1;
            }

            // Find the type - need to handle nested objects, arrays, unions
            let type_start = current_pos;
            let mut bracket_count = 0;
            let mut brace_count = 0;
            let mut in_quotes = false;
            let mut quote_char = ' ';

            while current_pos < chars.len() {
                let ch = chars[current_pos];

                // Handle quotes
                if !in_quotes && (ch == '\'' || ch == '"') {
                    in_quotes = true;
                    quote_char = ch;
                } else if in_quotes && ch == quote_char {
                    in_quotes = false;
                }

                if !in_quotes {
                    match ch {
                        '<' => bracket_count += 1,
                        '>' => bracket_count -= 1,
                        '{' => brace_count += 1,
                        '}' => brace_count -= 1,
                        ',' if bracket_count == 0 && brace_count == 0 => {
                            break;
                        }
                        _ => {}
                    }
                }

                current_pos += 1;
            }

            let type_str = chars[type_start..current_pos]
                .iter()
                .collect::<String>()
                .trim()
                .to_string();

            let field_type = Self::parse_type_string(&type_str);

            fields.insert(field_name, field_type);

            // Skip comma if present
            if current_pos < chars.len() && chars[current_pos] == ',' {
                current_pos += 1;
            }
        }

        ObjectType::Object(fields)
    }

    /// Parse a DEFINE FIELD statement
    fn parse_field_definition(statement: &str) -> Option<FieldDefinition> {
        // Example: "DEFINE FIELD name ON TABLE person TYPE string;"
        // More complex: "DEFINE FIELD items ON TABLE order TYPE array<record<product>> DEFAULT [] ASSERT $value != NONE;"
        // Object type: "DEFINE FIELD colors ON account TYPE { active: string, hover: string, main: string } DEFAULT { active: '', hover: '', main: '' };"

        // Basic validation
        if !statement.starts_with("DEFINE FIELD") {
            return None;
        }

        // Extract field name - it's after "DEFINE FIELD" and before "ON"
        let after_field = statement.strip_prefix("DEFINE FIELD")?.trim();

        // Check if this is an array wildcard field (e.g., phones[*])
        let (field_name, parent_array) = if let Some(bracket_pos) = after_field.find("[*]") {
            let base_name = &after_field[..bracket_pos];
            let actual_name = base_name
                .split_whitespace()
                .next()?
                .trim_start_matches('`')
                .trim_end_matches('`');
            (format!("{actual_name}[*]"), Some(actual_name.to_string()))
        } else {
            let name = after_field
                .split_whitespace()
                .next()?
                .trim_start_matches('`')
                .trim_end_matches('`');
            (name.to_string(), None)
        };

        // Extract type - it's after "TYPE" and before the next keyword or semicolon
        let type_pos = statement.find(" TYPE ")?;
        let after_type = &statement[type_pos + 6..].trim();

        // Find the end of the type definition
        let mut type_end = 0;
        let mut bracket_count = 0;
        let mut brace_count = 0;
        let mut in_quotes = false;
        let mut quote_char = ' ';

        for (i, ch) in after_type.char_indices() {
            // Handle quotes
            if !in_quotes && (ch == '\'' || ch == '"') {
                in_quotes = true;
                quote_char = ch;
            } else if in_quotes && ch == quote_char {
                in_quotes = false;
            }

            if !in_quotes {
                match ch {
                    '<' => bracket_count += 1,
                    '>' => bracket_count -= 1,
                    '{' => brace_count += 1,
                    '}' => brace_count -= 1,
                    ' ' if bracket_count == 0
                        && brace_count == 0
                        && after_type[i..].starts_with(" DEFAULT") =>
                    {
                        type_end = i;
                        break;
                    }
                    ' ' if bracket_count == 0
                        && brace_count == 0
                        && after_type[i..].starts_with(" ASSERT") =>
                    {
                        type_end = i;
                        break;
                    }
                    ' ' if bracket_count == 0
                        && brace_count == 0
                        && after_type[i..].starts_with(" PERMISSIONS") =>
                    {
                        type_end = i;
                        break;
                    }
                    ';' if bracket_count == 0 && brace_count == 0 => {
                        type_end = i;
                        break;
                    }
                    _ => {}
                }
            }
            type_end = i + 1;
        }

        let field_type_str = after_type[..type_end].trim().trim_end_matches(';');
        let field_type = Self::parse_type_string(field_type_str);

        // Check for DEFAULT value
        let has_default = statement.contains(" DEFAULT ");
        let default_value = if has_default {
            if let Some(default_pos) = statement.find(" DEFAULT ") {
                let after_default = &statement[default_pos + 9..].trim();

                // Find the end of the default value (handling objects)
                let mut default_end = 0;
                let mut brace_count = 0;
                let mut in_quotes = false;
                let mut quote_char = ' ';

                for (i, ch) in after_default.char_indices() {
                    if !in_quotes && (ch == '\'' || ch == '"') {
                        in_quotes = true;
                        quote_char = ch;
                    } else if in_quotes && ch == quote_char {
                        in_quotes = false;
                    }

                    if !in_quotes {
                        match ch {
                            '{' => brace_count += 1,
                            '}' => brace_count -= 1,
                            ' ' if brace_count == 0
                                && after_default[i..].starts_with(" ASSERT") =>
                            {
                                default_end = i;
                                break;
                            }
                            ' ' if brace_count == 0
                                && after_default[i..].starts_with(" PERMISSIONS") =>
                            {
                                default_end = i;
                                break;
                            }
                            ';' if brace_count == 0 => {
                                default_end = i;
                                break;
                            }
                            _ => {}
                        }
                    }
                    default_end = i + 1;
                }

                Some(after_default[..default_end].trim().to_string())
            } else {
                None
            }
        } else {
            None
        };

        // Extract assertions
        let assertions = if let Some(assert_pos) = statement.find(" ASSERT ") {
            let after_assert = &statement[assert_pos + 8..].trim();
            let assert_end = after_assert
                .find(" PERMISSIONS")
                .unwrap_or(after_assert.len());
            let assert_content = after_assert[..assert_end].trim_end_matches(';');
            vec![assert_content.to_string()]
        } else {
            Vec::new()
        };

        Some(FieldDefinition {
            name: field_name.to_string(),
            field_type,
            required: !has_default,
            default_value,
            assertions,
            parent_array_field: parent_array,
        })
    }

    /// Parse a DEFINE ACCESS statement
    fn parse_access_definition(statement: &str) -> Option<AccessDefinition> {
        // Example: "DEFINE ACCESS user ON DATABASE TYPE RECORD SIGNUP (...) SIGNIN (...) WITH JWT ALGORITHM HS512 KEY '...' WITH ISSUER KEY '...' DURATION FOR TOKEN 12h, FOR SESSION 12h;"

        if !statement.starts_with("DEFINE ACCESS") {
            return None;
        }

        // Extract access name
        let after_access = statement.strip_prefix("DEFINE ACCESS")?.trim();
        let name = after_access
            .split_whitespace()
            .next()?
            .trim_start_matches('`')
            .trim_end_matches('`')
            .to_string();

        // Check if it's ON DATABASE or ON NAMESPACE
        let database_level = statement.contains(" ON DATABASE ");

        // Extract TYPE
        let type_pos = statement.find(" TYPE ")?;
        let after_type = &statement[type_pos + 6..].trim();

        let access_type = if after_type.starts_with("RECORD") {
            AccessType::Record
        } else if after_type.starts_with("JWT") {
            AccessType::Jwt
        } else if after_type.starts_with("BEARER") {
            AccessType::Bearer
        } else {
            return None;
        };

        let mut access_def = AccessDefinition {
            name,
            access_type: access_type.clone(),
            database_level,
            signup_query: None,
            signin_query: None,
            jwt_algorithm: None,
            jwt_key: None,
            jwt_url: None,
            issuer_key: None,
            authenticate: None,
            duration_for_token: None,
            duration_for_session: None,
            bearer_for: None,
        };

        // Parse RECORD type specific fields
        if matches!(access_type, AccessType::Record) {
            // Extract SIGNUP
            if let Some(signup_pos) = statement.find(" SIGNUP ") {
                let after_signup = &statement[signup_pos + 8..];
                if let Some(signup_query) = Self::extract_parenthesized_content(after_signup) {
                    access_def.signup_query = Some(signup_query);
                }
            }

            // Extract SIGNIN
            if let Some(signin_pos) = statement.find(" SIGNIN ") {
                let after_signin = &statement[signin_pos + 8..];
                if let Some(signin_query) = Self::extract_parenthesized_content(after_signin) {
                    access_def.signin_query = Some(signin_query);
                }
            }
        }

        // Parse JWT configuration
        if statement.contains(" WITH JWT ") {
            // Extract ALGORITHM
            if let Some(algo_pos) = statement.find(" ALGORITHM ") {
                let after_algo = &statement[algo_pos + 11..].trim();
                let algo = after_algo
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_string();
                access_def.jwt_algorithm = Some(algo);
            }

            // Extract KEY
            if let Some(key_pos) = statement.find(" KEY '") {
                let after_key = &statement[key_pos + 5..];
                if let Some(end_quote) = after_key[1..].find("'") {
                    access_def.jwt_key = Some(after_key[1..end_quote + 1].to_string());
                }
            }

            // Extract ISSUER KEY
            if let Some(issuer_pos) = statement.find(" WITH ISSUER KEY '") {
                let after_issuer = &statement[issuer_pos + 18..];
                if let Some(end_quote) = after_issuer.find("'") {
                    access_def.issuer_key = Some(after_issuer[..end_quote].to_string());
                }
            }
        }

        // Parse BEARER specific fields
        if let Some(for_pos) = statement.find(" FOR ")
            && matches!(access_type, AccessType::Bearer)
        {
            let after_for = &statement[for_pos + 5..].trim();
            let bearer_for = after_for
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_string();
            access_def.bearer_for = Some(bearer_for);
        }

        // Extract DURATION
        if let Some(duration_pos) = statement.find(" DURATION ") {
            let after_duration = &statement[duration_pos + 10..];

            // Extract FOR TOKEN
            if let Some(token_pos) = after_duration.find("FOR TOKEN ") {
                let after_token = &after_duration[token_pos + 10..];
                let token_duration = after_token
                    .split(&[',', ' '][..])
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if !token_duration.is_empty() {
                    access_def.duration_for_token = Some(token_duration);
                }
            }

            // Extract FOR SESSION
            if let Some(session_pos) = after_duration.find("FOR SESSION ") {
                let after_session = &after_duration[session_pos + 12..];
                let session_duration = after_session
                    .split(&[';', ' '][..])
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                if !session_duration.is_empty() {
                    access_def.duration_for_session = Some(session_duration);
                }
            }
        }

        Some(access_def)
    }

    /// Extract content within parentheses, handling nested parentheses
    fn extract_parenthesized_content(text: &str) -> Option<String> {
        let start = text.find('(')?;
        let mut paren_count = 0;
        let mut end = start;

        for (i, ch) in text[start..].chars().enumerate() {
            match ch {
                '(' => paren_count += 1,
                ')' => {
                    paren_count -= 1;
                    if paren_count == 0 {
                        end = start + i;
                        break;
                    }
                }
                _ => {}
            }
        }

        if paren_count == 0 && end > start {
            Some(text[start + 1..end].to_string())
        } else {
            None
        }
    }
}
