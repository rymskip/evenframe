use crate::evenframe_log;
use serde_json::Value;
use surrealdb::IndexedResults;
use tracing::{debug, error, info, trace, warn};

#[derive(Debug)]
pub struct QueryValidationError {
    pub statement_index: usize,
    pub error_type: QueryErrorType,
    pub message: String,
    pub statement: Option<String>,
}

#[derive(Debug)]
pub enum QueryErrorType {
    ParseError,
    ValidationError,
    ConstraintViolation,
    RecordNotFound,
    PermissionDenied,
    TransactionRollback,
    PartialFailure,
    UnknownError,
}

/// Remove top-level SurrealQL comments (`-- …`, `// …`, `# …` and
/// `/* … */`) from `block`, keeping line breaks so statements stay apart.
///
/// Comments are only recognised outside string literals and outside
/// `{...}`/`(...)`/`[...]` groups: inside those, `--` or `//` can be real code
/// (e.g. `i--` in an embedded JavaScript `ASSERT` body), and a `;` there never
/// splits a statement anyway. Strip before [`split_surql_statements`] so a
/// `;` inside a comment doesn't produce a phantom statement and a
/// comment-only fragment isn't counted as one.
pub fn strip_surql_comments(block: &str) -> String {
    let bytes = block.as_bytes();
    let mut out = String::with_capacity(block.len());
    let mut copied = 0;
    let mut depth: i32 = 0;
    let mut quote: Option<u8> = None;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = quote {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        let next = bytes.get(i + 1).copied();
        let comment_end = match (c, next) {
            _ if depth > 0 => None,
            (b'-', Some(b'-')) | (b'/', Some(b'/')) | (b'#', _) => {
                // Line comment: drop up to (not including) the newline
                Some(block[i..].find('\n').map_or(block.len(), |n| i + n))
            }
            (b'/', Some(b'*')) => {
                // Block comment: drop through `*/`, or to the end if unclosed
                Some(
                    block[i + 2..]
                        .find("*/")
                        .map_or(block.len(), |n| i + 2 + n + 2),
                )
            }
            _ => None,
        };
        if let Some(end) = comment_end {
            out.push_str(&block[copied..i]);
            // Keep the block comment's line breaks so line structure survives
            out.extend(block[i..end].chars().filter(|&ch| ch == '\n'));
            copied = end;
            i = end;
            continue;
        }
        match c {
            b'"' | b'\'' | b'`' => quote = Some(c),
            b'{' | b'(' | b'[' => depth += 1,
            b'}' | b')' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
        i += 1;
    }
    out.push_str(&block[copied..]);
    out
}

/// Split a block of SurrealQL into individual statements on top-level `;`,
/// ignoring semicolons inside `{...}`/`(...)`/`[...]` groups (e.g. embedded
/// JavaScript `ASSERT function(){…}` bodies) and inside string literals.
///
/// Returned slices include their trailing `;` (matching `split_inclusive(';')`),
/// and a trailing fragment without a `;` is still returned. A naive
/// `split(';')` truncates DEFINE FIELD statements whose ASSERT is an embedded
/// JS function, so any code that routes/counts individual statements must use
/// this instead. It does not understand comments: run hand-written SurrealQL
/// through [`strip_surql_comments`] first.
pub fn split_surql_statements(block: &str) -> Vec<&str> {
    let bytes = block.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut depth: i32 = 0;
    let mut quote: Option<u8> = None;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match quote {
            Some(q) => {
                if c == b'\\' {
                    // Skip the escaped character.
                    i += 2;
                    continue;
                }
                if c == q {
                    quote = None;
                }
            }
            None => match c {
                b'"' | b'\'' | b'`' => quote = Some(c),
                b'{' | b'(' | b'[' => depth += 1,
                b'}' | b')' | b']' => depth = depth.saturating_sub(1),
                b';' if depth == 0 => {
                    out.push(&block[start..=i]);
                    start = i + 1;
                }
                _ => {}
            },
        }
        i += 1;
    }
    if start < block.len() {
        let tail = &block[start..];
        if !tail.trim().is_empty() {
            out.push(tail);
        }
    }
    out
}

