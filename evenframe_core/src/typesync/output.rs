//! Rendering and writing one configured output's files.

use crate::config::RECORD_LINK;
#[cfg(any(feature = "arktype", feature = "effect"))]
use crate::config::{ForeignTypeConfig, TsOutputMapping};
use crate::error::{EvenframeError, Result};
use crate::types::{ForeignTypeRegistry, StructConfig, TaggedUnion};
#[cfg(feature = "arktype")]
use crate::typesync::arktype::generate_arktype_type_string;
use crate::typesync::config::{OutputKind, OutputMode, TypesyncOutput};
#[cfg(feature = "effect")]
use crate::typesync::effect::{generate_effect_schema_for_types, generate_effect_schema_string};
use crate::typesync::file_grouping::TypeFileGroup;
#[cfg(any(feature = "arktype", feature = "effect"))]
use crate::typesync::foreign_ts::{
    Reading, RecordLinkMapping, foreign_types_used, import_lines, record_link_mapping,
};
#[cfg(feature = "effect")]
use crate::typesync::import_resolver::format_effect_imports;
#[cfg(any(feature = "effect", feature = "macroforge"))]
use crate::typesync::import_resolver::resolve_imports;
use crate::typesync::import_resolver::{
    barrel_filename, generate_barrel_file, import_specifier_suffix, type_name_to_filename,
};
use crate::typesync::type_index::TypeIndex;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// The types an output is generated from, indexed once for every output.
pub struct OutputTypes<'a> {
    pub index: TypeIndex<'a>,
    pub registry: &'a ForeignTypeRegistry,
}

impl<'a> OutputTypes<'a> {
    pub fn new(
        structs: &'a BTreeMap<String, StructConfig>,
        enums: &'a BTreeMap<String, TaggedUnion>,
        registry: &'a ForeignTypeRegistry,
    ) -> Result<Self> {
        Ok(Self {
            index: TypeIndex::new(structs, enums)?,
            registry,
        })
    }
}

/// A file an output produced. `changed` is false when the file already held
/// these bytes and was left untouched.
#[derive(Debug, Clone)]
pub struct GeneratedFile {
    pub path: PathBuf,
    pub bytes: usize,
    pub changed: bool,
    pub kind: OutputKind,
}

/// An output's files, rendered in full so that every output can be rendered
/// before any is written: an output that fails leaves every file as it was.
pub struct RenderedOutput {
    kind: OutputKind,
    files: Vec<(PathBuf, String)>,
    per_file: Option<PerFileDir>,
    /// Files this output owns but no longer produces, removed when present.
    removed: Vec<PathBuf>,
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
        for path in &self.removed {
            remove_owned_file(path)?;
        }
        self.files
            .iter()
            .map(|(path, content)| {
                let changed = write_file(path, content)?;
                debug!(changed, "Generated {}", path.display());
                Ok(GeneratedFile {
                    path: path.clone(),
                    bytes: content.len(),
                    changed,
                    kind: self.kind,
                })
            })
            .collect()
    }
}

/// Renders each output for its directory, in parallel. Every output renders
/// before any is written, so one that fails leaves every file as it was.
/// `file` is passed to each, as [`render_output`] takes it.
pub fn render_outputs(
    outputs: &[(&TypesyncOutput, PathBuf)],
    file: Option<&Path>,
    types: &OutputTypes,
) -> Result<Vec<RenderedOutput>> {
    outputs
        .par_iter()
        .map(|(output, dir)| render_output(output, dir, file, types))
        .collect()
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
    single_file(output, types, path)
}

