use crate::evenframe_log;
use serde_json::Value;
use surrealdb::IndexedResults;
use tracing::{debug, error, info, trace};

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

    // Split statements for error reporting
    let statement_lines: Vec<&str> = statements
        .split(';')
        .filter(|s| !s.trim().is_empty())
        .collect();

    // Process each result from the response
    for (index, statement) in statement_lines.iter().enumerate() {
        match response.take::<surrealdb::types::Value>(index) {
            Ok(surreal_value) => {
                let value: Value =
                    serde_json::to_value(&surreal_value).unwrap_or(Value::Null);
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

/// Executes a query and validates the response, panicking on any errors
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
