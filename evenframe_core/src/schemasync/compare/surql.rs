//! SurrealDB-specific schema comparison implementation
//!
//! This module contains the SurrealDB-specific schema comparison logic,
//! including the SchemaImporter for parsing SurrealQL exports and the
//! SurrealdbComparator for comparing schemas using in-memory databases.

use crate::{
    EvenframeError, Result, evenframe_log,
    schemasync::{
        config::AccessType,
        database::surql::access::setup_access_definitions,
    },
};
use super::types::{
    AccessDefinition, FieldDefinition, ObjectType, SchemaDefinition,
    SchemaType, TableDefinition,
};
use super::SchemaChanges;
use futures::StreamExt;
use std::collections::HashMap;
use surrealdb::engine::local::{Db, Mem};
use surrealdb::{Surreal, engine::remote::http::Client};
use tracing;

/// SurrealDB-specific schema comparator that uses in-memory databases
#[derive(Debug)]
pub struct SurrealdbComparator<'a> {
    db: &'a Surreal<Client>,
    schemasync_config: &'a crate::schemasync::config::SchemasyncConfig,

    // Runtime state
    remote_schema: Option<Surreal<Db>>,
    new_schema: Option<Surreal<Db>>,
    access_query: String,
    remote_schema_string: String,
    new_schema_string: String,
    schema_changes: Option<SchemaChanges>,
}

impl<'a> SurrealdbComparator<'a> {
    pub fn new(
        db: &'a Surreal<Client>,
        schemasync_config: &'a crate::schemasync::config::SchemasyncConfig,
    ) -> Self {
        Self {
            db,
            schemasync_config,
            remote_schema: None,
            new_schema: None,
            access_query: String::new(),
            remote_schema_string: String::new(),
            new_schema_string: String::new(),
            schema_changes: None,
        }
    }

    pub async fn run(&mut self, define_statements: &str) -> Result<()> {
        tracing::info!("Starting SurrealdbComparator pipeline");

        tracing::debug!("Setting up schemas");
        self.setup_schemas(define_statements).await?;

        tracing::debug!("Setting up access definitions");
        self.setup_access().await?;

        tracing::debug!("Exporting schemas for comparison");
        self.export_schemas().await?;

        tracing::debug!("Comparing schemas");
        self.compare_schemas().await?;

        tracing::info!("SurrealdbComparator pipeline completed successfully");
        Ok(())
    }

    /// Setup backup and create in-memory schemas
    async fn setup_schemas(&mut self, define_statements: &str) -> Result<()> {
        tracing::trace!("Creating backup and in-memory schemas");
        let (remote_schema, new_schema) = setup_backup_and_schemas(self.db).await?;
        self.remote_schema = Some(remote_schema);
        // Execute and check define statements
        let _ = new_schema.query(define_statements).await.map_err(|e| {
            EvenframeError::database(format!(
                "There was a problem executing the define statements on the new_schema embedded db: {e}"
            ))
        });

        self.new_schema = Some(new_schema);

        tracing::trace!("Schemas setup complete");
        Ok(())
    }

    /// Setup access definitions
    async fn setup_access(&mut self) -> Result<()> {
        tracing::trace!("Setting up access definitions");
        let new_schema = self.new_schema.as_ref().unwrap();
        self.access_query = setup_access_definitions(new_schema, self.schemasync_config).await?;
        tracing::trace!(
            access_query_length = self.access_query.len(),
            "Access query generated"
        );
        Ok(())
    }

    /// Export schemas for comparison
    async fn export_schemas(&mut self) -> Result<()> {
        tracing::trace!("Exporting schemas");
        let remote_schema = self.remote_schema.as_ref().unwrap();
        let new_schema = self.new_schema.as_ref().unwrap();

        let (remote_schema_string, new_schema_string) =
            export_schemas(remote_schema, new_schema).await?;

        tracing::trace!(
            remote_schema_size = remote_schema_string.len(),
            new_schema_size = new_schema_string.len(),
            "Schemas exported"
        );

        self.remote_schema_string = remote_schema_string;
        self.new_schema_string = new_schema_string;
        Ok(())
    }

