//! SurrealDB-specific schema comparison implementation
//!
//! This module contains the SurrealDB-specific schema comparison logic,
//! including the SchemaImporter for parsing SurrealQL exports and the
//! SurrealdbComparator for comparing schemas using in-memory databases.

use super::SchemaChanges;
use super::types::{
    AccessDefinition, AnalyzerDefinition, FieldDefinition, IndexDefinition, ObjectType,
    SchemaDefinition, SchemaType, TableDefinition,
};
use crate::{
    EvenframeError, Result, evenframe_log,
    schemasync::{config::AccessType, database::surql::access::setup_access_definitions},
};
use futures::StreamExt;
use std::collections::BTreeMap;
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

        // Analyzers first: FULLTEXT indexes in the define statements reference them.
        let resolved = &self.schemasync_config.database.resolved;
        if let Some(ref analyzers_surql) = resolved.analyzers_surql
            && !analyzers_surql.is_empty()
        {
            if analyzers_reference_functions(analyzers_surql)
                && let Some(ref functions_surql) = resolved.functions_surql
            {
                let _ = new_schema.query(functions_surql.as_str()).await;
            }
            tracing::debug!("Executing analyzer surql on embedded DB");
            let _ = new_schema
                .query(analyzers_surql.as_str())
                .await
                .map_err(|e| {
                    tracing::warn!(error = %e, "Failed to execute analyzer surql on embedded DB");
                });
        }

        // Execute and check define statements
        let _ = new_schema.query(define_statements).await.map_err(|e| {
            EvenframeError::database(format!(
                "There was a problem executing the define statements on the new_schema embedded db: {e}"
            ))
        });

        // Execute function surql on embedded DB if available (for validation)
        if let Some(ref functions_surql) = self.schemasync_config.database.resolved.functions_surql
            && !functions_surql.is_empty()
        {
            tracing::debug!("Executing function surql on embedded DB for validation");
            let _ = new_schema.query(functions_surql.as_str()).await.map_err(|e| {
                tracing::warn!(error = %e, "Failed to execute function surql on embedded DB");
                EvenframeError::database(format!(
                    "There was a problem executing function surql on the new_schema embedded db: {e}"
                ))
            });
        }

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
        let changes =
            compare_schemas(self.db, &self.remote_schema_string, &self.new_schema_string).await?;

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
        .analyzers(true)
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
        .analyzers(true)
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
/// Keywords that end the column list of a `DEFINE INDEX` statement.
const INDEX_CLAUSE_KEYWORDS: &[&str] = &[
    "UNIQUE",
    "COUNT",
    "FULLTEXT",
    "SEARCH",
    "HNSW",
    "DISKANN",
    "COMMENT",
    "CONCURRENTLY",
];

/// If `s` (after leading whitespace) starts with the whitespace-separated
/// `keyword` (case-insensitive, followed by whitespace or end of input),
/// return the remainder with leading whitespace trimmed.
fn strip_keyword<'s>(s: &'s str, keyword: &str) -> Option<&'s str> {
    let mut rest = s;
    for word in keyword.split_whitespace() {
        rest = rest.trim_start();
        if !rest
            .get(..word.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(word))
        {
            return None;
        }
        rest = &rest[word.len()..];
        if !(rest.is_empty() || rest.starts_with(char::is_whitespace)) {
            return None;
        }
    }
    Some(rest.trim_start())
}

/// Split off the first whitespace-delimited token, honouring backtick and
/// `⟨…⟩` quoting.
fn split_first_token(s: &str) -> Option<(&str, &str)> {
    let s = s.trim_start();
    if s.is_empty() {
        return None;
    }
    let end = if let Some(inner) = s.strip_prefix('`') {
        inner.find('`')? + 2
    } else if let Some(inner) = s.strip_prefix('⟨') {
        inner.find('⟩')? + '⟨'.len_utf8() + '⟩'.len_utf8()
    } else {
        s.find(char::is_whitespace).unwrap_or(s.len())
    };
    Some((&s[..end], &s[end..]))
}

