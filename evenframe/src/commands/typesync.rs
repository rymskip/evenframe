//! Typesync command - generates TypeScript types and schemas.

use crate::cli::{Cli, TypeFormat, TypesyncArgs, TypesyncCommands};
use crate::config_builders;
use evenframe_core::{
    config::EvenframeConfig,
    error::Result,
    typesync::{
        arktype::generate_arktype_type_string,
        config::{ArrayStyle, FileNamingConvention, OutputMode},
        effect::{generate_effect_schema_for_types, generate_effect_schema_string},
        file_grouping::{compute_file_grouping, FileOutputPlan},
        flatbuffers::generate_flatbuffers_schema_string,
        import_resolver::{
            barrel_filename, format_imports, generate_barrel_file, resolve_imports,
            type_name_to_filename,
        },
        macroforge::{
            compute_extra_imports, compute_macro_import_line, generate_macroforge_for_types,
            generate_macroforge_type_string,
        },
        protobuf::generate_protobuf_schema_string,
    },
};
use std::collections::HashSet;
use std::path::Path;
use tracing::{debug, error, info, warn};

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
    let build_config = config_builders::BuildConfig::from_toml()?;
    let (enums, tables, objects) = config_builders::build_all_configs(&build_config)?;
    let structs = config_builders::merge_tables_and_objects(&tables, &objects);

    info!(
        "Found {} enums, {} tables, {} objects",
        enums.len(),
        tables.len(),
        objects.len()
    );

    // Determine output mode: CLI flag overrides config.
    let output_mode = if args.per_file {
        OutputMode::PerFile
    } else {
        config.typesync.output.mode
    };
    let barrel_file = config.typesync.output.barrel_file;
    let file_naming = config.typesync.output.file_naming;
    let file_extension = &config.typesync.output.file_extension;
    let array_style = config.typesync.output.array_style;

    // Handle subcommands for specific formats
    if let Some(cmd) = args.command {
        match cmd {
            TypesyncCommands::Arktype(arktype_args) => {
                let output_path = arktype_args
                    .output
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| format!("{}arktype.ts", config.typesync.output_path));
                if output_mode == OutputMode::PerFile {
                    warn!("ArkType does not support per-file output (scope requires all types in one file). Falling back to single-file mode.");
                }
                generate_arktype(&structs, &enums, &output_path)?;
            }
            TypesyncCommands::Effect(effect_args) => {
                let output_path = effect_args
                    .output
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| format!("{}bindings.ts", config.typesync.output_path));
                match output_mode {
                    OutputMode::Single => generate_effect(&structs, &enums, &output_path)?,
                    OutputMode::PerFile => generate_effect_per_file(
                        &structs,
                        &enums,
                        &config.typesync.output_path,
                        "effect",
                        barrel_file,
                        file_naming,
                        file_extension,
                    )?,
                }
            }
            TypesyncCommands::Macroforge(macroforge_args) => {
                let output_path = macroforge_args
                    .output
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| format!("{}macroforge.ts", config.typesync.output_path));
                match output_mode {
                    OutputMode::Single => generate_macroforge(&structs, &enums, &output_path, array_style)?,
                    OutputMode::PerFile => generate_macroforge_per_file(
                        &structs,
                        &enums,
                        &config.typesync.output_path,
                        barrel_file,
                        file_naming,
                        file_extension,
                        array_style,
                    )?,
                }
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
                generate_protobuf(
                    &structs,
                    &enums,
                    &output_path,
                    package.as_deref(),
                    import_validate,
                )?;
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
                if output_mode == OutputMode::PerFile {
                    warn!("ArkType does not support per-file output (scope requires all types in one file). Falling back to single-file mode.");
                }
                let path = format!("{}arktype.ts", config.typesync.output_path);
                generate_arktype(&structs, &enums, &path)?;
            }
            TypeFormat::Effect => match output_mode {
                OutputMode::Single => {
                    let path = format!("{}bindings.ts", config.typesync.output_path);
                    generate_effect(&structs, &enums, &path)?;
                }
                OutputMode::PerFile => {
                    generate_effect_per_file(
                        &structs,
                        &enums,
                        &config.typesync.output_path,
                        "effect",
                        barrel_file,
                        file_naming,
                        file_extension,
                    )?;
                }
            },
            TypeFormat::Macroforge => match output_mode {
                OutputMode::Single => {
                    let path = format!("{}macroforge.ts", config.typesync.output_path);
                    generate_macroforge(&structs, &enums, &path, array_style)?;
                }
                OutputMode::PerFile => {
                    generate_macroforge_per_file(
                        &structs,
                        &enums,
                        &config.typesync.output_path,
                        barrel_file,
                        file_naming,
                        file_extension,
                        array_style,
                    )?;
                }
            },
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

