use crate::error::{EvenframeError, Result};
use crate::evenframe_log;
use std::io::Write;
use surrealdb::types::Variables;
use surrealdb::{Connection, IndexedResults, Surreal};
use tracing::{debug, error, info, trace, warn};

/// Results a transaction's `BEGIN` and `COMMIT` add to a response: one
/// each, before and after its statements' results.
const TRANSACTION_CONTROL_RESULTS: usize = 2;

/// What a statement reports when it was cancelled because another statement
/// in its transaction failed.
const CANCELLED_BY_TRANSACTION: &str = "failed transaction";

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
    let mut position = 0;
    while position < bytes.len() {
        let byte = bytes[position];
        if let Some(open) = quote {
            if byte == b'\\' {
                position += 2;
                continue;
            }
            if byte == open {
                quote = None;
            }
            position += 1;
            continue;
        }
        let next = bytes.get(position + 1).copied();
        let comment_end = match (byte, next) {
            _ if depth > 0 => None,
            (b'-', Some(b'-')) | (b'/', Some(b'/')) | (b'#', _) => {
                // Line comment: drop up to (not including) the newline
                Some(
                    block[position..]
                        .find('\n')
                        .map_or(block.len(), |offset| position + offset),
                )
            }
            (b'/', Some(b'*')) => {
                // Block comment: drop through `*/`, or to the end if unclosed
                Some(
                    block[position + 2..]
                        .find("*/")
                        .map_or(block.len(), |offset| position + 2 + offset + 2),
                )
            }
            _ => None,
        };
        if let Some(end) = comment_end {
            out.push_str(&block[copied..position]);
            // Keep the block comment's line breaks so line structure survives
            out.extend(block[position..end].chars().filter(|&ch| ch == '\n'));
            copied = end;
            position = end;
            continue;
        }
        match byte {
            b'"' | b'\'' | b'`' => quote = Some(byte),
            b'{' | b'(' | b'[' => depth += 1,
            b'}' | b')' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
        position += 1;
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
    let mut position = 0;
    while position < bytes.len() {
        let byte = bytes[position];
        match quote {
            Some(open) => {
                if byte == b'\\' {
                    // Skip the escaped character.
                    position += 2;
                    continue;
                }
                if byte == open {
                    quote = None;
                }
            }
            None => match byte {
                b'"' | b'\'' | b'`' => quote = Some(byte),
                b'{' | b'(' | b'[' => depth += 1,
                b'}' | b')' | b']' => depth = depth.saturating_sub(1),
                b';' if depth == 0 => {
                    out.push(&block[start..=position]);
                    start = position + 1;
                }
                _ => {}
            },
        }
        position += 1;
    }
    if start < block.len() {
        let tail = &block[start..];
        if !tail.trim().is_empty() {
            out.push(tail);
        }
    }
    out
}

/// Checks that `response` has one result for each statement in
/// `statements` and that none of them failed, returning how many ran. Each
/// failure is reported with its statement.
pub async fn validate_surql_response(
    mut response: IndexedResults,
    statements: &str,
) -> std::result::Result<usize, Vec<QueryValidationError>> {
    // Brace/string-aware so embedded JavaScript function bodies (which
    // contain their own `;`) aren't split mid-statement and miscounted
    // against the response. Comments are stripped first: SurrealDB returns
    // no result for them.
    let uncommented = strip_surql_comments(statements);
    let statement_lines: Vec<&str> = split_surql_statements(&uncommented)
        .into_iter()
        .filter(|statement| !statement.trim().is_empty())
        .collect();

    let mut errors: Vec<QueryValidationError> = response
        .take_errors()
        .into_iter()
        .map(|(index, error)| {
            let message = error.to_string();
            let lowered = message.to_lowercase();
            let error_type = match lowered {
                ref text if text.contains("parse") => QueryErrorType::ParseError,
                ref text if text.contains("validation") || text.contains("schema") => {
                    QueryErrorType::ValidationError
                }
                ref text if text.contains("constraint") => QueryErrorType::ConstraintViolation,
                ref text if text.contains("not found") => QueryErrorType::RecordNotFound,
                ref text if text.contains("permission") => QueryErrorType::PermissionDenied,
                ref text if text.contains("transaction") => QueryErrorType::TransactionRollback,
                _ => QueryErrorType::UnknownError,
            };
            QueryValidationError {
                statement_index: index,
                error_type,
                message,
                statement: statement_lines
                    .get(index)
                    .map(|statement| statement.to_string()),
            }
        })
        .collect();
    errors.sort_by_key(|error| error.statement_index);

    let results = response.num_statements() + errors.len();
    if results != statement_lines.len() {
        errors.push(QueryValidationError {
            statement_index: results,
            error_type: QueryErrorType::PartialFailure,
            message: format!(
                "{} statements were sent but {results} results came back",
                statement_lines.len()
            ),
            statement: None,
        });
    }
    if errors.is_empty() {
        Ok(statement_lines.len())
    } else {
        Err(errors)
    }
}

/// The largest request body sent to SurrealDB. Its HTTP endpoint refuses
/// bodies near 1 MB, so larger work is split across requests.
pub const RPC_SIZE_LIMIT: usize = 800_000;

/// How often a statement rolled back by a conflicting writer is retried.
const CONFLICT_RETRIES: u32 = 5;

/// Executes `statements` and returns every failed statement as an error.
/// Statements that together exceed [`RPC_SIZE_LIMIT`] are sent in several
/// requests, split between statements; a single statement over the limit is
/// imported instead (see [`import_oversized`]).
pub async fn execute_and_validate<C>(
    db: &Surreal<C>,
    statements: &str,
    operation_type: &str,
    table_name: &str,
) -> Result<usize>
where
    C: Connection,
{
    if statements.len() <= RPC_SIZE_LIMIT {
        return execute_request(
            db,
            statements,
            &Variables::default(),
            operation_type,
            table_name,
        )
        .await;
    }
    info!(
        operation_type = %operation_type,
        table_name = %table_name,
        size = statements.len(),
        "Splitting statements across requests"
    );
    let uncommented = strip_surql_comments(statements);
    let mut executed = 0;
    let mut request = String::new();
    for statement in split_surql_statements(&uncommented) {
        if statement.len() > RPC_SIZE_LIMIT {
            if !request.is_empty() {
                executed += execute_request(
                    db,
                    &request,
                    &Variables::default(),
                    operation_type,
                    table_name,
                )
                .await?;
                request.clear();
            }
            import_oversized(db, statement, operation_type, table_name).await?;
            executed += 1;
            continue;
        }
        if request.len() + statement.len() + 1 > RPC_SIZE_LIMIT {
            executed += execute_request(
                db,
                &request,
                &Variables::default(),
                operation_type,
                table_name,
            )
            .await?;
            request.clear();
        }
        request.push_str(statement);
        request.push('\n');
    }
    if !request.trim().is_empty() {
        executed += execute_request(
            db,
            &request,
            &Variables::default(),
            operation_type,
            table_name,
        )
        .await?;
    }
    Ok(executed)
}

/// Executes `statements` as one request with the query parameters they name
/// bound to `variables`, which keeps long values out of the statement text.
/// Validated and retried like [`execute_and_validate`].
pub async fn execute_bound<C>(
    db: &Surreal<C>,
    statements: &str,
    variables: &Variables,
    operation_type: &str,
    table_name: &str,
) -> Result<usize>
where
    C: Connection,
{
    execute_request(db, statements, variables, operation_type, table_name).await
}

/// Sends `statements` as one request, with `variables` bound, and validates
/// every statement's result.
async fn execute_request<C>(
    db: &Surreal<C>,
    statements: &str,
    variables: &Variables,
    operation_type: &str,
    table_name: &str,
) -> Result<usize>
where
    C: Connection,
{
    info!(operation_type = %operation_type, table_name = %table_name, statement_length = statements.len(), "Executing and validating statements");
    trace!("Statements: {}", statements);

    // A statement that hits a transaction conflict with a concurrent writer
    // is rolled back and can be run again; only those statements are
    // retried, since the others already committed.
    let mut pending = statements.to_string();
    let mut attempt = 0;
    let outcome = loop {
        debug!("Sending query to database");
        let response = db
            .query(pending.as_str())
            .bind(variables.clone())
            .await
            .map_err(|error| {
            error!(operation_type = %operation_type, table_name = %table_name, error = %error, "Database query failed");
            EvenframeError::database(format!(
                "{operation_type} on {table_name}: the request failed: {error}"
            ))
        })?;
        match validate_surql_response(response, &pending).await {
            Err(errors)
                if attempt < CONFLICT_RETRIES
                    && errors.iter().all(|error| is_retryable(&error.message)) =>
            {
                attempt += 1;
                warn!(
                    operation_type = %operation_type,
                    table_name = %table_name,
                    conflicts = errors.len(),
                    attempt,
                    "Retrying statements rolled back by a transaction conflict"
                );
                tokio::time::sleep(conflict_backoff(attempt)).await;
                pending = errors
                    .iter()
                    .filter_map(|error| error.statement.as_deref())
                    .map(|statement| format!("{statement};"))
                    .collect::<Vec<_>>()
                    .join("\n");
            }
            outcome => break outcome,
        }
    };

    match outcome {
        Ok(executed) => {
            evenframe_log!(
                &format!(
                    "Successfully executed {executed} {operation_type} statements for table {table_name}"
                ),
                "results.log",
                true
            );
            Ok(executed)
        }
        Err(errors) => {
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
            }

            Err(EvenframeError::database(format!(
                "SurrealDB query validation failed for {} on table {}:\n{}",
                operation_type,
                table_name,
                errors
                    .iter()
                    .map(|error| format!(
                        "  - Statement {}: {:?} - {}\n    {}",
                        error.statement_index,
                        error.error_type,
                        error.message,
                        error.statement.as_deref().unwrap_or("<no statement>")
                    ))
                    .collect::<Vec<_>>()
                    .join("\n")
            )))
        }
    }
}

