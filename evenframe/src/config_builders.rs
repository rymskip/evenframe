use crate::workspace_scanner::WorkspaceScanner;
use convert_case::{Case, Casing};
use evenframe_core::config::EvenframeConfig;
use evenframe_core::{
    derive::attributes::{
        parse_event_attributes, parse_format_attribute_bin, parse_mock_data_attribute,
        parse_relation_attribute, parse_table_validators,
    },
    schemasync::table::TableConfig,
    schemasync::{DefineConfig, EdgeConfig, EventConfig, PermissionsConfig},
    types::{FieldType, StructConfig, StructField, TaggedUnion, Variant, VariantData},
    validator::{StringValidator, Validator},
};
use std::collections::HashMap;
use std::fs;
use syn::{Fields, FieldsNamed, Item, ItemEnum, ItemStruct, parse_file};
use tracing::{debug, info, trace, warn};

pub fn build_all_configs() -> (
    HashMap<String, TaggedUnion>,
    HashMap<String, TableConfig>,
    HashMap<String, StructConfig>,
) {
    debug!("Starting build_all_configs");
    let mut enum_configs = HashMap::new();
    let mut table_configs = HashMap::new();
    let mut struct_configs = HashMap::new();

    // Load the configuration to get apply_aliases
    let config = match EvenframeConfig::new() {
        Ok(cfg) => cfg,
        Err(e) => {
            warn!("Error loading configuration: {}", e);
            return (HashMap::new(), HashMap::new(), HashMap::new());
        }
    };

    debug!("Creating workspace scanner");
    // Scan the workspace for all Evenframe types
    let scanner = WorkspaceScanner::new(config.general.apply_aliases)
        .expect("Something went wrong initializing the workspace scanner");
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

    debug!("Grouping types by file");
    // Group types by file for efficient parsing
    let mut types_by_file: HashMap<String, Vec<_>> = HashMap::new();
    for evenframe_type in types {
        types_by_file
            .entry(evenframe_type.file_path.clone())
            .or_insert_with(Vec::new)
            .push(evenframe_type);
    }
    debug!("Grouped into {} files", types_by_file.len());

    debug!("Starting first pass: parsing structs and enums");
    // First pass: Parse all structs and enums to build struct_configs
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

        // Process each item in the file
        for item in syntax.items {
            match item {
                Item::Struct(item_struct) => {
                    // Check if this struct is in our list of Evenframe types
                    if let Some(evenframe_type) =
                        file_types.iter().find(|&t| item_struct.ident == t.name)
                    {
                        debug!("Found Evenframe struct: {:?}", item_struct.ident);
                        if let Some(struct_config) = parse_struct_config(&item_struct) {
                            trace!(
                                "Inserting struct config {:?}: {:#?}",
                                &struct_config.struct_name, &struct_config
                            );
                            struct_configs
                                .insert(struct_config.struct_name.clone(), struct_config.clone());

                            if evenframe_type.has_id_field {
                                // Build table struct_config immediately (like before)
                                let table_name = struct_config.struct_name.to_case(Case::Snake);
                                debug!(
                                    "Building table struct_config for: {} (snake_case: {})",
                                    struct_config.struct_name, &table_name
                                );

                                // Parse mock data attribute which now returns MockGenerationConfig directly
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
                    // Check if this enum is in our list of Evenframe types
                    if file_types.iter().any(|t| item_enum.ident == t.name) {
                        debug!("Found Evenframe enum: {}", item_enum.ident);
                        if let Some(tagged_union) = parse_enum_config(&item_enum) {
                            trace!(
                                "Inserting enum config {:?}: {:#?}",
                                &tagged_union.enum_name, &tagged_union
                            );
                            enum_configs
                                .insert(tagged_union.enum_name.clone(), tagged_union.clone());

                            // Also extract inline structs from enum variants
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

    info!(
        "First pass complete. Found {} struct configs, {} enum configs, {} table configs",
        struct_configs.len(),
        enum_configs.len(),
        table_configs.len()
    );

    (enum_configs, table_configs, struct_configs)
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
        struct_name, // Keep original name, don't convert to snake_case
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

        // Parse field type directly to FieldType
        let field_type = FieldType::parse_syn_ty(&field.ty);

        // Parse attributes using the derive module's parsers
        let edge_config = EdgeConfig::parse(field).ok().flatten();
        let define_config = DefineConfig::parse(field).ok().flatten();

        // Parse format
        let format = parse_format_attribute_bin(&field.attrs).ok().flatten();

        // Parse validators - simplified for now
        let validators = vec![];

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

    // Extract StructConfig from each TableConfig and merge into struct_configs
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

#[cfg(test)]
mod tests {
    use super::*;
    use evenframe_core::types::FieldType;

    // ==================== parse_struct_config Tests ====================

    #[test]
    fn test_parse_struct_config_basic_struct() {
        let code = r#"
            pub struct User {
                pub id: String,
                pub name: String,
                pub age: i32,
            }
        "#;

        let file = syn::parse_file(code).unwrap();
        if let Item::Struct(item_struct) = &file.items[0] {
            let config = parse_struct_config(item_struct).unwrap();

            assert_eq!(config.struct_name, "User");
            assert_eq!(config.fields.len(), 3);

            let field_names: Vec<_> = config.fields.iter().map(|f| f.field_name.as_str()).collect();
            assert!(field_names.contains(&"id"));
            assert!(field_names.contains(&"name"));
            assert!(field_names.contains(&"age"));
        }
    }

    #[test]
    fn test_parse_struct_config_with_optional_fields() {
        let code = r#"
            pub struct Profile {
                pub id: String,
                pub bio: Option<String>,
                pub avatar_url: Option<String>,
            }
        "#;

        let file = syn::parse_file(code).unwrap();
        if let Item::Struct(item_struct) = &file.items[0] {
            let config = parse_struct_config(item_struct).unwrap();

            assert_eq!(config.struct_name, "Profile");
            assert_eq!(config.fields.len(), 3);

            // Find the bio field and check it's an Option
            let bio_field = config.fields.iter().find(|f| f.field_name == "bio").unwrap();
            assert!(matches!(bio_field.field_type, FieldType::Option(_)));
        }
    }

    #[test]
    fn test_parse_struct_config_with_vec_field() {
        let code = r#"
            pub struct Team {
                pub id: String,
                pub name: String,
                pub members: Vec<String>,
            }
        "#;

        let file = syn::parse_file(code).unwrap();
        if let Item::Struct(item_struct) = &file.items[0] {
            let config = parse_struct_config(item_struct).unwrap();

            let members_field = config.fields.iter().find(|f| f.field_name == "members").unwrap();
            assert!(matches!(members_field.field_type, FieldType::Vec(_)));
        }
    }

    #[test]
    fn test_parse_struct_config_empty_struct() {
        let code = r#"
            pub struct Empty {}
        "#;

        let file = syn::parse_file(code).unwrap();
        if let Item::Struct(item_struct) = &file.items[0] {
            let config = parse_struct_config(item_struct).unwrap();

            assert_eq!(config.struct_name, "Empty");
            assert!(config.fields.is_empty());
        }
    }

    #[test]
    fn test_parse_struct_config_raw_identifier() {
        let code = r#"
            pub struct Data {
                pub id: String,
                pub r#type: String,
            }
        "#;

        let file = syn::parse_file(code).unwrap();
        if let Item::Struct(item_struct) = &file.items[0] {
            let config = parse_struct_config(item_struct).unwrap();

            // Raw identifier r#type should become just "type"
            let type_field = config.fields.iter().find(|f| f.field_name == "type");
            assert!(type_field.is_some(), "Should find field named 'type' after stripping r#");
        }
    }

    // ==================== parse_enum_config Tests ====================

    #[test]
    fn test_parse_enum_config_unit_variants() {
        let code = r#"
            pub enum Status {
                Active,
                Inactive,
                Pending,
            }
        "#;

        let file = syn::parse_file(code).unwrap();
        if let Item::Enum(item_enum) = &file.items[0] {
            let config = parse_enum_config(item_enum).unwrap();

            assert_eq!(config.enum_name, "Status");
            assert_eq!(config.variants.len(), 3);

            // All variants should have no data
            for variant in &config.variants {
                assert!(variant.data.is_none());
            }

            let variant_names: Vec<_> = config.variants.iter().map(|v| v.name.as_str()).collect();
            assert!(variant_names.contains(&"Active"));
            assert!(variant_names.contains(&"Inactive"));
            assert!(variant_names.contains(&"Pending"));
        }
    }

    #[test]
    fn test_parse_enum_config_tuple_variants() {
        let code = r#"
            pub enum Message {
                Text(String),
                Number(i32),
            }
        "#;

        let file = syn::parse_file(code).unwrap();
        if let Item::Enum(item_enum) = &file.items[0] {
            let config = parse_enum_config(item_enum).unwrap();

            assert_eq!(config.enum_name, "Message");
            assert_eq!(config.variants.len(), 2);

            let text_variant = config.variants.iter().find(|v| v.name == "Text").unwrap();
            assert!(matches!(text_variant.data, Some(VariantData::DataStructureRef(_))));

            let number_variant = config.variants.iter().find(|v| v.name == "Number").unwrap();
            assert!(matches!(number_variant.data, Some(VariantData::DataStructureRef(_))));
        }
    }

    #[test]
    fn test_parse_enum_config_struct_variants() {
        let code = r#"
            pub enum Event {
                Click { x: i32, y: i32 },
                KeyPress { key: String },
            }
        "#;

        let file = syn::parse_file(code).unwrap();
        if let Item::Enum(item_enum) = &file.items[0] {
            let config = parse_enum_config(item_enum).unwrap();

            assert_eq!(config.enum_name, "Event");
            assert_eq!(config.variants.len(), 2);

            let click_variant = config.variants.iter().find(|v| v.name == "Click").unwrap();
            if let Some(VariantData::InlineStruct(struct_config)) = &click_variant.data {
                assert_eq!(struct_config.fields.len(), 2);
                let field_names: Vec<_> = struct_config.fields.iter().map(|f| f.field_name.as_str()).collect();
                assert!(field_names.contains(&"x"));
                assert!(field_names.contains(&"y"));
            } else {
                panic!("Expected InlineStruct variant data");
            }
        }
    }

    #[test]
    fn test_parse_enum_config_mixed_variants() {
        let code = r#"
            pub enum Action {
                None,
                Single(String),
                Multiple { first: String, second: i32 },
            }
        "#;

        let file = syn::parse_file(code).unwrap();
        if let Item::Enum(item_enum) = &file.items[0] {
            let config = parse_enum_config(item_enum).unwrap();

            assert_eq!(config.enum_name, "Action");
            assert_eq!(config.variants.len(), 3);

            let none_variant = config.variants.iter().find(|v| v.name == "None").unwrap();
            assert!(none_variant.data.is_none());

            let single_variant = config.variants.iter().find(|v| v.name == "Single").unwrap();
            assert!(matches!(single_variant.data, Some(VariantData::DataStructureRef(_))));

            let multiple_variant = config.variants.iter().find(|v| v.name == "Multiple").unwrap();
            assert!(matches!(multiple_variant.data, Some(VariantData::InlineStruct(_))));
        }
    }

    #[test]
    fn test_parse_enum_config_empty_enum() {
        let code = r#"
            pub enum Empty {}
        "#;

        let file = syn::parse_file(code).unwrap();
        if let Item::Enum(item_enum) = &file.items[0] {
            let config = parse_enum_config(item_enum).unwrap();

            assert_eq!(config.enum_name, "Empty");
            assert!(config.variants.is_empty());
        }
    }

    #[test]
    fn test_parse_enum_config_tuple_multiple_fields() {
        let code = r#"
            pub enum Coord {
                Point2D(i32, i32),
                Point3D(i32, i32, i32),
            }
        "#;

        let file = syn::parse_file(code).unwrap();
        if let Item::Enum(item_enum) = &file.items[0] {
            let config = parse_enum_config(item_enum).unwrap();

            let point2d = config.variants.iter().find(|v| v.name == "Point2D").unwrap();
            if let Some(VariantData::DataStructureRef(FieldType::Tuple(types))) = &point2d.data {
                assert_eq!(types.len(), 2);
            } else {
                panic!("Expected tuple type with 2 elements");
            }

            let point3d = config.variants.iter().find(|v| v.name == "Point3D").unwrap();
            if let Some(VariantData::DataStructureRef(FieldType::Tuple(types))) = &point3d.data {
                assert_eq!(types.len(), 3);
            } else {
                panic!("Expected tuple type with 3 elements");
            }
        }
    }

    // ==================== process_struct_fields Tests ====================

    #[test]
    fn test_process_struct_fields_basic() {
        let code = r#"
            pub struct Test {
                pub id: String,
                pub count: i32,
                pub active: bool,
            }
        "#;

        let file = syn::parse_file(code).unwrap();
        if let Item::Struct(item_struct) = &file.items[0] {
            if let Fields::Named(ref fields_named) = item_struct.fields {
                let fields = process_struct_fields(fields_named);

                assert_eq!(fields.len(), 3);

                let id_field = fields.iter().find(|f| f.field_name == "id").unwrap();
                assert!(matches!(id_field.field_type, FieldType::String));

                let count_field = fields.iter().find(|f| f.field_name == "count").unwrap();
                assert!(matches!(count_field.field_type, FieldType::I32));

                let active_field = fields.iter().find(|f| f.field_name == "active").unwrap();
                assert!(matches!(active_field.field_type, FieldType::Bool));
            }
        }
    }

    #[test]
    fn test_process_struct_fields_with_hashmap() {
        let code = r#"
            use std::collections::HashMap;
            pub struct Config {
                pub settings: HashMap<String, String>,
            }
        "#;

        let file = syn::parse_file(code).unwrap();
        for item in &file.items {
            if let Item::Struct(item_struct) = item {
                if let Fields::Named(ref fields_named) = item_struct.fields {
                    let fields = process_struct_fields(fields_named);

                    assert_eq!(fields.len(), 1);
                    let settings_field = &fields[0];
                    assert_eq!(settings_field.field_name, "settings");
                    // HashMap should parse to HashMap type
                    assert!(matches!(settings_field.field_type, FieldType::HashMap(_, _)));
                }
            }
        }
    }

    #[test]
    fn test_process_struct_fields_preserves_field_order() {
        let code = r#"
            pub struct Ordered {
                pub first: String,
                pub second: i32,
                pub third: bool,
                pub fourth: f64,
            }
        "#;

        let file = syn::parse_file(code).unwrap();
        if let Item::Struct(item_struct) = &file.items[0] {
            if let Fields::Named(ref fields_named) = item_struct.fields {
                let fields = process_struct_fields(fields_named);

                assert_eq!(fields.len(), 4);
                assert_eq!(fields[0].field_name, "first");
                assert_eq!(fields[1].field_name, "second");
                assert_eq!(fields[2].field_name, "third");
                assert_eq!(fields[3].field_name, "fourth");
            }
        }
    }

    // ==================== merge_tables_and_objects Tests ====================

    #[test]
    fn test_merge_tables_and_objects_empty() {
        let tables: HashMap<String, TableConfig> = HashMap::new();
        let objects: HashMap<String, StructConfig> = HashMap::new();

        let merged = merge_tables_and_objects(&tables, &objects);

        assert!(merged.is_empty());
    }

    #[test]
    fn test_merge_tables_and_objects_only_objects() {
        let tables: HashMap<String, TableConfig> = HashMap::new();
        let mut objects: HashMap<String, StructConfig> = HashMap::new();

        objects.insert(
            "Address".to_string(),
            StructConfig {
                struct_name: "Address".to_string(),
                fields: vec![],
                validators: vec![],
            },
        );

        let merged = merge_tables_and_objects(&tables, &objects);

        assert_eq!(merged.len(), 1);
        assert!(merged.contains_key("Address"));
    }

    #[test]
    fn test_merge_tables_and_objects_only_tables() {
        let mut tables: HashMap<String, TableConfig> = HashMap::new();
        let objects: HashMap<String, StructConfig> = HashMap::new();

        let struct_config = StructConfig {
            struct_name: "User".to_string(),
            fields: vec![],
            validators: vec![],
        };

        tables.insert(
            "user".to_string(),
            TableConfig {
                table_name: "user".to_string(),
                struct_config: struct_config.clone(),
                relation: None,
                permissions: None,
                mock_generation_config: None,
                events: vec![],
            },
        );

        let merged = merge_tables_and_objects(&tables, &objects);

        assert_eq!(merged.len(), 1);
        assert!(merged.contains_key("user"));
    }

    #[test]
    fn test_merge_tables_and_objects_combined() {
        let mut tables: HashMap<String, TableConfig> = HashMap::new();
        let mut objects: HashMap<String, StructConfig> = HashMap::new();

        // Add an object
        objects.insert(
            "Address".to_string(),
            StructConfig {
                struct_name: "Address".to_string(),
                fields: vec![],
                validators: vec![],
            },
        );

        // Add a table
        let user_struct = StructConfig {
            struct_name: "User".to_string(),
            fields: vec![],
            validators: vec![],
        };

        tables.insert(
            "user".to_string(),
            TableConfig {
                table_name: "user".to_string(),
                struct_config: user_struct,
                relation: None,
                permissions: None,
                mock_generation_config: None,
                events: vec![],
            },
        );

        let merged = merge_tables_and_objects(&tables, &objects);

        assert_eq!(merged.len(), 2);
        assert!(merged.contains_key("Address"));
        assert!(merged.contains_key("user"));
    }

    #[test]
    fn test_merge_tables_and_objects_table_overwrites_object() {
        let mut tables: HashMap<String, TableConfig> = HashMap::new();
        let mut objects: HashMap<String, StructConfig> = HashMap::new();

        // Add an object with same key as table will have
        objects.insert(
            "user".to_string(),
            StructConfig {
                struct_name: "OldUser".to_string(),
                fields: vec![],
                validators: vec![],
            },
        );

        // Add a table with same key
        let user_struct = StructConfig {
            struct_name: "NewUser".to_string(),
            fields: vec![],
            validators: vec![],
        };

        tables.insert(
            "user".to_string(),
            TableConfig {
                table_name: "user".to_string(),
                struct_config: user_struct,
                relation: None,
                permissions: None,
                mock_generation_config: None,
                events: vec![],
            },
        );

        let merged = merge_tables_and_objects(&tables, &objects);

        assert_eq!(merged.len(), 1);
        assert_eq!(merged.get("user").unwrap().struct_name, "NewUser");
    }

    #[test]
    fn test_merge_tables_and_objects_preserves_struct_config() {
        let mut tables: HashMap<String, TableConfig> = HashMap::new();
        let objects: HashMap<String, StructConfig> = HashMap::new();

        let field = StructField {
            field_name: "id".to_string(),
            field_type: FieldType::String,
            edge_config: None,
            define_config: None,
            format: None,
            validators: vec![],
            always_regenerate: false,
        };

        let user_struct = StructConfig {
            struct_name: "User".to_string(),
            fields: vec![field],
            validators: vec![],
        };

        tables.insert(
            "user".to_string(),
            TableConfig {
                table_name: "user".to_string(),
                struct_config: user_struct,
                relation: None,
                permissions: None,
                mock_generation_config: None,
                events: vec![],
            },
        );

        let merged = merge_tables_and_objects(&tables, &objects);

        let user_config = merged.get("user").unwrap();
        assert_eq!(user_config.struct_name, "User");
        assert_eq!(user_config.fields.len(), 1);
        assert_eq!(user_config.fields[0].field_name, "id");
    }
}