    /// Compare schemas to find changes
    async fn compare_schemas(&mut self) -> Result<()> {
        tracing::trace!("Starting schema comparison");
        let changes = compare_schemas(
            self.db,
            &self.remote_schema_string,
            &self.new_schema_string,
        )
        .await?;

        tracing::info!(
            new_tables = changes.new_tables.len(),
            removed_tables = changes.removed_tables.len(),
            modified_tables = changes.modified_tables.len(),
            "Schema changes detected"
        );

        self.schema_changes = Some(changes);
        Ok(())
    }

    // Getters for Mockmaker to access the results
    pub fn get_new_schema(&self) -> Option<&Surreal<Db>> {
        self.new_schema.as_ref()
    }

    pub fn get_access_query(&self) -> &str {
        &self.access_query
    }

    pub fn get_schema_changes(&self) -> Option<&SchemaChanges> {
        self.schema_changes.as_ref()
    }
}

/// Compare two schema export strings and return the differences
pub async fn compare_schemas(
    db: &Surreal<Client>,
    remote_schema_string: &str,
    new_schema_string: &str,
) -> Result<SchemaChanges> {
    tracing::debug!("Parsing and comparing schema exports");
    let importer = SchemaImporter::new(db);

    // Parse exports with error propagation instead of panicking
    let remote_schema = importer
        .parse_schema_from_export(remote_schema_string)
        .map_err(|e| {
            tracing::error!(
                error = %e,
                remote_len = remote_schema_string.len(),
                "Failed parsing remote schema export"
            );
            e
        })?;

    let new_schema = importer
        .parse_schema_from_export(new_schema_string)
        .map_err(|e| {
            tracing::error!(
                error = %e,
                new_len = new_schema_string.len(),
                "Failed parsing new schema export"
            );
            e
        })?;

    let schema_changes = super::Comparator::compare(&remote_schema, &new_schema)?;

    evenframe_log!(format!("{:#?}", schema_changes), "changes.log");
    Ok(schema_changes)
}

/// Export schemas from two in-memory databases
pub async fn export_schemas(
    remote_schema: &Surreal<Db>,
    new_schema: &Surreal<Db>,
) -> Result<(String, String)> {
    tracing::trace!("Exporting remote schema");
    let mut remote_stream = remote_schema
        .export(())
        .with_config()
        .versions(false)
        .accesses(true)
        .analyzers(false)
        .functions(false)
        .records(false)
        .params(false)
        .users(false)
        .await
        .map_err(|e| {
            EvenframeError::database(format!(
                "There was a problem exporting the 'remote_schema' embedded database's schema: {e}"
            ))
        })?;

    let mut remote_schema_string = String::new();
    while let Some(result) = remote_stream.next().await {
        let line = result.map_err(|e| {
            EvenframeError::database(format!("Error reading remote schema stream: {e}"))
        })?;
        remote_schema_string.push_str(&String::from_utf8_lossy(&line));
    }

    evenframe_log!(remote_schema_string, "remote_schema.surql");

    tracing::trace!("Exporting new schema");
    let mut new_stream = new_schema
        .export(())
        .with_config()
        .versions(false)
        .accesses(true)
        .analyzers(false)
        .functions(false)
        .records(false)
        .params(false)
        .users(false)
        .await
        .map_err(|e| {
            EvenframeError::database(format!(
                "There was a problem exporting the 'new_schema' embedded database's schema: {e}"
            ))
        })?;

    let mut new_schema_string = String::new();
    while let Some(result) = new_stream.next().await {
        let line = result.map_err(|e| {
            EvenframeError::database(format!("Error reading new schema stream: {e}"))
        })?;
        new_schema_string.push_str(&String::from_utf8_lossy(&line));
    }

    evenframe_log!(new_schema_string, "new_schema.surql");

    tracing::trace!("Schema export complete");
    Ok((remote_schema_string, new_schema_string))
}

