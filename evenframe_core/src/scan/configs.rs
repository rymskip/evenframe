//! Configuration builders for processing Evenframe types.

use super::paths::{Target, TypePaths, rust_path};
use super::workspace::{Scan, ScannedAst, ScannedItem};
use super::{EvenframeType, ScanConfig, WorkspaceScanner};
use crate::error::{EvenframeError, Result};
use crate::{
    derive::{
        attributes::{
            find_duplicate_index_name, indexable_fields, parse_doccom_attribute,
            parse_event_attributes, parse_field_index_attributes, parse_format_attribute_bin,
            parse_index_attributes, parse_mock_data_attribute, parse_relation_attribute,
            parse_rust_derives,
        },
        naming,
        schemasync_attributes::{
            parse_container_validator_overrides, parse_validator_overrides,
            refuse_container_validators,
        },
        typesync_attributes::{Position, TypesyncAttributes},
        validator_parser::{
            ElementValidators, FieldValidators, parse_element_validators, parse_field_validators,
        },
    },
    schemasync::mockmake::MockGenerationConfig,
    schemasync::table::TableConfig,
    schemasync::{DefineConfig, EdgeConfig, EventConfig, IndexConfig, PermissionsConfig},
    types::{
        AllConfigs, FieldType, ForeignTypeRegistry, NewtypeConfig, NewtypeKind, PathNames,
        STD_DURATION_PATHS, StructConfig, StructField, TaggedUnion, Variant, VariantData, Wire,
    },
    typesync::config::{CollisionStrategy, StructVariants},
    typesync::struct_variants::declare_payloads,
    validator::ValidatorOverrides,
};
use convert_case::{Case, Casing};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use syn::{Fields, FieldsNamed, ItemEnum, ItemStruct, spanned::Spanned};
use tracing::{debug, info, trace, warn};

/// Builds all configurations from the workspace using the provided build config.
pub fn build_all_configs(config: &ScanConfig) -> Result<AllConfigs> {
    debug!("Starting build_all_configs");
    let mut enum_configs = BTreeMap::new();
    let mut table_configs = BTreeMap::new();
    let mut struct_configs = BTreeMap::new();
    let mut newtype_configs = BTreeMap::new();

    debug!("Creating workspace scanner");
    let scanner = WorkspaceScanner::with_path(
        config.scan_path.clone(),
        config.apply_aliases.clone(),
        config.expand_macros,
    )
    .with_extra_files(config.include_files.clone())
    .with_excluded_files(config.exclude_files.clone());

    let scanned = scanner.scan()?;
    info!("Found {} Evenframe types", scanned.items.len());

    process_types(
        scanned,
        &mut enum_configs,
        &mut table_configs,
        &mut struct_configs,
        &mut newtype_configs,
        config.collision_strategy,
        &ForeignTypeRegistry::from_config(&config.foreign_types),
    )?;

    info!(
        "First pass complete. Found {} struct configs, {} enum configs, {} table configs",
        struct_configs.len(),
        enum_configs.len(),
        table_configs.len()
    );

    reject_scanned_record_link(&struct_configs, &enum_configs)?;
    reject_scanned_text_forms(&struct_configs, &enum_configs, &newtype_configs)?;

    // Before the plugins, so a named payload gets the rules any struct gets.
    if config.struct_variants == StructVariants::Named {
        declare_payloads(&mut struct_configs, &mut enum_configs)?;
    }

    // Apply output rule plugins to enrich configs with convention-based defaults
    #[cfg(feature = "wasm-plugins")]
    {
        info!(
            "Output rule plugins configured: {}",
            config.output_rule_plugins.len()
        );
        for (name, cfg) in &config.output_rule_plugins {
            debug!("  Output rule plugin '{}' -> {}", name, cfg.path);
        }
    }
    #[cfg(not(feature = "wasm-plugins"))]
    if !config.output_rule_plugins.is_empty() || !config.synthetic_item_plugins.is_empty() {
        return Err(crate::error::EvenframeError::config(
            "[general] configures output-rule or synthetic-item plugins, but this evenframe was \
             built without the `wasm-plugins` feature",
        ));
    }
    #[cfg(feature = "wasm-plugins")]
    if !config.output_rule_plugins.is_empty() {
        apply_rule_plugins(
            config,
            &mut table_configs,
            &mut struct_configs,
            &mut enum_configs,
            &mut newtype_configs,
        )?;
    }

    // Apply synthetic-item plugins. These add new structs/enums/tables
    // derived from the (now finalized) scanner+rule-plugin state.
    #[cfg(feature = "wasm-plugins")]
    if !config.synthetic_item_plugins.is_empty() {
        info!(
            "Synthetic-item plugins configured: {}",
            config.synthetic_item_plugins.len()
        );
        apply_synthetic_plugins(
            config,
            &mut table_configs,
            &mut struct_configs,
            &mut enum_configs,
        )?;
    }

    // Resolve relation `from`/`to` from `in`/`out` field types. Done once,
    // after every plugin has finished mutating the configs, so resolution
    // sees the final picture (synthetic projections registered, output_rule
    // overrides applied).
    resolve_relation_endpoints(&mut table_configs, &enum_configs, &struct_configs);

    for struct_config in struct_configs.values_mut().chain(
        table_configs
            .values_mut()
            .map(|table| &mut table.struct_config),
    ) {
        crate::typesync::naming::apply_struct(struct_config, config.ts_names).map_err(|error| {
            EvenframeError::type_sync(format!(
                "naming struct '{}': {error}",
                struct_config.struct_name
            ))
        })?;
    }
    for tagged_union in enum_configs.values_mut() {
        crate::typesync::naming::apply_enum(tagged_union, config.ts_names).map_err(|error| {
            EvenframeError::type_sync(format!("naming enum '{}': {error}", tagged_union.enum_name))
        })?;
    }

    Ok(AllConfigs {
        enums: enum_configs,
        tables: table_configs,
        objects: struct_configs,
        newtypes: newtype_configs,
    })
}

/// A scanned type named `RecordLink` in the schemasync pipeline would stand
/// in for evenframe's record link, which schemasync links tables through.
fn reject_scanned_record_link(
    struct_configs: &BTreeMap<String, crate::types::StructConfig>,
    enum_configs: &BTreeMap<String, TaggedUnion>,
) -> Result<()> {
    let record_link = crate::config::RECORD_LINK;
    let scanned = struct_configs
        .values()
        .filter(|struct_config| struct_config.struct_name == record_link)
        .map(|struct_config| struct_config.pipeline)
        .chain(
            enum_configs
                .values()
                .filter(|tagged_union| tagged_union.enum_name == record_link)
                .map(|tagged_union| tagged_union.pipeline),
        )
        .any(|pipeline| pipeline.includes_schemasync());
    if scanned {
        return Err(crate::error::EvenframeError::config(format!(
            "a scanned type is named `{record_link}`, which is evenframe's record link type in \
             schemasync. Rename it; to own the TypeScript `{record_link}`, point \
             foreign_types.{record_link} at your definition instead"
        )));
    }
    Ok(())
}

/// The names of evenframe's text-form types, which a field type naming one
/// always means.
const TEXT_FORMS: [&str; 4] = ["FromText", "JsonText", "IsoDate", "EpochMillis"];

/// A scanned type named after a text form would be read as evenframe's.
fn reject_scanned_text_forms(
    struct_configs: &BTreeMap<String, crate::types::StructConfig>,
    enum_configs: &BTreeMap<String, TaggedUnion>,
    newtype_configs: &BTreeMap<String, NewtypeConfig>,
) -> Result<()> {
    let taken = struct_configs
        .values()
        .map(|struct_config| struct_config.struct_name.as_str())
        .chain(
            enum_configs
                .values()
                .map(|tagged_union| tagged_union.enum_name.as_str()),
        )
        .chain(
            newtype_configs
                .values()
                .map(|newtype| newtype.name.as_str()),
        )
        .find(|name| TEXT_FORMS.contains(name));
    match taken {
        Some(name) => Err(crate::error::EvenframeError::config(format!(
            "a scanned type is named `{name}`, which is evenframe's text-form type of that \
             name wherever a field names it. Rename it"
        ))),
        None => Ok(()),
    }
}

/// Resolves `from`/`to` on relation tables by inspecting `in`/`out` field types.
///
/// Walks the `in` and `out` fields' `RecordLink<T>` types and resolves `T` to
/// the set of table names it references. If `T` is a persistable_union, every
/// variant's struct name is run through `effective()` so synthetic projections
/// resolve to the parent table; `from`/`to` are unconditionally rewritten with
/// whatever the resolver finds, which is correct because this runs once after
/// every plugin has finished mutating the configs.
fn resolve_relation_endpoints(
    table_configs: &mut BTreeMap<String, TableConfig>,
    enum_configs: &BTreeMap<String, TaggedUnion>,
    struct_configs: &BTreeMap<String, crate::types::StructConfig>,
) {
    // Snapshot table names to avoid borrow conflicts
    let known_tables: std::collections::BTreeSet<String> = table_configs.keys().cloned().collect();

    for table_config in table_configs.values_mut() {
        let Some(relation) = table_config.relation.as_mut() else {
            continue;
        };

        // Default edge_name from table_name if empty
        if relation.edge_name.is_empty() {
            relation.edge_name = table_config.table_name.clone();
        }

        if let Some(tables) = resolve_field_to_tables(
            &table_config.struct_config,
            "in",
            enum_configs,
            struct_configs,
            &known_tables,
        ) {
            debug!(
                "Auto-resolved relation.from for '{}': {:?}",
                table_config.table_name, tables
            );
            relation.from = tables;
        }
        if let Some(tables) = resolve_field_to_tables(
            &table_config.struct_config,
            "out",
            enum_configs,
            struct_configs,
            &known_tables,
        ) {
            debug!(
                "Auto-resolved relation.to for '{}': {:?}",
                table_config.table_name, tables
            );
            relation.to = tables;
        }
    }
}

/// Resolve a variant's struct reference back to the underlying table name.
///
/// A union variant may reference a synthetic projection whose own snake-case
/// name doesn't match a real table. Walk `effective()` so plugins can declare
/// such redirects via `output_override` and have schema generation follow
/// them to the parent struct's table.
fn variant_table_name(
    variant_struct_name: &str,
    struct_configs: &BTreeMap<String, crate::types::StructConfig>,
) -> String {
    if let Some(sc) = struct_configs.get(variant_struct_name) {
        sc.effective().struct_name.to_case(Case::Snake)
    } else {
        variant_struct_name.to_case(Case::Snake)
    }
}