fn is_retryable(message: &str) -> bool {
    message.contains("can be retried")
}

fn conflict_backoff(attempt: u32) -> std::time::Duration {
    std::time::Duration::from_millis(50 << attempt)
}

/// Imports one statement too large for a request through the database's
/// import endpoint, which takes a file that opens with `OPTION IMPORT`.
/// Import mode fires no events and does not process fields (no defaults,
/// `VALUE` or `ASSERT` clauses), so this is logged; its errors fail the run
/// like any statement's.
async fn import_oversized<C>(
    db: &Surreal<C>,
    statement: &str,
    operation_type: &str,
    table_name: &str,
) -> Result<()>
where
    C: Connection,
{
    let failed = |error: String| {
        EvenframeError::database(format!(
            "{operation_type} on {table_name}: importing a statement too large for one request failed: {error}"
        ))
    };
    let mut file = tempfile::Builder::new()
        .suffix(".surql")
        .tempfile()
        .map_err(|error| failed(error.to_string()))?;
    file.write_all(b"OPTION IMPORT;\n")
        .and_then(|()| file.write_all(statement.as_bytes()))
        .and_then(|()| file.flush())
        .map_err(|error| failed(error.to_string()))?;
    db.import(file.path())
        .await
        .map_err(|error| failed(error.to_string()))?;
    let what = if operation_type == "mock data" {
        "this record"
    } else {
        "this statement"
    };
    warn!(
        operation_type = %operation_type,
        table_name = %table_name,
        size = statement.len(),
        "A statement larger than one request was imported; events did not fire and fields were not processed for {what}"
    );
    Ok(())
}