/// Setup backup and in-memory schemas from a remote database
pub async fn setup_backup_and_schemas(db: &Surreal<Client>) -> Result<(Surreal<Db>, Surreal<Db>)> {
    tracing::trace!("Creating database backup");
    let mut backup_stream = db.export(()).await.map_err(|e| {
        EvenframeError::database(format!(
            "There was a problem exporting the remote database: {e}"
        ))
    })?;

    let mut backup = String::new();
    while let Some(result) = backup_stream.next().await {
        let line = result
            .map_err(|e| EvenframeError::database(format!("Error reading backup stream: {e}")))?;
        backup.push_str(&String::from_utf8_lossy(&line));
    }

    evenframe_log!(backup, "backup.surql");

    let remote_schema = Surreal::new::<Mem>(())
        .await
        .expect("Something went wrong starting the remote_schema in-memory db");

    tracing::trace!("Importing backup to remote in-memory schema");
    remote_schema
        .use_ns("remote")
        .use_db("backup")
        .await
        .map_err(|e| {
            EvenframeError::database(format!(
                "There was a problem using the namespace or db for 'remote_schema': {e}"
            ))
        })?;

    remote_schema.query(&backup).await.map_err(|e| {
        EvenframeError::database(format!(
            "Something went wrong importing the remote schema to the in-memory db: {e}"
        ))
    })?;

    let new_schema = Surreal::new::<Mem>(()).await.map_err(|e| {
        EvenframeError::database(format!(
            "Something went wrong starting the new_schema in-memory db: {e}"
        ))
    })?;

    tracing::trace!("Setting up new in-memory schema");
    new_schema
        .use_ns("new")
        .use_db("memory")
        .await
        .map_err(|e| {
            EvenframeError::database(format!(
                "There was a problem exporting the 'remote_schema' embedded database's schema: {e}"
            ))
        })?;

    tracing::trace!("In-memory schemas ready");
    Ok((remote_schema, new_schema))
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
        let mut table_events: HashMap<String, Vec<String>> = HashMap::new();

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
                        permissions: None,
                        indexes: Vec::new(),
                        events: table_events.remove(&table_name).unwrap_or_default(),
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
            // Parse DEFINE EVENT statements
            else if trimmed.starts_with("DEFINE EVENT") {
                if let Some((table_name, event_statement)) = Self::parse_event_definition(trimmed) {
                    table_events
                        .entry(table_name)
                        .or_default()
                        .push(event_statement);
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
                events: table_events.remove(&table_name).unwrap_or_default(),
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
        } else {
            // Default to Schemaless (includes explicit SCHEMALESS or unspecified)
            SchemaType::Schemaless
        }
    }

    /// Extract table name from DEFINE TABLE statement
    fn extract_table_name(statement: &str) -> Option<String> {
        let parts: Vec<&str> = statement.split_whitespace().collect();
        if parts.len() >= 3 && parts[0] == "DEFINE" && parts[1] == "TABLE" {
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

        value_stack
            .pop()
            .unwrap_or_else(|| ObjectType::Simple("unknown".to_string()))
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

    /// Parse a DEFINE EVENT statement and extract the associated table
    fn parse_event_definition(statement: &str) -> Option<(String, String)> {
        if !statement.starts_with("DEFINE EVENT") {
            return None;
        }

        let uppercase = statement.to_uppercase();
        let on_table = " ON TABLE ";
        let on_table_index = uppercase.find(on_table)?;
        let after_on_table = &statement[on_table_index + on_table.len()..];
        let mut parts = after_on_table.split_whitespace();
        let table_token = parts.next()?;
        let table_name = table_token
            .trim_matches('`')
            .trim_end_matches(';')
            .to_string();

        Some((table_name, statement.trim().to_string()))
    }

    /// Parse a DEFINE ACCESS statement
    fn parse_access_definition(statement: &str) -> Option<AccessDefinition> {
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