/// Resolves a relation field (`in` or `out`) to the table names it references.
fn resolve_field_to_tables(
    struct_config: &crate::types::StructConfig,
    field_name: &str,
    enum_configs: &BTreeMap<String, TaggedUnion>,
    struct_configs: &BTreeMap<String, crate::types::StructConfig>,
    known_tables: &std::collections::BTreeSet<String>,
) -> Option<Vec<String>> {
    let field = struct_config
        .fields
        .iter()
        .find(|f| f.field_name == field_name)?;

    // Extract the inner type name from RecordLink<T>
    let inner_type_name = match &field.field_type {
        FieldType::RecordLink(inner) => match inner.as_ref() {
            FieldType::Other(name) => name.clone(),
            _ => return None,
        },
        _ => return None,
    };

    // Try direct table match: first by literal snake_case, then via the
    // struct's `effective()` so a synthetic projection whose
    // `output_override` points at another struct still resolves to the
    // parent's table.
    let snake = inner_type_name.to_case(Case::Snake);
    if known_tables.contains(&snake) {
        return Some(vec![snake]);
    }
    let effective_snake = variant_table_name(&inner_type_name, struct_configs);
    if known_tables.contains(&effective_snake) {
        return Some(vec![effective_snake]);
    }

    // Try enum variant resolution
    if let Some(tagged) = enum_configs.get(&inner_type_name) {
        let mut tables = Vec::new();
        for variant in &tagged.variants {
            if let Some(linked) = variant
                .data
                .as_ref()
                .and_then(VariantData::linked_type_name)
            {
                let table = variant_table_name(linked, struct_configs);
                if known_tables.contains(&table) {
                    tables.push(table);
                }
            }
        }
        if !tables.is_empty() {
            return Some(tables);
        }
    }

    None
}

/// A scanned type's configuration, parsed while the scanner held its syntax
/// tree. Collision handling and table assembly wait for every file's types.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ParsedType {
    Struct {
        config: StructConfig,
        /// The table attributes of a struct with a database `id` field that is
        /// not `resolve_only`.
        table: Option<Box<TableAttributes>>,
        /// The type `#[serde(into = "...")]` writes the struct as, which the
        /// typesync outputs describe in place of its fields.
        wire_as: Option<FieldType>,
    },
    Enum(TaggedUnion),
    /// A struct serde writes as another type.
    Newtype(Box<NewtypeConfig>),
}

/// A table struct's table-level attributes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TableAttributes {
    pub relation: Option<EdgeConfig>,
    pub permissions: Option<PermissionsConfig>,
    pub mock_generation_config: Option<MockGenerationConfig>,
    pub events: Vec<String>,
    pub indexes: Vec<IndexConfig>,
}

/// The configuration of the scanned struct or enum `item`, found in
/// `file_path` as `evenframe_type`.
pub(super) fn parse_scanned_item(
    item: &ScannedAst,
    evenframe_type: &EvenframeType,
    file_path: &str,
) -> Result<ParsedType> {
    match item {
        ScannedAst::Struct(item_struct) => {
            let failed = |error: syn::Error| {
                EvenframeError::parse_error(
                    file_path,
                    format!("struct '{}': {error}", item_struct.ident),
                )
            };
            let wire =
                naming::resolve(&syn::DeriveInput::from(item_struct.clone())).map_err(failed)?;
            if wire.shape != naming::ItemShape::Named {
                let mut newtype = parse_newtype_config(item_struct, &wire).map_err(failed)?;
                newtype.pipeline = evenframe_type.pipeline;
                newtype.resolve_only = evenframe_type.resolve_only;
                return Ok(ParsedType::Newtype(Box::new(newtype)));
            }
            let wire_as = wire
                .wire_as
                .as_ref()
                .map(|written| FieldType::parse(written, PathNames::Written));
            let mut config = parse_struct_config(item_struct, wire).map_err(failed)?;
            config.pipeline = evenframe_type.pipeline;
            config.resolve_only = evenframe_type.resolve_only;
            // A `resolve_only` struct is registered for resolution, so
            // referencing fields inline its shape, but is never materialized
            // as a managed table: no `DEFINE TABLE`, mock, or diff.
            let table = if has_database_id(&config) && !evenframe_type.resolve_only {
                Some(Box::new(parse_table_attributes(
                    item_struct,
                    &config.struct_name,
                    file_path,
                )?))
            } else {
                None
            };
            Ok(ParsedType::Struct {
                config,
                table,
                wire_as,
            })
        }
        ScannedAst::Enum(item_enum) => {
            let mut tagged_union = parse_enum_config(item_enum).map_err(|error| {
                EvenframeError::parse_error(
                    file_path,
                    format!("enum '{}': {error}", item_enum.ident),
                )
            })?;
            tagged_union.pipeline = evenframe_type.pipeline;
            tagged_union.resolve_only = evenframe_type.resolve_only;
            Ok(ParsedType::Enum(tagged_union))
        }
    }
}

/// Whether the struct has a field the database stores as `id`, which makes
/// it a table.
pub(super) fn has_database_id(config: &StructConfig) -> bool {
    config.fields.iter().any(|field| field.db_name() == "id")
}

fn parse_table_attributes(
    item_struct: &ItemStruct,
    struct_name: &str,
    file_path: &str,
) -> Result<TableAttributes> {
    let failed = |attribute: &str, error: syn::Error| {
        EvenframeError::Config(format!(
            "Failed to parse {attribute} on struct '{struct_name}' in '{file_path}': {error}"
        ))
    };
    let attrs = &item_struct.attrs;
    let mock_generation_config =
        parse_mock_data_attribute(attrs).map_err(|error| failed("#[mock_data(...)]", error))?;
    let events = parse_event_attributes(attrs).map_err(|error| failed("#[event(...)]", error))?;
    let known_field_names = indexable_fields(&item_struct.fields);
    let mut indexes = parse_index_attributes(attrs, &known_field_names)
        .map_err(|error| failed("#[indexes(...)]", error))?
        .into_iter()
        .map(|(index, _span)| index)
        .collect::<Vec<_>>();
    for field in &item_struct.fields {
        let Some(ident) = &field.ident else {
            continue;
        };
        let field_name = ident.to_string();
        let field_indexes =
            parse_field_index_attributes(field_name.trim_start_matches("r#"), &field.attrs)
                .map_err(|error| {
                    EvenframeError::Config(format!(
                        "Failed to parse index attribute on field '{struct_name}.{field_name}' in '{file_path}': {error}"
                    ))
                })?;
        indexes.extend(field_indexes.into_iter().map(|(index, _span)| index));
    }
    Ok(TableAttributes {
        relation: parse_relation_attribute(attrs)
            .map_err(|error| failed("#[relation(...)]", error))?,
        permissions: PermissionsConfig::parse(attrs)
            .map_err(|error| failed("#[permissions(...)]", error))?,
        mock_generation_config,
        events,
        indexes,
    })
}

/// Processes scanned types into configurations, in file order and, within a
/// file, in source order, so collisions resolve the same way on every run.
/// Each type a field names is resolved, through the imports of the module
/// the field is written in, to the definition it means, and named as that
/// definition is generated.
fn process_types(
    scan: Scan,
    enum_configs: &mut BTreeMap<String, TaggedUnion>,
    table_configs: &mut BTreeMap<String, TableConfig>,
    struct_configs: &mut BTreeMap<String, StructConfig>,
    newtype_configs: &mut BTreeMap<String, NewtypeConfig>,
    collision_strategy: CollisionStrategy,
    registry: &ForeignTypeRegistry,
) -> Result<()> {
    let Scan { items, scopes } = scan;
    let mut by_file: BTreeMap<String, Vec<ScannedItem>> = BTreeMap::new();
    for item in items {
        by_file
            .entry(item.evenframe_type.file_path.clone())
            .or_default()
            .push(item);
    }
    debug!("Processing types from {} files", by_file.len());
    let definitions = by_file
        .into_values()
        .flatten()
        .map(|item| Ok((item.evenframe_type, item.parsed?)))
        .collect::<Result<Vec<(EvenframeType, ParsedType)>>>()?;

    let qualified: Vec<String> = definitions
        .iter()
        .map(|(evenframe_type, _)| {
            format!(
                "{}::{}",
                rust_path(&evenframe_type.module_path),
                evenframe_type.name
            )
        })
        .collect();
    let mut first_definition: BTreeMap<&str, &str> = BTreeMap::new();
    for (path, (evenframe_type, _)) in qualified.iter().zip(&definitions) {
        if let Some(first_file) = first_definition.insert(path, &evenframe_type.file_path) {
            return Err(EvenframeError::Config(format!(
                "'{path}' is defined twice, in '{first_file}' and '{}'",
                evenframe_type.file_path
            )));
        }
    }
    let names = output_names(&definitions, collision_strategy)?;
    let paths = TypePaths::new(&scopes, qualified.iter().cloned());
    let resolver = Resolver {
        paths: &paths,
        names: &names,
        qualified: &qualified,
        registry,
    };

    for ((evenframe_type, parsed), name) in definitions.into_iter().zip(names.iter()) {
        let module_path = evenframe_type.module_path.as_str();
        let file_path = evenframe_type.file_path.as_str();
        match parsed {
            ParsedType::Struct {
                mut config,
                table,
                wire_as,
            } => {
                config.struct_name = name.clone();
                resolver.fields(&mut config.fields, module_path, &config.struct_name)?;
                // serde writes the struct as another type, which the typesync
                // outputs describe; the database stores its own fields.
                if let Some(mut written) = wire_as {
                    if let Some(typesync) = config.pipeline.typesync_part() {
                        resolver.field_type(
                            &mut written,
                            module_path,
                            &format!("{name} (into)"),
                        )?;
                        newtype_configs.insert(
                            name.clone(),
                            NewtypeConfig {
                                name: name.clone(),
                                inner: written,
                                kind: NewtypeKind::Alias,
                                pipeline: typesync,
                                resolve_only: config.resolve_only,
                                ..NewtypeConfig::default()
                            },
                        );
                    }
                    match config.pipeline.schemasync_part() {
                        Some(schemasync) => config.pipeline = schemasync,
                        None => continue,
                    }
                }
                if let Some(table) = table {
                    let table_name = config.struct_name.to_case(Case::Snake);
                    if let Some((_, name)) = find_duplicate_index_name(&table_name, &table.indexes)
                    {
                        return Err(EvenframeError::Config(format!(
                            "Another index on struct '{}' in '{}' already uses the name '{}'; give one of them `name = \"...\"`",
                            config.struct_name, file_path, name
                        )));
                    }
                    table_configs.insert(
                        table_name.clone(),
                        TableConfig {
                            table_name,
                            struct_config: config.clone(),
                            relation: table.relation,
                            permissions: table.permissions,
                            mock_generation_config: table.mock_generation_config,
                            events: table
                                .events
                                .into_iter()
                                .map(|statement| EventConfig { statement })
                                .collect(),
                            indexes: table.indexes,
                            output_override: None,
                        },
                    );
                }
                struct_configs.insert(config.struct_name.clone(), config);
            }
            ParsedType::Enum(mut tagged_union) => {
                tagged_union.enum_name = name.clone();
                resolver.variants(
                    &mut tagged_union.variants,
                    module_path,
                    &tagged_union.enum_name,
                )?;
                enum_configs.insert(tagged_union.enum_name.clone(), tagged_union);
            }
            ParsedType::Newtype(mut newtype) => {
                // A foreign mapping for the name already describes it to
                // every output, and wins there.
                if registry.is_foreign(name) {
                    continue;
                }
                newtype.name = name.clone();
                let site = format!("{name}.0");
                resolver.field_type(&mut newtype.inner, module_path, &site)?;
                newtype_configs.insert(newtype.name.clone(), *newtype);
            }
        }
    }
    Ok(())
}