/// A single-file macroforge output, and beside it the helpers module its
/// validators name, `<file>-helpers`, which is removed when nothing uses it.
#[cfg(feature = "macroforge")]
fn macroforge_single_file(
    output: &TypesyncOutput,
    types: &OutputTypes,
    path: PathBuf,
) -> Result<RenderedOutput> {
    use crate::typesync::macroforge::{
        HelperModule, generate_macroforge_type_string, macro_import_lines,
    };
    let settings = &output.files;
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = file_name
        .strip_suffix(settings.file_extension.as_str())
        .map(str::to_owned)
        .or_else(|| {
            path.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    let helpers_name = format!("{stem}-helpers");
    let helpers_path = path.with_file_name(format!("{helpers_name}{}", settings.file_extension));
    let suffix = import_specifier_suffix(&settings.file_extension, settings.import_extension);
    let mut helpers = HelperModule::new(format!("./{helpers_name}{suffix}"));

    let all_types: Vec<String> = types.index.names().cloned().collect();
    let mut content: String = macro_import_lines(&all_types, &types.index, &output.macros)?
        .iter()
        .map(|line| format!("{line}\n"))
        .collect();
    content.push_str(&generate_macroforge_type_string(
        &types.index,
        settings.array_style,
        types.registry,
        &mut helpers,
    )?);
    let mut files = vec![(path, content)];
    let mut removed = Vec::new();
    if helpers.is_empty() {
        removed.push(helpers_path);
    } else {
        files.push((helpers_path, helpers.content()));
    }
    Ok(RenderedOutput {
        kind: output.kind,
        files,
        per_file: None,
        removed,
    })
}

/// Rejects a foreign type this output cannot name.
fn check_foreign_mappings(kind: OutputKind, registry: &ForeignTypeRegistry) -> Result<()> {
    let problems: Vec<String> = registry
        .configs()
        .iter()
        // The record link's own definition stands in for any output its
        // entry leaves out.
        .filter(|(name, _)| name.as_str() != RECORD_LINK)
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
/// `type_names` use in `output`, whose mapping of a foreign type `mapping`
/// reads, including what their record links need.
#[cfg(any(feature = "arktype", feature = "effect"))]
fn foreign_imports<M: TsOutputMapping>(
    type_names: &[String],
    types: &OutputTypes,
    output: OutputKind,
    mapping: impl for<'a> Fn(&'a ForeignTypeConfig) -> Option<&'a M>,
) -> Result<String> {
    let used = foreign_types_used(
        type_names,
        &types.index,
        types.registry,
        &Reading {
            struct_view: StructConfig::effective,
            enum_view: TaggedUnion::effective,
            expands_held_types: false,
        },
    );
    let record_link = if used.record_link {
        Some(
            match record_link_mapping(types.registry, output, &mapping)? {
                RecordLinkMapping::Configured(configured) => configured,
                RecordLinkMapping::Own { record_id } => record_id,
            },
        )
    } else {
        None
    };
    let imports = used
        .foreign
        .values()
        .filter_map(|foreign| mapping(foreign))
        .chain(record_link)
        .filter_map(TsOutputMapping::import);
    Ok(import_lines(imports)
        .into_iter()
        .map(|line| format!("{line}\n"))
        .collect())
}

/// A single-file output's file at `path`, with any module it writes beside it.
fn single_file(
    output: &TypesyncOutput,
    types: &OutputTypes,
    path: PathBuf,
) -> Result<RenderedOutput> {
    #[cfg(any(feature = "arktype", feature = "effect"))]
    let all_types: Vec<String> = types.index.names().cloned().collect();
    let content = match output.kind {
        OutputKind::Arktype => {
            #[cfg(not(feature = "arktype"))]
            return Err(not_built(OutputKind::Arktype, types));
            #[cfg(feature = "arktype")]
            {
                Ok(format!(
                    "import {{ scope }} from 'arktype';\n{}\n{}\nexport const validator = exported;\n",
                    foreign_imports(&all_types, types, OutputKind::Arktype, |foreign| foreign
                        .arktype
                        .as_ref())?,
                    generate_arktype_type_string(&types.index, types.registry)?
                ))
            }
        }
        OutputKind::Effect => {
            #[cfg(not(feature = "effect"))]
            return Err(not_built(OutputKind::Effect, types));
            #[cfg(feature = "effect")]
            {
                Ok(format!(
                    "import {{ Schema }} from \"effect\";\n{}\n{}",
                    foreign_imports(&all_types, types, OutputKind::Effect, |foreign| foreign
                        .effect
                        .as_ref())?,
                    generate_effect_schema_string(&types.index, false, types.registry)?
                ))
            }
        }
        OutputKind::Macroforge => {
            #[cfg(feature = "macroforge")]
            return macroforge_single_file(output, types, path);
            #[cfg(not(feature = "macroforge"))]
            return Err(not_built(OutputKind::Macroforge, types));
        }
        OutputKind::Flatbuffers => {
            #[cfg(feature = "flatbuffers")]
            let content = crate::typesync::flatbuffers::generate_flatbuffers_schema_string(
                types.index.structs(),
                types.index.enums(),
                output.namespace.as_deref(),
                types.registry,
            );
            #[cfg(not(feature = "flatbuffers"))]
            let content = Err(not_built(OutputKind::Flatbuffers, types));
            content
        }
        OutputKind::Protobuf => {
            #[cfg(feature = "protobuf")]
            let content = crate::typesync::protobuf::generate_protobuf_schema_string(
                types.index.structs(),
                types.index.enums(),
                output.package.as_deref(),
                output.import_validate,
                types.registry,
            );
            #[cfg(not(feature = "protobuf"))]
            let content = Err(not_built(OutputKind::Protobuf, types));
            content
        }
    }?;
    Ok(RenderedOutput {
        kind: output.kind,
        files: vec![(path, content)],
        per_file: None,
        removed: Vec::new(),
    })
}

#[cfg(not(all(
    feature = "arktype",
    feature = "effect",
    feature = "macroforge",
    feature = "flatbuffers",
    feature = "protobuf"
)))]
fn not_built(kind: OutputKind, types: &OutputTypes) -> EvenframeError {
    EvenframeError::config(format!(
        "this evenframe was built without the `{kind}` feature, so it cannot write `{kind}` outputs for {} types",
        types.index.names().count()
    ))
}

