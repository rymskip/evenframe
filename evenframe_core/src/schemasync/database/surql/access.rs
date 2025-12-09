use crate::evenframe_log;
use crate::schemasync::config::{AccessConfig, AccessType};
use std::env;
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
) -> Result<(), Box<dyn std::error::Error>> {
    tracing::debug!(query_length = access_query.len(), "Executing access query");
    let access_result = db.query(access_query).await;
    match access_result {
        Ok(_) => {
            let db_name = env::var("SURREALDB_DB").expect("SURREALDB_DB not set");
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
            let db_name = env::var("SURREALDB_DB").expect("SURREALDB_DB not set");
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
pub async fn setup_access_definitions(
    new_schema: &Surreal<Db>,
    schemasync_config: &crate::schemasync::config::SchemasyncConfig,
) -> Result<String, Box<dyn std::error::Error>> {
    tracing::info!("Setting up access definitions");
    let mut access_query = String::new();

    tracing::debug!(
        access_count = schemasync_config.database.accesses.len(),
        "Processing access configurations"
    );

    evenframe_log!(
        &format!("{:#?}", &schemasync_config.database.accesses),
        "access_config.surql"
    );

    for access in &schemasync_config.database.accesses {
        tracing::trace!(access_name = %access.name, "Processing access definition");
        access_query = generate_access_definition(access);
        if let Err(e) = new_schema.query(&access_query).await {
            tracing::error!(
                access_name = %access.name,
                error = %e,
                "Failed to create access"
            );
        } else {
            tracing::debug!(access_name = %access.name, "Access created successfully");
        }
    }

    tracing::debug!(
        total_query_length = access_query.len(),
        "Access definitions setup complete"
    );

    evenframe_log!(&access_query, "access_query.surql");
    Ok(access_query)
}
