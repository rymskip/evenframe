//! Init command - initializes evenframe.toml configuration.

use crate::cli::{Cli, DatabaseProvider, InitArgs};
use evenframe_core::error::Result;
use std::fs;
use std::path::Path;
use tracing::{error, info};

/// Runs the init command.
pub async fn run(_cli: &Cli, args: InitArgs) -> Result<()> {
    let config_path = Path::new("evenframe.toml");

    if config_path.exists() && !args.force {
        error!("evenframe.toml already exists. Use --force to overwrite.");
        return Ok(());
    }

    let content = if args.minimal {
        generate_minimal_config(&args.provider)
    } else {
        generate_full_config(&args.provider)
    };

    fs::write(config_path, content)?;
    info!("Created evenframe.toml");

    // Also create .env template if it doesn't exist
    let env_path = Path::new(".env");
    if !env_path.exists() {
        let env_content = generate_env_template(&args.provider);
        fs::write(env_path, env_content)?;
        info!("Created .env template");
    }

    info!("Evenframe initialized successfully!");
    info!("Next steps:");
    info!("  1. Edit evenframe.toml to configure your project");
    info!("  2. Add #[derive(Evenframe)] to your Rust structs");
    info!("  3. Run 'evenframe generate' to generate types and sync schema");

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
output_path = "./src/generated/"
should_generate_arktype_types = true
"#
        .to_string(),
        _ => r#"[general]
apply_aliases = []

[typesync]
output_path = "./src/generated/"
should_generate_arktype_types = true
"#
        .to_string(),
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
# Output directory for generated TypeScript files
output_path = "./src/generated/"

# TypeScript schema generators
should_generate_arktype_types = true
should_generate_effect_types = false
should_generate_macroforge_types = false

# Schema file generators
should_generate_flatbuffers_types = false
should_generate_protobuf_types = false

# FlatBuffers namespace (e.g., "com.example.app")
# flatbuffers_namespace = ""

# Protocol Buffers package (e.g., "com.example.app")
# protobuf_package = ""

# Include protoc-gen-validate import in generated .proto files
protobuf_import_validate = false
"#
        .to_string(),
        _ => format!(
            r#"# Evenframe Configuration
# See https://github.com/rymskip/evenframe for documentation

[general]
# Custom attribute macros that include Evenframe derive
apply_aliases = []

# Note: Database provider '{}' is not yet fully supported
# SchemaSync currently only works with SurrealDB

[typesync]
# Output directory for generated TypeScript files
output_path = "./src/generated/"

# TypeScript schema generators
should_generate_arktype_types = true
should_generate_effect_types = false
should_generate_macroforge_types = false

# Schema file generators
should_generate_flatbuffers_types = false
should_generate_protobuf_types = false
"#,
            format!("{:?}", provider).to_lowercase()
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
