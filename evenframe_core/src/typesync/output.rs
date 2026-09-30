//! Rendering and writing one configured output's files.

use crate::config::{ForeignTypeConfig, TsImport};
use crate::error::{EvenframeError, Result};
use crate::types::{ForeignTypeRegistry, StructConfig, TaggedUnion};
use crate::typesync::arktype::generate_arktype_type_string;
use crate::typesync::config::{OutputKind, OutputMode, TypesyncOutput};
use crate::typesync::effect::{generate_effect_schema_for_types, generate_effect_schema_string};
use crate::typesync::file_grouping::compute_file_grouping;
use crate::typesync::foreign_ts::{RECORD_LINK, Reading, foreign_types_used, import_lines};
use crate::typesync::import_resolver::{
    barrel_filename, format_imports, generate_barrel_file, import_specifier_suffix,
    resolve_imports, type_name_to_filename,
};
use convert_case::{Case, Casing};
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

/// An output's files, rendered in full so that every output can be rendered
/// before any is written: an output that fails leaves every file as it was.
pub struct RenderedOutput {
    kind: OutputKind,
    files: Vec<(PathBuf, String)>,
    per_file: Option<PerFileDir>,
}

/// The directory a per-file output owns, and the files it keeps there.
struct PerFileDir {
    dir: PathBuf,
    extension: String,
    keep: BTreeSet<String>,
}

impl RenderedOutput {
    /// Writes every file, first removing a per-file output's files that it
    /// no longer produces, such as a type that moved into another's file.
    pub fn write(&self) -> Result<Vec<GeneratedFile>> {
        if let Some(per_file) = &self.per_file {
            create_dir(&per_file.dir)?;
            remove_obsolete_files(per_file)?;
        }
        self.files
            .iter()
            .map(|(path, content)| {
                write_file(path, content)?;
                debug!("Written {}", path.display());
                Ok(GeneratedFile {
                    path: path.clone(),
                    bytes_written: content.len(),
                    kind: self.kind,
                })
            })
            .collect()
    }
}

/// Renders `output` for `dir`. A single-file output goes to `file` when
/// given, else to the output's own `file` in `dir`, else to its kind's
/// standard name there; a per-file output takes neither.
pub fn render_output(
    output: &TypesyncOutput,
    dir: &Path,
    file: Option<&Path>,
    types: &OutputTypes,
) -> Result<RenderedOutput> {
    check_foreign_mappings(output.kind, types.registry)?;
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
        return render_per_file(output, dir, types);
    }

    let path = file.unwrap_or_else(|| dir.join(output.kind.default_filename()));
    info!("Generating {} output to {}", output.kind, path.display());
    Ok(RenderedOutput {
        kind: output.kind,
        files: vec![(path, single_file_content(output, types)?)],
        per_file: None,
    })
}

/// Rejects a foreign type this output cannot name.
fn check_foreign_mappings(kind: OutputKind, registry: &ForeignTypeRegistry) -> Result<()> {
    let problems: Vec<String> = registry
        .configs()
        .iter()
        // The record link's own definition stands in for any output its
        // entry leaves out.
        .filter(|(name, _)| name.as_str() != crate::typesync::foreign_ts::RECORD_LINK)
        .filter_map(|(name, foreign)| {
            let missing = kind.missing_foreign_mappings(foreign);
            (!missing.is_empty()).then(|| format!("{name} (missing {})", missing.join(", ")))
        })
        .collect();
    if problems.is_empty() {
        return Ok(());
    }
    Err(EvenframeError::config(format!(
        "the `{kind}` output needs each foreign type to map to it: {}",
        problems.join("; ")
    )))
}

/// The import lines, each ending in a newline, for the foreign types
/// `type_names` use in an output whose mapping `import_of` reads, including a
/// configured `RecordLink`.
fn foreign_imports(
    type_names: &[String],
    types: &OutputTypes,
    import_of: impl for<'a> Fn(&'a ForeignTypeConfig) -> Option<&'a TsImport>,
) -> String {
    let used = foreign_types_used(
        type_names,
        types.structs,
        types.enums,
        types.registry,
        &Reading {
            struct_view: StructConfig::effective,
            enum_view: TaggedUnion::effective,
            expands_held_types: false,
        },
    );
    let record_link = used
        .record_link
        .then(|| types.registry.lookup(RECORD_LINK))
        .flatten();
    let imports = used
        .foreign
        .values()
        .copied()
        .chain(record_link)
        .filter_map(import_of);
    import_lines(imports)
        .into_iter()
        .map(|line| format!("{line}\n"))
        .collect()
}

