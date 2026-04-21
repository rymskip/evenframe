//! test-plugin command — runs output rule plugins against project types
//! and prints what they produce as JSON for scripted assertions.

use crate::cli::{Cli, TestPluginArgs};
use crate::config_builders;
use evenframe_core::{
    config::EvenframeConfig,
    error::Result,
    types::{ForeignTypeRegistry, StructConfig, TaggedUnion},
};
use std::collections::BTreeMap;
use tracing::info;

pub async fn run(_cli: &Cli, args: TestPluginArgs) -> Result<()> {
    let config = EvenframeConfig::new()?;
    let build_config = config_builders::BuildConfig::from_toml()?;

    let (enums, tables, objects) = config_builders::build_all_configs(&build_config)?;
    let (enums, tables, objects) = config_builders::filter_for_typesync(enums, tables, objects);
    let structs = config_builders::merge_tables_and_objects(&tables, &objects);

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
        let generated = evenframe_core::typesync::macroforge::generate_macroforge_type_string(
            &single,
            &empty_enums,
            false,
            evenframe_core::typesync::config::ArrayStyle::default(),
            &registry,
        );
        entry["generated_typesync"] = serde_json::Value::String(generated);

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
        let generated = evenframe_core::typesync::macroforge::generate_macroforge_type_string(
            &empty_structs,
            &single_enum,
            false,
            evenframe_core::typesync::config::ArrayStyle::default(),
            &registry,
        );
        entry["generated_typesync"] = serde_json::Value::String(generated);

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
