//! Configuration builders for processing Evenframe types.

use super::workspace_scanner::{ScannedAst, ScannedItem};
use super::{BuildConfig, EvenframeType, WorkspaceScanner};
use crate::error::{EvenframeError, Result};
use crate::{
    derive::{
        attributes::{
            find_duplicate_index_name, indexable_fields, parse_annotation_attributes,
            parse_doccom_attribute, parse_event_attributes, parse_field_index_attributes,
            parse_format_attribute_bin, parse_index_attributes, parse_macroforge_derive_attribute,
            parse_mock_data_attribute, parse_relation_attribute, parse_rust_derives,
            parse_table_validators,
        },
        validator_parser::parse_field_validators,
    },
    schemasync::mockmake::MockGenerationConfig,
    schemasync::table::TableConfig,
    schemasync::{DefineConfig, EdgeConfig, EventConfig, IndexConfig, PermissionsConfig},
    types::{FieldType, StructConfig, StructField, TaggedUnion, Variant, VariantData},
    typesync::config::{CollisionStrategy, StructVariants},
    typesync::struct_variants::declare_payloads,
    validator::{StringValidator, Validator},
};
use convert_case::{Case, Casing};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use syn::{Fields, FieldsNamed, ItemEnum, ItemStruct};
use tracing::{debug, info, trace, warn};

/// All configurations extracted from the workspace.
pub type AllConfigs = (
    BTreeMap<String, TaggedUnion>,
    BTreeMap<String, TableConfig>,
    BTreeMap<String, StructConfig>,
);