/// Statements that apply together or not at all, such as one table's
/// definitions.
#[derive(Debug, Clone)]
pub struct Transaction {
    /// What the statements define, for messages.
    pub label: String,
    pub statements: Vec<String>,
}

impl Transaction {
    fn surql(&self) -> String {
        let mut surql = String::from("BEGIN TRANSACTION;\n");
        for statement in &self.statements {
            surql.push_str(statement.trim().trim_end_matches(';'));
            surql.push_str(";\n");
        }
        surql.push_str("COMMIT TRANSACTION;\n");
        surql
    }
}

/// Applies `transactions`, packing as many into each request as fit under
/// [`RPC_SIZE_LIMIT`]. Each applies whole or not at all, and one failing
/// leaves the others applied. A conflict with a concurrent writer re-sends
/// the whole transaction. One too large for any request runs statement by
/// statement instead, which is logged because it is then not atomic. Fails
/// listing every transaction that did not apply.
pub async fn execute_transactions<C>(
    db: &Surreal<C>,
    transactions: &[Transaction],
    operation_type: &str,
) -> Result<()>
where
    C: Connection,
{
    let mut failures = Vec::new();
    let mut batch: Vec<&Transaction> = Vec::new();
    let mut batch_surql = String::new();
    for transaction in transactions {
        if transaction.statements.is_empty() {
            continue;
        }
        let surql = transaction.surql();
        if surql.len() > RPC_SIZE_LIMIT {
            warn!(
                operation_type = %operation_type,
                label = %transaction.label,
                size = surql.len(),
                "Too large for one request, so applied statement by statement and not atomically"
            );
            for statement in &transaction.statements {
                if let Err(error) =
                    execute_and_validate(db, statement, operation_type, &transaction.label).await
                {
                    failures.push(format!("{}: {error}", transaction.label));
                    break;
                }
            }
            continue;
        }
        if !batch.is_empty() && batch_surql.len() + surql.len() > RPC_SIZE_LIMIT {
            failures.extend(run_transactions(db, &batch, &batch_surql, operation_type).await?);
            batch.clear();
            batch_surql.clear();
        }
        batch.push(transaction);
        batch_surql.push_str(&surql);
    }
    if !batch.is_empty() {
        failures.extend(run_transactions(db, &batch, &batch_surql, operation_type).await?);
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(EvenframeError::database(format!(
            "{operation_type} failed for {} of {} tables, which were left unchanged:\n{}",
            failures.len(),
            transactions.len(),
            failures.join("\n")
        )))
    }
}