/// One file per primary type (with its exclusive dependents), plus an
/// optional barrel file, all directly in `dir`.
fn render_per_file(
    output: &TypesyncOutput,
    dir: &Path,
    types: &OutputTypes,
) -> Result<RenderedOutput> {
    let index = &types.index;
    let settings = &output.files;
    let plan = index.file_plan();
    #[cfg(feature = "macroforge")]
    let record_link = record_link_module(output, plan, types)?;
    #[cfg(not(feature = "macroforge"))]
    let record_link: Option<RecordLinkModule> = None;
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
        keep.insert(format!("{}{}", module.name, settings.file_extension));
    }
    info!(
        "Generating {} output (per-file) to {} ({} files)",
        output.kind,
        dir.display(),
        plan.groups.len()
    );

    #[cfg(feature = "macroforge")]
    let helpers_name = type_name_to_filename("Helpers", settings.file_naming);
    #[cfg(feature = "macroforge")]
    if plan.groups.iter().any(|group| {
        type_name_to_filename(&group.primary_type, settings.file_naming) == helpers_name
    }) {
        return Err(EvenframeError::config(format!(
            "a generated type's file `{helpers_name}{}` collides with the helpers module the \
             macroforge output writes",
            settings.file_extension
        )));
    }
    #[cfg(feature = "macroforge")]
    let helpers_source = format!(
        "./{helpers_name}{}",
        import_specifier_suffix(&settings.file_extension, settings.import_extension)
    );

    let render_group = |group: &TypeFileGroup| -> Result<RenderedGroup> {
        #[cfg(any(feature = "effect", feature = "macroforge"))]
        let imports = resolve_imports(
            group,
            plan,
            index,
            settings.file_naming,
            &settings.file_extension,
            settings.import_extension,
        );
        #[cfg(any(feature = "effect", feature = "macroforge"))]
        let type_names = group.all_types();
        #[cfg(any(feature = "effect", feature = "macroforge"))]
        let mut content = String::new();
        #[cfg(not(any(feature = "effect", feature = "macroforge")))]
        let content = String::new();
        #[cfg(feature = "macroforge")]
        let mut helpers = crate::typesync::macroforge::HelperModule::new(helpers_source.clone());
        match output.kind {
            OutputKind::Effect => {
                #[cfg(not(feature = "effect"))]
                Err::<(), _>(not_built(OutputKind::Effect, types))?;
                #[cfg(feature = "effect")]
                {
                    content.push_str("import { Schema } from \"effect\";\n");
                    content.push_str(&foreign_imports(
                        &type_names,
                        types,
                        OutputKind::Effect,
                        |foreign| foreign.effect.as_ref(),
                    )?);
                    push_imports(&mut content, &format_effect_imports(&imports));
                    content.push('\n');
                    content.push_str(&generate_effect_schema_for_types(
                        &type_names,
                        index,
                        types.registry,
                    )?);
                }
            }
            OutputKind::Macroforge => {
                #[cfg(feature = "macroforge")]
                macroforge_per_file_content(
                    &mut content,
                    &type_names,
                    &imports,
                    output,
                    types,
                    record_link.as_ref().map(|module| module.name.as_str()),
                    &mut helpers,
                )?;
                #[cfg(not(feature = "macroforge"))]
                Err::<(), _>(not_built(OutputKind::Macroforge, types))?;
            }
            kind => {
                return Err(EvenframeError::config(format!(
                    "the `{kind}` output cannot be written per-file"
                )));
            }
        }
        let filename = type_name_to_filename(&group.primary_type, settings.file_naming);
        Ok(RenderedGroup {
            file: (
                dir.join(format!("{filename}{}", settings.file_extension)),
                content,
            ),
            #[cfg(feature = "macroforge")]
            helpers,
        })
    };
    let groups = plan
        .groups
        .par_iter()
        .map(render_group)
        .collect::<Result<Vec<_>>>()?;
    #[cfg(feature = "macroforge")]
    let mut all_helpers = crate::typesync::macroforge::HelperModule::new(helpers_source);
    let mut files = Vec::with_capacity(groups.len());
    for group in groups {
        files.push(group.file);
        #[cfg(feature = "macroforge")]
        all_helpers.merge(group.helpers);
    }
    #[cfg(feature = "macroforge")]
    if !all_helpers.is_empty() {
        let helpers_file = format!("{helpers_name}{}", settings.file_extension);
        keep.insert(helpers_file.clone());
        files.push((dir.join(helpers_file), all_helpers.content()));
    }

    if let Some(module) = record_link.as_ref() {
        files.push((
            dir.join(format!("{}{}", module.name, settings.file_extension)),
            module.content.clone(),
        ));
    }

    if settings.barrel_file {
        let mut content = generate_barrel_file(
            plan,
            settings.file_naming,
            &settings.file_extension,
            settings.import_extension,
        );
        if let Some(module) = &record_link {
            let suffix =
                import_specifier_suffix(&settings.file_extension, settings.import_extension);
            content.push_str(&format!("\nexport * from \"./{}{suffix}\";", module.name));
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
        removed: Vec::new(),
    })
}

