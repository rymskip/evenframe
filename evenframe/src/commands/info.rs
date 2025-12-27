//! Info command - displays information about detected types and configuration.

use crate::cli::{Cli, InfoArgs, InfoFormat};
use crate::config_builders;
use crate::workspace_scanner::{TypeKind, WorkspaceScanner};
use evenframe_core::{config::EvenframeConfig, error::Result};
use tracing::{error, info};

/// Runs the info command.
pub async fn run(_cli: &Cli, args: InfoArgs) -> Result<()> {
    // If no specific flags, show all
    let show_all = !args.types && !args.config && !args.schema;

    if args.config || show_all {
        show_config(&args.format)?;
    }

    if args.types || show_all {
        show_types(&args.format)?;
    }

    if args.schema || show_all {
        show_schema(&args.format)?;
    }

    Ok(())
}

fn show_config(format: &InfoFormat) -> Result<()> {
    match EvenframeConfig::new() {
        Ok(config) => {
            match format {
                InfoFormat::Pretty => {
                    println!("\n=== Configuration ===\n");
                    println!("Output Path: {}", config.typesync.output_path);
                    println!("\nGenerators:");
                    println!(
                        "  ArkType:     {}",
                        if config.typesync.should_generate_arktype_types {
                            "enabled"
                        } else {
                            "disabled"
                        }
                    );
                    println!(
                        "  Effect:      {}",
                        if config.typesync.should_generate_effect_types {
                            "enabled"
                        } else {
                            "disabled"
                        }
                    );
                    println!(
                        "  Macroforge:  {}",
                        if config.typesync.should_generate_macroforge_types {
                            "enabled"
                        } else {
                            "disabled"
                        }
                    );
                    println!(
                        "  FlatBuffers: {}",
                        if config.typesync.should_generate_flatbuffers_types {
                            "enabled"
                        } else {
                            "disabled"
                        }
                    );
                    println!(
                        "  Protobuf:    {}",
                        if config.typesync.should_generate_protobuf_types {
                            "enabled"
                        } else {
                            "disabled"
                        }
                    );
                    println!(
                        "\nMock Generation: {}",
                        if config.schemasync.should_generate_mocks {
                            "enabled"
                        } else {
                            "disabled"
                        }
                    );
                    if !config.general.apply_aliases.is_empty() {
                        println!("\nApply Aliases: {:?}", config.general.apply_aliases);
                    }
                }
                InfoFormat::Json => {
                    // Simple JSON output
                    println!(
                        r#"{{
  "output_path": "{}",
  "generators": {{
    "arktype": {},
    "effect": {},
    "macroforge": {},
    "flatbuffers": {},
    "protobuf": {}
  }},
  "mock_generation": {},
  "apply_aliases": {:?}
}}"#,
                        config.typesync.output_path,
                        config.typesync.should_generate_arktype_types,
                        config.typesync.should_generate_effect_types,
                        config.typesync.should_generate_macroforge_types,
                        config.typesync.should_generate_flatbuffers_types,
                        config.typesync.should_generate_protobuf_types,
                        config.schemasync.should_generate_mocks,
                        config.general.apply_aliases
                    );
                }
                InfoFormat::Yaml => {
                    println!(
                        r#"output_path: {}
generators:
  arktype: {}
  effect: {}
  macroforge: {}
  flatbuffers: {}
  protobuf: {}
mock_generation: {}
apply_aliases: {:?}"#,
                        config.typesync.output_path,
                        config.typesync.should_generate_arktype_types,
                        config.typesync.should_generate_effect_types,
                        config.typesync.should_generate_macroforge_types,
                        config.typesync.should_generate_flatbuffers_types,
                        config.typesync.should_generate_protobuf_types,
                        config.schemasync.should_generate_mocks,
                        config.general.apply_aliases
                    );
                }
            }
        }
        Err(e) => {
            error!("Failed to load configuration: {}", e);
        }
    }
    Ok(())
}

