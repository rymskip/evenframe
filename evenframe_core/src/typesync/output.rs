//! Writing one configured output's files.

use crate::error::{EvenframeError, Result};
use crate::types::{ForeignTypeRegistry, StructConfig, TaggedUnion};
use crate::typesync::arktype::generate_arktype_type_string;
use crate::typesync::config::{OutputKind, OutputMode, TypesyncOutput};
use crate::typesync::effect::{generate_effect_schema_for_types, generate_effect_schema_string};
use crate::typesync::file_grouping::{FileOutputPlan, compute_file_grouping};
use crate::typesync::import_resolver::{
    barrel_filename, format_imports, generate_barrel_file, resolve_imports, type_name_to_filename,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// The types an output is generated from.
pub struct OutputTypes<'a> {
    pub structs: &'a BTreeMap<String, StructConfig>,
    pub enums: &'a BTreeMap<String, TaggedUnion>,
    pub registry: &'a ForeignTypeRegistry,
}

/// A file an output wrote.
#[derive(Debug, Clone)]
pub struct GeneratedFile {
    pub path: PathBuf,
    pub bytes_written: usize,
    pub kind: OutputKind,
}

/// Writes `output` into `dir`, returning every file written. A single-file
/// output goes to `file` when given, else to the output's own `file` in
/// `dir`, else to its kind's standard name there; a per-file output takes
/// neither.
pub fn write_output(
    output: &TypesyncOutput,
    dir: &Path,
    file: Option<&Path>,
    types: &OutputTypes,
) -> Result<Vec<GeneratedFile>> {
    let file = file
        .map(Path::to_path_buf)
        .or_else(|| output.file.as_ref().map(|name| dir.join(name)));
    if output.files.mode == OutputMode::PerFile {
        if let Some(file) = file {
            return Err(EvenframeError::config(format!(
                "{} names a single output file, but the `{}` output is per-file and writes a directory",
                file.display(),
                output.kind
            )));
        }
        return write_per_file(output, dir, types);
    }

    let path = file.unwrap_or_else(|| dir.join(output.kind.default_filename()));
    info!("Generating {} output to {}", output.kind, path.display());
    let content = single_file_content(output, types)?;
    write_file(&path, &content)?;
    Ok(vec![GeneratedFile {
        path,
        bytes_written: content.len(),
        kind: output.kind,
    }])
}

fn single_file_content(output: &TypesyncOutput, types: &OutputTypes) -> Result<String> {
    let OutputTypes {
        structs,
        enums,
        registry,
    } = types;
    match output.kind {
        OutputKind::Arktype => Ok(format!(
            "import {{ scope }} from 'arktype';\n\n{}\n\n export const validator = scope({{\n  ...bindings.export(),\n}}).export();",
            generate_arktype_type_string(structs, enums, false, registry)
        )),
        OutputKind::Effect => Ok(format!(
            "import {{ Schema }} from \"effect\";\n\n{}",
            generate_effect_schema_string(structs, enums, false, registry)
        )),
        OutputKind::Macroforge => {
            #[cfg(feature = "macroforge")]
            let content = Ok(
                crate::typesync::macroforge::generate_macroforge_type_string(
                    structs,
                    enums,
                    false,
                    output.files.array_style,
                    registry,
                    output.files.import_extension,
                ),
            );
            #[cfg(not(feature = "macroforge"))]
            let content = Err(not_built(OutputKind::Macroforge));
            content
        }
        OutputKind::Flatbuffers => {
            #[cfg(feature = "flatbuffers")]
            let content = Ok(
                crate::typesync::flatbuffers::generate_flatbuffers_schema_string(
                    structs,
                    enums,
                    output.namespace.as_deref(),
                    registry,
                ),
            );
            #[cfg(not(feature = "flatbuffers"))]
            let content = Err(not_built(OutputKind::Flatbuffers));
            content
        }
        OutputKind::Protobuf => {
            #[cfg(feature = "protobuf")]
            let content = Ok(crate::typesync::protobuf::generate_protobuf_schema_string(
                structs,
                enums,
                output.package.as_deref(),
                output.import_validate,
                registry,
            ));
            #[cfg(not(feature = "protobuf"))]
            let content = Err(not_built(OutputKind::Protobuf));
            content
        }
    }
}

#[cfg(not(all(feature = "macroforge", feature = "flatbuffers", feature = "protobuf")))]
fn not_built(kind: OutputKind) -> EvenframeError {
    EvenframeError::config(format!(
        "this evenframe was built without the `{kind}` feature, so it cannot write `{kind}` outputs"
    ))
}

