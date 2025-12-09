mod config_builders;
mod workspace_scanner;

use evenframe_core::evenframe_log;
use evenframe_core::schemasync::Schemasync; // Import your new struct
use evenframe_core::{
    config::EvenframeConfig,
    error::Result,
    typesync::{
        arktype::generate_arktype_type_string, effect::generate_effect_schema_string,
        macroforge::generate_macroforge_type_string,
    },
};
use tracing::{debug, error, info};

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().expect("Dotenv failed to initialize env variables");

    evenframe_log!("", "tracing.log");
    evenframe_log!("", "errors.log");

    // Initialize tracing with environment variable control
    // Set RUST_LOG=debug for debug output, RUST_LOG=info for info only
    tracing_subscriber::fmt::init();

    info!("Starting Evenframe code generation");
    debug!("Loading configuration...");

    // Load configuration
    let config = match EvenframeConfig::new() {
        Ok(cfg) => {
            info!("Configuration loaded successfully");
            cfg
        }
        Err(e) => {
            error!("Failed to load configuration: {}", e);
            return Err(e);
        }
    };

    let generate_arktype_types = config.typesync.should_generate_arktype_types;
    let generate_effect_schemas = config.typesync.should_generate_effect_types;
    let generate_macroforge_types = config.typesync.should_generate_macroforge_types;

    debug!(
        "Configuration flags - arktype: {}, effect: {}, macroforge: {}",
        generate_arktype_types, generate_effect_schemas, generate_macroforge_types
    );

    // Get the config builder closure
    info!("Building all configs...");
    let (enums, tables, objects) = config_builders::build_all_configs();
    info!(
        "Config building complete. Found {} enums, {} tables, {} objects",
        enums.len(),
        tables.len(),
        objects.len()
    );

    if generate_arktype_types {
        info!("Generating arktype types...");
        let structs = config_builders::merge_tables_and_objects(&tables, &objects);
        debug!("Merged {} structs for arktype generation", structs.len());

        let arktype_content = generate_arktype_type_string(&structs, &enums, false);
        debug!(
            "Generated arktype content: {} characters",
            arktype_content.len()
        );

        match std::fs::write(
            format!("{}arktype.ts", config.typesync.output_path),
            format!(
                "import {{ scope }} from 'arktype';\n\n{}\n\n export const validator = scope({{
  ...bindings.export(),
            }}).export();",
                arktype_content,
            ),
        ) {
            Ok(_) => info!("Arktype types written successfully to arktype.ts"),
            Err(e) => {
                error!("Failed to write arktype types: {}", e);
                return Err(e.into());
            }
        }
    } else {
        debug!("Skipping arktype type generation (disabled in config)");
    }

    if generate_effect_schemas {
        info!("Generating Effect schemas...");
        let structs = config_builders::merge_tables_and_objects(&tables, &objects);
        debug!("Merged {} structs for Effect generation", structs.len());

        let effect_content = generate_effect_schema_string(&structs, &enums, false);
        debug!(
            "Generated Effect content: {} characters",
            effect_content.len()
        );
        //TODO: This should not create directories if they dont exist, it should fail
        match std::fs::write(
            format!("{}bindings.ts", config.typesync.output_path),
            format!("import {{ Schema }} from \"effect\";\n\n{}", effect_content,),
        ) {
            Ok(_) => info!("Effect schemas written successfully to bindings.ts"),
            Err(e) => {
                error!("Failed to write Effect schemas: {}", e);
                return Err(e.into());
            }
        }
    } else {
        debug!("Skipping Effect schema generation (disabled in config)");
    }

    if generate_macroforge_types {
        info!("Generating Macroforge types...");
        let structs = config_builders::merge_tables_and_objects(&tables, &objects);
        debug!("Merged {} structs for Macroforge generation", structs.len());

        let macroforge_content = generate_macroforge_type_string(&structs, &enums, false);
        debug!(
            "Generated Macroforge content: {} characters",
            macroforge_content.len()
        );

        match std::fs::write(
            format!("{}macroforge.ts", config.typesync.output_path),
            macroforge_content,
        ) {
            Ok(_) => info!("Macroforge types written successfully to macroforge.ts"),
            Err(e) => {
                error!("Failed to write Macroforge types: {}", e);
                return Err(e.into());
            }
        }
    } else {
        debug!("Skipping Macroforge type generation (disabled in config)");
    }

    info!("Starting Schemasync");
    // Much simpler now!
    let schemasync = Schemasync::new()
        .with_tables(&tables)
        .with_objects(&objects)
        .with_enums(&enums);

    debug!(
        "Initialized Schemasync with {} tables, {} objects, {} enums",
        tables.len(),
        objects.len(),
        enums.len()
    );

    info!("Running Schemasync...");
    match schemasync.run().await {
        Ok(_) => info!("Schemasync completed successfully"),
        Err(e) => {
            error!("Schemasync failed: {}", e);
            return Err(e);
        }
    }

    info!("Evenframe code generation completed successfully");
    Ok(())
}
