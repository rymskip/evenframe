#![cfg(all(
    feature = "scan",
    feature = "arktype",
    feature = "effect",
    feature = "macroforge"
))]

use evenframe_core::config::EvenframeConfig;
use evenframe_core::scan::{ScanConfig, build_all_configs};
use evenframe_core::types::{AllConfigs, ForeignTypeRegistry};
use evenframe_core::typesync::output::{OutputTypes, render_output};
use std::fs;
use tempfile::TempDir;

fn project(source: &str) -> TempDir {
    let directory = TempDir::new().expect("create fixture directory");
    fs::create_dir(directory.path().join("src")).expect("create source directory");
    fs::write(
        directory.path().join("Cargo.toml"),
        "[package]\nname = \"ts_names_fixture\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
    )
    .expect("write fixture manifest");
    fs::write(directory.path().join("src/lib.rs"), source).expect("write source fixture");
    directory
}

#[test]
fn scan_and_render_all_ts_outputs_with_both_policies() {
    let directory = project(
        r#"
        #[derive(Typesync)]
        pub struct Profile {
            pub first_name: String,
            pub point_2d: String,
            #[serde(rename = "keep_name")]
            pub keep_name: String,
            #[evenframe(ts_name = "SCREAMING-KEBAB-CASE")]
            #[serde(rename = "ignored")]
            pub custom_name: String,
            #[serde(skip)]
            pub hidden_field: String,
            pub r#type: String,
        }
        #[derive(Typesync)]
        #[serde(rename_all = "snake_case")]
        pub struct SerdeProfile { pub last_name: String }
        #[derive(Typesync)]
        #[evenframe(all_ts_names = "PascalCase")]
        #[serde(rename_all = "kebab-case")]
        pub struct Overrides {
            pub street_name: String,
            #[evenframe(ts_name = "snake_case")]
            pub postal_code: String,
        }
        #[derive(Typesync)]
        #[serde(tag = "event_kind", rename_all = "kebab-case")]
        pub enum Event {
            FirstEvent { event_name: String },
            #[serde(rename = "ExactTag")]
            SecondEvent,
            #[serde(skip)]
            SkippedEvent { first_name: String, firstName: String },
        }
        #[derive(Typesync)]
        pub enum Choice { FirstChoice }
        #[derive(Schemasync)]
        pub enum DatabaseOnly { DatabasePayload { first_name: String, firstName: String } }
    "#,
    );
    for (policy, first_name, event_name, point_name) in [
        (None, "firstName", "eventName", "point2D"),
        (
            Some("respect_serde"),
            "first_name",
            "event_name",
            "point_2d",
        ),
    ] {
        let naming = policy
            .map(|policy| format!("ts_names = \"{policy}\"\n"))
            .unwrap_or_default();
        let config = EvenframeConfig::parse(
            &format!("[typesync]\n{naming}outputs = [{{kind = \"arktype\", dir = \"arktype\"}}, {{kind = \"effect\", dir = \"effect\"}}, {{kind = \"macroforge\", dir = \"macroforge\"}}]"),
            directory.path().join("evenframe.toml"), false,
        ).expect("parse naming config");
        let scanned =
            build_all_configs(&ScanConfig::from_config(&config)).expect("scan actual sources");
        assert!(scanned.tables.is_empty());
        assert_eq!(
            scanned.objects["Profile"].fields[0].serde_name(),
            "first_name"
        );
        assert_eq!(scanned.objects["Profile"].fields[0].db_name(), "first_name");
        let AllConfigs {
            enums,
            tables,
            objects: structs,
            newtypes,
        } = scanned.for_typesync().unwrap();
        assert!(tables.is_empty());
        assert!(!structs.contains_key("DatabasePayload"));
        let registry = ForeignTypeRegistry::default();
        let types =
            OutputTypes::new(&structs, &enums, &newtypes, &registry).expect("index scanned types");
        for output in &config.typesync.outputs {
            let rendered = render_output(output, &directory.path().join(&output.dir), None, &types)
                .expect("render scanned output");
            let files = rendered.write().expect("write generated output");
            let content = fs::read_to_string(&files[0].path).expect("read generated output");
            for expected in [
                first_name,
                point_name,
                event_name,
                "keep_name",
                "CUSTOM-NAME",
                "StreetName",
                "postal_code",
                "last_name",
                "first-event",
                "ExactTag",
                "event_kind",
                "FirstChoice",
            ] {
                assert!(
                    content.contains(expected),
                    "{} lacks {expected}: {content}",
                    output.kind
                );
            }
            assert!(!content.contains("hiddenField") && !content.contains("hidden_field"));
            assert!(!content.contains("DatabaseOnly") && !content.contains("DatabasePayload"));
            assert!(!content.contains("SkippedEvent") && !content.contains("skipped-event"));
        }
    }
}

#[test]
fn naming_collisions_are_rejected_by_the_real_scan() {
    let directory = project(
        "#[derive(Typesync)] pub struct Collision { pub first_name: String, pub firstName: String }",
    );
    let error = build_all_configs(&ScanConfig {
        scan_path: directory.path().to_path_buf(),
        ..ScanConfig::default()
    })
    .expect_err("reject duplicate TS key");
    assert!(error.to_string().contains("both emit key `firstName`"));
}