fn single_file_content(output: &TypesyncOutput, types: &OutputTypes) -> Result<String> {
    let OutputTypes {
        structs,
        enums,
        registry,
    } = types;
    let all_types: Vec<String> = structs
        .values()
        .map(|struct_config| struct_config.struct_name.to_case(Case::Pascal))
        .chain(
            enums
                .values()
                .map(|tagged_union| tagged_union.enum_name.to_case(Case::Pascal)),
        )
        .collect();
    match output.kind {
        OutputKind::Arktype => Ok(format!(
            "import {{ scope }} from 'arktype';\n{}\n{}\nexport const validator = exported;\n",
            foreign_imports(&all_types, types, |foreign| foreign
                .arktype
                .as_ref()?
                .import
                .as_ref()),
            generate_arktype_type_string(structs, enums, registry)?
        )),
        OutputKind::Effect => Ok(format!(
            "import {{ Schema }} from \"effect\";\n{}\n{}",
            foreign_imports(&all_types, types, |foreign| foreign
                .effect
                .as_ref()?
                .import
                .as_ref()),
            generate_effect_schema_string(structs, enums, false, registry)?
        )),
        OutputKind::Macroforge => {
            #[cfg(feature = "macroforge")]
            let content = crate::typesync::macroforge::macro_import_lines(
                &all_types,
                structs,
                enums,
                &output.macros,
            )
            .map(|import_lines| {
                format!(
                    "{}{}",
                    import_lines
                        .iter()
                        .map(|line| format!("{line}\n"))
                        .collect::<String>(),
                    crate::typesync::macroforge::generate_macroforge_type_string(
                        structs,
                        enums,
                        output.files.array_style,
                        registry,
                    )
                )
            });
            #[cfg(not(feature = "macroforge"))]
            let content = Err(not_built(OutputKind::Macroforge));
            content
        }
        OutputKind::Flatbuffers => {
            #[cfg(feature = "flatbuffers")]
            let content = crate::typesync::flatbuffers::generate_flatbuffers_schema_string(
                structs,
                enums,
                output.namespace.as_deref(),
                registry,
            );
            #[cfg(not(feature = "flatbuffers"))]
            let content = Err(not_built(OutputKind::Flatbuffers));
            content
        }
        OutputKind::Protobuf => {
            #[cfg(feature = "protobuf")]
            let content = crate::typesync::protobuf::generate_protobuf_schema_string(
                structs,
                enums,
                output.package.as_deref(),
                output.import_validate,
                registry,
            );
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
fn render_per_file(
    output: &TypesyncOutput,
    dir: &Path,
    types: &OutputTypes,
) -> Result<RenderedOutput> {
    let OutputTypes {
        structs,
        enums,
        registry,
    } = types;
    let settings = &output.files;
    let plan = compute_file_grouping(structs, enums);
    #[cfg(feature = "macroforge")]
    let record_link = record_link_module(output, &plan, types)?;
    #[cfg(not(feature = "macroforge"))]
    let record_link: Option<String> = None;
    let mut keep: BTreeSet<String> = plan
        .groups
        .iter()
        .map(|group| {
            format!(
                "{}{}",
                type_name_to_filename(&group.primary_type, settings.file_naming),
                settings.file_extension
            )
        })
        .collect();
    keep.insert(barrel_filename(&settings.file_extension));
    if let Some(module) = &record_link {
        keep.insert(format!("{module}{}", settings.file_extension));
    }
    info!(
        "Generating {} output (per-file) to {} ({} files)",
        output.kind,
        dir.display(),
        plan.groups.len()
    );

    let mut files = Vec::new();
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
                content.push_str(&foreign_imports(&type_names, types, |foreign| {
                    foreign.effect.as_ref()?.import.as_ref()
                }));
                push_imports(&mut content, &format_imports(&imports));
                content.push('\n');
                content.push_str(&generate_effect_schema_for_types(
                    &type_names,
                    structs,
                    enums,
                    registry,
                )?);
            }
            OutputKind::Macroforge => {
                #[cfg(feature = "macroforge")]
                macroforge_per_file_content(
                    &mut content,
                    &type_names,
                    &imports,
                    output,
                    types,
                    record_link.as_deref(),
                )?;
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
        files.push((
            dir.join(format!("{filename}{}", settings.file_extension)),
            content,
        ));
    }

    #[cfg(feature = "macroforge")]
    if let Some(module) = &record_link {
        files.push((
            dir.join(format!("{module}{}", settings.file_extension)),
            format!("{}\n", crate::typesync::macroforge::RECORD_LINK_TYPE),
        ));
    }

    if settings.barrel_file {
        let mut content = generate_barrel_file(
            &plan,
            settings.file_naming,
            &settings.file_extension,
            settings.import_extension,
        );
        if let Some(module) = &record_link {
            let suffix =
                import_specifier_suffix(&settings.file_extension, settings.import_extension);
            content.push_str(&format!("\nexport * from \"./{module}{suffix}\";"));
        }
        files.push((dir.join(barrel_filename(&settings.file_extension)), content));
    }
    Ok(RenderedOutput {
        kind: output.kind,
        files,
        per_file: Some(PerFileDir {
            dir: dir.to_path_buf(),
            extension: settings.file_extension.clone(),
            keep,
        }),
    })
}

/// The module a per-file macroforge output declares `RecordLink` in, when
/// any of its types uses one.
#[cfg(feature = "macroforge")]
fn record_link_module(
    output: &TypesyncOutput,
    plan: &crate::typesync::file_grouping::FileOutputPlan,
    types: &OutputTypes,
) -> Result<Option<String>> {
    if output.kind != OutputKind::Macroforge {
        return Ok(None);
    }
    let all_types: Vec<String> = plan
        .groups
        .iter()
        .flat_map(|group| group.all_types())
        .collect();
    let extras = crate::typesync::macroforge::compute_extra_imports(
        &all_types,
        types.structs,
        types.enums,
        types.registry,
    );
    if !extras.needs_record_link {
        return Ok(None);
    }
    if all_types.iter().any(|name| name == "RecordLink") {
        return Err(EvenframeError::config(
            "a generated type named `RecordLink` collides with the record link type the macroforge output declares",
        ));
    }
    Ok(Some(type_name_to_filename(
        "RecordLink",
        output.files.file_naming,
    )))
}

#[cfg(feature = "macroforge")]
fn macroforge_per_file_content(
    content: &mut String,
    type_names: &[String],
    imports: &[crate::typesync::import_resolver::ImportStatement],
    output: &TypesyncOutput,
    types: &OutputTypes,
    record_link: Option<&str>,
) -> Result<()> {
    use crate::typesync::macroforge::{
        compute_extra_imports, generate_macroforge_for_types, macro_import_lines,
    };
    for import_line in macro_import_lines(type_names, types.structs, types.enums, &output.macros)? {
        content.push_str(&import_line);
        content.push('\n');
    }
    let extras = compute_extra_imports(type_names, types.structs, types.enums, types.registry);
    for import_line in &extras.lines {
        content.push_str(import_line);
        content.push('\n');
    }
    if extras.needs_record_link {
        let module = record_link.ok_or_else(|| {
            EvenframeError::config("a type uses RecordLink but no RecordLink module was planned")
        })?;
        let suffix =
            import_specifier_suffix(&output.files.file_extension, output.files.import_extension);
        content.push_str(&format!(
            "import type {{ RecordLink }} from './{module}{suffix}';\n"
        ));
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
    Ok(())
}

fn push_imports(content: &mut String, import_lines: &str) {
    if !import_lines.is_empty() {
        content.push_str(import_lines);
        content.push('\n');
    }
}

/// Removes the files with a per-file output's extension in its directory that
/// it does not keep.
fn remove_obsolete_files(per_file: &PerFileDir) -> Result<()> {
    let PerFileDir {
        dir,
        extension,
        keep,
    } = per_file;
    let unreadable = |e: std::io::Error| {
        EvenframeError::config(format!("Failed to read {}: {e}", dir.display()))
    };
    for entry in fs::read_dir(dir).map_err(unreadable)? {
        let path = entry.map_err(unreadable)?.path();
        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if file_name.ends_with(extension.as_str()) && !keep.contains(&file_name) {
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
        render_output(output, dir, file, &types)?.write()
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
    fn rendering_writes_nothing_until_written() {
        let tmp = TempDir::new().unwrap();
        let registry = ForeignTypeRegistry::default();
        let types = OutputTypes {
            structs: &BTreeMap::new(),
            enums: &BTreeMap::new(),
            registry: &registry,
        };
        let mut per_file = TypesyncOutput::new(OutputKind::Effect, "unused");
        per_file.files.mode = OutputMode::PerFile;
        per_file.files.barrel_file = true;
        let single = TypesyncOutput::new(OutputKind::Arktype, "unused");
        let rendered = [
            render_output(&single, tmp.path(), None, &types).unwrap(),
            render_output(&per_file, &tmp.path().join("effect"), None, &types).unwrap(),
        ];
        assert_eq!(fs::read_dir(tmp.path()).unwrap().count(), 0);

        for output in &rendered {
            output.write().unwrap();
        }
        assert!(tmp.path().join("arktype.ts").is_file());
        assert!(tmp.path().join("effect/index.ts").is_file());
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

    #[test]
    fn an_output_rejects_foreign_types_that_do_not_map_to_it() {
        let tmp = TempDir::new().unwrap();
        let foreign: crate::config::ForeignTypeConfig = toml::from_str(
            "rust_type_names = [\"DateTime\"]\neffect = { type = \"Schema.Date\", encoded = \"string\" }",
        )
        .unwrap();
        let registry =
            ForeignTypeRegistry::from_config(&BTreeMap::from([("DateTime".to_string(), foreign)]));
        let types = OutputTypes {
            structs: &BTreeMap::new(),
            enums: &BTreeMap::new(),
            registry: &registry,
        };

        let arktype = TypesyncOutput::new(OutputKind::Arktype, "unused");
        let error = render_output(&arktype, tmp.path(), None, &types)
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(
            error.contains("DateTime (missing arktype, default_value_ts)"),
            "{error}"
        );

        let effect = TypesyncOutput::new(OutputKind::Effect, "unused");
        assert!(render_output(&effect, tmp.path(), None, &types).is_ok());
    }

    #[cfg(feature = "macroforge")]
    #[test]
    fn per_file_macroforge_declares_record_link_in_its_own_module() {
        use crate::types::{FieldType, StructConfig, StructField};
        let tmp = TempDir::new().unwrap();
        let mut output = TypesyncOutput::new(OutputKind::Macroforge, "unused");
        output.files.mode = OutputMode::PerFile;
        output.files.barrel_file = true;
        let post = StructConfig {
            struct_name: "Post".to_string(),
            fields: vec![StructField {
                field_name: "author".to_string(),
                field_type: FieldType::RecordLink(Box::new(FieldType::String)),
                ..Default::default()
            }],
            ..Default::default()
        };
        let registry = ForeignTypeRegistry::default();
        let types = OutputTypes {
            structs: &BTreeMap::from([("Post".to_string(), post)]),
            enums: &BTreeMap::new(),
            registry: &registry,
        };
        render_output(&output, tmp.path(), None, &types)
            .unwrap()
            .write()
            .unwrap();

        let module = fs::read_to_string(tmp.path().join("record-link.ts")).unwrap();
        assert_eq!(module.trim(), crate::typesync::macroforge::RECORD_LINK_TYPE);
        let post_file = fs::read_to_string(tmp.path().join("post.ts")).unwrap();
        assert!(
            post_file.contains("import type { RecordLink } from './record-link';"),
            "{post_file}"
        );
        let barrel = fs::read_to_string(tmp.path().join("index.ts")).unwrap();
        assert!(
            barrel.contains("export * from \"./record-link\";"),
            "{barrel}"
        );
    }
}
