//! Init command - initializes evenframe.toml configuration.

use crate::cli::{Cli, InitArgs};
use evenframe_core::config::EvenframeConfig;
use evenframe_core::error::{EvenframeError, Result};
use evenframe_core::schemasync::config::DatabaseProvider;
use std::fs;
use std::path::Path;

/// Runs the init command, writing to `--config` when given.
pub async fn run(cli: &Cli, args: InitArgs) -> Result<()> {
    let config_path = cli.config.as_deref().unwrap_or(Path::new("evenframe.toml"));

    if config_path.exists() && !args.force {
        return Err(EvenframeError::config(format!(
            "{} already exists; use --force to overwrite it",
            config_path.display()
        )));
    }
    if let Some(dir) = config_path.parent()
        && !dir.as_os_str().is_empty()
    {
        fs::create_dir_all(dir).map_err(|e| {
            EvenframeError::config(format!("Failed to create {}: {e}", dir.display()))
        })?;
    }

    let content = if args.minimal {
        generate_minimal_config(&args.provider)
    } else {
        generate_full_config(&args.provider)
    };

    fs::write(config_path, content).map_err(|e| {
        EvenframeError::config(format!("Failed to write {}: {e}", config_path.display()))
    })?;
    println!("Created {}", config_path.display());

    let env_path = EvenframeConfig::project_root_of(config_path).join(".env");
    if !env_path.exists() {
        fs::write(&env_path, generate_env_template(&args.provider)).map_err(|e| {
            EvenframeError::config(format!("Failed to write {}: {e}", env_path.display()))
        })?;
        println!("Created {}", env_path.display());
    }

    println!();
    println!("Next steps:");
    println!(
        "  1. Edit {} to configure your project",
        config_path.display()
    );
    println!("  2. Add #[derive(Evenframe)] to your Rust structs");
    println!("  3. Run `evenframe generate` to generate types and sync the schema");

    Ok(())
}

fn generate_minimal_config(provider: &DatabaseProvider) -> String {
    match provider {
        DatabaseProvider::Surrealdb => r#"[general]
apply_aliases = []

[schemasync]
should_generate_mocks = false

[schemasync.database]
url = "${SURREALDB_URL}"
namespace = "${SURREALDB_NS}"
database = "${SURREALDB_DB}"

[typesync]
output = { kind = "arktype", dir = "./src/generated" }
"#
        .to_string(),
        _ => format!(
            r#"[general]
apply_aliases = []

[schemasync]
should_generate_mocks = false

[schemasync.database]
provider = "{provider}"
url = "${{DATABASE_URL}}"

[typesync]
output = {{ kind = "arktype", dir = "./src/generated" }}
"#
        ),
    }
}

fn generate_full_config(provider: &DatabaseProvider) -> String {
    match provider {
        DatabaseProvider::Surrealdb => r#"# Evenframe Configuration
# See https://github.com/rymskip/evenframe for documentation

[general]
# Custom attribute macros that include Evenframe derive
apply_aliases = []

[schemasync]
# Enable mock data generation
should_generate_mocks = true

[schemasync.database]
# SurrealDB connection settings (use environment variables)
url = "${SURREALDB_URL}"
namespace = "${SURREALDB_NS}"
database = "${SURREALDB_DB}"

[schemasync.mock_gen_config]
# When true, deletes all existing data before generating mocks
full_refresh_mode = false

[typesync]
# Each generated output names its kind and the directory it writes to. Use
# `output = { ... }` for one kind, or `outputs = [ ... ]` for several, each in
# its own directory. effect and macroforge also take mode = "per_file" (one
# file per type), barrel_file, file_naming, file_extension, array_style and
# import_extension. A single-file output can rename its file with file = "...".
outputs = [
  { kind = "arktype", dir = "./src/generated/arktype" },
  # { kind = "effect", dir = "./src/generated/effect" },
  # { kind = "macroforge", dir = "./src/generated/types", mode = "per_file" },
  # { kind = "flatbuffers", dir = "./schemas/flatbuffers", namespace = "com.example.app" },
  # { kind = "protobuf", dir = "./schemas/protobuf", package = "com.example.app", import_validate = false },
]
"#
        .to_string(),
        _ => format!(
            r#"# Evenframe Configuration
# See https://github.com/rymskip/evenframe for documentation

[general]
# Custom attribute macros that include Evenframe derive
apply_aliases = []

# Note: Database provider '{provider}' is not yet fully supported
# SchemaSync currently only works with SurrealDB

[schemasync]
should_generate_mocks = false

[schemasync.database]
provider = "{provider}"
url = "${{DATABASE_URL}}"

[typesync]
# Each generated output names its kind and the directory it writes to. Use
# `output = {{ ... }}` for one kind, or `outputs = [ ... ]` for several, each in
# its own directory. effect and macroforge also take mode = "per_file" (one
# file per type), barrel_file, file_naming, file_extension, array_style and
# import_extension. A single-file output can rename its file with file = "...".
outputs = [
  {{ kind = "arktype", dir = "./src/generated/arktype" }},
  # {{ kind = "effect", dir = "./src/generated/effect" }},
  # {{ kind = "macroforge", dir = "./src/generated/types", mode = "per_file" }},
  # {{ kind = "flatbuffers", dir = "./schemas/flatbuffers", namespace = "com.example.app" }},
  # {{ kind = "protobuf", dir = "./schemas/protobuf", package = "com.example.app", import_validate = false }},
]
"#
        ),
    }
}

fn generate_env_template(provider: &DatabaseProvider) -> String {
    match provider {
        DatabaseProvider::Surrealdb => r#"# SurrealDB Connection
SURREALDB_URL=http://localhost:8000
SURREALDB_NS=test
SURREALDB_DB=test
SURREALDB_USER=root
SURREALDB_PASSWORD=root
"#
        .to_string(),
        DatabaseProvider::Postgres => r#"# PostgreSQL Connection
DATABASE_URL=postgres://user:password@localhost:5432/database
"#
        .to_string(),
        DatabaseProvider::Mysql => r#"# MySQL Connection
DATABASE_URL=mysql://user:password@localhost:3306/database
"#
        .to_string(),
        DatabaseProvider::Sqlite => r#"# SQLite Connection
DATABASE_URL=sqlite:./database.db
"#
        .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evenframe_core::config::EvenframeConfig;

    const PROVIDERS: [DatabaseProvider; 4] = [
        DatabaseProvider::Surrealdb,
        DatabaseProvider::Postgres,
        DatabaseProvider::Mysql,
        DatabaseProvider::Sqlite,
    ];

    fn parse(template: &str) -> EvenframeConfig {
        toml::from_str(template)
            .unwrap_or_else(|e| panic!("template is not a valid config: {e}\n{template}"))
    }

    #[test]
    fn every_template_is_a_valid_config() {
        for provider in PROVIDERS {
            parse(&generate_minimal_config(&provider));
            parse(&generate_full_config(&provider));
        }
    }

    #[test]
    fn surrealdb_templates_resolve_to_these_configs() {
        insta::assert_debug_snapshot!(
            "minimal",
            parse(&generate_minimal_config(&DatabaseProvider::Surrealdb))
        );
        insta::assert_debug_snapshot!(
            "full",
            parse(&generate_full_config(&DatabaseProvider::Surrealdb))
        );
    }
}