/// The name each of `definitions` is generated under: its own, unless an
/// earlier struct or enum took it. Then the strategy either fails naming both
/// files or prefixes the name with the end of its module path.
fn output_names(
    definitions: &[(EvenframeType, ParsedType)],
    collision_strategy: CollisionStrategy,
) -> Result<Vec<String>> {
    let mut taken: BTreeMap<String, &str> = BTreeMap::new();
    let mut names = Vec::with_capacity(definitions.len());
    for (evenframe_type, _) in definitions {
        let name = &evenframe_type.name;
        let file_path = evenframe_type.file_path.as_str();
        let output = match taken.get(name) {
            None => name.clone(),
            Some(existing_file) => match collision_strategy {
                CollisionStrategy::Error => {
                    return Err(EvenframeError::Config(format!(
                        "Type name collision: '{name}' is defined in both '{existing_file}' and '{file_path}'. \
                         Rename one of them, or set collision_strategy = \"auto_rename\" in [typesync] config."
                    )));
                }
                CollisionStrategy::AutoRename => {
                    let segments: Vec<&str> = evenframe_type.module_path.split("::").collect();
                    let renamed = (1..=segments.len())
                        .map(|count| {
                            let prefix = segments[segments.len() - count..].join("_");
                            format!("{}{name}", prefix.to_case(Case::Pascal))
                        })
                        .find(|candidate| !taken.contains_key(candidate))
                        .ok_or_else(|| {
                            EvenframeError::Config(format!(
                                "Type '{name}' in '{file_path}' collides with the one in \
                                 '{existing_file}', and every name its module path offers is taken"
                            ))
                        })?;
                    warn!(
                        "Type '{name}' in '{file_path}' renamed to '{renamed}' to avoid collision with '{existing_file}'"
                    );
                    renamed
                }
            },
        };
        taken.insert(output.clone(), file_path);
        names.push(output);
    }
    Ok(names)
}

/// Resolves the type paths scanned fields name to the name each definition
/// they refer to is generated under.
struct Resolver<'a> {
    paths: &'a TypePaths<'a>,
    /// Each definition's generated name, by definition index.
    names: &'a [String],
    /// Each definition's absolute path, by definition index.
    qualified: &'a [String],
    registry: &'a ForeignTypeRegistry,
}

impl Resolver<'_> {
    fn fields(&self, fields: &mut [StructField], module_path: &str, owner: &str) -> Result<()> {
        for field in fields {
            let site = format!("{owner}.{}", field.field_name);
            self.field_type(&mut field.field_type, module_path, &site)?;
        }
        Ok(())
    }

    fn variants(&self, variants: &mut [Variant], module_path: &str, owner: &str) -> Result<()> {
        for variant in variants {
            let site = format!("{owner}::{}", variant.name);
            match &mut variant.data {
                Some(VariantData::DataStructureRef(field_type)) => {
                    self.field_type(field_type, module_path, &site)?;
                }
                Some(VariantData::InlineStruct(inline)) => {
                    self.fields(&mut inline.fields, module_path, &site)?;
                }
                None => {}
            }
        }
        Ok(())
    }

    fn field_type(&self, field_type: &mut FieldType, module_path: &str, site: &str) -> Result<()> {
        match field_type {
            FieldType::Other(path) => {
                let resolved = self.resolve(path, module_path, site)?;
                *field_type = resolved;
            }
            FieldType::Option(inner)
            | FieldType::Vec(inner)
            | FieldType::RecordLink(inner)
            | FieldType::JsonText(inner) => {
                self.field_type(inner, module_path, site)?;
            }
            FieldType::HashMap(key, value) | FieldType::BTreeMap(key, value) => {
                self.field_type(key, module_path, site)?;
                self.field_type(value, module_path, site)?;
            }
            FieldType::Tuple(items) => {
                for item in items {
                    self.field_type(item, module_path, site)?;
                }
            }
            FieldType::Struct(members) => {
                for (name, member) in members {
                    self.field_type(member, module_path, &format!("{site}.{name}"))?;
                }
            }
            FieldType::String
            | FieldType::Char
            | FieldType::Bool
            | FieldType::Unit
            | FieldType::F32
            | FieldType::F64
            | FieldType::I8
            | FieldType::I16
            | FieldType::I32
            | FieldType::I64
            | FieldType::I128
            | FieldType::Isize
            | FieldType::U8
            | FieldType::U16
            | FieldType::U32
            | FieldType::U64
            | FieldType::U128
            | FieldType::Usize
            | FieldType::Duration
            | FieldType::FromText(_)
            | FieldType::IsoDate
            | FieldType::EpochMillis => {}
        }
        Ok(())
    }

    /// The field type `path`, written at `site` in the module at
    /// `module_path`, names.
    fn resolve(&self, path: &str, module_path: &str, site: &str) -> Result<FieldType> {
        Ok(match self.paths.resolve(module_path, path) {
            Target::Definition(definition) => FieldType::Other(self.names[definition].clone()),
            Target::External(absolute) => {
                let name = absolute.rsplit("::").next().unwrap_or(&absolute);
                // A path no import resolves names the standard `Duration`
                // the way the prelude-less source can only mean it, unless a
                // foreign type claims the name.
                let std_duration =
                    absolute == "Duration" || STD_DURATION_PATHS.contains(&absolute.as_str());
                if std_duration
                    && !self.registry.is_foreign(name)
                    && !self.registry.is_foreign(&absolute)
                {
                    FieldType::Duration
                } else {
                    FieldType::Other(name.to_string())
                }
            }
            Target::Ambiguous(definitions) => {
                let candidates = definitions
                    .iter()
                    .map(|definition| format!("`{}`", self.qualified[*definition]))
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(EvenframeError::Config(format!(
                    "`{site}` names `{path}`, which no import in `{module_path}` resolves, and \
                     {candidates} all define it; import the one it means"
                )));
            }
        })
    }
}

fn parse_struct_config(
    item_struct: &ItemStruct,
    wire: naming::ItemWire,
) -> syn::Result<StructConfig> {
    let struct_name = item_struct.ident.to_string();
    trace!("Parsing struct config for: {}", struct_name);
    let mut fields = Vec::new();

    if let Fields::Named(ref fields_named) = item_struct.fields {
        debug!(
            "Processing {} fields for struct {}",
            fields_named.named.len(),
            struct_name
        );
        fields = process_struct_fields(fields_named, wire.fields)?;
    }

    let doccom = parse_doccom_attribute(&item_struct.attrs)?;
    refuse_container_validators(&item_struct.attrs)?;
    let TypesyncAttributes {
        macroforge_derives,
        annotations,
        ..
    } = TypesyncAttributes::parse(&item_struct.attrs, Position::Container)?;
    let rust_derives = parse_rust_derives(&item_struct.attrs);
    let raw_attributes = collect_raw_attributes(&item_struct.attrs);

    Ok(StructConfig {
        struct_name,
        fields,
        doccom,
        macroforge_derives,
        annotations,
        pipeline: crate::types::Pipeline::default(),
        rust_derives,
        output_override: None,
        resolve_only: false,
        raw_attributes,
    })
}

/// A struct serde writes as `shape`: the field it writes, a tuple of its
/// fields, or nothing. Container validators check that value.
fn parse_newtype_config(
    item_struct: &ItemStruct,
    wire: &naming::ItemWire,
) -> syn::Result<NewtypeConfig> {
    let shape = &wire.shape;
    let (inner, kind) = match shape {
        naming::ItemShape::Newtype { member } => {
            let field = item_struct
                .fields
                .iter()
                .enumerate()
                .find(|(position, field)| match (member, &field.ident) {
                    (syn::Member::Named(name), Some(ident)) => name == ident,
                    (syn::Member::Unnamed(index), None) => index.index as usize == *position,
                    _ => false,
                })
                .map(|(_, field)| field)
                .ok_or_else(|| {
                    syn::Error::new(item_struct.ident.span(), "the newtype's field is missing")
                })?;
            (
                FieldType::parse(&field.ty, PathNames::Written),
                NewtypeKind::Branded,
            )
        }
        naming::ItemShape::Tuple(_) => (
            FieldType::Tuple(
                item_struct
                    .fields
                    .iter()
                    .map(|field| FieldType::parse(&field.ty, PathNames::Written))
                    .collect(),
            ),
            NewtypeKind::Alias,
        ),
        naming::ItemShape::Unit => (FieldType::Unit, NewtypeKind::Alias),
        naming::ItemShape::Named => {
            return Err(syn::Error::new(
                item_struct.ident.span(),
                "a struct of named fields is not a newtype",
            ));
        }
    };
    let (morphs, validators, validator_overrides) = match kind {
        NewtypeKind::Branded => {
            let container = parse_field_validators(&item_struct.attrs)?;
            let mut morphs = container.morphs;
            let mut validators = container.validators;
            let mut overrides = parse_container_validator_overrides(&item_struct.attrs)?;
            // The field's own validators check the same value.
            if let naming::ItemShape::Newtype { member } = shape
                && let Some(field) =
                    item_struct
                        .fields
                        .iter()
                        .enumerate()
                        .find_map(|(position, field)| match (member, &field.ident) {
                            (syn::Member::Named(name), Some(ident)) if name == ident => Some(field),
                            (syn::Member::Unnamed(index), None)
                                if index.index as usize == position =>
                            {
                                Some(field)
                            }
                            _ => None,
                        })
            {
                let own = parse_field_validators(&field.attrs)?;
                morphs.extend(own.morphs);
                validators.extend(own.validators);
                overrides = overrides.followed_by(parse_validator_overrides(&field.attrs)?);
            }
            (morphs, validators, overrides)
        }
        NewtypeKind::Alias => {
            refuse_container_validators(&item_struct.attrs)?;
            (Vec::new(), Vec::new(), ValidatorOverrides::default())
        }
    };
    let elements = match shape {
        naming::ItemShape::Tuple(_) => parse_element_validators(&item_struct.fields)?,
        naming::ItemShape::Newtype { .. } | naming::ItemShape::Unit | naming::ItemShape::Named => {
            ElementValidators::default()
        }
    };
    let TypesyncAttributes {
        macroforge_derives,
        annotations,
        ..
    } = TypesyncAttributes::parse(&item_struct.attrs, Position::Container)?;
    Ok(NewtypeConfig {
        name: item_struct.ident.to_string(),
        inner,
        kind,
        morphs,
        validators,
        doccom: parse_doccom_attribute(&item_struct.attrs)?,
        annotations,
        macroforge_derives,
        rust_derives: parse_rust_derives(&item_struct.attrs),
        pipeline: crate::types::Pipeline::default(),
        resolve_only: false,
        raw_attributes: collect_raw_attributes(&item_struct.attrs),
        element_morphs: elements.morphs,
        element_validators: elements.validators,
        storage: newtype_storage(wire),
        element_validator_overrides: elements.overrides,
        validator_overrides,
        output_override: None,
    })
}

