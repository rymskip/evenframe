//! Configuration builders for processing Evenframe types.

use super::{BuildConfig, EvenframeType, WorkspaceScanner};
use crate::error::Result;
use crate::{
    derive::{
        attributes::{
            parse_event_attributes, parse_format_attribute_bin, parse_mock_data_attribute,
            parse_relation_attribute, parse_table_validators,
        },
        validator_parser::parse_field_validators_as_enums,
    },
    schemasync::table::TableConfig,
    schemasync::{DefineConfig, EdgeConfig, EventConfig, PermissionsConfig},
    types::{FieldType, StructConfig, StructField, TaggedUnion, Variant, VariantData},
    validator::{StringValidator, Validator},
};
use convert_case::{Case, Casing};
use std::collections::HashMap;
use std::fs;
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
    let scanner = WorkspaceScanner::with_path(config.scan_path.clone(), config.apply_aliases.clone());

    let types = scanner.scan_for_evenframe_types()?;
    info!("Found {} Evenframe types", types.len());

    process_types(
        &types,
        &mut enum_configs,
        &mut table_configs,
        &mut struct_configs,
    )?;

    info!(
        "First pass complete. Found {} struct configs, {} enum configs, {} table configs",
        struct_configs.len(),
        enum_configs.len(),
        table_configs.len()
    );

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
    let scanner = WorkspaceScanner::with_path(config.scan_path.clone(), config.apply_aliases.clone());

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

    (enum_configs, table_configs, struct_configs)
}

/// Processes found Evenframe types into configurations.
fn process_types(
    types: &[EvenframeType],
    enum_configs: &mut HashMap<String, TaggedUnion>,
    table_configs: &mut HashMap<String, TableConfig>,
    struct_configs: &mut HashMap<String, StructConfig>,
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
                        if let Some(struct_config) = parse_struct_config(&item_struct) {
                            trace!(
                                "Inserting struct config {:?}: {:#?}",
                                &struct_config.struct_name,
                                &struct_config
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
                                    &table_config.table_name,
                                    &struct_config
                                );
                                table_configs.insert(table_name, table_config);
                            }
                        }
                    }
                }
                Item::Enum(item_enum) => {
                    if file_types.iter().any(|t| item_enum.ident == t.name) {
                        debug!("Found Evenframe enum: {}", item_enum.ident);
                        if let Some(tagged_union) = parse_enum_config(&item_enum) {
                            trace!(
                                "Inserting enum config {:?}: {:#?}",
                                &tagged_union.enum_name,
                                &tagged_union
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

    Ok(())
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

    Some(StructConfig {
        struct_name,
        fields,
        validators: table_validators
            .into_iter()
            .map(|v| Validator::StringValidator(StringValidator::StringEmbedded(v)))
            .collect(),
    })
}

fn parse_enum_config(item_enum: &ItemEnum) -> Option<TaggedUnion> {
    let enum_name = item_enum.ident.to_string();
    trace!("Parsing enum config for: {}", enum_name);
    let mut variants = Vec::new();

    for variant in &item_enum.variants {
        let variant_name = variant.ident.to_string();
        trace!("Processing variant: {} in enum {}", variant_name, enum_name);

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
                }))
            }
        };

        variants.push(Variant {
            name: variant_name,
            data,
        });
    }

    Some(TaggedUnion {
        enum_name,
        variants,
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

        struct_fields.push(StructField {
            field_name,
            field_type,
            edge_config,
            define_config,
            format,
            validators,
            always_regenerate: false,
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
