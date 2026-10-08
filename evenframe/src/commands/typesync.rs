//! Typesync command - generates TypeScript types and schemas.

use crate::cli::{Cli, TypesyncArgs, TypesyncCommands};
use crate::target::Target;
use evenframe_core::scan::merge_tables_and_objects;
use evenframe_core::{
    config::EvenframeConfig,
    error::{EvenframeError, Result},
    types::{AllConfigs, ForeignTypeRegistry},
    typesync::{
        checks::check_types,
        config::{OutputKind, OutputMode, TypesyncOutput},
        output::{OutputTypes, render_outputs},
    },
};
use std::path::PathBuf;
use tracing::info;

/// Runs the typesync command: one output for the project, or for every
/// project of a workspace together.
pub async fn run(cli: &Cli, args: TypesyncArgs) -> Result<()> {
    let target = Target::discover()?;
    let scanned = target.scan_all()?;
    let (config, configs) = target.typesync_input(&scanned)?;
    generate(cli, args, &config, &configs)
}

/// Generates the types for an already-scanned project, so `generate` can
/// share its scan.
pub(crate) fn generate(
    cli: &Cli,
    args: TypesyncArgs,
    config: &EvenframeConfig,
    configs: &AllConfigs,
) -> Result<()> {
    info!("Starting type generation");
    let registry = ForeignTypeRegistry::from_config(&config.general.foreign_types);
    check_types(configs, &registry)?;
    let AllConfigs {
        enums,
        tables,
        objects,
        newtypes,
    } = configs.for_typesync()?;
    let structs = merge_tables_and_objects(tables, objects);
    let types = OutputTypes::new(&structs, &enums, &newtypes, &registry)?;

    let (mut outputs, file) = select_outputs(cli, &args, config)?;
    if outputs.is_empty() {
        return Err(EvenframeError::config(
            "No outputs selected; configure them under [typesync] as `output` or `outputs`",
        ));
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

    // `--output` is taken as given (relative to where the command runs); a
    // configured `dir` is relative to the project root.
    let targets: Vec<_> = outputs
        .iter()
        .map(|output| {
            let dir = dir_override
                .clone()
                .unwrap_or_else(|| output.resolve_dir(config.project_root()));
            (output, dir)
        })
        .collect();
    let rendered = render_outputs(&targets, file.as_deref(), &types)?;
    for ((output, dir), rendered) in targets.iter().zip(rendered) {
        let written = rendered.write()?;
        match output.files.mode {
            OutputMode::Single => {
                for generated in &written {
                    let verb = if generated.changed {
                        "Wrote"
                    } else {
                        "Unchanged"
                    };
                    println!("{verb} {}", generated.path.display());
                }
            }
            OutputMode::PerFile => {
                println!(
                    "Generated {} (files: {}, changed: {})",
                    dir.display(),
                    written.len(),
                    written.iter().filter(|generated| generated.changed).count()
                );
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
            .filter(|output| {
                args.formats
                    .as_ref()
                    .is_none_or(|formats| formats.contains(&output.kind))
            })
            .filter(|output| {
                args.skip
                    .as_ref()
                    .is_none_or(|skipped| !skipped.contains(&output.kind))
            })
            .cloned()
            .collect();
        return Ok((outputs, None));
    };

    let (kind, file) = match command {
        TypesyncCommands::Arktype(command_args) => (OutputKind::Arktype, command_args.file.clone()),
        TypesyncCommands::Effect(command_args) => (OutputKind::Effect, command_args.file.clone()),
        TypesyncCommands::Macroforge(command_args) => {
            (OutputKind::Macroforge, command_args.file.clone())
        }
        TypesyncCommands::Flatbuffers(command_args) => {
            (OutputKind::Flatbuffers, command_args.file.clone())
        }
        TypesyncCommands::Protobuf(command_args) => {
            (OutputKind::Protobuf, command_args.file.clone())
        }
    };
    let mut outputs: Vec<TypesyncOutput> = configured
        .iter()
        .filter(|output| output.kind == kind)
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
            TypesyncCommands::Flatbuffers(command_args) => {
                if let Some(namespace) = &command_args.namespace {
                    output.namespace = Some(namespace.clone());
                }
            }
            TypesyncCommands::Protobuf(command_args) => {
                if let Some(package) = &command_args.package {
                    output.package = Some(package.clone());
                }
                if command_args.import_validate {
                    output.import_validate = true;
                } else if command_args.no_import_validate {
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