/// How the database stores a newtype, by its own `#[surreal]` keys.
pub(crate) fn newtype_storage(wire: &naming::ItemWire) -> crate::types::Storage {
    crate::types::Storage {
        tuple: wire.stored_tuple,
        opaque_elements: wire.opaque_elements.clone(),
        value: wire.unit_value.as_ref().map(naming::UnitValue::surql),
        ..crate::types::Storage::default()
    }
}

fn parse_enum_config(item_enum: &ItemEnum) -> syn::Result<TaggedUnion> {
    let enum_name = item_enum.ident.to_string();
    trace!("Parsing enum config for: {}", enum_name);
    let mut variants = Vec::new();

    let enum_doccom = parse_doccom_attribute(&item_enum.attrs)?;
    refuse_container_validators(&item_enum.attrs)?;
    let TypesyncAttributes {
        macroforge_derives: enum_macroforge_derives,
        annotations: enum_annotations,
        ..
    } = TypesyncAttributes::parse(&item_enum.attrs, Position::Container)?;
    let wire = naming::resolve(&syn::DeriveInput::from(item_enum.clone()))?;
    let representation = wire.representation;
    let enum_rust_derives = parse_rust_derives(&item_enum.attrs);

    for (variant, variant_wire) in item_enum.variants.iter().zip(wire.variants) {
        let variant_name = variant.ident.to_string();
        trace!("Processing variant: {} in enum {}", variant_name, enum_name);

        let variant_doccom = parse_doccom_attribute(&variant.attrs)?;
        let variant_position = match variant.fields {
            Fields::Named(_) => Position::StructVariant,
            Fields::Unnamed(_) | Fields::Unit => Position::Variant,
        };
        let TypesyncAttributes {
            macroforge_derives: variant_macroforge_derives,
            annotations: variant_annotations,
            ..
        } = TypesyncAttributes::parse(&variant.attrs, variant_position)?;

        let data = match &variant.fields {
            Fields::Unit => None,
            Fields::Unnamed(fields) => Some(VariantData::DataStructureRef(
                FieldType::parse_tuple_variant(fields, PathNames::Written),
            )),
            Fields::Named(fields_named) => {
                debug!(
                    "Processing {} fields for enum struct {}",
                    fields_named.named.len(),
                    variant_name
                );
                let struct_fields = process_struct_fields(fields_named, variant_wire.fields)?;

                Some(VariantData::InlineStruct(StructConfig {
                    struct_name: variant_name.clone(),
                    fields: struct_fields,
                    macroforge_derives: variant_macroforge_derives,
                    ..Default::default()
                }))
            }
        };

        let variant_raw_attributes = collect_raw_attributes(&variant.attrs);
        let is_default_variant = variant.attrs.iter().any(|a| a.path().is_ident("default"));
        let elements = match &variant.fields {
            Fields::Unnamed(fields) => parse_element_validators(&fields.unnamed)?,
            Fields::Named(_) | Fields::Unit => ElementValidators::default(),
        };

        variants.push(Variant {
            name: variant_name,
            data,
            wire: variant_wire.wire,
            doccom: variant_doccom,
            annotations: variant_annotations,
            output_override: None,
            raw_attributes: variant_raw_attributes,
            is_default: is_default_variant,
            element_morphs: elements.morphs,
            element_validators: elements.validators,
            element_validator_overrides: elements.overrides,
        });
    }

    let enum_raw_attributes = collect_raw_attributes(&item_enum.attrs);

    Ok(TaggedUnion {
        enum_name,
        variants,
        representation,
        doccom: enum_doccom,
        macroforge_derives: enum_macroforge_derives,
        annotations: enum_annotations,
        pipeline: crate::types::Pipeline::default(),
        rust_derives: enum_rust_derives,
        output_override: None,
        resolve_only: false,
        raw_attributes: enum_raw_attributes,
    })
}

/// The named fields' configs, `wires` holding each field's wire form in order.
fn process_struct_fields(
    fields_named: &FieldsNamed,
    wires: Vec<Wire>,
) -> syn::Result<Vec<StructField>> {
    let mut struct_fields = Vec::new();
    for (field, wire) in fields_named.named.iter().zip(wires) {
        let field_name = field
            .ident
            .as_ref()
            .map(naming::unraw)
            .ok_or_else(|| syn::Error::new(field.span(), "a named field has no identifier"))?;

        let field_type = FieldType::parse(&field.ty, PathNames::Written);

        let edge_config = EdgeConfig::parse(field)?;
        let define_config = DefineConfig::parse(field)?;
        let format = parse_format_attribute_bin(&field.attrs)?;
        let FieldValidators {
            morphs, validators, ..
        } = parse_field_validators(&field.attrs)?;
        let doccom = parse_doccom_attribute(&field.attrs)?;
        let annotations = TypesyncAttributes::parse(&field.attrs, Position::Field)?.annotations;

        let field_raw_attributes = collect_raw_attributes(&field.attrs);

        let unique = field
            .attrs
            .iter()
            .any(|attr| attr.path().is_ident("unique"));

        struct_fields.push(StructField {
            field_name,
            field_type,
            wire,
            edge_config,
            define_config,
            format,
            morphs,
            validators,
            always_regenerate: false,
            doccom,
            annotations,
            unique,
            output_override: None,
            raw_attributes: field_raw_attributes,
            validator_overrides: parse_validator_overrides(&field.attrs)?,
        });
    }
    Ok(struct_fields)
}

/// Attributes that evenframe's own attribute parsers handle. Everything
/// else ends up in `StructConfig::raw_attributes` so plugins can see it.
const KNOWN_ATTRS: &[&str] = &[
    "evenframe",
    "apply",
    "define_field_statement",
    "derive",
    "doc",
    "doccom",
    "edge",
    "event",
    "fetch",
    "format",
    "fulltext",
    "hnsw",
    "diskann",
    "index",
    "indexes",
    "mock_data",
    "permissions",
    "relation",
    "serde",
    "schemasync",
    "subquery",
    "surreal",
    "typesync",
    "unique",
    "morphs",
    "validators",
    // These are handled by proc-macros but aren't plugin-relevant metadata.
    "cfg",
    "cfg_attr",
    "allow",
    "warn",
    "deny",
    "forbid",
    "must_use",
    "non_exhaustive",
    "repr",
    "automatically_derived",
];

/// Collects every attribute that evenframe doesn't natively handle into
/// a map of `attr_name → [raw_body, …]`. The "body" is the stringified
/// token stream inside the parens (or empty string for path-only attrs).
fn collect_raw_attributes(attrs: &[syn::Attribute]) -> BTreeMap<String, Vec<String>> {
    use quote::ToTokens;

    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for attr in attrs {
        let Some(ident) = attr.path().get_ident() else {
            continue;
        };
        let name = ident.to_string();
        if KNOWN_ATTRS.contains(&name.as_str()) {
            continue;
        }
        let body = match &attr.meta {
            syn::Meta::Path(_) => String::new(),
            syn::Meta::List(list) => list.tokens.to_token_stream().to_string(),
            syn::Meta::NameValue(nv) => nv.value.to_token_stream().to_string(),
        };
        out.entry(name).or_default().push(body);
    }
    out
}

/// Merges tables and objects into a single struct config map.
pub fn merge_tables_and_objects(
    tables: BTreeMap<String, TableConfig>,
    objects: BTreeMap<String, StructConfig>,
) -> BTreeMap<String, StructConfig> {
    debug!(
        "Merging {} tables and {} objects",
        tables.len(),
        objects.len()
    );
    let mut struct_configs = objects;

    // Tables are present in `objects` under their PascalCase `struct_name`
    // (inserted by `process_types` during the scan) and carry whatever
    // output_override was set by the rule-plugin struct loop. We also want
    // the table's authoritative struct_config (with the output_override set
    // by the rule-plugin table loop) accessible under the snake_case key.
    //
    // Drop the PascalCase duplicate before inserting the table entry so
    // consumers that look types up by name see exactly one entry per table,
    // the authoritative one from the table loop.
    for (name, table_config) in tables {
        trace!("Merging table config for: {}", name);
        struct_configs.remove(&table_config.struct_config.struct_name);
        struct_configs.insert(name, table_config.struct_config);
    }

    debug!(
        "Merge complete. Total struct configs: {}",
        struct_configs.len()
    );
    struct_configs
}

// ============================================================
// Rule plugin application
// ============================================================