/// Validates a SurrealDB response and panics if any errors are found
/// This includes checking for:
/// - Parse errors
/// - Validation errors
/// - Partial failures (some statements succeed, some fail)
/// - Empty results when records should have been created
pub async fn validate_surql_response(
    mut response: IndexedResults,
    statements: &str,
    expected_operation: &str,
) -> Result<Vec<Value>, Vec<QueryValidationError>> {
    info!(expected_operation = %expected_operation, statement_length = statements.len(), "Validating SurrealQL response");
    trace!("Statements to validate: {}", statements);
    let mut errors = Vec::new();
    let mut results = Vec::new();
    debug!("Initialized validation state");

    // Split statements for error reporting. Brace/string-aware so embedded
    // JavaScript function bodies (which contain their own `;`) aren't split
    // mid-statement and miscounted against the response. Comments are
    // stripped first: SurrealDB returns no result for them, so a `;` inside a
    // comment or a comment-only fragment would throw the count off.
    let uncommented = strip_surql_comments(statements);
    let statement_lines: Vec<&str> = split_surql_statements(&uncommented)
        .into_iter()
        .filter(|s| !s.trim().is_empty())
        .collect();

    // Process each result from the response
    for (index, statement) in statement_lines.iter().enumerate() {
        match response.take::<surrealdb::types::Value>(index) {
            Ok(surreal_value) => {
                let value: Value = serde_json::to_value(&surreal_value).unwrap_or(Value::Null);
                // Check if the result is an error disguised as success
                if let Some(obj) = value.as_object() {
                    // Check for error indicators in the response
                    if obj.contains_key("error") || obj.contains_key("code") {
                        errors.push(QueryValidationError {
                            statement_index: index,
                            error_type: QueryErrorType::UnknownError,
                            message: format!("Hidden error in response: {:?}", obj),
                            statement: Some(statement.to_string()),
                        });
                    } else if expected_operation == "UPSERT" || expected_operation == "INSERT" {
                        // For UPSERT/INSERT, we expect a non-empty result
                        if value.is_null()
                            || (value.is_array() && value.as_array().unwrap().is_empty())
                        {
                            errors.push(QueryValidationError {
                                statement_index: index,
                                error_type: QueryErrorType::PartialFailure,
                                message: "UPSERT/INSERT returned empty result".to_string(),
                                statement: Some(statement.to_string()),
                            });
                        }
                    }
                }

                // Check for specific error patterns in string results
                if let Some(s) = value.as_str()
                    && (s.contains("error") || s.contains("failed") || s.contains("violation"))
                {
                    errors.push(QueryValidationError {
                        statement_index: index,
                        error_type: QueryErrorType::UnknownError,
                        message: format!("Potential error in string result: {}", s),
                        statement: Some(statement.to_string()),
                    });
                }

                results.push(value);
            }
            Err(e) => {
                let error_string = e.to_string().to_lowercase();
                let error_type = match error_string {
                    s if s.contains("parse") => QueryErrorType::ParseError,
                    s if s.contains("validation") || s.contains("schema") => {
                        QueryErrorType::ValidationError
                    }
                    s if s.contains("constraint") => QueryErrorType::ConstraintViolation,
                    s if s.contains("not found") => QueryErrorType::RecordNotFound,
                    s if s.contains("permission") => QueryErrorType::PermissionDenied,
                    s if s.contains("transaction") => QueryErrorType::TransactionRollback,
                    _ => QueryErrorType::UnknownError,
                };

                errors.push(QueryValidationError {
                    statement_index: index,
                    error_type,
                    message: e.to_string(),
                    statement: Some(statement.to_string()),
                });
            }
        }
    }

    if errors.is_empty() {
        Ok(results)
    } else {
        Err(errors)
    }
}