/// Why a transaction did not apply.
struct TransactionFailure {
    /// Each failed statement with its error.
    errors: Vec<(String, String)>,
    retryable: bool,
}

impl TransactionFailure {
    fn describe(&self, label: &str) -> String {
        let errors = self
            .errors
            .iter()
            .map(|(statement, message)| format!("  - {message}\n    {statement}"))
            .collect::<Vec<_>>()
            .join("\n");
        format!("{label}:\n{errors}")
    }
}

/// Sends `surql`, the packed `batch`, as one request and returns a message
/// for each transaction that did not apply, after retrying conflicts.
async fn run_transactions<C>(
    db: &Surreal<C>,
    batch: &[&Transaction],
    surql: &str,
    operation_type: &str,
) -> Result<Vec<String>>
where
    C: Connection,
{
    let outcomes = send_transactions(db, batch, surql, operation_type).await?;
    let mut failures = Vec::new();
    for (transaction, outcome) in batch.iter().zip(outcomes) {
        let mut outcome = outcome;
        let mut attempt = 0;
        while let Some(failure) = &outcome
            && failure.retryable
            && attempt < CONFLICT_RETRIES
        {
            attempt += 1;
            warn!(
                label = %transaction.label,
                attempt,
                "Retrying a transaction rolled back by a conflict"
            );
            tokio::time::sleep(conflict_backoff(attempt)).await;
            outcome = send_transactions(db, &[transaction], &transaction.surql(), operation_type)
                .await?
                .pop()
                .flatten();
        }
        if let Some(failure) = outcome {
            failures.push(failure.describe(&transaction.label));
        }
    }
    Ok(failures)
}