/// One per-file group's file, and the helpers its macroforge types name.
struct RenderedGroup {
    file: (PathBuf, String),
    #[cfg(feature = "macroforge")]
    helpers: crate::typesync::macroforge::HelperModule,
}

/// The module a per-file macroforge output declares evenframe's own
/// `RecordLink` in: its file name, without the extension, and its content.
struct RecordLinkModule {
    name: String,
    content: String,
}

/// The module a per-file macroforge output declares `RecordLink` in, when
/// any of its types uses one the project does not configure.
#[cfg(feature = "macroforge")]
fn record_link_module(
    output: &TypesyncOutput,
    plan: &crate::typesync::file_grouping::FileOutputPlan,
    types: &OutputTypes,
) -> Result<Option<RecordLinkModule>> {
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
        &types.index,
        types.registry,
        false,
    )?;
    let Some(own_record_link) = extras.own_record_link else {
        return Ok(None);
    };
    if all_types.iter().any(|name| name == "RecordLink") {
        return Err(EvenframeError::config(
            "a generated type named `RecordLink` collides with the record link type the macroforge output declares",
        ));
    }
    Ok(Some(RecordLinkModule {
        name: type_name_to_filename("RecordLink", output.files.file_naming),
        content: own_record_link.module(),
    }))
}