/// Executes a query and validates the response, panicking on any errors.
/// If the request fails with 413 Payload Too Large, falls back to writing
/// a `.surql` file and importing it via `surreal import`.
pub async fn execute_and_validate<C>(
    db: &surrealdb::Surreal<C>,
    statements: &str,
    operation_type: &str,
    table_name: &str,
) -> Result<Vec<Value>, Box<dyn std::error::Error>>
where
    C: surrealdb::Connection,
{
    info!(operation_type = %operation_type, table_name = %table_name, statement_length = statements.len(), "Executing and validating statements");
    trace!("Statements: {}", statements);

    // SurrealDB's HTTP RPC has a ~1MB payload limit. For large statements,
    // skip the RPC path entirely and import via the CLI.
    const RPC_SIZE_LIMIT: usize = 800_000; // 800KB threshold (conservative)
    if statements.len() > RPC_SIZE_LIMIT {
        info!(
            operation_type = %operation_type,
            table_name = %table_name,
            size = statements.len(),
            "Statement exceeds RPC size limit, using surreal import"
        );
        return import_via_cli(statements, operation_type, table_name).await;
    }

    debug!("Sending query to database");
    let response = db.query(statements).await.map_err(|e| {
        error!(operation_type = %operation_type, table_name = %table_name, error = %e, "Database query failed");
        e
    })?;

    match validate_surql_response(response, statements, operation_type).await {
        Ok(results) => {
            // Log success with details
            evenframe_log!(
                &format!(
                    "Successfully executed {} {} statements for table {} with {} results",
                    results.len(),
                    operation_type,
                    table_name,
                    results.iter().filter(|v| !v.is_null()).count()
                ),
                "results.log",
                true
            );
            Ok(results)
        }
        Err(errors) => {
            // Log all errors before panicking
            evenframe_log!(
                &format!(
                    "ERRORS executing {} for table {}: {} errors found",
                    operation_type,
                    table_name,
                    errors.len()
                ),
                "errors.log",
                true
            );

            for error in &errors {
                evenframe_log!(
                    &format!(
                        "Statement {}: {:?} - {}",
                        error.statement_index, error.error_type, error.message
                    ),
                    "errors.log",
                    true
                );

                #[cfg(feature = "dev-mode")]
                if let Some(stmt) = &error.statement {
                    evenframe_log!(&format!("Failed statement: {}", stmt), "errors.log", true);
                }
            }

            // Panic with detailed error information
            panic!(
                "SurrealDB query validation failed for {} on table {}:\n{}",
                operation_type,
                table_name,
                errors
                    .iter()
                    .map(|e| format!(
                        "  - Statement {}: {:?} - {}\n    {}",
                        e.statement_index,
                        e.error_type,
                        e.message,
                        e.statement
                            .as_ref()
                            .unwrap_or(&"<no statement>".to_string())
                    ))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }
    }
}

/// Fallback: write statements to a temp `.surql` file and import via `surreal import` CLI.
/// Used when the HTTP RPC body exceeds SurrealDB's payload limit (413).
async fn import_via_cli(
    statements: &str,
    operation_type: &str,
    table_name: &str,
) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    use std::io::Write;

    // Target the database this process connected to (which may come from
    // CLI overrides), falling back to the configured one.
    let connection = match crate::schemasync::active_connection() {
        Some(connection) => connection,
        None => crate::config::EvenframeConfig::new()?.schemasync.database,
    };
    let url = &connection.url;
    let namespace = &connection.namespace;
    let database = &connection.database;
    let username = std::env::var("SURREALDB_USER").unwrap_or_else(|_| "root".to_string());
    let password = std::env::var("SURREALDB_PASSWORD").unwrap_or_else(|_| "root".to_string());

    // Ensure the endpoint has the http:// scheme for the CLI
    let endpoint = if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else {
        format!("http://{url}")
    };

    let mut tmp = tempfile::NamedTempFile::with_suffix(".surql")?;
    tmp.write_all(b"OPTION IMPORT;\n")?;
    tmp.write_all(statements.as_bytes())?;
    tmp.flush()?;
    let tmp_path = tmp.path().to_path_buf();

    debug!(
        operation_type = %operation_type,
        table_name = %table_name,
        file = %tmp_path.display(),
        size = statements.len(),
        "Importing via surreal CLI"
    );

    let output = std::process::Command::new("surreal")
        .arg("import")
        .arg("--endpoint")
        .arg(&endpoint)
        .arg("--namespace")
        .arg(namespace)
        .arg("--database")
        .arg(database)
        .arg("--username")
        .arg(&username)
        .arg("--password")
        .arg(&password)
        .arg(&tmp_path)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        error!(
            operation_type = %operation_type,
            table_name = %table_name,
            "surreal import failed: {stderr}"
        );
        return Err(format!(
            "surreal import failed for {operation_type} on {table_name}: {stderr}"
        )
        .into());
    }

    warn!(
        operation_type = %operation_type,
        table_name = %table_name,
        "Executed via surreal import (payload was too large for RPC)"
    );

    Ok(vec![])
}

