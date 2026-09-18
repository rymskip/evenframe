//! Info command - displays information about detected types and configuration.

use crate::cli::{InfoArgs, InfoFormat};
use crate::config_builders;
use crate::workspace_scanner::{TypeKind, WorkspaceScanner};
use evenframe_core::{
    config::EvenframeConfig,
    error::{EvenframeError, Result},
    typesync::config::{OutputMode, TypesyncOutput},
};
use serde::Serialize;

/// Runs the info command.
pub async fn run(args: InfoArgs) -> Result<()> {
    let show_all = !args.types && !args.settings && !args.schema;
    let report = Report {
        config: (args.settings || show_all)
            .then(ConfigSummary::load)
            .transpose()?,
        types: (args.types || show_all)
            .then(TypesSummary::scan)
            .transpose()?,
        schema: (args.schema || show_all)
            .then(SchemaSummary::build)
            .transpose()?,
    };

    match args.format {
        InfoFormat::Pretty => report.print(),
        InfoFormat::Json => println!(
            "{}",
            serde_json::to_string_pretty(&report).map_err(|e| {
                EvenframeError::config(format!("Failed to serialize info as JSON: {e}"))
            })?
        ),
        InfoFormat::Yaml => print!(
            "{}",
            yaml_serde::to_string(&report).map_err(|e| {
                EvenframeError::config(format!("Failed to serialize info as YAML: {e}"))
            })?
        ),
    }
    Ok(())
}

/// Every requested section, serialized as one document.
#[derive(Serialize)]
struct Report {
    #[serde(skip_serializing_if = "Option::is_none")]
    config: Option<ConfigSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    types: Option<TypesSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    schema: Option<SchemaSummary>,
}

impl Report {
    fn print(&self) {
        if let Some(config) = &self.config {
            config.print();
        }
        if let Some(types) = &self.types {
            types.print();
        }
        if let Some(schema) = &self.schema {
            schema.print();
        }
    }
}

#[derive(Serialize)]
struct ConfigSummary {
    outputs: Vec<TypesyncOutput>,
    mock_generation: bool,
    apply_aliases: Vec<String>,
}

impl ConfigSummary {
    fn load() -> Result<Self> {
        let config = EvenframeConfig::new_offline()?;
        Ok(Self {
            outputs: config.typesync.outputs,
            mock_generation: config.schemasync.should_generate_mocks,
            apply_aliases: config.general.apply_aliases,
        })
    }

    fn print(&self) {
        println!("\n=== Configuration ===\n");
        println!("Outputs:");
        for output in &self.outputs {
            let layout = match output.files.mode {
                OutputMode::Single => "",
                OutputMode::PerFile => " (per-file)",
            };
            println!("  {} -> {}{layout}", output.kind, output.dir);
        }
        println!(
            "\nMock Generation: {}",
            if self.mock_generation {
                "enabled"
            } else {
                "disabled"
            }
        );
        if !self.apply_aliases.is_empty() {
            println!("\nApply Aliases: {}", self.apply_aliases.join(", "));
        }
    }
}

#[derive(Serialize)]
struct TypeEntry {
    name: String,
    module: String,
}

#[derive(Serialize)]
struct TypesSummary {
    tables: Vec<TypeEntry>,
    objects: Vec<TypeEntry>,
    enums: Vec<TypeEntry>,
}

impl TypesSummary {
    fn scan() -> Result<Self> {
        let config = EvenframeConfig::new_offline()?;
        let extra_files = config.resolved_include_files();
        let scanner =
            WorkspaceScanner::new(config.general.apply_aliases, config.general.expand_macros)?
                .with_extra_files(extra_files);
        let mut summary = Self {
            tables: Vec::new(),
            objects: Vec::new(),
            enums: Vec::new(),
        };
        for t in scanner.scan_for_evenframe_types()? {
            let entry = TypeEntry {
                name: t.name,
                module: t.module_path,
            };
            match (t.kind, t.has_id_field) {
                (TypeKind::Struct, true) => summary.tables.push(entry),
                (TypeKind::Struct, false) => summary.objects.push(entry),
                (TypeKind::Enum, _) => summary.enums.push(entry),
            }
        }
        Ok(summary)
    }

    fn print(&self) {
        println!("\n=== Detected Types ===\n");
        let total = self.tables.len() + self.objects.len() + self.enums.len();
        println!("Total: {total}\n");
        for (label, entries) in [
            ("Tables", &self.tables),
            ("Objects", &self.objects),
            ("Enums", &self.enums),
        ] {
            if entries.is_empty() {
                continue;
            }
            println!("{label} ({}):", entries.len());
            for entry in entries {
                println!("  - {} ({})", entry.name, entry.module);
            }
            println!();
        }
    }
}

#[derive(Serialize)]
struct TableSummary {
    name: String,
    fields: usize,
    relation: bool,
    permissions: bool,
    mock_config: bool,
}

#[derive(Serialize)]
struct SchemaSummary {
    tables: Vec<TableSummary>,
    objects: usize,
    enums: usize,
}

impl SchemaSummary {
    fn build() -> Result<Self> {
        let build_config = config_builders::BuildConfig::discover()?;
        let (enums, tables, objects) = config_builders::build_and_record(&build_config)?;
        Ok(Self {
            tables: tables
                .into_iter()
                .map(|(name, table)| TableSummary {
                    name,
                    fields: table.struct_config.fields.len(),
                    relation: table.relation.is_some(),
                    permissions: table.permissions.is_some(),
                    mock_config: table.mock_generation_config.is_some(),
                })
                .collect(),
            objects: objects.len(),
            enums: enums.len(),
        })
    }

    fn print(&self) {
        println!("\n=== Schema Summary ===\n");
        println!("Tables: {}", self.tables.len());
        println!("Objects: {}", self.objects);
        println!("Enums: {}", self.enums);
        if self.tables.is_empty() {
            return;
        }
        println!("\nTable Details:");
        for table in &self.tables {
            println!("  {}:", table.name);
            println!("    Fields: {}", table.fields);
            for (present, label) in [
                (table.relation, "Relation"),
                (table.permissions, "Permissions"),
                (table.mock_config, "Mock config"),
            ] {
                if present {
                    println!("    {label}: yes");
                }
            }
        }
    }
}