/// Apply output rule plugins to set output overrides on configs.
///
/// Every plugin sees each table, struct, enum and newtype as the scan produced
/// it, and
/// their overrides apply in plugin order. The typesync and schemasync
/// generators read a config's `output_override` before computing output.
#[cfg(feature = "wasm-plugins")]
fn apply_rule_plugins(
    config: &ScanConfig,
    table_configs: &mut BTreeMap<String, TableConfig>,
    struct_configs: &mut BTreeMap<String, StructConfig>,
    enum_configs: &mut BTreeMap<String, TaggedUnion>,
    newtype_configs: &mut BTreeMap<String, NewtypeConfig>,
) -> Result<()> {
    use crate::typesync::plugin::OutputRulePluginManager;
    use crate::typesync::plugin_types::OutputRulePluginInput;

    let mut plugins = OutputRulePluginManager::new(&config.output_rule_plugins, &config.scan_path)
        .map_err(|error| {
            EvenframeError::Plugin(format!("Failed to load output rule plugins: {error}"))
        })?;

    info!(
        "Applying output rule plugins to {} tables",
        table_configs.len()
    );
    for (table_name, table) in table_configs.iter_mut() {
        let input = OutputRulePluginInput::Table {
            pipeline: format!("{:?}", table.struct_config.pipeline),
            generator: String::new(),
            struct_config: &table.struct_config,
            table_config: table,
        };
        for output in rule_plugin_outputs(&mut plugins, &input, "table", table_name)? {
            override_struct(&mut table.struct_config, &output);
            let type_override = &output.type_override;
            if let Some(permissions) = &type_override.permissions {
                table.permissions = Some(PermissionsConfig {
                    all_permissions: None,
                    select_permissions: Some(permissions.select.clone()),
                    create_permissions: Some(permissions.create.clone()),
                    update_permissions: Some(permissions.update.clone()),
                    delete_permissions: Some(permissions.delete.clone()),
                });
            }
            table
                .events
                .extend(type_override.events.iter().map(|event| EventConfig {
                    statement: event.statement.clone(),
                }));
        }
    }

    info!(
        "Applying output rule plugins to {} structs",
        struct_configs.len()
    );
    for (struct_name, struct_config) in struct_configs.iter_mut() {
        let input = OutputRulePluginInput::Struct {
            pipeline: format!("{:?}", struct_config.pipeline),
            generator: String::new(),
            config: struct_config,
        };
        for output in rule_plugin_outputs(&mut plugins, &input, "struct", struct_name)? {
            override_struct(struct_config, &output);
        }
    }

    info!(
        "Applying output rule plugins to {} enums",
        enum_configs.len()
    );
    for (enum_name, enum_config) in enum_configs.iter_mut() {
        let input = OutputRulePluginInput::Enum {
            pipeline: format!("{:?}", enum_config.pipeline),
            generator: String::new(),
            config: enum_config,
        };
        for output in rule_plugin_outputs(&mut plugins, &input, "enum", enum_name)? {
            override_enum(enum_config, &output);
        }
    }

    info!(
        "Applying output rule plugins to {} newtypes",
        newtype_configs.len()
    );
    for (newtype_name, newtype_config) in newtype_configs
        .iter_mut()
        .filter(|(_, newtype_config)| !newtype_config.resolve_only)
    {
        let input = OutputRulePluginInput::Newtype {
            pipeline: format!("{:?}", newtype_config.pipeline),
            generator: String::new(),
            config: newtype_config,
        };
        for output in rule_plugin_outputs(&mut plugins, &input, "newtype", newtype_name)? {
            override_newtype(newtype_config, &output);
        }
    }

    info!("Output rule plugins applied to all configs");
    Ok(())
}

/// Every rule plugin's output for one type, or the error a plugin reported
/// for it.
#[cfg(feature = "wasm-plugins")]
fn rule_plugin_outputs(
    plugins: &mut crate::typesync::plugin::OutputRulePluginManager,
    input: &crate::typesync::plugin_types::OutputRulePluginInput,
    kind: &str,
    name: &str,
) -> Result<Vec<crate::typesync::plugin_types::OutputRulePluginOutput>> {
    plugins
        .transform(input)
        .map_err(|error| {
            EvenframeError::Plugin(format!(
                "Output rule plugins failed for {kind} '{name}': {error}"
            ))
        })?
        .into_iter()
        .map(|(plugin_name, output)| match &output.error {
            Some(error) => Err(EvenframeError::Plugin(format!(
                "Output rule plugin '{plugin_name}' reported an error for {kind} '{name}': {error}"
            ))),
            None => Ok(output),
        })
        .collect()
}

/// Applies a rule plugin's overrides to a struct. Field annotations go
/// first, so the type-level snapshot that consumers read through
/// `effective()` carries them.
#[cfg(feature = "wasm-plugins")]
fn override_struct(
    struct_config: &mut StructConfig,
    output: &crate::typesync::plugin_types::OutputRulePluginOutput,
) {
    for (field_name, field_override) in &output.field_overrides {
        if field_override.annotations.is_empty() {
            continue;
        }
        if let Some(field) = struct_config
            .fields
            .iter_mut()
            .find(|field| &field.field_name == field_name)
        {
            let mut overridden = field.clone();
            overridden.output_override = None;
            overridden.annotations = field_override.annotations.clone();
            field.output_override = Some(Box::new(overridden));
        }
    }
    let type_override = &output.type_override;
    if type_override.macroforge_derives.is_empty() && type_override.annotations.is_empty() {
        return;
    }
    let mut overridden = struct_config.clone();
    overridden.output_override = None;
    if !type_override.macroforge_derives.is_empty() {
        overridden.macroforge_derives = type_override.macroforge_derives.clone();
    }
    push_missing(&mut overridden.annotations, &type_override.annotations);
    struct_config.output_override = Some(Box::new(overridden));
}

/// Applies a rule plugin's overrides to an enum. Variant annotations go
/// first, so the type-level snapshot carries them, as for a struct.
#[cfg(feature = "wasm-plugins")]
fn override_enum(
    enum_config: &mut TaggedUnion,
    output: &crate::typesync::plugin_types::OutputRulePluginOutput,
) {
    for (variant_name, variant_override) in &output.field_overrides {
        if let Some(variant) = enum_config
            .variants
            .iter_mut()
            .find(|variant| &variant.name == variant_name)
        {
            push_missing(&mut variant.annotations, &variant_override.annotations);
        }
    }
    let type_override = &output.type_override;
    if type_override.macroforge_derives.is_empty() && type_override.annotations.is_empty() {
        return;
    }
    let mut overridden = enum_config.clone();
    overridden.output_override = None;
    if !type_override.macroforge_derives.is_empty() {
        overridden.macroforge_derives = type_override.macroforge_derives.clone();
    }
    push_missing(&mut overridden.annotations, &type_override.annotations);
    enum_config.output_override = Some(Box::new(overridden));
}

/// Applies a rule plugin's type-level overrides to a newtype, which has no
/// fields or variants of its own to annotate.
#[cfg(feature = "wasm-plugins")]
fn override_newtype(
    newtype_config: &mut NewtypeConfig,
    output: &crate::typesync::plugin_types::OutputRulePluginOutput,
) {
    let type_override = &output.type_override;
    if type_override.macroforge_derives.is_empty() && type_override.annotations.is_empty() {
        return;
    }
    let mut overridden = newtype_config.effective().clone();
    overridden.output_override = None;
    if !type_override.macroforge_derives.is_empty() {
        overridden.macroforge_derives = type_override.macroforge_derives.clone();
    }
    push_missing(&mut overridden.annotations, &type_override.annotations);
    newtype_config.output_override = Some(Box::new(overridden));
}

/// Appends each of `annotations` that `existing` does not hold yet.
#[cfg(feature = "wasm-plugins")]
fn push_missing(existing: &mut Vec<String>, annotations: &[String]) {
    for annotation in annotations {
        if !existing.contains(annotation) {
            existing.push(annotation.clone());
        }
    }
}

// ============================================================
// Synthetic-item plugin application
// ============================================================

/// Call each configured synthetic-item plugin in sequence, merging the items
/// it emits into the running config maps. Each plugin sees the output of
/// all previous plugins, matching the sequential composition of
/// `apply_rule_plugins`.
///
/// Collisions between plugin output and existing items honor the configured
/// [`CollisionStrategy`]:
///
/// - `Error` → abort the whole build with a collision message.
/// - `AutoRename` → prefix the synthetic item's name with the plugin name,
///   Pascal-cased.
#[cfg(feature = "wasm-plugins")]
fn apply_synthetic_plugins(
    config: &ScanConfig,
    table_configs: &mut BTreeMap<String, TableConfig>,
    struct_configs: &mut BTreeMap<String, StructConfig>,
    enum_configs: &mut BTreeMap<String, TaggedUnion>,
) -> Result<()> {
    use crate::typesync::synthetic_plugin::SyntheticItemPluginManager;
    use crate::typesync::synthetic_plugin_types::SyntheticPluginInput;

    let mut plugins =
        SyntheticItemPluginManager::new(&config.synthetic_item_plugins, &config.scan_path)
            .map_err(|error| {
                EvenframeError::Plugin(format!("Failed to load synthetic-item plugins: {error}"))
            })?;

    for plugin_name in plugins.plugin_names() {
        let input = SyntheticPluginInput {
            structs: struct_configs,
            enums: enum_configs,
            tables: table_configs,
        };
        let output = plugins
            .generate_items(&plugin_name, &input)
            .map_err(|error| {
                EvenframeError::Plugin(format!(
                    "Synthetic-item plugin '{plugin_name}' failed: {error}"
                ))
            })?;

        if let Some(error) = output.error {
            return Err(EvenframeError::Plugin(format!(
                "Synthetic-item plugin '{plugin_name}' reported error: {error}"
            )));
        }

        info!(
            "Synthetic-item plugin '{}' produced: {} structs, {} enums, {} tables",
            plugin_name,
            output.new_structs.len(),
            output.new_enums.len(),
            output.new_tables.len()
        );

        merge_synthetic_output(
            &plugin_name,
            output,
            table_configs,
            struct_configs,
            enum_configs,
            config.collision_strategy,
        )?;
    }

    Ok(())
}

#[cfg(feature = "wasm-plugins")]
fn merge_synthetic_output(
    plugin_name: &str,
    output: crate::typesync::synthetic_plugin_types::SyntheticPluginOutput,
    table_configs: &mut BTreeMap<String, TableConfig>,
    struct_configs: &mut BTreeMap<String, StructConfig>,
    enum_configs: &mut BTreeMap<String, TaggedUnion>,
    collision_strategy: CollisionStrategy,
) -> Result<()> {
    let prefix = plugin_name.to_case(Case::Pascal);

    let new_fields = output
        .new_structs
        .iter()
        .chain(output.new_tables.iter().map(|table| &table.struct_config))
        .flat_map(|struct_config| {
            struct_config
                .fields
                .iter()
                .map(move |field| (&struct_config.struct_name, field))
        });
    for (struct_name, field) in new_fields {
        crate::validator::bounds::check_validators(&field.validators).map_err(|problem| {
            crate::error::EvenframeError::Plugin(format!(
                "Synthetic-item plugin '{plugin_name}' produced field '{struct_name}.{}' with invalid validators: {problem}",
                field.field_name
            ))
        })?;
    }

    for mut sc in output.new_structs {
        let original = sc.struct_name.clone();
        if struct_configs.contains_key(&sc.struct_name) {
            match collision_strategy {
                CollisionStrategy::Error => {
                    return Err(crate::error::EvenframeError::Config(format!(
                        "Synthetic-item plugin '{}' produced struct '{}' which collides with an \
                         existing type. Rename it inside the plugin, or set \
                         collision_strategy = \"auto_rename\" in [typesync] config.",
                        plugin_name, sc.struct_name
                    )));
                }
                CollisionStrategy::AutoRename => {
                    sc.struct_name = format!("{}{}", prefix, sc.struct_name);
                    warn!(
                        "Synthetic struct '{}' from plugin '{}' renamed to '{}' to avoid collision",
                        original, plugin_name, sc.struct_name
                    );
                }
            }
        }
        debug!(
            "Synthetic plugin '{}' added struct '{}'",
            plugin_name, sc.struct_name
        );
        struct_configs.insert(sc.struct_name.clone(), sc);
    }

    for mut ec in output.new_enums {
        let original = ec.enum_name.clone();
        if enum_configs.contains_key(&ec.enum_name) {
            match collision_strategy {
                CollisionStrategy::Error => {
                    return Err(crate::error::EvenframeError::Config(format!(
                        "Synthetic-item plugin '{}' produced enum '{}' which collides with an \
                         existing type. Rename it inside the plugin, or set \
                         collision_strategy = \"auto_rename\" in [typesync] config.",
                        plugin_name, ec.enum_name
                    )));
                }
                CollisionStrategy::AutoRename => {
                    ec.enum_name = format!("{}{}", prefix, ec.enum_name);
                    warn!(
                        "Synthetic enum '{}' from plugin '{}' renamed to '{}' to avoid collision",
                        original, plugin_name, ec.enum_name
                    );
                }
            }
        }
        debug!(
            "Synthetic plugin '{}' added enum '{}'",
            plugin_name, ec.enum_name
        );
        enum_configs.insert(ec.enum_name.clone(), ec);
    }

    for mut tc in output.new_tables {
        let original = tc.table_name.clone();
        if table_configs.contains_key(&tc.table_name) {
            match collision_strategy {
                CollisionStrategy::Error => {
                    return Err(crate::error::EvenframeError::Config(format!(
                        "Synthetic-item plugin '{}' produced table '{}' which collides with an \
                         existing table. Rename it inside the plugin, or set \
                         collision_strategy = \"auto_rename\" in [typesync] config.",
                        plugin_name, tc.table_name
                    )));
                }
                CollisionStrategy::AutoRename => {
                    let new_table = format!("{}_{}", prefix.to_case(Case::Snake), tc.table_name);
                    tc.table_name = new_table;
                    tc.struct_config.struct_name =
                        format!("{}{}", prefix, tc.struct_config.struct_name);
                    warn!(
                        "Synthetic table '{}' from plugin '{}' renamed to '{}' to avoid collision",
                        original, plugin_name, tc.table_name
                    );
                }
            }
        }
        debug!(
            "Synthetic plugin '{}' added table '{}'",
            plugin_name, tc.table_name
        );
        table_configs.insert(tc.table_name.clone(), tc);
    }

    Ok(())
}