/// Builds all configurations from the workspace using the provided build config.
///
/// Returns a tuple of (enums, tables, objects).
pub fn build_all_configs(config: &BuildConfig) -> Result<AllConfigs> {
    debug!("Starting build_all_configs");
    let mut enum_configs = BTreeMap::new();
    let mut table_configs = BTreeMap::new();
    let mut struct_configs = BTreeMap::new();

    debug!("Creating workspace scanner");
    let scanner = WorkspaceScanner::with_path(
        config.scan_path.clone(),
        config.apply_aliases.clone(),
        config.expand_macros,
    )
    .with_extra_files(config.include_files.clone());

    let scanned = scanner.scan()?;
    info!("Found {} Evenframe types", scanned.len());

    process_types(
        scanned,
        &mut enum_configs,
        &mut table_configs,
        &mut struct_configs,
        config.collision_strategy,
    )?;

    info!(
        "First pass complete. Found {} struct configs, {} enum configs, {} table configs",
        struct_configs.len(),
        enum_configs.len(),
        table_configs.len()
    );

    reject_scanned_record_link(&struct_configs, &enum_configs)?;

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

    Ok((enum_configs, table_configs, struct_configs))
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
        /// The table attributes of a struct with an `id` field that is not
        /// `resolve_only`.
        table: Option<Box<TableAttributes>>,
    },
    Enum(TaggedUnion),
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
            let mut config = parse_struct_config(item_struct).map_err(|error| {
                EvenframeError::parse_error(
                    file_path,
                    format!("struct '{}': {error}", item_struct.ident),
                )
            })?;
            config.pipeline = evenframe_type.pipeline;
            config.resolve_only = evenframe_type.resolve_only;
            // A `resolve_only` struct is registered for resolution, so
            // referencing fields inline its shape, but is never materialized
            // as a managed table: no `DEFINE TABLE`, mock, or diff.
            let table = if evenframe_type.has_id_field && !evenframe_type.resolve_only {
                Some(Box::new(parse_table_attributes(
                    item_struct,
                    &config.struct_name,
                    file_path,
                )?))
            } else {
                None
            };
            Ok(ParsedType::Struct { config, table })
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
fn process_types(
    scanned: Vec<ScannedItem>,
    enum_configs: &mut BTreeMap<String, TaggedUnion>,
    table_configs: &mut BTreeMap<String, TableConfig>,
    struct_configs: &mut BTreeMap<String, StructConfig>,
    collision_strategy: CollisionStrategy,
) -> Result<()> {
    let mut by_file: BTreeMap<String, Vec<Result<ParsedType>>> = BTreeMap::new();
    for item in scanned {
        by_file
            .entry(item.evenframe_type.file_path)
            .or_default()
            .push(item.parsed);
    }
    debug!("Processing types from {} files", by_file.len());

    // Which file each type name was first defined in, for collision messages
    let mut struct_origins: BTreeMap<String, String> = BTreeMap::new();
    let mut enum_origins: BTreeMap<String, String> = BTreeMap::new();
    // Renames, applied to field references once every type is collected
    let mut renames: BTreeMap<String, String> = BTreeMap::new();

    for (file_path, parsed_types) in by_file {
        for parsed in parsed_types {
            match parsed? {
                ParsedType::Struct { mut config, table } => {
                    resolve_collision(
                        &mut config.struct_name,
                        &struct_origins,
                        &file_path,
                        collision_strategy,
                        &mut renames,
                    )?;
                    struct_origins.insert(config.struct_name.clone(), file_path.clone());
                    if let Some(table) = table {
                        let table_name = config.struct_name.to_case(Case::Snake);
                        if let Some((_, name)) =
                            find_duplicate_index_name(&table_name, &table.indexes)
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
                    resolve_collision(
                        &mut tagged_union.enum_name,
                        &enum_origins,
                        &file_path,
                        collision_strategy,
                        &mut renames,
                    )?;
                    enum_origins.insert(tagged_union.enum_name.clone(), file_path.clone());
                    enum_configs.insert(tagged_union.enum_name.clone(), tagged_union);
                }
            }
        }
    }

    // Propagate renames through field type references
    if !renames.is_empty() {
        debug!(
            "Propagating {} type renames through field references",
            renames.len()
        );
        for struct_config in struct_configs.values_mut() {
            for field in &mut struct_config.fields {
                rename_field_type(&mut field.field_type, &renames);
            }
        }
        for table_config in table_configs.values_mut() {
            for field in &mut table_config.struct_config.fields {
                rename_field_type(&mut field.field_type, &renames);
            }
        }
    }

    Ok(())
}

/// Renames `name` when another file already defines a type by that name and
/// the strategy allows it, or fails naming both files.
fn resolve_collision(
    name: &mut String,
    origins: &BTreeMap<String, String>,
    file_path: &str,
    collision_strategy: CollisionStrategy,
    renames: &mut BTreeMap<String, String>,
) -> Result<()> {
    let Some(existing_file) = origins.get(name.as_str()) else {
        return Ok(());
    };
    match collision_strategy {
        CollisionStrategy::Error => Err(EvenframeError::Config(format!(
            "Type name collision: '{name}' is defined in both '{existing_file}' and '{file_path}'. \
             Rename one of them, or set collision_strategy = \"auto_rename\" in [typesync] config."
        ))),
        CollisionStrategy::AutoRename => {
            let stem = Path::new(file_path)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("unknown");
            let new_name = format!("{}{name}", stem.to_case(Case::Pascal));
            warn!(
                "Type '{name}' in '{file_path}' renamed to '{new_name}' to avoid collision with '{existing_file}'"
            );
            renames.insert(name.clone(), new_name.clone());
            *name = new_name;
            Ok(())
        }
    }
}

/// Recursively updates type references that were renamed due to collisions.
fn rename_field_type(field_type: &mut FieldType, renames: &BTreeMap<String, String>) {
    match field_type {
        FieldType::Other(name) => {
            if let Some(new_name) = renames.get(name.as_str()) {
                *name = new_name.clone();
            }
        }
        FieldType::Option(inner) | FieldType::Vec(inner) | FieldType::RecordLink(inner) => {
            rename_field_type(inner, renames);
        }
        FieldType::HashMap(k, v) | FieldType::BTreeMap(k, v) => {
            rename_field_type(k, renames);
            rename_field_type(v, renames);
        }
        FieldType::Tuple(items) => {
            for item in items {
                rename_field_type(item, renames);
            }
        }
        FieldType::Struct(fields) => {
            for (_, ft) in fields {
                rename_field_type(ft, renames);
            }
        }
        _ => {}
    }
}

fn parse_struct_config(item_struct: &ItemStruct) -> syn::Result<StructConfig> {
    let struct_name = item_struct.ident.to_string();
    trace!("Parsing struct config for: {}", struct_name);
    let mut fields = Vec::new();

    if let Fields::Named(ref fields_named) = item_struct.fields {
        debug!(
            "Processing {} fields for struct {}",
            fields_named.named.len(),
            struct_name
        );
        fields = process_struct_fields(fields_named)?;
    }

    let table_validators = parse_table_validators(&item_struct.attrs)?;

    let doccom = parse_doccom_attribute(&item_struct.attrs)?;
    let macroforge_derives = parse_macroforge_derive_attribute(&item_struct.attrs)?;
    let annotations = parse_annotation_attributes(&item_struct.attrs)?;
    let rust_derives = parse_rust_derives(&item_struct.attrs);
    let raw_attributes = collect_raw_attributes(&item_struct.attrs);

    Ok(StructConfig {
        struct_name,
        fields,
        validators: table_validators
            .into_iter()
            .map(|v| Validator::StringValidator(StringValidator::StringEmbedded(v)))
            .collect(),
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

fn parse_enum_config(item_enum: &ItemEnum) -> syn::Result<TaggedUnion> {
    let enum_name = item_enum.ident.to_string();
    trace!("Parsing enum config for: {}", enum_name);
    let mut variants = Vec::new();

    let enum_doccom = parse_doccom_attribute(&item_enum.attrs)?;
    let enum_macroforge_derives = parse_macroforge_derive_attribute(&item_enum.attrs)?;
    let enum_annotations = parse_annotation_attributes(&item_enum.attrs)?;
    let representation =
        crate::derive::attributes::parse_serde_enum_representation(&item_enum.attrs)?;
    let enum_rust_derives = parse_rust_derives(&item_enum.attrs);

    for variant in &item_enum.variants {
        let variant_name = variant.ident.to_string();
        trace!("Processing variant: {} in enum {}", variant_name, enum_name);

        let variant_doccom = parse_doccom_attribute(&variant.attrs)?;
        let variant_annotations = parse_annotation_attributes(&variant.attrs)?;
        let variant_macroforge_derives = parse_macroforge_derive_attribute(&variant.attrs)?;

        let data = match &variant.fields {
            Fields::Unit => None,
            Fields::Unnamed(fields) => Some(VariantData::DataStructureRef(
                FieldType::parse_tuple_variant(fields),
            )),
            Fields::Named(fields_named) => {
                debug!(
                    "Processing {} fields for enum struct {}",
                    fields_named.named.len(),
                    variant_name
                );
                let struct_fields = process_struct_fields(fields_named)?;

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

        variants.push(Variant {
            name: variant_name,
            data,
            doccom: variant_doccom,
            annotations: variant_annotations,
            output_override: None,
            raw_attributes: variant_raw_attributes,
            is_default: is_default_variant,
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

fn process_struct_fields(fields_named: &FieldsNamed) -> syn::Result<Vec<StructField>> {
    let mut struct_fields = Vec::new();
    for field in &fields_named.named {
        let field_name = field
            .ident
            .as_ref()
            .expect("Something went wrong getting the field name")
            .to_string();
        let field_name = field_name.trim_start_matches("r#").to_string();

        let field_type = FieldType::parse_syn_ty(&field.ty);

        let edge_config = EdgeConfig::parse(field)?;
        let define_config = DefineConfig::parse(field)?;
        let format = parse_format_attribute_bin(&field.attrs)?;
        let validators = parse_field_validators(&field.attrs)?.validators;
        let doccom = parse_doccom_attribute(&field.attrs)?;
        let annotations = parse_annotation_attributes(&field.attrs)?;

        let field_raw_attributes = collect_raw_attributes(&field.attrs);

        let unique = field
            .attrs
            .iter()
            .any(|attr| attr.path().is_ident("unique"));

        struct_fields.push(StructField {
            field_name,
            field_type,
            edge_config,
            define_config,
            format,
            validators,
            always_regenerate: false,
            doccom,
            annotations,
            unique,
            output_override: None,
            raw_attributes: field_raw_attributes,
        });
    }
    Ok(struct_fields)
}

/// Attributes that evenframe's own attribute parsers handle. Everything
/// else ends up in `StructConfig::raw_attributes` so plugins can see it.
const KNOWN_ATTRS: &[&str] = &[
    "annotation",
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
    "macroforge_derive",
    "mock_data",
    "permissions",
    "relation",
    "serde",
    "subquery",
    "unique",
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
    // downstream consumers that dedup by PascalCase `struct_name` (e.g.
    // `generate_macroforge_for_types` via its `seen_structs` set) see
    // exactly one entry per table (the authoritative one from the table
    // loop) rather than racing HashMap iteration order.
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

/// The types that take part in the typesync pipeline, copied out of the
/// scan, which stays whole for schemasync.
pub fn filter_for_typesync(
    enums: &BTreeMap<String, TaggedUnion>,
    tables: &BTreeMap<String, TableConfig>,
    objects: &BTreeMap<String, StructConfig>,
) -> (
    BTreeMap<String, TaggedUnion>,
    BTreeMap<String, TableConfig>,
    BTreeMap<String, StructConfig>,
) {
    (
        enums
            .iter()
            .filter(|(_, tagged_union)| tagged_union.pipeline.includes_typesync())
            .map(|(name, tagged_union)| (name.clone(), tagged_union.clone()))
            .collect(),
        tables
            .iter()
            .filter(|(_, table)| table.struct_config.pipeline.includes_typesync())
            .map(|(name, table)| (name.clone(), table.clone()))
            .collect(),
        objects
            .iter()
            .filter(|(_, object)| object.pipeline.includes_typesync())
            .map(|(name, object)| (name.clone(), object.clone()))
            .collect(),
    )
}

/// Filter configs to only types that participate in the schemasync pipeline.
pub fn filter_for_schemasync(
    enums: BTreeMap<String, TaggedUnion>,
    tables: BTreeMap<String, TableConfig>,
    objects: BTreeMap<String, StructConfig>,
) -> (
    BTreeMap<String, TaggedUnion>,
    BTreeMap<String, TableConfig>,
    BTreeMap<String, StructConfig>,
) {
    (
        enums
            .into_iter()
            .filter(|(_, tagged_union)| tagged_union.pipeline.includes_schemasync())
            .collect(),
        tables
            .into_iter()
            .filter(|(_, table)| table.struct_config.pipeline.includes_schemasync())
            .collect(),
        objects
            .into_iter()
            .filter(|(_, object)| object.pipeline.includes_schemasync())
            .collect(),
    )
}

// ============================================================
// Rule plugin application
// ============================================================

/// Apply output rule plugins to set output overrides on configs.
///
/// Every plugin sees each table, struct and enum as the scan produced it, and
/// their overrides apply in plugin order. The typesync and schemasync
/// generators read a config's `output_override` before computing output.
#[cfg(feature = "wasm-plugins")]
fn apply_rule_plugins(
    config: &BuildConfig,
    table_configs: &mut BTreeMap<String, TableConfig>,
    struct_configs: &mut BTreeMap<String, StructConfig>,
    enum_configs: &mut BTreeMap<String, TaggedUnion>,
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
    config: &BuildConfig,
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
mod tests {
    use super::{BTreeMap, ItemStruct, parse_struct_config, reject_scanned_record_link};

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

        let config = parse_struct_config(&item_struct).expect("expected a struct config");
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
