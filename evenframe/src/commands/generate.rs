//! Generate command - runs the full pipeline (typesync + schemasync).

use crate::cli::{Cli, GenerateArgs};
use crate::config_builders;
use evenframe_core::{
    config::EvenframeConfig,
    error::Result,
    schemasync::Schemasync,
    typesync::{
        arktype::generate_arktype_type_string, effect::generate_effect_schema_string,
        flatbuffers::generate_flatbuffers_schema_string,
        macroforge::generate_macroforge_type_string,
        protobuf::generate_protobuf_schema_string,
    },
};
use tracing::{debug, error, info};

/// Runs the full generation pipeline with default settings.
pub async fn run_default(cli: &Cli) -> Result<()> {
    let args = GenerateArgs {
        skip_typesync: false,
        skip_schemasync: false,
        no_mocks: false,
        watch: false,
    };
    run(cli, args).await
}

/// Runs the full generation pipeline.
pub async fn run(_cli: &Cli, args: GenerateArgs) -> Result<()> {
    info!("Starting Evenframe code generation");

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

    // Build all configs
    info!("Building all configs...");
    let (enums, tables, objects) = config_builders::build_all_configs();
    info!(
        "Config building complete. Found {} enums, {} tables, {} objects",
        enums.len(),
        tables.len(),
        objects.len()
    );

    // TypeSync phase
    if !args.skip_typesync {
        run_typesync(&config, &enums, &tables, &objects)?;
    } else {
        debug!("Skipping typesync phase");
    }

    // SchemaSync phase
    if !args.skip_schemasync {
        run_schemasync(&config, &enums, &tables, &objects, args.no_mocks).await?;
    } else {
        debug!("Skipping schemasync phase");
    }

    info!("Evenframe code generation completed successfully");
    Ok(())
}

fn run_typesync(
    config: &EvenframeConfig,
    enums: &std::collections::HashMap<String, evenframe_core::types::TaggedUnion>,
    tables: &std::collections::HashMap<String, evenframe_core::schemasync::table::TableConfig>,
    objects: &std::collections::HashMap<String, evenframe_core::types::StructConfig>,
) -> Result<()> {
    let structs = config_builders::merge_tables_and_objects(tables, objects);

    if config.typesync.should_generate_arktype_types {
        info!("Generating arktype types...");
        let arktype_content = generate_arktype_type_string(&structs, enums, false);
        debug!(
            "Generated arktype content: {} characters",
            arktype_content.len()
        );

        std::fs::write(
            format!("{}arktype.ts", config.typesync.output_path),
            format!(
                "import {{ scope }} from 'arktype';\n\n{}\n\n export const validator = scope({{\n  ...bindings.export(),\n}}).export();",
                arktype_content,
            ),
        )?;
        info!("Arktype types written successfully to arktype.ts");
    }

    if config.typesync.should_generate_effect_types {
        info!("Generating Effect schemas...");
        let effect_content = generate_effect_schema_string(&structs, enums, false);
        debug!(
            "Generated Effect content: {} characters",
            effect_content.len()
        );

        std::fs::write(
            format!("{}bindings.ts", config.typesync.output_path),
            format!("import {{ Schema }} from \"effect\";\n\n{}", effect_content),
        )?;
        info!("Effect schemas written successfully to bindings.ts");
    }

    if config.typesync.should_generate_macroforge_types {
        info!("Generating Macroforge types...");
        let macroforge_content = generate_macroforge_type_string(&structs, enums, false);
        debug!(
            "Generated Macroforge content: {} characters",
            macroforge_content.len()
        );

        std::fs::write(
            format!("{}macroforge.ts", config.typesync.output_path),
            macroforge_content,
        )?;
        info!("Macroforge types written successfully to macroforge.ts");
    }

    if config.typesync.should_generate_flatbuffers_types {
        info!("Generating FlatBuffers schema...");
        let flatbuffers_content = generate_flatbuffers_schema_string(
            &structs,
            enums,
            config.typesync.flatbuffers_namespace.as_deref(),
        );
        debug!(
            "Generated FlatBuffers content: {} characters",
            flatbuffers_content.len()
        );

        std::fs::write(
            format!("{}schema.fbs", config.typesync.output_path),
            flatbuffers_content,
        )?;
        info!("FlatBuffers schema written successfully to schema.fbs");
    }

    if config.typesync.should_generate_protobuf_types {
        info!("Generating Protocol Buffers schema...");
        let protobuf_content = generate_protobuf_schema_string(
            &structs,
            enums,
            config.typesync.protobuf_package.as_deref(),
            config.typesync.protobuf_import_validate,
        );
        debug!(
            "Generated Protocol Buffers content: {} characters",
            protobuf_content.len()
        );

        std::fs::write(
            format!("{}schema.proto", config.typesync.output_path),
            protobuf_content,
        )?;
        info!("Protocol Buffers schema written successfully to schema.proto");
    }

    Ok(())
}

async fn run_schemasync(
    _config: &EvenframeConfig,
    enums: &std::collections::HashMap<String, evenframe_core::types::TaggedUnion>,
    tables: &std::collections::HashMap<String, evenframe_core::schemasync::table::TableConfig>,
    objects: &std::collections::HashMap<String, evenframe_core::types::StructConfig>,
    _no_mocks: bool,
) -> Result<()> {
    info!("Starting Schemasync");

    let schemasync = Schemasync::new()
        .with_tables(tables)
        .with_objects(objects)
        .with_enums(enums);

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

    Ok(())
}