#[cfg(test)]
mod resolution_tests {
    use super::{
        AllConfigs, BTreeMap, CollisionStrategy, FieldType, NewtypeKind, Result, ScanConfig,
        StructVariants, VariantData, build_all_configs,
    };
    use crate::types::FieldOwner;
    use crate::validator::{StringValidator, Validator};
    use std::fs;
    use tempfile::TempDir;

    const AUTH: &str = r#"
        use evenframe::Evenframe;

        #[derive(Evenframe)]
        pub enum Status { Active, Banned }

        #[derive(Evenframe)]
        pub struct Account { pub id: String, pub status: Status }
    "#;

    const BILLING: &str = r#"
        use evenframe::Evenframe;

        #[derive(Evenframe)]
        pub enum Status { Open, Paid }

        #[derive(Evenframe)]
        pub struct Invoice {
            pub id: String,
            pub status: Status,
            pub account_status: super::auth::Status,
        }

        #[derive(Evenframe)]
        pub enum Event { Changed(Status), Moved { from: Status, to: Status } }
    "#;

    const NEWTYPES: &str = r#"
        use evenframe::Evenframe;

        #[derive(Evenframe)]
        #[validators(StringValidator::NonEmpty)]
        pub struct NonEmptyString(String);

        #[derive(Evenframe)]
        #[validators(StringValidator::Email)]
        pub struct Email(NonEmptyString);

        #[derive(Evenframe)]
        #[serde(transparent)]
        pub struct Label {
            #[validators(StringValidator::MinLength(2))]
            value: String,
        }

        #[derive(Evenframe)]
        pub enum Status { Open, Closed }

        #[derive(Evenframe)]
        pub struct Code(Status);

        #[derive(Evenframe)]
        pub struct Pair(String, u32);

        #[derive(Evenframe)]
        pub struct Marker;

        #[derive(Evenframe)]
        pub struct Profile {
            pub id: String,
            pub name: NonEmptyString,
            pub email: Option<Email>,
            pub tags: Vec<NonEmptyString>,
            pub code: Code,
        }
    "#;

    const FLATTENED: &str = r#"
        use evenframe::Evenframe;
        use std::collections::HashMap;

        #[derive(Evenframe)]
        pub struct Audit { pub created_by: String, pub note: Option<String> }

        #[derive(Evenframe)]
        pub struct Geo { pub lat: f64, pub lon: f64 }

        #[derive(Evenframe)]
        pub struct Extras {
            pub label: String,
            #[serde(flatten)]
            pub rest: HashMap<String, String>,
        }

        #[derive(Evenframe)]
        pub struct Post {
            pub id: String,
            pub title: String,
            #[serde(flatten)]
            pub audit: Audit,
            #[serde(flatten)]
            pub geo: Option<Geo>,
            pub extras: Extras,
        }

        #[derive(Evenframe)]
        pub struct Event {
            pub id: String,
            pub kind: String,
            #[serde(flatten)]
            pub data: HashMap<String, i64>,
        }
    "#;

    const TYPESYNC: &str = r#"
        use evenframe::Evenframe;

        #[derive(Evenframe)]
        #[typesync(macroforge(derives = [Default, Encode]), annotation("@form"))]
        pub struct Profile {
            pub id: String,
            #[typesync(macroforge(attributes = [endec(rename = "full_name"), hidden]))]
            pub name: String,
        }

        #[derive(Evenframe)]
        #[typesync(macroforge(attributes = [endec(tag = "kind")]))]
        pub enum Shape {
            #[typesync(macroforge(derives = [Decode]), annotation("@circle"))]
            Circle { radius: f64 },
            #[typesync(annotation("@dot"))]
            Dot,
        }

        #[derive(Evenframe)]
        #[typesync(macroforge(derives = [Encode]))]
        pub struct Slug(String);
    "#;

