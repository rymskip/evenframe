use crate::evenframe_log;
use crate::schemasync::config::{AccessConfig, AccessType, AccessesSource};
use surrealdb::{
    Surreal,
    engine::{local::Db, remote::http::Client},
};
use tracing;

/// Generate a DEFINE ACCESS statement for SurrealDB
/// This creates access methods with OVERWRITE always enabled as requested
pub fn generate_access_definition(access_config: &AccessConfig) -> String {
    tracing::debug!(access_name = %access_config.name, access_type = ?access_config.access_type, "Generating access definition");
    let access_name = &access_config.name;

    // Start with DEFINE ACCESS OVERWRITE statement
    let mut query = format!("DEFINE ACCESS OVERWRITE {} ON DATABASE", access_name);

    // Add TYPE and configuration based on access type
    match &access_config.access_type {
        AccessType::Record => {
            tracing::trace!(table = %access_config.table_name, "Generating RECORD type access");
            query.push_str(&format!(
                " TYPE RECORD
    SIGNUP ( CREATE {} SET email = $email, password = crypto::argon2::generate($password) )
    SIGNIN ( SELECT * FROM {} WHERE email = $email AND crypto::argon2::compare(password, $password) )
    DURATION FOR TOKEN 15m, FOR SESSION 6h",
                access_config.table_name, access_config.table_name
            ));
        }
        AccessType::Jwt => {
            tracing::trace!("Generating JWT type access");
            // Basic JWT configuration - can be expanded based on needs
            query.push_str(
                " TYPE JWT
    ALGORITHM HS256
    KEY 'your-secret-key-here'",
            );
        }
        AccessType::Bearer => {
            tracing::trace!("Generating BEARER type access");
            // Bearer for record users by default
            query.push_str(" TYPE BEARER FOR RECORD");
        }
        AccessType::System => {
            tracing::trace!("Skipping SYSTEM type access - not defined via DEFINE ACCESS");
            // System access is typically not defined via DEFINE ACCESS
            // Return empty string or handle differently based on requirements
            return String::new();
        }
    }

    // Add semicolon to complete the statement
    query.push(';');

    tracing::trace!(query_length = query.len(), "Access definition generated");
    query
}
pub async fn execute_access_query(
    db: &Surreal<Client>,
    access_query: &str,
    db_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::debug!(query_length = access_query.len(), "Executing access query");
    let access_result = db.query(access_query).await;
    match access_result {
        Ok(_) => {
            tracing::info!(db = %db_name, "Successfully executed access statements");
            evenframe_log!(
                &format!(
                    "Successfully executed define access statements for db {}",
                    db_name
                ),
                "results.log",
                true
            )
        }
        Err(e) => {
            tracing::error!(db = %db_name, error = %e, "Failed to execute access statements");
            let error_msg = format!(
                "Failed to execute define access statements for db {}: {}",
                db_name, e
            );
            evenframe_log!(&error_msg, "results.log", true);
            return Err(e.into());
        }
    }
    Ok(())
}
/// The `DEFINE ACCESS` statements for the configured accesses: generated for
/// inline configs, or the resolved surql for path-based ones.
pub fn access_definitions_surql(database: &crate::schemasync::config::DatabaseConfig) -> String {
    match &database.accesses {
        AccessesSource::Inline(accesses) => accesses
            .iter()
            .map(generate_access_definition)
            .filter(|statement| !statement.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        AccessesSource::Path { .. } => database.resolved.access_surql.clone().unwrap_or_default(),
    }
}

pub async fn setup_access_definitions(
    new_schema: &Surreal<Db>,
    schemasync_config: &crate::schemasync::config::SchemasyncConfig,
) -> Result<String, Box<dyn std::error::Error>> {
    tracing::info!("Setting up access definitions");

    evenframe_log!(
        &format!("{:#?}", &schemasync_config.database.accesses),
        "access_config.surql"
    );

    let access_query = match &schemasync_config.database.accesses {
        AccessesSource::Inline(accesses) => {
            tracing::debug!(
                access_count = accesses.len(),
                "Processing inline access configurations"
            );
            for access in accesses {
                tracing::trace!(access_name = %access.name, "Processing access definition");
                let query = generate_access_definition(access);
                if let Err(e) = new_schema.query(&query).await {
                    tracing::error!(
                        access_name = %access.name,
                        error = %e,
                        "Failed to create access"
                    );
                } else {
                    tracing::debug!(access_name = %access.name, "Access created successfully");
                }
            }
            // Every access, not just the last one, is applied to the live DB
            access_definitions_surql(&schemasync_config.database)
        }
        AccessesSource::Path { .. } => {
            // Path-based access: use resolved surql content
            if let Some(ref surql) = schemasync_config.database.resolved.access_surql {
                tracing::debug!(
                    surql_length = surql.len(),
                    "Executing path-based access surql on embedded DB"
                );
                if let Err(e) = new_schema.query(surql.as_str()).await {
                    tracing::error!(error = %e, "Failed to execute path-based access surql");
                } else {
                    tracing::debug!("Path-based access surql executed successfully");
                }
                surql.clone()
            } else {
                tracing::warn!("AccessesSource::Path set but no resolved surql found");
                String::new()
            }
        }
    };

    tracing::debug!(
        total_query_length = access_query.len(),
        "Access definitions setup complete"
    );

    evenframe_log!(&access_query, "access_query.surql");
    Ok(access_query)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schemasync::config::DatabaseConfig;

    fn access(name: &str, access_type: AccessType) -> AccessConfig {
        AccessConfig {
            name: name.to_string(),
            access_type,
            table_name: "user".to_string(),
        }
    }

    #[test]
    fn inline_accesses_include_every_definition() {
        let mut database = DatabaseConfig::for_testing();
        database.accesses = AccessesSource::Inline(vec![
            access("user", AccessType::Record),
            access("system", AccessType::System),
            access("api", AccessType::Bearer),
        ]);
        let surql = access_definitions_surql(&database);
        assert!(surql.contains("DEFINE ACCESS OVERWRITE user ON DATABASE TYPE RECORD"));
        assert!(surql.contains("DEFINE ACCESS OVERWRITE api ON DATABASE TYPE BEARER FOR RECORD;"));
        assert!(
            !surql.contains("system"),
            "SYSTEM accesses aren't defined: {surql}"
        );
    }

    #[test]
    fn path_accesses_use_resolved_surql() {
        let mut database = DatabaseConfig::for_testing();
        database.accesses = AccessesSource::Path {
            path: "surql/access.surql".to_string(),
        };
        assert_eq!(access_definitions_surql(&database), "");
        database.resolved.access_surql = Some("DEFINE ACCESS a ON DATABASE TYPE JWT;".to_string());
        assert_eq!(
            access_definitions_surql(&database),
            "DEFINE ACCESS a ON DATABASE TYPE JWT;"
        );
    }
}
