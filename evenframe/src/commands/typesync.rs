//! Typesync command - generates TypeScript types and schemas.

use crate::cli::{Cli, TypeFormat, TypesyncArgs, TypesyncCommands};
use crate::config_builders;
use evenframe_core::{
    config::EvenframeConfig,
    error::Result,
    typesync::{
        arktype::generate_arktype_type_string, effect::generate_effect_schema_string,
        flatbuffers::generate_flatbuffers_schema_string,
        macroforge::generate_macroforge_type_string,
        protobuf::generate_protobuf_schema_string,
    },
};
use std::collections::HashSet;
use tracing::{debug, error, info};

/// Runs the typesync command.
pub async fn run(_cli: &Cli, args: TypesyncArgs) -> Result<()> {
    info!("Starting type generation");

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
    let (enums, tables, objects) = config_builders::build_all_configs();
    let structs = config_builders::merge_tables_and_objects(&tables, &objects);

    info!(
        "Found {} enums, {} tables, {} objects",
        enums.len(),
        tables.len(),
        objects.len()
    );

    // Handle subcommands for specific formats
    if let Some(cmd) = args.command {
        match cmd {
            TypesyncCommands::Arktype(arktype_args) => {
                let output_path = arktype_args
                    .output
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| format!("{}arktype.ts", config.typesync.output_path));
                generate_arktype(&structs, &enums, &output_path)?;
            }
            TypesyncCommands::Effect(effect_args) => {
                let output_path = effect_args
                    .output
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| format!("{}bindings.ts", config.typesync.output_path));
                generate_effect(&structs, &enums, &output_path)?;
            }
            TypesyncCommands::Macroforge(macroforge_args) => {
                let output_path = macroforge_args
                    .output
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| format!("{}macroforge.ts", config.typesync.output_path));
                generate_macroforge(&structs, &enums, &output_path)?;
            }
            TypesyncCommands::Flatbuffers(fbs_args) => {
                let output_path = fbs_args
                    .output
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| format!("{}schema.fbs", config.typesync.output_path));
                let namespace = fbs_args
                    .namespace
                    .or(config.typesync.flatbuffers_namespace.clone());
                generate_flatbuffers(&structs, &enums, &output_path, namespace.as_deref())?;
            }
            TypesyncCommands::Protobuf(proto_args) => {
                let output_path = proto_args
                    .output
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| format!("{}schema.proto", config.typesync.output_path));
                let package = proto_args
                    .package
                    .or(config.typesync.protobuf_package.clone());
                let import_validate = if proto_args.no_import_validate {
                    false
                } else if proto_args.import_validate {
                    true
                } else {
                    config.typesync.protobuf_import_validate
                };
                generate_protobuf(&structs, &enums, &output_path, package.as_deref(), import_validate)?;
            }
        }
        return Ok(());
    }

    // Determine which formats to generate
    let mut formats_to_generate: HashSet<TypeFormat> = HashSet::new();

    if let Some(ref formats) = args.formats {
        // Use explicit formats from CLI
        formats_to_generate.extend(formats.iter().cloned());
    } else {
        // Use config file settings
        if config.typesync.should_generate_arktype_types {
            formats_to_generate.insert(TypeFormat::Arktype);
        }
        if config.typesync.should_generate_effect_types {
            formats_to_generate.insert(TypeFormat::Effect);
        }
        if config.typesync.should_generate_macroforge_types {
            formats_to_generate.insert(TypeFormat::Macroforge);
        }
        if config.typesync.should_generate_flatbuffers_types {
            formats_to_generate.insert(TypeFormat::Flatbuffers);
        }
        if config.typesync.should_generate_protobuf_types {
            formats_to_generate.insert(TypeFormat::Protobuf);
        }
    }

    // Remove skipped formats
    if let Some(ref skip) = args.skip {
        for format in skip {
            formats_to_generate.remove(format);
        }
    }

    // Generate each format
    for format in &formats_to_generate {
        match format {
            TypeFormat::Arktype => {
                let path = format!("{}arktype.ts", config.typesync.output_path);
                generate_arktype(&structs, &enums, &path)?;
            }
            TypeFormat::Effect => {
                let path = format!("{}bindings.ts", config.typesync.output_path);
                generate_effect(&structs, &enums, &path)?;
            }
            TypeFormat::Macroforge => {
                let path = format!("{}macroforge.ts", config.typesync.output_path);
                generate_macroforge(&structs, &enums, &path)?;
            }
            TypeFormat::Flatbuffers => {
                let path = format!("{}schema.fbs", config.typesync.output_path);
                generate_flatbuffers(
                    &structs,
                    &enums,
                    &path,
                    config.typesync.flatbuffers_namespace.as_deref(),
                )?;
            }
            TypeFormat::Protobuf => {
                let path = format!("{}schema.proto", config.typesync.output_path);
                generate_protobuf(
                    &structs,
                    &enums,
                    &path,
                    config.typesync.protobuf_package.as_deref(),
                    config.typesync.protobuf_import_validate,
                )?;
            }
        }
    }

    info!(
        "Type generation complete. Generated {} format(s)",
        formats_to_generate.len()
    );
    Ok(())
}