/// One file per primary type (with its exclusive dependents), plus an
/// optional barrel file, all directly in `dir`.
fn write_per_file(
    output: &TypesyncOutput,
    dir: &Path,
    types: &OutputTypes,
) -> Result<Vec<GeneratedFile>> {
    let OutputTypes {
        structs,
        enums,
        registry,
    } = types;
    let settings = &output.files;
    let plan = compute_file_grouping(structs, enums);
    create_dir(dir)?;
    remove_obsolete_files(dir, &plan, output)?;
    info!(
        "Generating {} output (per-file) to {} ({} files)",
        output.kind,
        dir.display(),
        plan.groups.len()
    );

    let mut written = Vec::new();
    for group in &plan.groups {
        let imports = resolve_imports(
            group,
            &plan,
            structs,
            enums,
            settings.file_naming,
            &settings.file_extension,
            settings.import_extension,
        );
        let type_names = group.all_types();
        let mut content = String::new();
        match output.kind {
            OutputKind::Effect => {
                content.push_str("import { Schema } from \"effect\";\n");
                push_imports(&mut content, &format_imports(&imports));
                content.push('\n');
                content.push_str(&generate_effect_schema_for_types(
                    &type_names,
                    structs,
                    enums,
                    registry,
                ));
            }
            OutputKind::Macroforge => {
                #[cfg(feature = "macroforge")]
                macroforge_per_file_content(&mut content, &type_names, &imports, output, types);
                #[cfg(not(feature = "macroforge"))]
                return Err(not_built(OutputKind::Macroforge));
            }
            kind => {
                return Err(EvenframeError::config(format!(
                    "the `{kind}` output cannot be written per-file"
                )));
            }
        }
        let filename = type_name_to_filename(&group.primary_type, settings.file_naming);
        let path = dir.join(format!("{filename}{}", settings.file_extension));
        write_file(&path, &content)?;
        debug!("Written {}", path.display());
        written.push(GeneratedFile {
            path,
            bytes_written: content.len(),
            kind: output.kind,
        });
    }

    if settings.barrel_file {
        let content = generate_barrel_file(
            &plan,
            settings.file_naming,
            &settings.file_extension,
            settings.import_extension,
        );
        let path = dir.join(barrel_filename(&settings.file_extension));
        write_file(&path, &content)?;
        written.push(GeneratedFile {
            path,
            bytes_written: content.len(),
            kind: output.kind,
        });
    }
    Ok(written)
}

#[cfg(feature = "macroforge")]
fn macroforge_per_file_content(
    content: &mut String,
    type_names: &[String],
    imports: &[crate::typesync::import_resolver::ImportStatement],
    output: &TypesyncOutput,
    types: &OutputTypes,
) {
    use crate::typesync::macroforge::{
        compute_extra_imports, compute_macro_import_line, generate_macroforge_for_types,
    };
    if let Some(macro_import) = compute_macro_import_line(type_names, types.structs, types.enums) {
        content.push_str(&macro_import);
        content.push('\n');
    }
    for import_line in compute_extra_imports(
        type_names,
        types.structs,
        types.enums,
        types.registry,
        output.files.import_extension,
    ) {
        content.push_str(&import_line);
        content.push('\n');
    }
    push_imports(content, &format_imports(imports));
    if !content.is_empty() {
        content.push('\n');
    }
    content.push_str(&generate_macroforge_for_types(
        type_names,
        types.structs,
        types.enums,
        output.files.array_style,
        types.registry,
    ));
}

fn push_imports(content: &mut String, import_lines: &str) {
    if !import_lines.is_empty() {
        content.push_str(import_lines);
        content.push('\n');
    }
}

/// Removes this output's files in `dir` that the current plan no longer
/// produces, such as a type that moved into another type's file.
fn remove_obsolete_files(dir: &Path, plan: &FileOutputPlan, output: &TypesyncOutput) -> Result<()> {
    let settings = &output.files;
    let mut expected: BTreeSet<String> = plan
        .groups
        .iter()
        .map(|g| {
            format!(
                "{}{}",
                type_name_to_filename(&g.primary_type, settings.file_naming),
                settings.file_extension
            )
        })
        .collect();
    expected.insert(barrel_filename(&settings.file_extension));

    let unreadable = |e: std::io::Error| {
        EvenframeError::config(format!("Failed to read {}: {e}", dir.display()))
    };
    for entry in fs::read_dir(dir).map_err(unreadable)? {
        let path = entry.map_err(unreadable)?.path();
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if file_name.ends_with(&settings.file_extension) && !expected.contains(&file_name) {
            info!("Removing obsolete file: {}", path.display());
            fs::remove_file(&path).map_err(|e| {
                EvenframeError::config(format!("Failed to remove {}: {e}", path.display()))
            })?;
        }
    }
    Ok(())
}

fn create_dir(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir).map_err(|e| {
        EvenframeError::config(format!(
            "Failed to create output directory {}: {e}",
            dir.display()
        ))
    })
}

/// Writes one file, creating its directory first.
fn write_file(path: &Path, content: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        create_dir(dir)?;
    }
    fs::write(path, content)
        .map_err(|e| EvenframeError::config(format!("Failed to write {}: {e}", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(
        output: &TypesyncOutput,
        dir: &Path,
        file: Option<&Path>,
    ) -> Result<Vec<GeneratedFile>> {
        let registry = ForeignTypeRegistry::default();
        let types = OutputTypes {
            structs: &BTreeMap::new(),
            enums: &BTreeMap::new(),
            registry: &registry,
        };
        write_output(output, dir, file, &types)
    }

    #[test]
    fn single_file_name_comes_from_the_flag_then_the_output_then_the_kind() {
        let tmp = TempDir::new().unwrap();
        let mut output = TypesyncOutput::new(OutputKind::Arktype, "unused");
        let written = write(&output, tmp.path(), None).unwrap();
        assert_eq!(written[0].path, tmp.path().join("arktype.ts"));

        output.file = Some("schemas.ts".to_string());
        let written = write(&output, tmp.path(), None).unwrap();
        assert_eq!(written[0].path, tmp.path().join("schemas.ts"));

        let flag = tmp.path().join("from-flag.ts");
        let written = write(&output, tmp.path(), Some(&flag)).unwrap();
        assert_eq!(written[0].path, flag);
        assert!(flag.is_file());
    }

    #[test]
    fn per_file_output_rejects_a_file_name() {
        let tmp = TempDir::new().unwrap();
        let mut output = TypesyncOutput::new(OutputKind::Effect, "unused");
        output.files.mode = OutputMode::PerFile;
        output.file = Some("schemas.ts".to_string());
        let err = write(&output, tmp.path(), None).unwrap_err().to_string();
        assert!(err.contains("per-file"), "{err}");
    }
}
