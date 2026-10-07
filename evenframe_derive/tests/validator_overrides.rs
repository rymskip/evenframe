//! `#[typesync(validators(...))]` and `#[schemasync(validators(...))]`
//! replace `#[validators(...)]` in their pipeline, while the Rust read runs
//! only `#[validators(...)]`, so a field with overrides alone is not
//! validated in Rust.

use evenframe::Evenframe;
use evenframe::registry::all_configs;
use evenframe::schemasync::dump::tables_surql;
use evenframe::schemasync::format::Format;
use evenframe::types::{ForeignTypeRegistry, StructField};
use evenframe::validator::{StringValidator, Validator};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct Member {
    pub id: String,
    #[validators(StringValidator::NonEmpty)]
    #[typesync(validators(StringValidator::RegexLiteral(Format::Custom("/^\\p{Lu}/u"))))]
    #[schemasync(validators(StringValidator::MinLength(2)))]
    pub name: String,
    /// Checked by the outputs and the schema, but read in Rust as any text.
    #[typesync(validators(non_empty))]
    #[schemasync(validators(max_length = 12))]
    pub nickname: String,
}

fn field<'a>(fields: &'a [StructField], name: &str) -> &'a StructField {
    fields
        .iter()
        .find(|field| field.field_name == name)
        .expect("the field is described")
}

#[test]
fn the_rust_read_runs_only_the_shared_validators() {
    let read = |name: &str, nickname: &str| {
        serde_json::from_value::<Member>(serde_json::json!({
            "id": "member:ada",
            "name": name,
            "nickname": nickname,
        }))
    };
    assert!(read("", "ada").is_err(), "NonEmpty is shared");
    let member = read("a", "").expect("the overrides do not run in Rust");
    assert_eq!(member.name, "a");
    assert_eq!(member.nickname, "");
}

#[test]
fn each_pipeline_view_holds_its_own_validators() {
    let configs = all_configs();
    let typesync = configs.for_typesync().expect("the typesync view builds");
    let typesync_fields = &typesync.objects["Member"].fields;
    let [Validator::StringValidator(StringValidator::RegexLiteral(Format::Custom(pattern)))] =
        field(typesync_fields, "name").validators.as_slice()
    else {
        panic!(
            "expected the JavaScript pattern, got {:?}",
            field(typesync_fields, "name").validators
        );
    };
    assert_eq!(pattern.as_str(), r"^\p{Lu}");
    assert_eq!(pattern.flags(), Some("u"));
    assert_eq!(
        field(typesync_fields, "nickname").validators,
        vec![Validator::StringValidator(StringValidator::NonEmpty)]
    );

    let schemasync = configs
        .into_schemasync()
        .expect("the schemasync view builds");
    let schema_fields = &schemasync.tables["member"].struct_config.fields;
    assert_eq!(
        field(schema_fields, "name").validators,
        vec![Validator::StringValidator(StringValidator::MinLength(2))]
    );
    assert_eq!(
        field(schema_fields, "nickname").validators,
        vec![Validator::StringValidator(StringValidator::MaxLength(12))]
    );

    let schema = tables_surql(
        &schemasync,
        &ForeignTypeRegistry::from_config(&BTreeMap::new()),
        false,
    )
    .expect("the schema dump generates");
    let definition = |name: &str| {
        let prefix = format!("DEFINE FIELD OVERWRITE {name} ON TABLE member ");
        schema
            .lines()
            .find(|line| line.starts_with(&prefix))
            .unwrap_or_else(|| panic!("`{name}` is defined:\n{schema}"))
            .to_owned()
    };
    assert!(
        definition("name").contains("string::len($value) >= 2"),
        "{schema}"
    );
    assert!(
        !schema.contains("p{Lu}"),
        "the JavaScript pattern stays out of the schema"
    );
    assert!(
        definition("nickname").contains("string::len($value) <= 12"),
        "{schema}"
    );
}