#[cfg(feature = "macroforge")]
fn macroforge_per_file_content(
    content: &mut String,
    type_names: &[String],
    imports: &[crate::typesync::import_resolver::ImportStatement],
    output: &TypesyncOutput,
    types: &OutputTypes,
    record_link: Option<&str>,
    helpers: &mut crate::typesync::macroforge::HelperModule,
) -> Result<()> {
    use crate::typesync::import_resolver::format_imports;
    use crate::typesync::macroforge::{
        compute_extra_imports, generate_macroforge_for_types, macro_import_lines,
    };
    for import_line in macro_import_lines(type_names, &types.index, &output.macros)? {
        content.push_str(&import_line);
        content.push('\n');
    }
    let extras = compute_extra_imports(type_names, &types.index, types.registry, false)?;
    for import_line in &extras.lines {
        content.push_str(import_line);
        content.push('\n');
    }
    if extras.own_record_link.is_some() {
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
        &types.index,
        output.files.array_style,
        types.registry,
        helpers,
    )?);
    Ok(())
}

#[cfg(any(feature = "effect", feature = "macroforge"))]
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
    let unreadable = |error: std::io::Error| {
        EvenframeError::config(format!("Failed to read {}: {error}", dir.display()))
    };
    for entry in fs::read_dir(dir).map_err(unreadable)? {
        let path = entry.map_err(unreadable)?.path();
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        if file_name.ends_with(extension.as_str()) && !keep.contains(&file_name) {
            info!("Removing obsolete file: {}", path.display());
            fs::remove_file(&path).map_err(|error| {
                EvenframeError::config(format!("Failed to remove {}: {error}", path.display()))
            })?;
        }
    }
    Ok(())
}

/// Removes a file this output owns and no longer writes, if it exists.
fn remove_owned_file(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => {
            info!("Removing obsolete file: {}", path.display());
            Ok(())
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(EvenframeError::config(format!(
            "Failed to remove {}: {error}",
            path.display()
        ))),
    }
}

fn create_dir(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir).map_err(|error| {
        EvenframeError::config(format!(
            "Failed to create output directory {}: {error}",
            dir.display()
        ))
    })
}