/// Each transaction's failure, if it had one, from a request of `batch`.
async fn send_transactions<C>(
    db: &Surreal<C>,
    batch: &[&Transaction],
    surql: &str,
    operation_type: &str,
) -> Result<Vec<Option<TransactionFailure>>>
where
    C: Connection,
{
    trace!("Transactions: {}", surql);
    let mut response = db.query(surql).await.map_err(|error| {
        EvenframeError::database(format!("{operation_type}: the request failed: {error}"))
    })?;
    let expected: usize = batch
        .iter()
        .map(|transaction| transaction.statements.len() + TRANSACTION_CONTROL_RESULTS)
        .sum();
    if response.num_statements() != expected {
        return Err(EvenframeError::database(format!(
            "{operation_type}: expected {expected} results for {} transactions, got {}",
            batch.len(),
            response.num_statements()
        )));
    }
    let mut errors = response.take_errors();
    let mut index = 0;
    let mut outcomes = Vec::with_capacity(batch.len());
    for transaction in batch {
        let mut control = Vec::new();
        if let Some(error) = errors.remove(&index) {
            control.push(("BEGIN TRANSACTION;".to_string(), error.to_string()));
        }
        index += TRANSACTION_CONTROL_RESULTS / 2;
        let mut failed = Vec::new();
        let mut cancelled = Vec::new();
        for statement in &transaction.statements {
            if let Some(error) = errors.remove(&index) {
                let message = error.to_string();
                let entry = (statement.clone(), message.clone());
                if message.contains(CANCELLED_BY_TRANSACTION) {
                    cancelled.push(entry);
                } else {
                    failed.push(entry);
                }
            }
            index += 1;
        }
        if let Some(error) = errors.remove(&index) {
            control.push(("COMMIT TRANSACTION;".to_string(), error.to_string()));
        }
        index += TRANSACTION_CONTROL_RESULTS - TRANSACTION_CONTROL_RESULTS / 2;
        // A statement that failed explains the transaction; without one, the
        // BEGIN or COMMIT that failed does, such as a commit that conflicted
        // with a concurrent writer. The rest only report that they were
        // cancelled with it.
        let errors = if !failed.is_empty() {
            failed
        } else if !control.is_empty() {
            control
        } else {
            cancelled
        };
        outcomes.push((!errors.is_empty()).then(|| TransactionFailure {
            retryable: errors.iter().all(|(_, message)| is_retryable(message)),
            errors,
        }));
    }
    Ok(outcomes)
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
        // The embedded JS body has its own `;`, which must not split the
        // DEFINE FIELD statement.
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

#[cfg(test)]
mod transaction_tests {
    use super::{Transaction, execute_transactions};
    use surrealdb::Surreal;
    use surrealdb::engine::local::{Db, Mem};

    async fn database() -> Surreal<Db> {
        let db = Surreal::new::<Mem>(()).await.unwrap();
        db.use_ns("test").use_db("test").await.unwrap();
        db
    }

    async fn table_names(db: &Surreal<Db>) -> Vec<String> {
        let mut names: Vec<String> = db
            .query("RETURN object::keys((INFO FOR DB).tables)")
            .await
            .unwrap()
            .take(0)
            .unwrap();
        names.sort();
        names
    }

    #[tokio::test]
    async fn a_statement_over_the_request_limit_is_imported() {
        let db = database().await;
        let comment = "x".repeat(super::RPC_SIZE_LIMIT + 1);
        let statements = format!(
            "DEFINE TABLE big SCHEMAFULL;\nDEFINE FIELD note ON big TYPE string COMMENT '{comment}';\n"
        );
        super::execute_and_validate(&db, &statements, "define", "big")
            .await
            .unwrap();
        let fields: Vec<String> = db
            .query("RETURN object::keys((INFO FOR TABLE big).fields)")
            .await
            .unwrap()
            .take(0)
            .unwrap();
        assert_eq!(fields, vec!["note".to_string()]);
    }

    fn transaction(label: &str, statements: &[&str]) -> Transaction {
        Transaction {
            label: label.to_string(),
            statements: statements
                .iter()
                .map(|statement| statement.to_string())
                .collect(),
        }
    }

    #[tokio::test]
    async fn packed_transactions_all_apply() {
        let db = database().await;
        execute_transactions(
            &db,
            &[
                transaction(
                    "first",
                    &[
                        "DEFINE TABLE first SCHEMAFULL",
                        "DEFINE FIELD name ON TABLE first TYPE string",
                    ],
                ),
                transaction("second", &["DEFINE TABLE second SCHEMAFULL"]),
            ],
            "define",
        )
        .await
        .unwrap();
        assert_eq!(table_names(&db).await, ["first", "second"]);
    }

    #[tokio::test]
    async fn a_failed_transaction_leaves_only_its_own_table_unchanged() {
        let db = database().await;
        let error = execute_transactions(
            &db,
            &[
                transaction(
                    "first",
                    &[
                        "DEFINE TABLE first SCHEMAFULL",
                        "DEFINE FIELD name ON TABLE first TYPE string",
                    ],
                ),
                transaction(
                    "broken",
                    &[
                        "DEFINE TABLE broken SCHEMAFULL",
                        "DEFINE TABLE broken SCHEMAFULL",
                    ],
                ),
                transaction("last", &["DEFINE TABLE last SCHEMAFULL"]),
            ],
            "define",
        )
        .await
        .unwrap_err()
        .to_string();
        println!("{error}");
        assert!(error.contains("broken:"), "{error}");
        assert!(error.contains("already exists"), "{error}");
        assert!(
            !error.contains("first:") && !error.contains("last:"),
            "{error}"
        );
        assert_eq!(table_names(&db).await, ["first", "last"]);
    }
}
