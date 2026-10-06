//! test-plugin command: runs output rule plugins against project types
//! and prints what they produce as JSON for scripted assertions.

use crate::cli::TestPluginArgs;
use crate::scan_cache::build_and_record;
use evenframe_core::scan::{ScanConfig, merge_tables_and_objects};
use evenframe_core::{
    config::EvenframeConfig,
    error::Result,
    types::{AllConfigs, ForeignTypeRegistry, NewtypeConfig, StructConfig, TaggedUnion},
    typesync::{macroforge::HelperModule, type_index::TypeIndex},
};
use std::collections::BTreeMap;
use tracing::info;

pub async fn run(args: TestPluginArgs) -> Result<()> {
    let config = EvenframeConfig::new_offline()?;
    let build_config = ScanConfig::from_config(&config);

    let AllConfigs {
        enums,
        tables,
        objects,
        newtypes,
    } = build_and_record(&build_config)?.for_typesync()?;
    let structs = merge_tables_and_objects(tables, objects);

    let registry = ForeignTypeRegistry::from_config(&config.general.foreign_types);

    let mut results: Vec<serde_json::Value> = Vec::new();

    for (name, sc) in &structs {
        if let Some(ref filter) = args.type_name
            && !name.contains(filter)
        {
            continue;
        }

        let has_override =
            sc.output_override.is_some() || sc.fields.iter().any(|f| f.output_override.is_some());

        if args.changed_only && !has_override {
            continue;
        }

        let mut entry = serde_json::json!({
            "name": name,
            "kind": "Struct",
            "has_override": has_override,
        });

        if let Some(ref ov) = sc.output_override {
            entry["override_derives"] = serde_json::json!(ov.macroforge_derives);
            entry["override_annotations"] = serde_json::json!(ov.annotations);
        }

        let mut single = BTreeMap::new();
        single.insert(name.clone(), sc.clone());
        let empty_enums: BTreeMap<String, TaggedUnion> = BTreeMap::new();
        let mut helpers = HelperModule::new("./helpers".to_owned());
        let generated = evenframe_core::typesync::macroforge::generate_macroforge_type_string(
            &TypeIndex::new(&single, &empty_enums)?,
            evenframe_core::typesync::config::ArrayStyle::default(),
            &registry,
            &mut helpers,
            None,
        )?;
        entry["generated_typesync"] = serde_json::Value::String(generated);
        if !helpers.is_empty() {
            entry["generated_helpers"] = serde_json::Value::String(helpers.content());
        }

        let mut field_entries: Vec<serde_json::Value> = Vec::new();
        for field in &sc.fields {
            if let Some(ref ov) = field.output_override {
                field_entries.push(serde_json::json!({
                    "field_name": field.field_name,
                    "override_annotations": ov.annotations,
                }));
            }
        }
        if !field_entries.is_empty() {
            entry["field_overrides"] = serde_json::Value::Array(field_entries);
        }

        results.push(entry);
    }

    for (name, eu) in &enums {
        if let Some(ref filter) = args.type_name
            && !name.contains(filter)
        {
            continue;
        }

        let has_override = eu.output_override.is_some();
        if args.changed_only && !has_override {
            continue;
        }

        let mut entry = serde_json::json!({
            "name": name,
            "kind": "Enum",
            "has_override": has_override,
        });

        if let Some(ref ov) = eu.output_override {
            entry["override_derives"] = serde_json::json!(ov.macroforge_derives);
            entry["override_annotations"] = serde_json::json!(ov.annotations);
        }

        let empty_structs: BTreeMap<String, StructConfig> = BTreeMap::new();
        let mut single_enum = BTreeMap::new();
        single_enum.insert(name.clone(), eu.clone());
        let mut helpers = HelperModule::new("./helpers".to_owned());
        let generated = evenframe_core::typesync::macroforge::generate_macroforge_type_string(
            &TypeIndex::new(&empty_structs, &single_enum)?,
            evenframe_core::typesync::config::ArrayStyle::default(),
            &registry,
            &mut helpers,
            None,
        )?;
        entry["generated_typesync"] = serde_json::Value::String(generated);
        if !helpers.is_empty() {
            entry["generated_helpers"] = serde_json::Value::String(helpers.content());
        }

        results.push(entry);
    }

    for (name, newtype) in newtypes.iter().filter(|(_, newtype)| !newtype.resolve_only) {
        if let Some(ref filter) = args.type_name
            && !name.contains(filter)
        {
            continue;
        }

        let has_override = newtype.output_override.is_some();
        if args.changed_only && !has_override {
            continue;
        }

        let mut entry = serde_json::json!({
            "name": name,
            "kind": "Newtype",
            "has_override": has_override,
        });

        if let Some(ref overridden) = newtype.output_override {
            entry["override_derives"] = serde_json::json!(overridden.macroforge_derives);
            entry["override_annotations"] = serde_json::json!(overridden.annotations);
        }

        let empty_structs: BTreeMap<String, StructConfig> = BTreeMap::new();
        let empty_enums: BTreeMap<String, TaggedUnion> = BTreeMap::new();
        let single_newtype: BTreeMap<String, NewtypeConfig> =
            BTreeMap::from([(name.clone(), newtype.clone())]);
        let mut helpers = HelperModule::new("./helpers".to_owned());
        let generated = evenframe_core::typesync::macroforge::generate_macroforge_type_string(
            &TypeIndex::with_newtypes(&empty_structs, &empty_enums, &single_newtype)?,
            evenframe_core::typesync::config::ArrayStyle::default(),
            &registry,
            &mut helpers,
            None,
        )?;
        entry["generated_typesync"] = serde_json::Value::String(generated);
        if !helpers.is_empty() {
            entry["generated_helpers"] = serde_json::Value::String(helpers.content());
        }

        results.push(entry);
    }

    let count = results.len();
    let override_count = results.iter().filter(|r| r["has_override"] == true).count();
    let output_json = serde_json::to_string_pretty(&results)?;
    println!("{}", output_json);
    info!(
        "{} types processed ({} with overrides)",
        count, override_count
    );

    Ok(())
}
