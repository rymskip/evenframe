//! Configuration builders for processing Evenframe types.

use super::{BuildConfig, EvenframeType, WorkspaceScanner};
use crate::error::Result;
use crate::{
    derive::{
        attributes::{
            parse_annotation_attributes, parse_doccom_attribute, parse_event_attributes,
            parse_format_attribute_bin, parse_macroforge_derive_attribute,
            parse_mock_data_attribute, parse_relation_attribute, parse_table_validators,
        },
        validator_parser::parse_field_validators_as_enums,
    },
    schemasync::table::TableConfig,
    schemasync::{DefineConfig, EdgeConfig, EventConfig, PermissionsConfig},
    types::{FieldType, StructConfig, StructField, TaggedUnion, Variant, VariantData},
    typesync::config::CollisionStrategy,
    validator::{StringValidator, Validator},
};
use convert_case::{Case, Casing};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use syn::{Fields, FieldsNamed, Item, ItemEnum, ItemStruct, parse_file};
use tracing::{debug, info, trace, warn};

/// All configurations extracted from the workspace.
pub type AllConfigs = (
    HashMap<String, TaggedUnion>,
    HashMap<String, TableConfig>,
    HashMap<String, StructConfig>,
);

/// Builds all configurations from the workspace using the provided build config.
///
/// Returns a tuple of (enums, tables, objects).
pub fn build_all_configs(config: &BuildConfig) -> Result<AllConfigs> {
    debug!("Starting build_all_configs");
    let mut enum_configs = HashMap::new();
    let mut table_configs = HashMap::new();
    let mut struct_configs = HashMap::new();

    debug!("Creating workspace scanner");
    let scanner =
        WorkspaceScanner::with_path(config.scan_path.clone(), config.apply_aliases.clone());

    let types = scanner.scan_for_evenframe_types()?;
    info!("Found {} Evenframe types", types.len());

    process_types(
        &types,
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

    resolve_relation_endpoints(&mut table_configs, &enum_configs);

    Ok((enum_configs, table_configs, struct_configs))
}

/// Builds all configurations from the workspace using default configuration.
///
/// This loads config from `evenframe.toml` if available, otherwise uses defaults.
/// Returns a tuple of (enums, tables, objects).
pub fn build_all_configs_default() -> AllConfigs {
    debug!("Starting build_all_configs_default");
    let mut enum_configs = HashMap::new();
    let mut table_configs = HashMap::new();
    let mut struct_configs = HashMap::new();

    // Try to load config, fall back to defaults
    let config = match BuildConfig::from_toml() {
        Ok(cfg) => cfg,
        Err(e) => {
            warn!("Error loading configuration: {}, using defaults", e);
            BuildConfig::default()
        }
    };

    debug!("Creating workspace scanner");
    let scanner =
        WorkspaceScanner::with_path(config.scan_path.clone(), config.apply_aliases.clone());

    let types = match scanner.scan_for_evenframe_types() {
        Ok(types) => {
            info!("Found {} Evenframe types", types.len());
            types
        }
        Err(e) => {
            warn!("Error scanning workspace: {}", e);
            return (HashMap::new(), HashMap::new(), HashMap::new());
        }
    };

    if let Err(e) = process_types(
        &types,
        &mut enum_configs,
        &mut table_configs,
        &mut struct_configs,
        CollisionStrategy::Error,
    ) {
        warn!("Error processing types: {}", e);
        return (HashMap::new(), HashMap::new(), HashMap::new());
    }

    info!(
        "First pass complete. Found {} struct configs, {} enum configs, {} table configs",
        struct_configs.len(),
        enum_configs.len(),
        table_configs.len()
    );

    resolve_relation_endpoints(&mut table_configs, &enum_configs);

    (enum_configs, table_configs, struct_configs)
}

/// Resolves `from`/`to` on relation tables by inspecting `in`/`out` field types.
///
/// For each table with a relation config that has empty `from`/`to`, this looks at
/// the `in` and `out` fields' `RecordLink<T>` types and resolves `T` to table names.
/// If `T` is a persistable_union enum, all variant table names are collected.
fn resolve_relation_endpoints(
    table_configs: &mut HashMap<String, TableConfig>,
    enum_configs: &HashMap<String, TaggedUnion>,
) {
    // Snapshot table names to avoid borrow conflicts
    let known_tables: std::collections::HashSet<String> = table_configs.keys().cloned().collect();

    for table_config in table_configs.values_mut() {
        let Some(relation) = table_config.relation.as_mut() else {
            continue;
        };

        // Default edge_name from table_name if empty
        if relation.edge_name.is_empty() {
            relation.edge_name = table_config.table_name.clone();
        }

        // Resolve from/to from in/out field types
        if relation.from.is_empty()
            && let Some(tables) = resolve_field_to_tables(
                &table_config.struct_config,
                "in",
                enum_configs,
                &known_tables,
            )
        {
            debug!(
                "Auto-resolved relation.from for '{}': {:?}",
                table_config.table_name, tables
            );
            relation.from = tables;
        }
        if relation.to.is_empty()
            && let Some(tables) = resolve_field_to_tables(
                &table_config.struct_config,
                "out",
                enum_configs,
                &known_tables,
            )
        {
            debug!(
                "Auto-resolved relation.to for '{}': {:?}",
                table_config.table_name, tables
            );
            relation.to = tables;
        }
    }
}

/// Resolves a relation field (`in` or `out`) to the table names it references.
fn resolve_field_to_tables(
    struct_config: &crate::types::StructConfig,
    field_name: &str,
    enum_configs: &HashMap<String, TaggedUnion>,
    known_tables: &std::collections::HashSet<String>,
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

    // Try direct table match
    let snake = inner_type_name.to_case(Case::Snake);
    if known_tables.contains(&snake) {
        return Some(vec![snake]);
    }

    // Try enum variant resolution
    if let Some(tagged) = enum_configs.get(&inner_type_name) {
        let mut tables = Vec::new();
        for variant in &tagged.variants {
            if let Some(data) = &variant.data {
                let struct_name = match data {
                    VariantData::InlineStruct(s) => &s.struct_name,
                    VariantData::DataStructureRef(FieldType::Other(name)) => name,
                    _ => continue,
                };
                let t = struct_name.to_case(Case::Snake);
                if known_tables.contains(&t) {
                    tables.push(t);
                }
            }
        }
        if !tables.is_empty() {
            return Some(tables);
        }
    }

    None
}

/// Processes found Evenframe types into configurations.
fn process_types(
    types: &[EvenframeType],
    enum_configs: &mut HashMap<String, TaggedUnion>,
    table_configs: &mut HashMap<String, TableConfig>,
    struct_configs: &mut HashMap<String, StructConfig>,
    collision_strategy: CollisionStrategy,
) -> Result<()> {
    debug!("Grouping types by file");
    let mut types_by_file: HashMap<String, Vec<_>> = HashMap::new();
    for evenframe_type in types {
        types_by_file
            .entry(evenframe_type.file_path.clone())
            .or_default()
            .push(evenframe_type);
    }
    debug!("Grouped into {} files", types_by_file.len());

    // Track which file each type name was first defined in (for collision messages)
    let mut struct_origins: HashMap<String, String> = HashMap::new();
    let mut enum_origins: HashMap<String, String> = HashMap::new();
    // Track renames so we can update field references after all types are collected
    let mut renames: HashMap<String, String> = HashMap::new();

    debug!("Starting first pass: parsing structs and enums");
    for (file_path, file_types) in &types_by_file {
        trace!("Processing file: {}", file_path);
        let content = match fs::read_to_string(file_path) {
            Ok(content) => content,
            Err(e) => {
                warn!("Error reading file {}: {}", file_path, e);
                continue;
            }
        };

        let syntax = match parse_file(&content) {
            Ok(syntax) => syntax,
            Err(e) => {
                warn!("Error parsing file {}: {}", file_path, e);
                continue;
            }
        };
        trace!("Parsed {} items from {}", syntax.items.len(), file_path);

        for item in syntax.items {
            match item {
                Item::Struct(item_struct) => {
                    if let Some(evenframe_type) =
                        file_types.iter().find(|&t| item_struct.ident == t.name)
                    {
                        debug!("Found Evenframe struct: {:?}", item_struct.ident);
                        if let Some(mut struct_config) = parse_struct_config(&item_struct) {
                            // Check for name collision
                            if let Some(existing_file) =
                                struct_origins.get(&struct_config.struct_name)
                            {
                                match collision_strategy {
                                    CollisionStrategy::Error => {
                                        return Err(crate::error::EvenframeError::Config(format!(
                                            "Type name collision: '{}' is defined in both '{}' and '{}'. \
                                             Rename one of them, or set collision_strategy = \"auto_rename\" \
                                             in [typesync] config.",
                                            struct_config.struct_name, existing_file, file_path
                                        )));
                                    }
                                    CollisionStrategy::AutoRename => {
                                        let stem = Path::new(file_path)
                                            .file_stem()
                                            .and_then(|s| s.to_str())
                                            .unwrap_or("unknown");
                                        let prefix = stem.to_case(Case::Pascal);
                                        let old_name = struct_config.struct_name.clone();
                                        let new_name = format!("{}{}", prefix, old_name);
                                        warn!(
                                            "Type '{}' in '{}' renamed to '{}' to avoid collision with '{}'",
                                            old_name, file_path, new_name, existing_file
                                        );
                                        struct_config.struct_name = new_name.clone();
                                        renames.insert(old_name, new_name);
                                    }
                                }
                            }

                            struct_origins
                                .insert(struct_config.struct_name.clone(), file_path.clone());
                            trace!(
                                "Inserting struct config {:?}: {:#?}",
                                &struct_config.struct_name, &struct_config
                            );
                            struct_configs
                                .insert(struct_config.struct_name.clone(), struct_config.clone());

                            if evenframe_type.has_id_field {
                                let table_name = struct_config.struct_name.to_case(Case::Snake);
                                debug!(
                                    "Building table config for: {} (snake_case: {})",
                                    struct_config.struct_name, &table_name
                                );

                                let mock_generation_config =
                                    parse_mock_data_attribute(&item_struct.attrs).ok().flatten();

                                let events = match parse_event_attributes(&item_struct.attrs) {
                                    Ok(events) => events,
                                    Err(e) => {
                                        warn!(
                                            error = %e,
                                            struct_name = %struct_config.struct_name,
                                            "Failed to parse event attributes, skipping"
                                        );
                                        Vec::new()
                                    }
                                };

                                let table_config = TableConfig {
                                    table_name: table_name.clone(),
                                    struct_config: struct_config.clone(),
                                    relation: parse_relation_attribute(&item_struct.attrs)
                                        .ok()
                                        .flatten(),
                                    permissions: PermissionsConfig::parse(&item_struct.attrs)
                                        .ok()
                                        .flatten(),
                                    mock_generation_config,
                                    events: events
                                        .into_iter()
                                        .map(|statement| EventConfig { statement })
                                        .collect(),
                                };
                                trace!(
                                    "Inserting table config {:?}: {:#?}",
                                    &table_config.table_name, &struct_config
                                );
                                table_configs.insert(table_name, table_config);
                            }
                        }
                    }
                }
                Item::Enum(item_enum) => {
                    if file_types.iter().any(|t| item_enum.ident == t.name) {
                        debug!("Found Evenframe enum: {}", item_enum.ident);
                        if let Some(mut tagged_union) = parse_enum_config(&item_enum) {
                            // Check for name collision
                            if let Some(existing_file) = enum_origins.get(&tagged_union.enum_name) {
                                match collision_strategy {
                                    CollisionStrategy::Error => {
                                        return Err(crate::error::EvenframeError::Config(format!(
                                            "Type name collision: '{}' is defined in both '{}' and '{}'. \
                                             Rename one of them, or set collision_strategy = \"auto_rename\" \
                                             in [typesync] config.",
                                            tagged_union.enum_name, existing_file, file_path
                                        )));
                                    }
                                    CollisionStrategy::AutoRename => {
                                        let stem = Path::new(file_path)
                                            .file_stem()
                                            .and_then(|s| s.to_str())
                                            .unwrap_or("unknown");
                                        let prefix = stem.to_case(Case::Pascal);
                                        let old_name = tagged_union.enum_name.clone();
                                        let new_name = format!("{}{}", prefix, old_name);
                                        warn!(
                                            "Type '{}' in '{}' renamed to '{}' to avoid collision with '{}'",
                                            old_name, file_path, new_name, existing_file
                                        );
                                        tagged_union.enum_name = new_name.clone();
                                        renames.insert(old_name, new_name);
                                    }
                                }
                            }

                            enum_origins.insert(tagged_union.enum_name.clone(), file_path.clone());
                            trace!(
                                "Inserting enum config {:?}: {:#?}",
                                &tagged_union.enum_name, &tagged_union
                            );
                            enum_configs
                                .insert(tagged_union.enum_name.clone(), tagged_union.clone());

                            for variant in &tagged_union.variants {
                                if let Some(VariantData::InlineStruct(ref enum_struct)) =
                                    variant.data
                                {
                                    struct_configs.insert(
                                        enum_struct.struct_name.clone(),
                                        enum_struct.clone(),
                                    );
                                }
                            }
                        }
                    }
                }
                _ => {}
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

/// Recursively updates type references that were renamed due to collisions.
fn rename_field_type(field_type: &mut FieldType, renames: &HashMap<String, String>) {
    match field_type {
        FieldType::Other(name) => {
            if let Some(new_name) = renames.get(name.as_str()) {
                *name = new_name.clone();
            }
        }
        FieldType::Option(inner)
        | FieldType::Vec(inner)
        | FieldType::RecordLink(inner)
        | FieldType::OrderedFloat(inner) => {
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

fn parse_struct_config(item_struct: &ItemStruct) -> Option<StructConfig> {
    let struct_name = item_struct.ident.to_string();
    trace!("Parsing struct config for: {}", struct_name);
    let mut fields = Vec::new();

    if let Fields::Named(ref fields_named) = item_struct.fields {
        debug!(
            "Processing {} fields for struct {}",
            fields_named.named.len(),
            struct_name
        );
        fields = process_struct_fields(fields_named);
    }

    let table_validators = parse_table_validators(&item_struct.attrs)
        .ok()
        .unwrap_or_default();

    let doccom = parse_doccom_attribute(&item_struct.attrs).ok().flatten();
    let macroforge_derives = parse_macroforge_derive_attribute(&item_struct.attrs)
        .ok()
        .unwrap_or_default();
    let annotations = parse_annotation_attributes(&item_struct.attrs)
        .ok()
        .unwrap_or_default();

    Some(StructConfig {
        struct_name,
        fields,
        validators: table_validators
            .into_iter()
            .map(|v| Validator::StringValidator(StringValidator::StringEmbedded(v)))
            .collect(),
        doccom,
        macroforge_derives,
        annotations,
    })
}

fn parse_enum_config(item_enum: &ItemEnum) -> Option<TaggedUnion> {
    let enum_name = item_enum.ident.to_string();
    trace!("Parsing enum config for: {}", enum_name);
    let mut variants = Vec::new();

    let enum_doccom = parse_doccom_attribute(&item_enum.attrs).ok().flatten();
    let enum_macroforge_derives = parse_macroforge_derive_attribute(&item_enum.attrs)
        .ok()
        .unwrap_or_default();
    let enum_annotations = parse_annotation_attributes(&item_enum.attrs)
        .ok()
        .unwrap_or_default();

    for variant in &item_enum.variants {
        let variant_name = variant.ident.to_string();
        trace!("Processing variant: {} in enum {}", variant_name, enum_name);

        let variant_doccom = parse_doccom_attribute(&variant.attrs).ok().flatten();
        let variant_annotations = parse_annotation_attributes(&variant.attrs)
            .ok()
            .unwrap_or_default();
        let variant_macroforge_derives = parse_macroforge_derive_attribute(&variant.attrs)
            .ok()
            .unwrap_or_default();

        let data = match &variant.fields {
            Fields::Unit => None,
            Fields::Unnamed(fields) => {
                if fields.unnamed.is_empty() {
                    None
                } else if fields.unnamed.len() == 1 {
                    let field = &fields.unnamed[0];
                    let field_type = FieldType::parse_syn_ty(&field.ty);
                    Some(VariantData::DataStructureRef(field_type))
                } else {
                    let field_types: Vec<_> = fields
                        .unnamed
                        .iter()
                        .map(|f| FieldType::parse_syn_ty(&f.ty))
                        .collect();
                    Some(VariantData::DataStructureRef(FieldType::Tuple(field_types)))
                }
            }
            Fields::Named(fields_named) => {
                debug!(
                    "Processing {} fields for enum struct {}",
                    fields_named.named.len(),
                    variant_name
                );
                let struct_fields = process_struct_fields(fields_named);

                Some(VariantData::InlineStruct(StructConfig {
                    struct_name: variant_name.clone(),
                    fields: struct_fields,
                    validators: vec![],
                    doccom: None,
                    macroforge_derives: variant_macroforge_derives,
                    annotations: vec![],
                }))
            }
        };

        variants.push(Variant {
            name: variant_name,
            data,
            doccom: variant_doccom,
            annotations: variant_annotations,
        });
    }

    Some(TaggedUnion {
        enum_name,
        variants,
        doccom: enum_doccom,
        macroforge_derives: enum_macroforge_derives,
        annotations: enum_annotations,
    })
}

fn process_struct_fields(fields_named: &FieldsNamed) -> Vec<StructField> {
    let mut struct_fields = Vec::new();
    for field in &fields_named.named {
        let field_name = field
            .ident
            .as_ref()
            .expect("Something went wrong getting the field name")
            .to_string();
        let field_name = field_name.trim_start_matches("r#").to_string();

        let field_type = FieldType::parse_syn_ty(&field.ty);

        let edge_config = EdgeConfig::parse(field).ok().flatten();
        let define_config = DefineConfig::parse(field).ok().flatten();
        let format = parse_format_attribute_bin(&field.attrs).ok().flatten();
        let validators = parse_field_validators_as_enums(&field.attrs);
        let doccom = parse_doccom_attribute(&field.attrs).ok().flatten();
        let annotations = parse_annotation_attributes(&field.attrs)
            .ok()
            .unwrap_or_default();

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
            unique: false,
            mock_plugin: None,
        });
    }
    struct_fields
}

/// Merges tables and objects into a single struct config map.
pub fn merge_tables_and_objects(
    tables: &HashMap<String, TableConfig>,
    objects: &HashMap<String, StructConfig>,
) -> HashMap<String, StructConfig> {
    debug!(
        "Merging {} tables and {} objects",
        tables.len(),
        objects.len()
    );
    let mut struct_configs = objects.clone();

    for (name, table_config) in tables {
        trace!("Merging table config for: {}", name);
        struct_configs.insert(name.clone(), table_config.struct_config.clone());
    }

    debug!(
        "Merge complete. Total struct configs: {}",
        struct_configs.len()
    );
    struct_configs
}
