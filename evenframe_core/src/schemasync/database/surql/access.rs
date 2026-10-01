use crate::error::{EvenframeError, Result};
use crate::schemasync::config::{AccessConfig, AccessType, AccessesSource, DatabaseConfig};

/// The `DEFINE ACCESS` statement for an inline access, or `None` for a
/// SYSTEM access, which `DEFINE ACCESS` does not define. A JWT access needs
/// its key, which an inline access cannot give, so it is rejected.
pub fn generate_access_definition(access_config: &AccessConfig) -> Result<Option<String>> {
    tracing::debug!(access_name = %access_config.name, access_type = ?access_config.access_type, "Generating access definition");
    let name = &access_config.name;
    let table = &access_config.table_name;
    let kind = match &access_config.access_type {
        AccessType::Record => format!(
            " TYPE RECORD
    SIGNUP ( CREATE {table} SET email = $email, password = crypto::argon2::generate($password) )
    SIGNIN ( SELECT * FROM {table} WHERE email = $email AND crypto::argon2::compare(password, $password) )
    DURATION FOR TOKEN 15m, FOR SESSION 6h"
        ),
        AccessType::Bearer => " TYPE BEARER FOR RECORD".to_string(),
        AccessType::Jwt => {
            return Err(EvenframeError::config(format!(
                "the JWT access `{name}` needs its key, which an inline access cannot set; \
                 define it in a .surql file with `accesses = {{ path = \"...\" }}`"
            )));
        }
        AccessType::System => return Ok(None),
    };
    Ok(Some(format!(
        "DEFINE ACCESS OVERWRITE {name} ON DATABASE{kind};"
    )))
}

/// The `DEFINE ACCESS` statements for the configured accesses: generated for
/// inline configs, or the resolved surql for path-based ones.
pub fn access_definitions_surql(database: &DatabaseConfig) -> Result<String> {
    match &database.accesses {
        AccessesSource::Inline(accesses) => Ok(accesses
            .iter()
            .map(generate_access_definition)
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join("\n")),
        AccessesSource::Path { .. } => {
            Ok(database.resolved.access_surql.clone().unwrap_or_default())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AccessConfig, AccessType, AccessesSource, DatabaseConfig, access_definitions_surql,
    };

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
        let surql = access_definitions_surql(&database).unwrap();
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
        assert_eq!(access_definitions_surql(&database).unwrap(), "");
        database.resolved.access_surql = Some("DEFINE ACCESS a ON DATABASE TYPE JWT;".to_string());
        assert_eq!(
            access_definitions_surql(&database).unwrap(),
            "DEFINE ACCESS a ON DATABASE TYPE JWT;"
        );
    }

    #[test]
    fn an_inline_jwt_access_is_rejected() {
        let mut database = DatabaseConfig::for_testing();
        database.accesses = AccessesSource::Inline(vec![access("api", AccessType::Jwt)]);
        let error = access_definitions_surql(&database).unwrap_err().to_string();
        assert!(error.contains("JWT access `api` needs its key"), "{error}");
    }
}