fn generate_effect_per_file(
    structs: &std::collections::HashMap<String, evenframe_core::types::StructConfig>,
    enums: &std::collections::HashMap<String, evenframe_core::types::TaggedUnion>,
    base_output_path: &str,
    subdir: &str,
    barrel_file: bool,
    naming: FileNamingConvention,
    file_ext: &str,
) -> Result<()> {
    let plan = compute_file_grouping(structs, enums);
    let dir = Path::new(base_output_path).join(subdir);
    std::fs::create_dir_all(&dir)?;
    cleanup_obsolete_files(&dir, &plan, naming, file_ext)?;

    info!(
        "Generating Effect schemas (per-file) to {} ({} files)",
        dir.display(),
        plan.groups.len()
    );

    for group in &plan.groups {
        let imports = resolve_imports(group, &plan, structs, enums, naming, file_ext);
        let type_names = group.all_types();
        let body = generate_effect_schema_for_types(&type_names, structs, enums);

        let mut file_content = String::new();
        file_content.push_str("import { Schema } from \"effect\";\n");
        let import_lines = format_imports(&imports);
        if !import_lines.is_empty() {
            file_content.push_str(&import_lines);
            file_content.push('\n');
        }
        file_content.push('\n');
        file_content.push_str(&body);

        let filename = type_name_to_filename(&group.primary_type, naming);
        let file_path = dir.join(format!("{}{}", filename, file_ext));
        std::fs::write(&file_path, file_content)?;
        debug!("Written {}", file_path.display());
    }

    if barrel_file {
        let barrel_content = generate_barrel_file(&plan, naming, file_ext);
        let barrel_path = dir.join(barrel_filename(file_ext));
        std::fs::write(&barrel_path, barrel_content)?;
        debug!("Written barrel file {}", barrel_path.display());
    }

    info!("Effect per-file generation complete");
    Ok(())
}

fn generate_macroforge(
    structs: &std::collections::HashMap<String, evenframe_core::types::StructConfig>,
    enums: &std::collections::HashMap<String, evenframe_core::types::TaggedUnion>,
    output_path: &str,
    array_style: ArrayStyle,
) -> Result<()> {
    info!("Generating Macroforge types to {}", output_path);
    let content = generate_macroforge_type_string(structs, enums, false, array_style);
    std::fs::write(output_path, content)?;
    debug!("Macroforge types written successfully");
    Ok(())
}

fn generate_macroforge_per_file(
    structs: &std::collections::HashMap<String, evenframe_core::types::StructConfig>,
    enums: &std::collections::HashMap<String, evenframe_core::types::TaggedUnion>,
    base_output_path: &str,
    barrel_file: bool,
    naming: FileNamingConvention,
    file_ext: &str,
    array_style: ArrayStyle,
) -> Result<()> {
    let plan = compute_file_grouping(structs, enums);
    let dir = Path::new(base_output_path);
    std::fs::create_dir_all(dir)?;
    cleanup_obsolete_files(dir, &plan, naming, file_ext)?;

    info!(
        "Generating Macroforge types (per-file) to {} ({} files)",
        dir.display(),
        plan.groups.len()
    );

    for group in &plan.groups {
        let imports = resolve_imports(group, &plan, structs, enums, naming, file_ext);
        let type_names = group.all_types();
        let body = generate_macroforge_for_types(&type_names, structs, enums, array_style);

        let mut file_content = String::new();

        // Add macro import line if types have non-standard derives
        if let Some(macro_import) = compute_macro_import_line(&type_names, structs, enums) {
            file_content.push_str(&macro_import);
            file_content.push('\n');
        }

        // Add extra imports (effect types, RecordLink)
        let extra_imports = compute_extra_imports(&type_names, structs, enums);
        for import_line in &extra_imports {
            file_content.push_str(import_line);
            file_content.push('\n');
        }

        let import_lines = format_imports(&imports);
        if !import_lines.is_empty() {
            file_content.push_str(&import_lines);
            file_content.push('\n');
        }
        if !file_content.is_empty() {
            file_content.push('\n');
        }
        file_content.push_str(&body);

        let filename = type_name_to_filename(&group.primary_type, naming);
        let file_path = dir.join(format!("{}{}", filename, file_ext));
        std::fs::write(&file_path, file_content)?;
        debug!("Written {}", file_path.display());
    }

    if barrel_file {
        let barrel_content = generate_barrel_file(&plan, naming, file_ext);
        let barrel_path = dir.join(barrel_filename(file_ext));
        std::fs::write(&barrel_path, barrel_content)?;
        debug!("Written barrel file {}", barrel_path.display());
    }

    info!("Macroforge per-file generation complete");
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

/// Removes files in `dir` matching `*{file_ext}` that are not part of the current output plan.
/// This cleans up obsolete files when types are regrouped into different files.
fn cleanup_obsolete_files(
    dir: &Path,
    plan: &FileOutputPlan,
    naming: FileNamingConvention,
    file_ext: &str,
) -> Result<()> {
    let mut expected: HashSet<String> = plan
        .groups
        .iter()
        .map(|g| format!("{}{}", type_name_to_filename(&g.primary_type, naming), file_ext))
        .collect();
    expected.insert(barrel_filename(file_ext));

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Ok(()),
    };

    for entry in entries.flatten() {
        let file_name = entry.file_name().to_string_lossy().to_string();
        if file_name.ends_with(file_ext) && !expected.contains(&file_name) {
            info!("Removing obsolete file: {}", entry.path().display());
            std::fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}