fn show_types(format: &InfoFormat) -> Result<()> {
    let config = EvenframeConfig::new()?;
    let scanner = WorkspaceScanner::new(config.general.apply_aliases)?;
    let types = scanner.scan_for_evenframe_types()?;

    match format {
        InfoFormat::Pretty => {
            println!("\n=== Detected Types ===\n");
            println!("Total: {} types found\n", types.len());

            let tables: Vec<_> = types
                .iter()
                .filter(|t| t.kind == TypeKind::Struct && t.has_id_field)
                .collect();
            let objects: Vec<_> = types
                .iter()
                .filter(|t| t.kind == TypeKind::Struct && !t.has_id_field)
                .collect();
            let enums: Vec<_> = types.iter().filter(|t| t.kind == TypeKind::Enum).collect();

            if !tables.is_empty() {
                println!("Tables ({}):", tables.len());
                for t in &tables {
                    println!("  - {} ({})", t.name, t.module_path);
                }
                println!();
            }

            if !objects.is_empty() {
                println!("Objects ({}):", objects.len());
                for t in &objects {
                    println!("  - {} ({})", t.name, t.module_path);
                }
                println!();
            }

            if !enums.is_empty() {
                println!("Enums ({}):", enums.len());
                for t in &enums {
                    println!("  - {} ({})", t.name, t.module_path);
                }
            }
        }
        InfoFormat::Json => {
            let tables: Vec<_> = types
                .iter()
                .filter(|t| t.kind == TypeKind::Struct && t.has_id_field)
                .map(|t| format!(r#"{{"name": "{}", "module": "{}"}}"#, t.name, t.module_path))
                .collect();
            let objects: Vec<_> = types
                .iter()
                .filter(|t| t.kind == TypeKind::Struct && !t.has_id_field)
                .map(|t| format!(r#"{{"name": "{}", "module": "{}"}}"#, t.name, t.module_path))
                .collect();
            let enums: Vec<_> = types
                .iter()
                .filter(|t| t.kind == TypeKind::Enum)
                .map(|t| format!(r#"{{"name": "{}", "module": "{}"}}"#, t.name, t.module_path))
                .collect();

            println!(
                r#"{{
  "tables": [{}],
  "objects": [{}],
  "enums": [{}]
}}"#,
                tables.join(", "),
                objects.join(", "),
                enums.join(", ")
            );
        }
        InfoFormat::Yaml => {
            println!("types:");
            println!("  tables:");
            for t in types
                .iter()
                .filter(|t| t.kind == TypeKind::Struct && t.has_id_field)
            {
                println!("    - name: {}", t.name);
                println!("      module: {}", t.module_path);
            }
            println!("  objects:");
            for t in types
                .iter()
                .filter(|t| t.kind == TypeKind::Struct && !t.has_id_field)
            {
                println!("    - name: {}", t.name);
                println!("      module: {}", t.module_path);
            }
            println!("  enums:");
            for t in types.iter().filter(|t| t.kind == TypeKind::Enum) {
                println!("    - name: {}", t.name);
                println!("      module: {}", t.module_path);
            }
        }
    }

    Ok(())
}

fn show_schema(_format: &InfoFormat) -> Result<()> {
    let (enums, tables, objects) = config_builders::build_all_configs();

    println!("\n=== Schema Summary ===\n");
    println!("Tables: {}", tables.len());
    println!("Objects: {}", objects.len());
    println!("Enums: {}", enums.len());

    if !tables.is_empty() {
        println!("\nTable Details:");
        for (name, table) in &tables {
            println!("  {}:", name);
            println!("    Fields: {}", table.struct_config.fields.len());
            if table.relation.is_some() {
                println!("    Relation: yes");
            }
            if table.permissions.is_some() {
                println!("    Permissions: yes");
            }
            if table.mock_generation_config.is_some() {
                println!("    Mock config: yes");
            }
        }
    }

    info!("Use 'evenframe info --types' for detailed type information");

    Ok(())
}