fn generate_arktype(
    structs: &std::collections::HashMap<String, evenframe_core::types::StructConfig>,
    enums: &std::collections::HashMap<String, evenframe_core::types::TaggedUnion>,
    output_path: &str,
) -> Result<()> {
    info!("Generating ArkType types to {}", output_path);
    let content = generate_arktype_type_string(structs, enums, false);
    let full_content = format!(
        "import {{ scope }} from 'arktype';\n\n{}\n\n export const validator = scope({{\n  ...bindings.export(),\n}}).export();",
        content
    );
    std::fs::write(output_path, full_content)?;
    debug!("ArkType types written successfully");
    Ok(())
}

fn generate_effect(
    structs: &std::collections::HashMap<String, evenframe_core::types::StructConfig>,
    enums: &std::collections::HashMap<String, evenframe_core::types::TaggedUnion>,
    output_path: &str,
) -> Result<()> {
    info!("Generating Effect schemas to {}", output_path);
    let content = generate_effect_schema_string(structs, enums, false);
    let full_content = format!("import {{ Schema }} from \"effect\";\n\n{}", content);
    std::fs::write(output_path, full_content)?;
    debug!("Effect schemas written successfully");
    Ok(())
}

fn generate_macroforge(
    structs: &std::collections::HashMap<String, evenframe_core::types::StructConfig>,
    enums: &std::collections::HashMap<String, evenframe_core::types::TaggedUnion>,
    output_path: &str,
) -> Result<()> {
    info!("Generating Macroforge types to {}", output_path);
    let content = generate_macroforge_type_string(structs, enums, false);
    std::fs::write(output_path, content)?;
    debug!("Macroforge types written successfully");
    Ok(())
}

fn generate_flatbuffers(
    structs: &std::collections::HashMap<String, evenframe_core::types::StructConfig>,
    enums: &std::collections::HashMap<String, evenframe_core::types::TaggedUnion>,
    output_path: &str,
    namespace: Option<&str>,
) -> Result<()> {
    info!("Generating FlatBuffers schema to {}", output_path);
    let content = generate_flatbuffers_schema_string(structs, enums, namespace);
    std::fs::write(output_path, content)?;
    debug!("FlatBuffers schema written successfully");
    Ok(())
}

fn generate_protobuf(
    structs: &std::collections::HashMap<String, evenframe_core::types::StructConfig>,
    enums: &std::collections::HashMap<String, evenframe_core::types::TaggedUnion>,
    output_path: &str,
    package: Option<&str>,
    import_validate: bool,
) -> Result<()> {
    info!("Generating Protocol Buffers schema to {}", output_path);
    let content = generate_protobuf_schema_string(structs, enums, package, import_validate);
    std::fs::write(output_path, content)?;
    debug!("Protocol Buffers schema written successfully");
    Ok(())
}