fn unquote_ident(token: &str) -> String {
    token
        .trim_matches('`')
        .trim_start_matches('⟨')
        .trim_end_matches('⟩')
        .to_string()
}

/// Call `visit(byte_index, char)` for every char of `s` that is outside
/// quotes and bracket nesting. `visit` returns `false` to stop early.
fn for_each_top_level_char(s: &str, mut visit: impl FnMut(usize, char) -> bool) {
    let mut quote: Option<char> = None;
    let mut depth = 0usize;
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '\'' | '"' | '`' => quote = Some(c),
            '⟨' => quote = Some('⟩'),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ if depth == 0 && !visit(i, c) => return,
            _ => {}
        }
    }
}

/// Byte offset of the earliest top-level, whitespace-preceded occurrence of
/// any of `keywords` (case-insensitive, whole word).
fn find_top_level_keyword(s: &str, keywords: &[&str]) -> Option<usize> {
    let mut found = None;
    let mut prev_is_space = true;
    for_each_top_level_char(s, |i, c| {
        if prev_is_space && !c.is_whitespace() {
            let rest = &s[i..];
            let word_end = rest
                .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                .unwrap_or(rest.len());
            if keywords
                .iter()
                .any(|k| k.eq_ignore_ascii_case(&rest[..word_end]))
            {
                found = Some(i);
                return false;
            }
        }
        prev_is_space = c.is_whitespace();
        true
    });
    found
}

fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    for_each_top_level_char(s, |i, c| {
        if c == ',' {
            parts.push(&s[start..i]);
            start = i + 1;
        }
        true
    });
    parts.push(&s[start..]);
    parts
}