    /// Builds a crate named `shop` from `files`, paths relative to `src/`.
    fn build(collision_strategy: CollisionStrategy, files: &[(&str, &str)]) -> Result<AllConfigs> {
        let root = TempDir::new().expect("temp dir");
        fs::write(
            root.path().join("Cargo.toml"),
            "[package]\nname = \"shop\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .expect("manifest");
        for (path, content) in files {
            let file = root.path().join("src").join(path);
            fs::create_dir_all(file.parent().expect("parent")).expect("source dir");
            fs::write(file, content).expect("source file");
        }
        build_all_configs(&ScanConfig {
            scan_path: root.path().to_path_buf(),
            collision_strategy,
            struct_variants: StructVariants::Inline,
            ..ScanConfig::default()
        })
    }

    fn other(name: &str) -> FieldType {
        FieldType::Other(name.to_string())
    }

    fn field(configs: &AllConfigs, struct_name: &str, field_name: &str) -> FieldType {
        configs.objects[struct_name]
            .fields
            .iter()
            .find(|field| field.field_name == field_name)
            .map(|field| field.field_type.clone())
            .expect("field")
    }

    #[test]
    fn typesync_attributes_reach_every_position_they_apply_to() {
        let configs = build(CollisionStrategy::Error, &[("lib.rs", TYPESYNC)]).expect("configs");
        let profile = &configs.objects["Profile"];
        assert_eq!(profile.macroforge_derives, ["Default", "Encode"]);
        assert_eq!(profile.annotations, ["@form"]);
        let name = profile
            .fields
            .iter()
            .find(|field| field.field_name == "name")
            .expect("the name field");
        assert_eq!(
            name.annotations,
            [r#"@endec({ rename: "full_name" })"#, "@hidden"]
        );

        let shape = &configs.enums["Shape"];
        assert_eq!(shape.annotations, [r#"@endec({ tag: "kind" })"#]);
        assert_eq!(shape.variants[0].annotations, ["@circle"]);
        match &shape.variants[0].data {
            Some(VariantData::InlineStruct(circle)) => {
                assert_eq!(circle.macroforge_derives, ["Decode"]);
            }
            other => panic!("expected the circle's fields, got {other:?}"),
        }
        assert_eq!(shape.variants[1].annotations, ["@dot"]);
        assert_eq!(configs.newtypes["Slug"].macroforge_derives, ["Encode"]);
    }

    #[test]
    fn a_struct_serde_writes_as_another_type_is_scanned_as_a_newtype() {
        let configs = build(CollisionStrategy::Error, &[("lib.rs", NEWTYPES)]).expect("configs");
        let names: Vec<&str> = configs.newtypes.keys().map(String::as_str).collect();
        assert_eq!(
            names,
            ["Code", "Email", "Label", "Marker", "NonEmptyString", "Pair"]
        );
        for name in &names {
            assert!(
                !configs.objects.contains_key(*name),
                "{name} is not an object"
            );
        }
        let newtype = |name: &str| &configs.newtypes[name];
        assert_eq!(newtype("NonEmptyString").inner, FieldType::String);
        assert_eq!(newtype("NonEmptyString").kind, NewtypeKind::Branded);
        assert_eq!(
            newtype("NonEmptyString").validators,
            [Validator::StringValidator(StringValidator::NonEmpty)]
        );
        assert_eq!(
            newtype("Email").inner,
            FieldType::Other("NonEmptyString".to_owned())
        );
        assert_eq!(newtype("Label").inner, FieldType::String);
        assert_eq!(newtype("Label").validators.len(), 1);
        assert_eq!(newtype("Code").inner, FieldType::Other("Status".to_owned()));
        assert_eq!(newtype("Pair").kind, NewtypeKind::Alias);
        assert_eq!(
            newtype("Pair").inner,
            FieldType::Tuple(vec![FieldType::String, FieldType::U32])
        );
        assert_eq!(newtype("Marker").inner, FieldType::Unit);
    }

    #[test]
    fn schemasync_stores_a_newtype_as_its_inner_value_with_its_validators() {
        let configs = build(CollisionStrategy::Error, &[("lib.rs", NEWTYPES)]).expect("configs");
        let tables = configs
            .into_schemasync()
            .expect("schemasync configs")
            .tables;
        let field = |name: &str| {
            tables["profile"]
                .struct_config
                .fields
                .iter()
                .find(|field| field.field_name == name)
                .expect("field")
        };
        let non_empty = Validator::StringValidator(StringValidator::NonEmpty);
        let email = Validator::StringValidator(StringValidator::Email);
        assert_eq!(field("name").field_type, FieldType::String);
        assert_eq!(field("name").validators, std::slice::from_ref(&non_empty));
        assert_eq!(
            field("email").field_type,
            FieldType::Option(Box::new(FieldType::String))
        );
        // The innermost newtype's validators first, as Rust reads them.
        assert_eq!(field("email").validators, [non_empty, email]);
        assert_eq!(
            field("tags").field_type,
            FieldType::Vec(Box::new(FieldType::String))
        );
        assert!(field("tags").validators.is_empty());
        assert_eq!(
            field("code").field_type,
            FieldType::Other("Status".to_owned())
        );
    }

    #[test]
    fn schemasync_keeps_the_declared_type_of_each_field_holding_a_newtype() {
        let configs = build(CollisionStrategy::Error, &[("lib.rs", NEWTYPES)]).expect("configs");
        let declared = configs
            .into_schemasync()
            .expect("schemasync configs")
            .declared;
        let profile = FieldOwner::Table("profile".to_owned());
        let other = |name: &str| FieldType::Other(name.to_owned());
        assert_eq!(
            declared.field(&profile, "tags"),
            Some(&FieldType::Vec(Box::new(other("NonEmptyString"))))
        );
        assert_eq!(
            declared.field(&profile, "name"),
            Some(&other("NonEmptyString"))
        );
        assert_eq!(declared.field(&profile, "id"), None);
        let non_empty = Validator::StringValidator(StringValidator::NonEmpty);
        let email = Validator::StringValidator(StringValidator::Email);
        assert_eq!(
            declared.newtype(&other("Email")),
            Some((&FieldType::String, [non_empty, email].as_slice()))
        );
        assert_eq!(declared.newtype(&other("Status")), None);
    }

    const POSITIONS: &str = r#"
        use evenframe::Evenframe;
        use std::collections::BTreeMap;

        #[derive(Evenframe)]
        #[validators(StringValidator::NonEmpty)]
        pub struct StepId(String);

        #[derive(Evenframe)]
        pub struct Waiting { pub step: StepId, pub parents: Vec<StepId> }

        #[derive(Evenframe)]
        pub enum Change { Renamed { to: StepId }, Moved(StepId), Idle }

        #[derive(Evenframe)]
        #[serde(untagged)]
        pub enum Loose { Named { to: StepId }, Plain(u32) }

        #[derive(Evenframe)]
        #[serde(untagged)]
        pub enum Vague { Maybe(Option<u8>), Named { to: StepId } }

        #[derive(Evenframe)]
        #[serde(tag = "kind")]
        pub enum Signal { Named { to: StepId }, Quiet }

        #[derive(Evenframe)]
        #[serde(tag = "kind", content = "data")]
        pub enum Message { Text(StepId), Empty }

        #[derive(Evenframe)]
        pub struct Node { pub label: StepId, pub children: Vec<Node> }

        #[derive(Evenframe)]
        pub struct Run {
            pub id: String,
            pub waiting: Vec<Waiting>,
            pub labels: BTreeMap<StepId, Option<StepId>>,
            pub pair: (StepId, u32),
            pub change: Change,
            pub loose: Loose,
            pub signal: Signal,
            pub message: Message,
            pub tree: Node,
            pub vague: Vague,
            pub current: Waiting,
        }
    "#;

    /// The `DEFINE FIELD` statement of `field` on `table`, and the positions
    /// whose validators it could not assert.
    #[cfg(feature = "schemadump")]
    fn defined(
        types: &crate::types::SchemasyncTypes,
        table: &str,
        field: &str,
    ) -> (
        String,
        Vec<crate::schemasync::database::surql::shape::Unasserted>,
    ) {
        let context = crate::schemasync::database::surql::shape::DefineContext {
            tables: &types.tables,
            objects: &types.objects,
            enums: &types.enums,
            declared: &types.declared,
            registry: &crate::types::ForeignTypeRegistry::default(),
            options: crate::schemasync::config::SurqlOptions::default(),
        };
        let table_config = types.tables[table].effective();
        let mut unasserted = Vec::new();
        let statement = table_config
            .struct_config
            .fields
            .iter()
            .find(|candidate| candidate.field_name == field)
            .expect("the field exists")
            .generate_define_statement(
                &FieldOwner::Table(table_config.table_name.clone()),
                table,
                &context,
                &mut unasserted,
            )
            .expect("the field defines");
        (statement, unasserted)
    }

    #[cfg(feature = "schemadump")]
    #[test]
    fn a_newtype_below_a_fields_own_value_is_asserted_where_it_sits() {
        let configs = build(CollisionStrategy::Error, &[("lib.rs", NEWTYPES)]).expect("configs");
        let types = configs.into_schemasync().expect("schemasync configs");
        let (tags, unasserted) = defined(&types, "profile", "tags");
        assert!(
            tags.contains(" ASSERT array::all($value, |$item0| string::len($item0) > 0)"),
            "{tags}"
        );
        assert_eq!(unasserted, []);
        let (name, _) = defined(&types, "profile", "name");
        assert!(name.contains(" ASSERT string::len($value) > 0"), "{name}");
    }

    #[cfg(feature = "schemadump")]
    #[test]
    fn every_stored_position_asserts_the_validators_declared_there() {
        let configs = build(CollisionStrategy::Error, &[("lib.rs", POSITIONS)]).expect("configs");
        let types = configs.into_schemasync().expect("schemasync configs");

        let (waiting, _) = defined(&types, "run", "waiting");
        assert!(
            waiting.contains(
                " ASSERT array::all($value, |$item0| (string::len($item0.step) > 0) AND \
                 (array::all($item0.parents, |$item1| string::len($item1) > 0)))"
            ),
            "{waiting}"
        );

        let (labels, _) = defined(&types, "run", "labels");
        assert!(
            labels.contains(
                "(array::all(object::keys($value), |$item0| string::len($item0) > 0)) AND \
                 (array::all(object::values($value), |$item0| $item0 = NONE OR \
                 (string::len($item0) > 0)))"
            ),
            "{labels}"
        );

        let (pair, _) = defined(&types, "run", "pair");
        assert!(
            pair.contains(" ASSERT string::len($value[0]) > 0"),
            "{pair}"
        );
        // Its zero, `['', 0]`, fails the check, so the field has no fallback.
        assert!(!pair.contains(" DEFAULT "), "{pair}");
        let (current, _) = defined(&types, "run", "current");
        assert!(!current.contains(" DEFAULT "), "{current}");

        let (change, _) = defined(&types, "run", "change");
        assert!(
            change.contains(
                "($value.Renamed = NONE OR (string::len($value.Renamed.to) > 0)) AND \
                 ($value.Moved = NONE OR (string::len($value.Moved) > 0))"
            ),
            "{change}"
        );

        let (signal, _) = defined(&types, "run", "signal");
        assert!(
            signal.contains(" ASSERT $value.kind != 'Named' OR (string::len($value.to) > 0)"),
            "{signal}"
        );

        let (message, _) = defined(&types, "run", "message");
        assert!(
            message.contains(" ASSERT $value.kind != 'Text' OR (string::len($value.data) > 0)"),
            "{message}"
        );

        let (tree, unasserted) = defined(&types, "run", "tree");
        assert!(
            tree.contains(" ASSERT string::len($value.label) > 0"),
            "{tree}"
        );
        assert_eq!(
            unasserted
                .iter()
                .map(|finding| (finding.location.as_str(), finding.reason))
                .collect::<Vec<_>>(),
            [(
                "run.tree.children[].label",
                crate::schemasync::database::surql::shape::UnassertedReason::Recursive
            )]
        );

        let (loose, unasserted) = defined(&types, "run", "loose");
        assert!(
            loose.contains(
                " ASSERT !(type::is_object($value) AND $value.to != NONE) OR \
                 (string::len($value.to) > 0)"
            ),
            "{loose}"
        );
        assert_eq!(unasserted, []);

        // Serde reads any value as `Maybe` first, which SurrealQL cannot
        // state, so the variant after it cannot be told apart.
        let (vague, unasserted) = defined(&types, "run", "vague");
        assert!(!vague.contains(" ASSERT "), "{vague}");
        assert_eq!(
            unasserted
                .iter()
                .map(|finding| finding.location.as_str())
                .collect::<Vec<_>>(),
            ["run.vague::Named.to"]
        );
    }

    #[test]
    fn a_flattened_struct_is_written_as_its_fields_and_a_flattened_map_stays() {
        let configs = build(CollisionStrategy::Error, &[("lib.rs", FLATTENED)]).expect("configs");
        let typesync = configs.for_typesync().expect("typesync view");
        let post = &typesync.objects["Post"];
        let names: Vec<&str> = post
            .fields
            .iter()
            .map(|field| field.field_name.as_str())
            .collect();
        assert_eq!(
            names,
            ["id", "title", "created_by", "note", "lat", "lon", "extras"]
        );
        let lat = &post.fields[4];
        assert_eq!(lat.field_type, FieldType::Option(Box::new(FieldType::F64)));
        assert!(
            lat.wire.serde_optional,
            "a flattened Option's keys may be absent"
        );
        assert!(!post.is_open());
        assert!(typesync.objects["Extras"].is_open());

        let stored = configs.into_schemasync().expect("schemasync view");
        assert_eq!(stored.tables["post"].struct_config.fields.len(), 7);
        assert!(stored.tables["event"].struct_config.is_open());
    }

    #[cfg(feature = "schemadump")]
    #[test]
    fn a_record_with_keys_known_only_from_a_value_is_schemaless_or_flexible() {
        let configs = build(CollisionStrategy::Error, &[("lib.rs", FLATTENED)]).expect("configs");
        let stored = configs.into_schemasync().expect("schemasync view");
        let schema = crate::schemasync::dump::tables_surql(
            &stored,
            &crate::types::ForeignTypeRegistry::default(),
            crate::schemasync::config::SurqlOptions::default(),
        )
        .expect("the schema generates");
        assert!(
            schema.contains("DEFINE TABLE OVERWRITE event SCHEMALESS"),
            "{schema}"
        );
        assert!(
            !schema.contains("FIELD OVERWRITE data ON TABLE event"),
            "{schema}"
        );
        assert!(
            schema.contains("DEFINE TABLE OVERWRITE post SCHEMAFULL"),
            "{schema}"
        );
        assert!(
            schema.contains("DEFINE FIELD OVERWRITE created_by ON TABLE post TYPE string"),
            "{schema}"
        );
        assert!(
            schema.contains("DEFINE FIELD OVERWRITE extras ON TABLE post FLEXIBLE TYPE object"),
            "{schema}"
        );
    }

    #[test]
    fn a_struct_that_flattens_itself_is_refused() {
        let source = r#"
            use evenframe::Evenframe;

            #[derive(Evenframe)]
            pub struct Node { pub name: String, #[serde(flatten)] pub next: Option<Box<Node>> }
        "#;
        let configs = build(CollisionStrategy::Error, &[("lib.rs", source)]).expect("configs");
        let error = configs.for_typesync().unwrap_err().to_string();
        assert!(
            error.contains("`Node` flattens itself (Node → Node)"),
            "{error}"
        );
    }

    #[test]
    fn a_struct_serde_writes_as_another_type_is_described_as_it_and_stored_as_its_fields() {
        let source = r#"
            use evenframe::Evenframe;

            #[derive(Evenframe)]
            #[serde(into = "String", from = "String")]
            pub struct Slug { pub value: String }

            #[derive(Evenframe)]
            pub struct Page { pub id: String, pub slug: Slug }
        "#;
        let configs = build(CollisionStrategy::Error, &[("lib.rs", source)]).expect("configs");
        let slug = &configs.newtypes["Slug"];
        assert_eq!(slug.kind, NewtypeKind::Alias);
        assert_eq!(slug.inner, FieldType::String);
        assert_eq!(slug.pipeline, crate::types::Pipeline::Typesync);
        assert_eq!(
            configs.objects["Slug"].pipeline,
            crate::types::Pipeline::Schemasync
        );

        let typesync = configs.for_typesync().expect("typesync view");
        assert!(!typesync.objects.contains_key("Slug"));
        assert!(typesync.newtypes.contains_key("Slug"));
        let stored = configs.into_schemasync().expect("schemasync view");
        assert!(stored.objects.contains_key("Slug"));
    }

    #[test]
    fn each_tuple_element_keeps_its_validators() {
        let source = r#"
            use evenframe::Evenframe;

            #[derive(Evenframe)]
            pub struct Range(#[validators(NumberValidator::Positive)] i32, String);

            #[derive(Evenframe)]
            pub enum Shape {
                Circle(#[validators(NumberValidator::Positive)] f64),
                Dot,
            }
        "#;
        let configs = build(CollisionStrategy::Error, &[("lib.rs", source)]).expect("configs");
        let positive = Validator::NumberValidator(crate::validator::NumberValidator::Positive);
        assert_eq!(
            configs.newtypes["Range"].element_validators,
            [vec![positive.clone()], Vec::new()]
        );
        let shape = &configs.enums["Shape"];
        assert_eq!(shape.variants[0].element_validators, [vec![positive]]);
        assert!(shape.variants[1].element_validators.is_empty());
    }

    #[test]
    fn a_newtype_that_holds_itself_has_no_stored_form() {
        let source = r#"
            use evenframe::Evenframe;

            #[derive(Evenframe)]
            pub struct Tree(Vec<Tree>);

            #[derive(Evenframe)]
            pub struct Forest { pub id: String, pub tree: Tree }
        "#;
        let configs = build(CollisionStrategy::Error, &[("lib.rs", source)]).expect("configs");
        let error = configs.into_schemasync().unwrap_err().to_string();
        assert!(
            error.contains("`Tree` holds itself (Tree → Tree)"),
            "{error}"
        );
    }

    #[test]
    fn each_reference_resolves_to_the_definition_its_module_names() {
        let configs = build(
            CollisionStrategy::AutoRename,
            &[
                ("lib.rs", "pub mod models;\npub mod handlers;\n"),
                ("models/mod.rs", "pub mod auth;\npub mod billing;\n"),
                ("models/auth.rs", AUTH),
                ("models/billing.rs", BILLING),
                (
                    "handlers.rs",
                    r#"
                    use crate::models::auth::Status;
                    use crate::models::billing::Status as InvoiceState;

                    #[derive(Evenframe)]
                    pub struct Audit {
                        pub account: Status,
                        pub invoice: InvoiceState,
                        pub history: Vec<Option<crate::models::billing::Status>>,
                    }
                    "#,
                ),
            ],
        )
        .expect("configs");

        assert_eq!(field(&configs, "Account", "status"), other("Status"));
        assert_eq!(field(&configs, "Invoice", "status"), other("BillingStatus"));
        assert_eq!(
            field(&configs, "Invoice", "account_status"),
            other("Status")
        );
        assert_eq!(field(&configs, "Audit", "account"), other("Status"));
        assert_eq!(field(&configs, "Audit", "invoice"), other("BillingStatus"));
        assert_eq!(
            field(&configs, "Audit", "history"),
            FieldType::Vec(Box::new(FieldType::Option(Box::new(other(
                "BillingStatus"
            )))))
        );
    }

    #[test]
    fn enum_variant_payloads_follow_a_renamed_definition() {
        let configs = build(
            CollisionStrategy::AutoRename,
            &[("auth.rs", AUTH), ("billing.rs", BILLING)],
        )
        .expect("configs");

        let event = &configs.enums["Event"];
        let payload = |name: &str| {
            event
                .variants
                .iter()
                .find(|variant| variant.name == name)
                .and_then(|variant| variant.data.clone())
                .expect("payload")
        };
        assert_eq!(
            payload("Changed"),
            VariantData::DataStructureRef(other("BillingStatus"))
        );
        let VariantData::InlineStruct(moved) = payload("Moved") else {
            panic!("Moved is a struct variant");
        };
        assert!(
            moved
                .fields
                .iter()
                .all(|field| field.field_type == other("BillingStatus")),
            "{moved:?}"
        );
        assert!(
            configs.enums.contains_key("Status") && configs.enums.contains_key("BillingStatus")
        );
    }

    #[test]
    fn a_struct_and_an_enum_sharing_a_name_collide() {
        let struct_status = "#[derive(Evenframe)]\npub struct Status { pub code: u32 }\n";
        let enum_status = "#[derive(Evenframe)]\npub enum Status { Open, Closed }\n";
        let files = [("a.rs", struct_status), ("b.rs", enum_status)];

        let error = build(CollisionStrategy::Error, &files)
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(
            error.contains("'Status'") && error.contains("a.rs") && error.contains("b.rs"),
            "{error}"
        );

        let configs = build(CollisionStrategy::AutoRename, &files).expect("configs");
        assert!(configs.objects.contains_key("Status"));
        assert!(
            configs.enums.contains_key("BStatus"),
            "{:?}",
            configs.enums.keys()
        );
    }

    #[test]
    fn re_exports_and_glob_imports_resolve_to_their_definition() {
        let configs = build(
            CollisionStrategy::AutoRename,
            &[
                ("lib.rs", "pub mod models;\npub mod reports;\npub mod views;\n"),
                (
                    "models/mod.rs",
                    "pub mod auth;\npub mod billing;\npub use billing::Status as InvoiceStatus;\n",
                ),
                ("models/auth.rs", AUTH),
                ("models/billing.rs", BILLING),
                (
                    "reports.rs",
                    "use crate::models::InvoiceStatus;\n#[derive(Evenframe)]\npub struct Report { pub state: InvoiceStatus }\n",
                ),
                (
                    "views.rs",
                    "use crate::models::billing::*;\n#[derive(Evenframe)]\npub struct View { pub state: Status }\n",
                ),
            ],
        )
        .expect("configs");

        assert_eq!(field(&configs, "Report", "state"), other("BillingStatus"));
        assert_eq!(field(&configs, "View", "state"), other("BillingStatus"));
    }

    #[test]
    fn only_the_standard_library_duration_is_native() {
        let source = r#"
            use std::time::Duration;
            use chrono::Duration as Elapsed;

            #[derive(Evenframe)]
            pub struct Timer {
                pub limit: Duration,
                pub spent: Option<core::time::Duration>,
                pub chrono: Elapsed,
            }
        "#;
        let configs = build(CollisionStrategy::Error, &[("lib.rs", source)]).expect("configs");
        assert_eq!(field(&configs, "Timer", "limit"), FieldType::Duration);
        assert_eq!(
            field(&configs, "Timer", "spent"),
            FieldType::Option(Box::new(FieldType::Duration))
        );
        assert_eq!(field(&configs, "Timer", "chrono"), other("Duration"));

        let scanned = r#"
            #[derive(Evenframe)]
            pub struct Duration { pub minutes: u32 }

            #[derive(Evenframe)]
            pub struct Timer { pub limit: Duration }
        "#;
        let configs = build(CollisionStrategy::Error, &[("lib.rs", scanned)]).expect("configs");
        assert_eq!(field(&configs, "Timer", "limit"), other("Duration"));
    }

    #[test]
    fn a_foreign_type_claiming_duration_wins() {
        let root = TempDir::new().expect("temp dir");
        fs::write(
            root.path().join("Cargo.toml"),
            "[package]\nname = \"shop\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .expect("manifest");
        fs::create_dir_all(root.path().join("src")).expect("source dir");
        fs::write(
            root.path().join("src/lib.rs"),
            "use std::time::Duration;\n#[derive(Evenframe)]\npub struct Timer { pub limit: Duration }\n",
        )
        .expect("source file");
        let foreign = toml::from_str("rust_type_names = [\"Duration\"]\nsurrealdb = \"int\"")
            .expect("foreign type");
        let objects = build_all_configs(&ScanConfig {
            scan_path: root.path().to_path_buf(),
            foreign_types: BTreeMap::from([("Duration".to_string(), foreign)]),
            ..ScanConfig::default()
        })
        .expect("configs")
        .objects;
        assert_eq!(objects["Timer"].fields[0].field_type, other("Duration"));
    }

    #[test]
    fn an_unresolvable_reference_to_a_shared_name_is_rejected() {
        let error = build(
            CollisionStrategy::AutoRename,
            &[
                ("auth.rs", AUTH),
                ("billing.rs", BILLING),
                (
                    "orphan.rs",
                    "#[derive(Evenframe)]\npub struct Orphan { pub state: Status }\n",
                ),
            ],
        )
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
        assert!(
            error.contains("Orphan.state")
                && error.contains("shop::auth::Status")
                && error.contains("shop::billing::Status"),
            "{error}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{BTreeMap, ItemStruct, naming, parse_struct_config, reject_scanned_record_link};

    #[test]
    fn a_scanned_record_link_is_rejected_in_schemasync() {
        let record_link = |pipeline| crate::types::StructConfig {
            struct_name: "RecordLink".to_string(),
            pipeline,
            ..Default::default()
        };
        let structs =
            |pipeline| BTreeMap::from([("RecordLink".to_string(), record_link(pipeline))]);
        let error =
            reject_scanned_record_link(&structs(crate::types::Pipeline::Both), &BTreeMap::new())
                .err()
                .map(|error| error.to_string())
                .unwrap_or_default();
        assert!(
            error.contains("evenframe's record link type in schemasync"),
            "{error}"
        );
        assert!(
            reject_scanned_record_link(
                &structs(crate::types::Pipeline::Typesync),
                &BTreeMap::new()
            )
            .is_ok()
        );
    }

    #[test]
    fn test_process_struct_fields_parses_unique_attribute() {
        let item_struct: ItemStruct = syn::parse_str(
            r#"
            struct User {
                #[unique]
                email: String,
                name: String,
            }
            "#,
        )
        .expect("failed to parse test struct");

        let wire =
            naming::resolve(&syn::DeriveInput::from(item_struct.clone())).expect("names resolve");
        let config = parse_struct_config(&item_struct, wire).expect("expected a struct config");
        let email = config
            .fields
            .iter()
            .find(|f| f.field_name == "email")
            .expect("email field missing");
        let name = config
            .fields
            .iter()
            .find(|f| f.field_name == "name")
            .expect("name field missing");

        assert!(email.unique, "#[unique] field should be marked unique");
        assert!(
            !name.unique,
            "unannotated field should not be marked unique"
        );
    }
}
