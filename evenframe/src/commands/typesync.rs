//! Typesync command - generates TypeScript types and schemas.

use crate::cli::{Cli, TypesyncArgs, TypesyncCommands};
use crate::config_builders;
use evenframe_core::{
    config::EvenframeConfig,
    error::{EvenframeError, Result},
    schemasync::table::TableConfig,
    types::{ForeignTypeRegistry, StructConfig, TaggedUnion},
    typesync::{
        config::{OutputKind, OutputMode, TypesyncOutput},
        output::{OutputTypes, write_output},
    },
};
use std::collections::BTreeMap;
use std::path::PathBuf;
use tracing::info;

/// Runs the typesync command.
pub async fn run(cli: &Cli, args: TypesyncArgs) -> Result<()> {
    let config = EvenframeConfig::new()?;
    let build_config = config_builders::BuildConfig::discover()?;
    let (enums, tables, objects) = config_builders::build_and_record(&build_config)?;
    generate(cli, args, &config, enums, tables, objects)
}

/// Generates the types for an already-scanned project, so `generate` can
/// share its scan.
pub(crate) fn generate(
    cli: &Cli,
    args: TypesyncArgs,
    config: &EvenframeConfig,
    enums: BTreeMap<String, TaggedUnion>,
    tables: BTreeMap<String, TableConfig>,
    objects: BTreeMap<String, StructConfig>,
) -> Result<()> {
    info!("Starting type generation");
    let (enums, tables, objects) = config_builders::filter_for_typesync(enums, tables, objects);
    let structs = config_builders::merge_tables_and_objects(&tables, &objects);
    let registry = ForeignTypeRegistry::from_config(&config.general.foreign_types);
    let types = OutputTypes {
        structs: &structs,
        enums: &enums,
        registry: &registry,
    };

    let (mut outputs, file) = select_outputs(cli, &args, config)?;
    if outputs.is_empty() {
        println!("No outputs selected; configure them under [typesync] as `output` or `outputs`");
        return Ok(());
    }
    if args.per_file {
        for output in outputs.iter_mut() {
            if output.kind.supports_per_file() {
                output.files.mode = OutputMode::PerFile;
            }
        }
    }
    let dir_override = match (&cli.output_dir, outputs.as_slice()) {
        (Some(dir), [_]) => Some(dir.clone()),
        (Some(_), _) => {
            return Err(EvenframeError::config(format!(
                "--output sets one output's directory, but {} outputs are selected; \
                 give each a `dir` in [typesync] or select one",
                outputs.len()
            )));
        }
        (None, _) => None,
    };

    for output in &outputs {
        // `--output` is taken as given (relative to where the command runs);
        // a configured `dir` is relative to the project root.
        let dir = dir_override
            .clone()
            .unwrap_or_else(|| output.resolve_dir(config.project_root()));
        let written = write_output(output, &dir, file.as_deref(), &types)?;
        match output.files.mode {
            OutputMode::Single => {
                for generated in &written {
                    println!("Wrote {}", generated.path.display());
                }
            }
            OutputMode::PerFile => {
                println!("Wrote {} (files: {})", dir.display(), written.len());
            }
        }
    }
    Ok(())
}

/// The outputs this run generates, and the file a subcommand's `-o` names.
fn select_outputs(
    cli: &Cli,
    args: &TypesyncArgs,
    config: &EvenframeConfig,
) -> Result<(Vec<TypesyncOutput>, Option<PathBuf>)> {
    let configured = &config.typesync.outputs;
    let Some(command) = &args.command else {
        let outputs = configured
            .iter()
            .filter(|o| args.formats.as_ref().is_none_or(|f| f.contains(&o.kind)))
            .filter(|o| args.skip.as_ref().is_none_or(|s| !s.contains(&o.kind)))
            .cloned()
            .collect();
        return Ok((outputs, None));
    };

    let (kind, file) = match command {
        TypesyncCommands::Arktype(a) => (OutputKind::Arktype, a.file.clone()),
        TypesyncCommands::Effect(a) => (OutputKind::Effect, a.file.clone()),
        TypesyncCommands::Macroforge(a) => (OutputKind::Macroforge, a.file.clone()),
        TypesyncCommands::Flatbuffers(a) => (OutputKind::Flatbuffers, a.file.clone()),
        TypesyncCommands::Protobuf(a) => (OutputKind::Protobuf, a.file.clone()),
    };
    let mut outputs: Vec<TypesyncOutput> = configured
        .iter()
        .filter(|o| o.kind == kind)
        .cloned()
        .collect();
    if outputs.is_empty() {
        // Unconfigured, the kind still runs once told where to write.
        let dir = match (&cli.output_dir, &file) {
            (Some(dir), _) => dir.clone(),
            (None, Some(file)) => file.parent().map(PathBuf::from).unwrap_or_default(),
            (None, None) => {
                return Err(EvenframeError::config(format!(
                    "no `{kind}` output is configured; add one to [typesync] or pass --output <dir>"
                )));
            }
        };
        outputs.push(TypesyncOutput::new(kind, dir.to_string_lossy()));
    }
    if file.is_some() && outputs.len() > 1 {
        return Err(EvenframeError::config(format!(
            "-o/--file names one file, but {} `{kind}` outputs are configured",
            outputs.len()
        )));
    }

    for output in outputs.iter_mut() {
        match command {
            TypesyncCommands::Flatbuffers(a) => {
                if let Some(namespace) = &a.namespace {
                    output.namespace = Some(namespace.clone());
                }
            }
            TypesyncCommands::Protobuf(a) => {
                if let Some(package) = &a.package {
                    output.package = Some(package.clone());
                }
                if a.import_validate {
                    output.import_validate = true;
                } else if a.no_import_validate {
                    output.import_validate = false;
                }
            }
            TypesyncCommands::Arktype(_)
            | TypesyncCommands::Effect(_)
            | TypesyncCommands::Macroforge(_) => {}
        }
    }
    Ok((outputs, file))
}