/// Whether any `DEFINE ANALYZER` in `surql` uses a `FUNCTION fn::...`
/// preprocessor, which must exist before the analyzer is defined.
pub fn analyzers_reference_functions(surql: &str) -> bool {
    surql.to_uppercase().contains("FUNCTION FN::")
}

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
        let mut tables = BTreeMap::new();
        let edges = BTreeMap::new();
        let mut accesses = Vec::new();
        let mut analyzers = Vec::new();
        let mut current_table: Option<String> = None;
        let mut current_table_statement: Option<String> = None;
        let mut current_fields: BTreeMap<String, FieldDefinition> = BTreeMap::new();
        let mut current_wildcard_fields: BTreeMap<String, FieldDefinition> = BTreeMap::new();
        let mut table_events: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut table_indexes: BTreeMap<String, Vec<IndexDefinition>> = BTreeMap::new();

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
                        indexes: table_indexes.remove(&table_name).unwrap_or_default(),
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
            // Parse DEFINE ANALYZER statements
            else if trimmed.starts_with("DEFINE ANALYZER") {
                if let Some(analyzer_def) = Self::parse_analyzer_definition(trimmed) {
                    analyzers.push(analyzer_def);
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
            else if trimmed.starts_with("DEFINE INDEX")
                && let Some((table_name, index_def)) = Self::parse_index_definition(trimmed)
            {
                table_indexes.entry(table_name).or_default().push(index_def);
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
                indexes: table_indexes.remove(&table_name).unwrap_or_default(),
                events: table_events.remove(&table_name).unwrap_or_default(),
            };
            tables.insert(table_name, table_def);
        }

        Ok(SchemaDefinition {
            tables,
            edges,
            accesses,
            analyzers,
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
                    '|' if brace_count == 0
                        && bracket_count == 0
                        // Check if this is part of " | "
                        && i > 0
                        && i < chars.len() - 1
                        && chars[i - 1] == ' '
                        && chars[i + 1] == ' ' =>
                    {
                        // Add the part before this union separator
                        let part = &type_str[current_start..i - 1];
                        if !part.trim().is_empty() {
                            parts.push(part.trim());
                        }
                        current_start = i + 2; // Skip past " | "
                        i += 1; // Extra increment to skip the space after |
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
        let mut fields = BTreeMap::new();

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
        // Strip OVERWRITE / IF NOT EXISTS if present
        let after_field = statement.strip_prefix("DEFINE FIELD")?.trim();
        let after_field = if after_field.starts_with("OVERWRITE ") {
            after_field.strip_prefix("OVERWRITE ")?.trim()
        } else if after_field.starts_with("IF NOT EXISTS ") {
            after_field.strip_prefix("IF NOT EXISTS ")?.trim()
        } else {
            after_field
        };

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

        // Check for COMPUTED expression (SurrealDB 3.0)
        let computed_expression = if let Some(computed_pos) = statement.find(" COMPUTED ") {
            let after_computed = &statement[computed_pos + 10..].trim();

            // Find the end of the computed expression (before TYPE, FLEXIBLE, PERMISSIONS, COMMENT, or ;)
            let mut expr_end = 0;
            let mut paren_count = 0;
            let mut brace_count = 0;
            let mut in_quotes = false;
            let mut quote_char = ' ';

            for (i, ch) in after_computed.char_indices() {
                if !in_quotes && (ch == '\'' || ch == '"') {
                    in_quotes = true;
                    quote_char = ch;
                } else if in_quotes && ch == quote_char {
                    in_quotes = false;
                }

                if !in_quotes {
                    match ch {
                        '(' => paren_count += 1,
                        ')' => paren_count -= 1,
                        '{' => brace_count += 1,
                        '}' => brace_count -= 1,
                        ' ' if paren_count == 0
                            && brace_count == 0
                            && (after_computed[i..].starts_with(" TYPE ")
                                || after_computed[i..].starts_with(" FLEXIBLE")
                                || after_computed[i..].starts_with(" PERMISSIONS")
                                || after_computed[i..].starts_with(" COMMENT")) =>
                        {
                            expr_end = i;
                            break;
                        }
                        ';' if paren_count == 0 && brace_count == 0 => {
                            expr_end = i;
                            break;
                        }
                        _ => {}
                    }
                }
                expr_end = i + 1;
            }

            Some(
                after_computed[..expr_end]
                    .trim()
                    .trim_end_matches(';')
                    .to_string(),
            )
        } else {
            None
        };

        // For computed fields, TYPE is optional; for regular fields, TYPE is required
        let field_type = if let Some(type_pos) = statement.find(" TYPE ") {
            let after_type = &statement[type_pos + 6..].trim();

            // Find the end of the type definition
            let mut type_end = 0;
            let mut bracket_count = 0;
            let mut brace_count = 0;
            let mut in_quotes = false;
            let mut quote_char = ' ';

            for (i, ch) in after_type.char_indices() {
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
                            && (after_type[i..].starts_with(" DEFAULT")
                                || after_type[i..].starts_with(" ASSERT")
                                || after_type[i..].starts_with(" READONLY")
                                || after_type[i..].starts_with(" VALUE")
                                || after_type[i..].starts_with(" PERMISSIONS")
                                || after_type[i..].starts_with(" COMMENT")) =>
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
            Self::parse_type_string(field_type_str)
        } else if computed_expression.is_some() {
            // Computed fields without explicit TYPE default to "any"
            ObjectType::Simple("any".to_string())
        } else {
            // Regular fields without TYPE - shouldn't normally happen but handle gracefully
            return None;
        };

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
                                && (after_default[i..].starts_with(" ASSERT")
                                    || after_default[i..].starts_with(" READONLY")
                                    || after_default[i..].starts_with(" VALUE")
                                    || after_default[i..].starts_with(" PERMISSIONS")
                                    || after_default[i..].starts_with(" COMMENT")) =>
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
                .or_else(|| after_assert.find(" COMMENT"))
                .unwrap_or(after_assert.len());
            let assert_content = after_assert[..assert_end].trim_end_matches(';');
            vec![assert_content.to_string()]
        } else {
            Vec::new()
        };

        // Extract COMMENT
        let comment = if let Some(comment_pos) = statement.find(" COMMENT ") {
            let after_comment = &statement[comment_pos + 9..].trim();
            // Comment is a quoted string - extract it
            let comment_str = after_comment.trim_end_matches(';').trim();
            if (comment_str.starts_with('\'') && comment_str.ends_with('\''))
                || (comment_str.starts_with('"') && comment_str.ends_with('"'))
            {
                Some(comment_str[1..comment_str.len() - 1].to_string())
            } else {
                Some(comment_str.to_string())
            }
        } else {
            None
        };

        Some(FieldDefinition {
            name: field_name.to_string(),
            field_type,
            required: !has_default && computed_expression.is_none(),
            default_value,
            assertions,
            parent_array_field: parent_array,
            computed_expression,
            comment,
        })
    }

    /// Parse a DEFINE EVENT statement and extract the associated table.
    ///
    /// SurrealDB's `DEFINE EVENT` syntax treats the `TABLE` keyword as
    /// optional — `... ON TABLE foo ...` and `... ON foo ...` are both
    /// valid. INFO/EXPORT round-trips strip the keyword, so the
    /// schema-import parser has to accept both forms or it silently
    /// drops every existing event from the loaded schema. When that
    /// happens, the diff sees zero pre-existing events for the table,
    /// the comparator emits no `removed_events`, and a deleted
    /// `#[event(...)]` declaration leaves a stale event live in the DB
    /// forever (most visibly: a previously auto-attached `track_activity`
    /// keeps firing on tables that have since opted out, 500-ing every
    /// PATCH against the orphaned audit edge).
    fn parse_event_definition(statement: &str) -> Option<(String, String)> {
        if !statement.starts_with("DEFINE EVENT") {
            return None;
        }

        let uppercase = statement.to_uppercase();
        let on_index = uppercase.find(" ON ")?;
        let after_on = &statement[on_index + " ON ".len()..];
        let trimmed = after_on.trim_start();
        // Skip the optional TABLE keyword (case-insensitive).
        let after_table_kw = if trimmed
            .get(..6)
            .map(|s| s.eq_ignore_ascii_case("TABLE "))
            .unwrap_or(false)
        {
            &trimmed[6..]
        } else {
            trimmed
        };

        let mut parts = after_table_kw.split_whitespace();
        let table_token = parts.next()?;
        let table_name = table_token
            .trim_matches('`')
            .trim_end_matches(';')
            .to_string();

        Some((table_name, statement.trim().to_string()))
    }

    /// Parse a DEFINE INDEX statement into (table_name, IndexDefinition)
    ///
    /// Format: `DEFINE INDEX [OVERWRITE | IF NOT EXISTS] <name> ON [TABLE] <table>
    /// [FIELDS|COLUMNS <col1>, <col2>] [UNIQUE | COUNT .. | FULLTEXT .. | HNSW .. | DISKANN ..]
    /// [COMMENT ..] [CONCURRENTLY];`
    ///
    /// Everything after the column list is kept verbatim (minus the trailing
    /// `;`) in [`IndexDefinition::definition`]. Both sides of a comparison come
    /// from SurrealDB exports, which normalize that clause, so a plain string
    /// comparison detects changed index kinds and parameters.
    pub(crate) fn parse_index_definition(statement: &str) -> Option<(String, IndexDefinition)> {
        let statement = statement.trim().trim_end_matches(';').trim_end();
        let after_define = strip_keyword(statement, "DEFINE INDEX")?;
        let after_kind = strip_keyword(after_define, "OVERWRITE")
            .or_else(|| strip_keyword(after_define, "IF NOT EXISTS"))
            .unwrap_or(after_define);

        let (name_token, rest) = split_first_token(after_kind)?;
        let index_name = unquote_ident(name_token);

        let rest = strip_keyword(rest, "ON")?;
        let rest = strip_keyword(rest, "TABLE").unwrap_or(rest);
        let (table_token, rest) = split_first_token(rest)?;
        let table_name = unquote_ident(table_token);

        let (columns, definition) = match strip_keyword(rest, "FIELDS")
            .or_else(|| strip_keyword(rest, "COLUMNS"))
        {
            Some(after_columns) => {
                let columns_end = find_top_level_keyword(after_columns, INDEX_CLAUSE_KEYWORDS)
                    .unwrap_or(after_columns.len());
                let columns: Vec<String> = split_top_level_commas(&after_columns[..columns_end])
                    .into_iter()
                    .map(|c| c.trim().trim_matches('`').to_string())
                    .filter(|c| !c.is_empty())
                    .collect();
                if columns.is_empty() {
                    return None;
                }
                (columns, after_columns[columns_end..].trim())
            }
            // COUNT indexes have no columns
            None => (Vec::new(), rest.trim()),
        };

        let unique = strip_keyword(definition, "UNIQUE").is_some();

        Some((
            table_name,
            IndexDefinition {
                name: index_name,
                columns,
                unique,
                definition: definition.to_string(),
            },
        ))
    }

    /// Parse a DEFINE ANALYZER statement. The stored statement has the
    /// `OVERWRITE` / `IF NOT EXISTS` modifier and trailing `;` stripped so
    /// exports from different databases compare equal.
    pub(crate) fn parse_analyzer_definition(statement: &str) -> Option<AnalyzerDefinition> {
        let statement = statement.trim().trim_end_matches(';').trim_end();
        let after_define = strip_keyword(statement, "DEFINE ANALYZER")?;
        let after_kind = strip_keyword(after_define, "OVERWRITE")
            .or_else(|| strip_keyword(after_define, "IF NOT EXISTS"))
            .unwrap_or(after_define);
        let (name_token, rest) = split_first_token(after_kind)?;
        let rest = rest.trim();
        let normalized = if rest.is_empty() {
            format!("DEFINE ANALYZER {name_token}")
        } else {
            format!("DEFINE ANALYZER {name_token} {rest}")
        };
        Some(AnalyzerDefinition {
            name: unquote_ident(name_token),
            statement: normalized,
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_index(stmt: &str) -> (String, IndexDefinition) {
        SchemaImporter::parse_index_definition(stmt)
            .unwrap_or_else(|| panic!("failed to parse: {stmt}"))
    }

    #[test]
    fn parse_index_definitions_from_surrealdb_export() {
        // Statements as written by a SurrealDB 3.2 export.
        let (table, idx) = parse_index(
            "DEFINE INDEX post_search ON post FIELDS body FULLTEXT ANALYZER english \
             BM25(1.2,0.75) HIGHLIGHTS COMMENT 'full-text search';",
        );
        assert_eq!(table, "post");
        assert_eq!(idx.name, "post_search");
        assert_eq!(idx.columns, vec!["body"]);
        assert!(!idx.unique);
        assert_eq!(
            idx.definition,
            "FULLTEXT ANALYZER english BM25(1.2,0.75) HIGHLIGHTS COMMENT 'full-text search'"
        );

        let (_, idx) = parse_index(
            "DEFINE INDEX idx_post_embedding ON post FIELDS embedding HNSW DIMENSION 3 \
             DIST COSINE TYPE F32 EFC 100 M 8 M0 16 LM 0.48089834696298783f;",
        );
        assert_eq!(idx.columns, vec!["embedding"]);
        assert!(idx.definition.starts_with("HNSW DIMENSION 3"));

        let (_, idx) = parse_index(
            "DEFINE INDEX post_ann ON post FIELDS embedding DISKANN DIMENSION 3 DIST EUCLIDEAN \
             TYPE F32 DEGREE 16 L_BUILD 50 ALPHA 1.2f CONCURRENTLY;",
        );
        assert!(idx.definition.ends_with("CONCURRENTLY"));

        let (_, idx) =
            parse_index("DEFINE INDEX idx_post_tags_created_at ON post FIELDS tags.*, created_at;");
        assert_eq!(idx.columns, vec!["tags.*", "created_at"]);
        assert_eq!(idx.definition, "");

        let (table, idx) = parse_index("DEFINE INDEX idx_post_count ON post COUNT;");
        assert_eq!(table, "post");
        assert!(idx.columns.is_empty());
        assert_eq!(idx.definition, "COUNT");

        let (_, idx) =
            parse_index("DEFINE INDEX post_published_count ON post COUNT WHERE published = true;");
        assert!(idx.columns.is_empty());
        assert_eq!(idx.definition, "COUNT WHERE published = true");

        let (_, idx) = parse_index("DEFINE INDEX idx_post_slug ON post FIELDS slug UNIQUE;");
        assert!(idx.unique);
        assert_eq!(idx.definition, "UNIQUE");
    }

    #[test]
    fn parse_index_definition_generated_forms() {
        // OVERWRITE / IF NOT EXISTS, ON TABLE, COLUMNS, backtick-quoted names.
        let (table, idx) = parse_index(
            "DEFINE INDEX OVERWRITE idx_reaction_user_message ON TABLE reaction \
             FIELDS user, message UNIQUE;",
        );
        assert_eq!(table, "reaction");
        assert_eq!(idx.name, "idx_reaction_user_message");
        assert_eq!(idx.columns, vec!["user", "message"]);
        assert!(idx.unique);

        let (table, idx) =
            parse_index("DEFINE INDEX IF NOT EXISTS `my idx` ON `my table` COLUMNS `a`, b");
        assert_eq!(table, "my table");
        assert_eq!(idx.name, "my idx");
        assert_eq!(idx.columns, vec!["a", "b"]);
    }

    #[test]
    fn parse_index_definition_ignores_keywords_inside_strings() {
        // `UNIQUE` inside the comment must neither end the column list early
        // nor mark the index unique.
        let (_, idx) = parse_index(
            "DEFINE INDEX idx_t_a ON t FIELDS a COMMENT 'not UNIQUE, uses FULLTEXT soon';",
        );
        assert_eq!(idx.columns, vec!["a"]);
        assert!(!idx.unique);
        assert_eq!(idx.definition, "COMMENT 'not UNIQUE, uses FULLTEXT soon'");

        // A column literally named like a keyword prefix is not a keyword.
        let (_, idx) = parse_index("DEFINE INDEX idx_t_u ON t FIELDS unique_code, counter;");
        assert_eq!(idx.columns, vec!["unique_code", "counter"]);
        assert!(!idx.unique);
    }

    #[test]
    fn parse_analyzer_definitions() {
        let a = SchemaImporter::parse_analyzer_definition(
            "DEFINE ANALYZER english TOKENIZERS BLANK,CLASS FILTERS LOWERCASE, SNOWBALL(ENGLISH);",
        )
        .unwrap();
        assert_eq!(a.name, "english");
        assert_eq!(
            a.statement,
            "DEFINE ANALYZER english TOKENIZERS BLANK,CLASS FILTERS LOWERCASE, SNOWBALL(ENGLISH)"
        );

        let b = SchemaImporter::parse_analyzer_definition(
            "DEFINE ANALYZER OVERWRITE english TOKENIZERS BLANK,CLASS FILTERS LOWERCASE, SNOWBALL(ENGLISH)",
        )
        .unwrap();
        assert_eq!(a, b, "OVERWRITE and trailing `;` must not affect equality");

        let bare = SchemaImporter::parse_analyzer_definition("DEFINE ANALYZER IF NOT EXISTS bare;")
            .unwrap();
        assert_eq!(bare.name, "bare");
        assert_eq!(bare.statement, "DEFINE ANALYZER bare");
    }

    #[test]
    fn detects_function_preprocessors_in_analyzers() {
        assert!(analyzers_reference_functions(
            "DEFINE ANALYZER a FUNCTION fn::strip TOKENIZERS blank;"
        ));
        assert!(!analyzers_reference_functions(
            "DEFINE ANALYZER a TOKENIZERS blank;"
        ));
    }

    #[test]
    fn parse_computed_field_definition() {
        let stmt = "DEFINE FIELD upper_name ON TABLE user COMPUTED string::uppercase($value.name) TYPE string PERMISSIONS FOR select FULL FOR create FULL FOR update FULL";
        let field = SchemaImporter::parse_field_definition(stmt).unwrap();

        assert_eq!(field.name, "upper_name");
        assert_eq!(
            field.computed_expression,
            Some("string::uppercase($value.name)".to_string())
        );
        assert_eq!(field.field_type, ObjectType::Simple("string".to_string()));
        assert!(!field.required); // computed fields are not required
    }

    #[test]
    fn parse_computed_field_without_type() {
        let stmt = "DEFINE FIELD upper_name ON TABLE user COMPUTED string::uppercase($value.name) PERMISSIONS FOR select FULL";
        let field = SchemaImporter::parse_field_definition(stmt).unwrap();

        assert_eq!(field.name, "upper_name");
        assert_eq!(
            field.computed_expression,
            Some("string::uppercase($value.name)".to_string())
        );
        assert_eq!(field.field_type, ObjectType::Simple("any".to_string()));
    }

    #[test]
    fn parse_field_with_overwrite() {
        let stmt = "DEFINE FIELD OVERWRITE name ON TABLE user TYPE string DEFAULT '' PERMISSIONS FOR select FULL";
        let field = SchemaImporter::parse_field_definition(stmt).unwrap();

        assert_eq!(field.name, "name");
        assert_eq!(field.field_type, ObjectType::Simple("string".to_string()));
        assert!(field.computed_expression.is_none());
    }

    #[test]
    fn parse_field_with_if_not_exists() {
        let stmt = "DEFINE FIELD IF NOT EXISTS name ON TABLE user TYPE string DEFAULT ''";
        let field = SchemaImporter::parse_field_definition(stmt).unwrap();

        assert_eq!(field.name, "name");
        assert_eq!(field.field_type, ObjectType::Simple("string".to_string()));
    }

    #[test]
    fn parse_field_with_comment() {
        let stmt = "DEFINE FIELD email ON TABLE user TYPE string DEFAULT '' PERMISSIONS FOR select FULL COMMENT 'User email address'";
        let field = SchemaImporter::parse_field_definition(stmt).unwrap();

        assert_eq!(field.name, "email");
        assert_eq!(field.comment, Some("User email address".to_string()));
    }

    #[test]
    fn parse_computed_field_with_comment() {
        let stmt = "DEFINE FIELD upper_name ON TABLE user COMPUTED string::uppercase($value.name) TYPE string COMMENT 'Auto-uppercased'";
        let field = SchemaImporter::parse_field_definition(stmt).unwrap();

        assert_eq!(field.name, "upper_name");
        assert_eq!(
            field.computed_expression,
            Some("string::uppercase($value.name)".to_string())
        );
        assert_eq!(field.comment, Some("Auto-uppercased".to_string()));
    }

    #[test]
    fn parse_regular_field_no_computed() {
        let stmt = "DEFINE FIELD name ON TABLE user TYPE string DEFAULT '' PERMISSIONS FOR select FULL FOR create FULL FOR update FULL";
        let field = SchemaImporter::parse_field_definition(stmt).unwrap();

        assert_eq!(field.name, "name");
        assert!(field.computed_expression.is_none());
        assert!(field.comment.is_none());
        assert_eq!(field.field_type, ObjectType::Simple("string".to_string()));
    }
}
