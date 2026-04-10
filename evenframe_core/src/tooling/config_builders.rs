//! Configuration builders for processing Evenframe types.

use super::{BuildConfig, EvenframeType, WorkspaceScanner};
use crate::error::Result;
use crate::{
    derive::{
        attributes::{
            parse_annotation_attributes, parse_doccom_attribute, parse_event_attributes,
            parse_format_attribute_bin, parse_macroforge_derive_attribute,
            parse_mock_data_attribute, parse_relation_attribute, parse_rust_derives,
            parse_table_validators,
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
    let scanner = WorkspaceScanner::with_path(
        config.scan_path.clone(),
        config.apply_aliases.clone(),
        config.expand_macros,
    );

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
    #[cfg(feature = "wasm-plugins")]
    if !config.output_rule_plugins.is_empty() {
        apply_rule_plugins(
            config,
            &mut table_configs,
            &mut struct_configs,
            &mut enum_configs,
        );
    }

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
    let scanner = WorkspaceScanner::with_path(
        config.scan_path.clone(),
        config.apply_aliases.clone(),
        config.expand_macros,
    );

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

/// Recursively collects all `Item::Struct` and `Item::Enum` from nested `mod` blocks.
/// For non-expanded files (no `Item::Mod`), this returns the same items unchanged.
fn collect_items_flat(items: Vec<Item>) -> Vec<Item> {
    let mut result = Vec::new();
    for item in items {
        match item {
            Item::Mod(ref m) => {
                if let Some((_, ref mod_items)) = m.content {
                    result.extend(collect_items_flat(mod_items.clone()));
                }
            }
            item @ (Item::Struct(_) | Item::Enum(_)) => result.push(item),
            _ => {}
        }
    }
    result
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

        for item in collect_items_flat(syntax.items) {
            match item {
                Item::Struct(item_struct) => {
                    if let Some(evenframe_type) =
                        file_types.iter().find(|&t| item_struct.ident == t.name)
                    {
                        debug!("Found Evenframe struct: {:?}", item_struct.ident);
                        if let Some(mut struct_config) = parse_struct_config(&item_struct) {
                            struct_config.pipeline = evenframe_type.pipeline;
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
                    if let Some(evenframe_type) =
                        file_types.iter().find(|&t| item_enum.ident == t.name)
                    {
                        debug!("Found Evenframe enum: {}", item_enum.ident);
                        if let Some(mut tagged_union) = parse_enum_config(&item_enum) {
                            tagged_union.pipeline = evenframe_type.pipeline;
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
    let rust_derives = parse_rust_derives(&item_struct.attrs);

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
        pipeline: crate::types::Pipeline::default(),
        rust_derives,
        output_override: None,
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
    let representation =
        crate::derive::attributes::parse_serde_enum_representation(&item_enum.attrs)
            .ok()
            .unwrap_or_default();
    let enum_rust_derives = parse_rust_derives(&item_enum.attrs);

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
                    pipeline: crate::types::Pipeline::default(),
                    rust_derives: vec![],
                    output_override: None,
                }))
            }
        };

        variants.push(Variant {
            name: variant_name,
            data,
            doccom: variant_doccom,
            annotations: variant_annotations,
            output_override: None,
        });
    }

    Some(TaggedUnion {
        enum_name,
        variants,
        representation,
        doccom: enum_doccom,
        macroforge_derives: enum_macroforge_derives,
        annotations: enum_annotations,
        pipeline: crate::types::Pipeline::default(),
        rust_derives: enum_rust_derives,
        output_override: None,
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
            output_override: None,
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

/// Filter configs to only types that participate in the typesync pipeline.
pub fn filter_for_typesync(
    enums: HashMap<String, TaggedUnion>,
    tables: HashMap<String, TableConfig>,
    objects: HashMap<String, StructConfig>,
) -> (
    HashMap<String, TaggedUnion>,
    HashMap<String, TableConfig>,
    HashMap<String, StructConfig>,
) {
    (
        enums
            .into_iter()
            .filter(|(_, v)| v.pipeline.includes_typesync())
            .collect(),
        tables
            .into_iter()
            .filter(|(_, v)| v.struct_config.pipeline.includes_typesync())
            .collect(),
        objects
            .into_iter()
            .filter(|(_, v)| v.pipeline.includes_typesync())
            .collect(),
    )
}

/// Filter configs to only types that participate in the schemasync pipeline.
pub fn filter_for_schemasync(
    enums: HashMap<String, TaggedUnion>,
    tables: HashMap<String, TableConfig>,
    objects: HashMap<String, StructConfig>,
) -> (
    HashMap<String, TaggedUnion>,
    HashMap<String, TableConfig>,
    HashMap<String, StructConfig>,
) {
    (
        enums
            .into_iter()
            .filter(|(_, v)| v.pipeline.includes_schemasync())
            .collect(),
        tables
            .into_iter()
            .filter(|(_, v)| v.struct_config.pipeline.includes_schemasync())
            .collect(),
        objects
            .into_iter()
            .filter(|(_, v)| v.pipeline.includes_schemasync())
            .collect(),
    )
}

// ============================================================
// Rule plugin application
// ============================================================

/// Apply output rule plugins to set output overrides on configs.
///
/// For each table/struct/enum, calls the plugin and sets the `output_override`
/// field directly. The typesync and schemasync generators check this field
/// before computing output — if set, they use the override string directly.
#[cfg(feature = "wasm-plugins")]
#[cfg(feature = "wasm-plugins")]
fn apply_rule_plugins(
    config: &BuildConfig,
    table_configs: &mut HashMap<String, TableConfig>,
    struct_configs: &mut HashMap<String, StructConfig>,
    enum_configs: &mut HashMap<String, TaggedUnion>,
) {
    use crate::typesync::plugin::OutputRulePluginManager;
    use crate::typesync::plugin_types::TypeKind;

    let mut pm = match OutputRulePluginManager::new(&config.output_rule_plugins, &config.scan_path)
    {
        Ok(pm) => pm,
        Err(e) => {
            warn!("Failed to load output rule plugins: {}", e);
            return;
        }
    };

    // Process table configs (handler structs)
    let table_names: Vec<String> = table_configs.keys().cloned().collect();
    info!(
        "Applying output rule plugins to {} tables",
        table_names.len()
    );
    for table_name in &table_names {
        let tc = &table_configs[table_name];
        let input = build_plugin_input(
            &tc.struct_config,
            TypeKind::Struct,
            &tc.table_name,
            tc.relation.is_some(),
        );

        for plugin_name in pm.plugin_names().to_vec() {
            match pm.transform_type(&plugin_name, &input) {
                Ok(plugin_output) => {
                    if plugin_output.error.is_some() {
                        continue;
                    }
                    let tc = table_configs.get_mut(table_name).unwrap();
                    let to = &plugin_output.type_override;
                    if !to.macroforge_derives.is_empty() || !to.annotations.is_empty() {
                        let mut ov = tc.struct_config.clone();
                        ov.output_override = None;
                        if !to.macroforge_derives.is_empty() {
                            ov.macroforge_derives = to.macroforge_derives.clone();
                        }
                        if !to.annotations.is_empty() {
                            for ann in &to.annotations {
                                if !ov.annotations.contains(ann) {
                                    ov.annotations.push(ann.clone());
                                }
                            }
                        }
                        tc.struct_config.output_override = Some(Box::new(ov));
                    }
                    if let Some(ref perms) = to.permissions {
                        tc.permissions = Some(crate::schemasync::PermissionsConfig {
                            all_permissions: None,
                            select_permissions: Some(perms.select.clone()),
                            create_permissions: Some(perms.create.clone()),
                            update_permissions: Some(perms.update.clone()),
                            delete_permissions: Some(perms.delete.clone()),
                        });
                    }
                    for event in &to.events {
                        tc.events.push(crate::schemasync::EventConfig {
                            statement: event.statement.clone(),
                        });
                    }
                    for (field_name, field_override) in &plugin_output.field_overrides {
                        if let Some(field) = tc
                            .struct_config
                            .fields
                            .iter_mut()
                            .find(|f| &f.field_name == field_name)
                            && !field_override.annotations.is_empty()
                        {
                            let mut ov = field.clone();
                            ov.output_override = None;
                            ov.annotations = field_override.annotations.clone();
                            field.output_override = Some(Box::new(ov));
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        "Output rule plugin '{}' failed for table '{}': {}",
                        plugin_name, table_name, e
                    );
                }
            }
        }
    }

    // Process non-table struct configs
    let struct_names: Vec<String> = struct_configs.keys().cloned().collect();
    info!(
        "Applying output rule plugins to {} structs",
        struct_names.len()
    );
    for struct_name in &struct_names {
        let sc = &struct_configs[struct_name];
        let input = build_plugin_input(sc, TypeKind::Struct, "", false);

        for plugin_name in pm.plugin_names().to_vec() {
            match pm.transform_type(&plugin_name, &input) {
                Ok(plugin_output) => {
                    if plugin_output.error.is_some() {
                        continue;
                    }
                    let sc = struct_configs.get_mut(struct_name).unwrap();
                    let to = &plugin_output.type_override;
                    if !to.macroforge_derives.is_empty() || !to.annotations.is_empty() {
                        let mut ov = sc.clone();
                        ov.output_override = None;
                        if !to.macroforge_derives.is_empty() {
                            ov.macroforge_derives = to.macroforge_derives.clone();
                        }
                        if !to.annotations.is_empty() {
                            for ann in &to.annotations {
                                if !ov.annotations.contains(ann) {
                                    ov.annotations.push(ann.clone());
                                }
                            }
                        }
                        sc.output_override = Some(Box::new(ov));
                    }
                    for (field_name, field_override) in &plugin_output.field_overrides {
                        if let Some(field) =
                            sc.fields.iter_mut().find(|f| &f.field_name == field_name)
                            && !field_override.annotations.is_empty()
                        {
                            let mut ov = field.clone();
                            ov.output_override = None;
                            ov.annotations = field_override.annotations.clone();
                            field.output_override = Some(Box::new(ov));
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        "Output rule plugin '{}' failed for struct '{}': {}",
                        plugin_name, struct_name, e
                    );
                }
            }
        }
    }

    // Process enum configs
    let enum_names: Vec<String> = enum_configs.keys().cloned().collect();
    info!("Applying output rule plugins to {} enums", enum_names.len());
    for enum_name in &enum_names {
        let ec = &enum_configs[enum_name];
        let input = build_plugin_input_enum(ec);

        for plugin_name in pm.plugin_names().to_vec() {
            match pm.transform_type(&plugin_name, &input) {
                Ok(plugin_output) => {
                    if plugin_output.error.is_some() {
                        continue;
                    }
                    let ec = enum_configs.get_mut(enum_name).unwrap();
                    let to = &plugin_output.type_override;
                    if !to.macroforge_derives.is_empty() || !to.annotations.is_empty() {
                        let mut ov = ec.clone();
                        ov.output_override = None;
                        if !to.macroforge_derives.is_empty() {
                            ov.macroforge_derives = to.macroforge_derives.clone();
                        }
                        if !to.annotations.is_empty() {
                            for ann in &to.annotations {
                                if !ov.annotations.contains(ann) {
                                    ov.annotations.push(ann.clone());
                                }
                            }
                        }
                        ec.output_override = Some(Box::new(ov));
                    }
                }
                Err(e) => {
                    warn!(
                        "Output rule plugin '{}' failed for enum '{}': {}",
                        plugin_name, enum_name, e
                    );
                }
            }
        }
    }

    info!("Output rule plugins applied to all configs");
}

#[cfg(feature = "wasm-plugins")]
fn build_plugin_input(
    sc: &StructConfig,
    kind: crate::typesync::plugin_types::TypeKind,
    table_name: &str,
    is_relation: bool,
) -> crate::typesync::plugin_types::OutputRulePluginInput {
    use crate::typesync::plugin_types::{OutputRulePluginFieldInfo, OutputRulePluginInput};

    OutputRulePluginInput {
        type_name: sc.struct_name.clone(),
        kind,
        rust_derives: sc.rust_derives.clone(),
        annotations: sc.annotations.clone(),
        pipeline: format!("{:?}", sc.pipeline),
        generator: String::new(),
        fields: sc
            .fields
            .iter()
            .map(|f| OutputRulePluginFieldInfo {
                field_name: f.field_name.clone(),
                field_type: f.field_type.canonical_name(),
                annotations: f.annotations.clone(),
                validators: f.validators.iter().map(|v| format!("{:?}", v)).collect(),
                is_optional: matches!(&f.field_type, FieldType::Option(_)),
                record_link_target: match &f.field_type {
                    FieldType::RecordLink(inner) => Some(inner.canonical_name()),
                    FieldType::Option(inner) => match inner.as_ref() {
                        FieldType::RecordLink(inner) => Some(inner.canonical_name()),
                        _ => None,
                    },
                    _ => None,
                },
                vec_inner_type: match &f.field_type {
                    FieldType::Vec(inner) => Some(format!("{:?}", inner)),
                    _ => None,
                },
                has_explicit_format: f.format.is_some(),
                existing_format: f.format.as_ref().map(|fmt| format!("{:?}", fmt)),
                has_explicit_define: f.define_config.is_some(),
            })
            .collect(),
        table_name: table_name.to_string(),
        is_relation,
        has_explicit_permissions: false, // Not used in override model
        has_explicit_events: false,
        has_explicit_mock_data: false,
        existing_macroforge_derives: sc.macroforge_derives.clone(),
    }
}

#[cfg(feature = "wasm-plugins")]
fn build_plugin_input_enum(
    ec: &TaggedUnion,
) -> crate::typesync::plugin_types::OutputRulePluginInput {
    use crate::typesync::plugin_types::{
        OutputRulePluginFieldInfo, OutputRulePluginInput, TypeKind,
    };

    OutputRulePluginInput {
        type_name: ec.enum_name.clone(),
        kind: TypeKind::Enum,
        rust_derives: ec.rust_derives.clone(),
        annotations: ec.annotations.clone(),
        pipeline: format!("{:?}", ec.pipeline),
        generator: String::new(),
        fields: ec
            .variants
            .iter()
            .map(|v| OutputRulePluginFieldInfo {
                field_name: v.name.clone(),
                field_type: match &v.data {
                    Some(d) => format!("{:?}", d),
                    None => "Unit".to_string(),
                },
                annotations: v.annotations.clone(),
                validators: vec![],
                is_optional: false,
                record_link_target: None,
                vec_inner_type: None,
                has_explicit_format: false,
                existing_format: None,
                has_explicit_define: false,
            })
            .collect(),
        table_name: String::new(),
        is_relation: false,
        has_explicit_permissions: false,
        has_explicit_events: false,
        has_explicit_mock_data: false,
        existing_macroforge_derives: ec.macroforge_derives.clone(),
    }
}