/// Writes one file, creating its directory first, unless it already holds
/// `content`: an untouched file keeps its mtime, so file watchers do not
/// rebuild on it. Returns whether the file was written.
fn write_file(path: &Path, content: &str) -> Result<bool> {
    match fs::read(path) {
        Ok(existing) if existing == content.as_bytes() => return Ok(false),
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => {
            return Err(EvenframeError::config(format!(
                "Failed to read {}: {error}",
                path.display()
            )));
        }
    }
    if let Some(dir) = path.parent() {
        create_dir(dir)?;
    }
    fs::write(path, content).map_err(|error| {
        EvenframeError::config(format!("Failed to write {}: {error}", path.display()))
    })?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::{
        BTreeMap, ForeignTypeRegistry, GeneratedFile, OutputKind, OutputMode, OutputTypes, Path,
        Result, TypesyncOutput, fs, render_output,
    };
    use tempfile::TempDir;

    fn write(
        output: &TypesyncOutput,
        dir: &Path,
        file: Option<&Path>,
    ) -> Result<Vec<GeneratedFile>> {
        let registry = ForeignTypeRegistry::default();
        let (structs, enums) = (BTreeMap::new(), BTreeMap::new());
        let types = OutputTypes::new(&structs, &enums, &registry).unwrap();
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
    fn an_unchanged_file_is_left_untouched() {
        let tmp = TempDir::new().unwrap();
        let output = TypesyncOutput::new(OutputKind::Arktype, "unused");
        let first = write(&output, tmp.path(), None).unwrap();
        assert!(first[0].changed);
        let modified = fs::metadata(&first[0].path).unwrap().modified().unwrap();

        let second = write(&output, tmp.path(), None).unwrap();
        assert!(!second[0].changed);
        let untouched = fs::metadata(&second[0].path).unwrap().modified().unwrap();
        assert_eq!(untouched, modified);

        fs::write(&first[0].path, "stale").unwrap();
        assert!(write(&output, tmp.path(), None).unwrap()[0].changed);
    }

    #[test]
    fn rendering_writes_nothing_until_written() {
        let tmp = TempDir::new().unwrap();
        let registry = ForeignTypeRegistry::default();
        let (structs, enums) = (BTreeMap::new(), BTreeMap::new());
        let types = OutputTypes::new(&structs, &enums, &registry).unwrap();
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
        let (structs, enums) = (BTreeMap::new(), BTreeMap::new());
        let types = OutputTypes::new(&structs, &enums, &registry).unwrap();

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
    fn post_linking_an_author() -> BTreeMap<String, crate::types::StructConfig> {
        use crate::types::{FieldType, StructConfig, StructField};
        let post = StructConfig {
            struct_name: "Post".to_string(),
            fields: vec![StructField {
                field_name: "author".to_string(),
                field_type: FieldType::RecordLink(Box::new(FieldType::String)),
                ..Default::default()
            }],
            ..Default::default()
        };
        BTreeMap::from([("Post".to_string(), post)])
    }

    #[cfg(feature = "macroforge")]
    #[test]
    fn per_file_macroforge_declares_record_link_in_its_own_module() {
        let tmp = TempDir::new().unwrap();
        let mut output = TypesyncOutput::new(OutputKind::Macroforge, "unused");
        output.files.mode = OutputMode::PerFile;
        output.files.barrel_file = true;
        let record_id: crate::config::ForeignTypeConfig = toml::from_str(
            "macroforge = { type = \"RecordIdEncoded\", import = { from = \"../record-id.ts\", name = \"RecordIdEncoded\" } }",
        )
        .unwrap();
        let registry = ForeignTypeRegistry::from_config(&BTreeMap::from([(
            "RecordId".to_string(),
            record_id,
        )]));
        let (structs, enums) = (post_linking_an_author(), BTreeMap::new());
        let types = OutputTypes::new(&structs, &enums, &registry).unwrap();
        render_output(&output, tmp.path(), None, &types)
            .unwrap()
            .write()
            .unwrap();

        let module = fs::read_to_string(tmp.path().join("record-link.ts")).unwrap();
        assert_eq!(
            module,
            "import type { RecordIdEncoded } from '../record-id.ts';\n\n\
             export type RecordLink<T> = RecordIdEncoded | T;\n"
        );
        let post_file = fs::read_to_string(tmp.path().join("post.ts")).unwrap();
        assert!(
            post_file.contains("import type { RecordLink } from './record-link';"),
            "{post_file}"
        );
        assert!(!post_file.contains("RecordIdEncoded"), "{post_file}");
        let barrel = fs::read_to_string(tmp.path().join("index.ts")).unwrap();
        assert!(
            barrel.contains("export * from \"./record-link\";"),
            "{barrel}"
        );
    }

    #[cfg(feature = "macroforge")]
    #[test]
    fn a_record_link_needs_a_record_id_mapping() {
        let tmp = TempDir::new().unwrap();
        let registry = ForeignTypeRegistry::default();
        let (structs, enums) = (post_linking_an_author(), BTreeMap::new());
        let types = OutputTypes::new(&structs, &enums, &registry).unwrap();
        for kind in [
            OutputKind::Arktype,
            OutputKind::Effect,
            OutputKind::Macroforge,
        ] {
            let error = render_output(
                &TypesyncOutput::new(kind, "unused"),
                tmp.path(),
                None,
                &types,
            )
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
            assert!(error.contains("foreign_types.RecordId"), "{kind}: {error}");
        }
    }
}