#[cfg(test)]
mod split_tests {
    use super::{split_surql_statements, strip_surql_comments};

    #[test]
    fn strips_line_and_block_comments() {
        let block = "-- header; with a semicolon\n\
                     DEFINE ANALYZER a TOKENIZERS blank; // trailing; note\n\
                     # hash comment;\n\
                     /* block;\n   comment */ DEFINE ANALYZER b TOKENIZERS class;\n\
                     -- trailing comment only\n";
        let stripped = strip_surql_comments(block);
        assert!(!stripped.contains("header"), "{stripped}");
        assert!(!stripped.contains("note"), "{stripped}");
        assert!(!stripped.contains("hash"), "{stripped}");
        assert!(!stripped.contains("block;"), "{stripped}");
        assert_eq!(
            stripped.lines().count(),
            block.lines().count(),
            "line structure must survive: {stripped:?}"
        );

        let parts = split_surql_statements(&stripped);
        assert_eq!(parts.len(), 2, "{parts:?}");
        assert!(parts[0].contains("ANALYZER a"));
        assert!(parts[1].contains("ANALYZER b"));
    }

    #[test]
    fn keeps_comment_markers_inside_strings_and_groups() {
        let block = "DEFINE FIELD url ON t TYPE string VALUE 'http://x -- y # z';\n\
                     DEFINE FIELD n ON t TYPE int ASSERT function($value) { let i = 1; i--; return i >= 0; };\n\
                     DEFINE EVENT e ON t WHEN true THEN { -- inner comment stays\n CREATE log; };\n";
        let stripped = strip_surql_comments(block);
        assert_eq!(stripped, block);
        assert_eq!(split_surql_statements(&stripped).len(), 3);
    }

    #[test]
    fn strips_unclosed_block_comment_to_end() {
        assert_eq!(
            strip_surql_comments("DEFINE ANALYZER a; /* never closed; \n"),
            "DEFINE ANALYZER a; \n"
        );
    }

    #[test]
    fn splits_simple_statements() {
        let parts = split_surql_statements(
            "DEFINE FIELD a ON t TYPE string;\nDEFINE FIELD b ON t TYPE int;\n",
        );
        assert_eq!(parts.len(), 2);
        assert!(parts[0].contains("FIELD a"));
        assert!(parts[1].contains("FIELD b"));
    }

    #[test]
    fn does_not_split_inside_js_function_body() {
        // The embedded JS body has its own `;` — they must not split the
        // DEFINE FIELD statement (regression for the playground apply failure).
        let block = "DEFINE FIELD card ON t TYPE string ASSERT function($value) { const v = arguments[0]; if (v) { return true; } return false; };\nDEFINE FIELD next ON t TYPE int;\n";
        let parts = split_surql_statements(block);
        assert_eq!(
            parts.len(),
            2,
            "JS body semicolons split the statement: {parts:?}"
        );
        assert!(parts[0].contains("return false; }"));
        assert!(parts[1].contains("FIELD next"));
    }

    #[test]
    fn does_not_split_inside_string_literals() {
        let parts = split_surql_statements(
            "DEFINE FIELD a ON t TYPE string ASSERT $value = \"x;y\";\nDEFINE FIELD b ON t TYPE int;\n",
        );
        assert_eq!(parts.len(), 2);
        assert!(parts[0].contains("\"x;y\""));
    }

    #[test]
    fn handles_escaped_quote_in_string() {
        let parts = split_surql_statements(
            "DEFINE FIELD a ON t TYPE string ASSERT string::starts_with($value, \"a\\\";b\");\n",
        );
        assert_eq!(parts.len(), 1);
    }
}
