#![cfg(feature = "typesync")]

use evenframe_core::config::ForeignTypeConfig;
use evenframe_core::types::{ForeignTypeRegistry, StructConfig, TaggedUnion};
use evenframe_core::typesync::config::StructVariants;
use evenframe_core::typesync::struct_variants::declare_payloads;
use std::collections::BTreeMap;

#[derive(serde::Deserialize)]
struct TypesyncFixture {
    structs: BTreeMap<String, StructConfig>,
    enums: BTreeMap<String, TaggedUnion>,
    #[serde(default)]
    foreign_types: BTreeMap<String, ForeignTypeConfig>,
    #[serde(default)]
    struct_variants: StructVariants,
}

fn load_typesync_fixture(
    path: &str,
) -> (
    BTreeMap<String, StructConfig>,
    BTreeMap<String, TaggedUnion>,
    ForeignTypeRegistry,
) {
    let input = std::fs::read_to_string(path).unwrap();
    let mut fixture: TypesyncFixture = serde_json::from_str(&input).unwrap();
    if fixture.struct_variants == StructVariants::Named {
        declare_payloads(&mut fixture.structs, &mut fixture.enums).unwrap();
    }
    let registry = ForeignTypeRegistry::from_config(&fixture.foreign_types);
    (fixture.structs, fixture.enums, registry)
}

#[cfg(feature = "arktype")]
mod arktype {
    pub fn run(
        spec_input_file: &str,
        _expected_file: &str,
        _test_directory: &str,
        _file_type: &str,
    ) {
        let (structs, enums, registry) = crate::load_typesync_fixture(spec_input_file);
        let output = evenframe_core::typesync::arktype::generate_arktype_type_string(
            &evenframe_core::typesync::type_index::TypeIndex::new(&structs, &enums).unwrap(),
            &registry,
        )
        .unwrap();
        let name = std::path::Path::new(spec_input_file)
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap();
        insta::assert_snapshot!(format!("arktype_{name}"), output);
    }

    tests_macros::gen_tests! { "tests/specs/typesync/*.json", crate::arktype::run, "typesync" }
}

#[cfg(feature = "effect")]
mod effect {
    pub fn run(
        spec_input_file: &str,
        _expected_file: &str,
        _test_directory: &str,
        _file_type: &str,
    ) {
        let (structs, enums, registry) = crate::load_typesync_fixture(spec_input_file);
        let output = evenframe_core::typesync::effect::generate_effect_schema_string(
            &evenframe_core::typesync::type_index::TypeIndex::new(&structs, &enums).unwrap(),
            true,
            &registry,
        )
        .unwrap();
        let name = std::path::Path::new(spec_input_file)
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap();
        insta::assert_snapshot!(format!("effect_{name}"), output);
    }

    tests_macros::gen_tests! { "tests/specs/typesync/*.json", crate::effect::run, "typesync" }
}

#[cfg(feature = "macroforge")]
mod macroforge {
    pub fn run(
        spec_input_file: &str,
        _expected_file: &str,
        _test_directory: &str,
        _file_type: &str,
    ) {
        let (structs, enums, registry) = crate::load_typesync_fixture(spec_input_file);
        let mut helpers =
            evenframe_core::typesync::macroforge::HelperModule::new("./helpers".to_owned());
        let interfaces = evenframe_core::typesync::macroforge::generate_macroforge_type_string(
            &evenframe_core::typesync::type_index::TypeIndex::new(&structs, &enums).unwrap(),
            Default::default(),
            &registry,
            &mut helpers,
        )
        .unwrap();
        let output = if helpers.is_empty() {
            interfaces
        } else {
            format!("{interfaces}\n// helpers\n{}", helpers.content())
        };
        let name = std::path::Path::new(spec_input_file)
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap();
        insta::assert_snapshot!(format!("macroforge_{name}"), output);
    }

    tests_macros::gen_tests! { "tests/specs/typesync/*.json", crate::macroforge::run, "typesync" }
}

#[cfg(feature = "protobuf")]
mod protobuf {
    pub fn run(
        spec_input_file: &str,
        _expected_file: &str,
        _test_directory: &str,
        _file_type: &str,
    ) {
        let (structs, enums, registry) = crate::load_typesync_fixture(spec_input_file);
        let output = evenframe_core::typesync::protobuf::generate_protobuf_schema_string(
            &structs, &enums, None, false, &registry,
        )
        .unwrap();
        let name = std::path::Path::new(spec_input_file)
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap();
        insta::assert_snapshot!(format!("protobuf_{name}"), output);
    }

    tests_macros::gen_tests! { "tests/specs/typesync/*.json", crate::protobuf::run, "typesync" }
}

/// The protobuf output with protoc-gen-validate rules, which only
/// `import_validate` writes.
#[cfg(feature = "protobuf")]
mod protobuf_validated {
    pub fn run(
        spec_input_file: &str,
        _expected_file: &str,
        _test_directory: &str,
        _file_type: &str,
    ) {
        let (structs, enums, registry) = crate::load_typesync_fixture(spec_input_file);
        let output = evenframe_core::typesync::protobuf::generate_protobuf_schema_string(
            &structs, &enums, None, true, &registry,
        )
        .unwrap();
        let name = std::path::Path::new(spec_input_file)
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap();
        insta::assert_snapshot!(format!("protobuf_validated_{name}"), output);
    }

    tests_macros::gen_tests! { "tests/specs/typesync/*.json", crate::protobuf_validated::run, "typesync" }
}

#[cfg(feature = "flatbuffers")]
mod flatbuffers {
    pub fn run(
        spec_input_file: &str,
        _expected_file: &str,
        _test_directory: &str,
        _file_type: &str,
    ) {
        let (structs, enums, registry) = crate::load_typesync_fixture(spec_input_file);
        let output = evenframe_core::typesync::flatbuffers::generate_flatbuffers_schema_string(
            &structs, &enums, None, &registry,
        )
        .unwrap();
        let name = std::path::Path::new(spec_input_file)
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap();
        insta::assert_snapshot!(format!("flatbuffers_{name}"), output);
    }

    tests_macros::gen_tests! { "tests/specs/typesync/*.json", crate::flatbuffers::run, "typesync" }
}

#[cfg(feature = "schemasync")]
mod surrealql {
    use evenframe_core::schemasync::TableConfig;
    use evenframe_core::types::{ForeignTypeRegistry, StructConfig, TaggedUnion};
    use std::collections::BTreeMap;

    #[derive(serde::Deserialize)]
    struct SurrealqlFixture {
        table_name: String,
        table_config: TableConfig,
        query_details: BTreeMap<String, TableConfig>,
        server_only: BTreeMap<String, StructConfig>,
        enums: BTreeMap<String, TaggedUnion>,
    }

    pub fn run(
        spec_input_file: &str,
        _expected_file: &str,
        _test_directory: &str,
        _file_type: &str,
    ) {
        let input = std::fs::read_to_string(spec_input_file).unwrap();
        let fixture: SurrealqlFixture = serde_json::from_str(&input).unwrap();
        let registry = ForeignTypeRegistry::default();
        let output =
            evenframe_core::schemasync::database::surql::define::generate_define_statements(
                &fixture.table_name,
                &fixture.table_config,
                &fixture.query_details,
                &fixture.server_only,
                &fixture.enums,
                &registry,
                true,
            )
            .unwrap();
        let name = std::path::Path::new(spec_input_file)
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap();
        insta::assert_snapshot!(format!("surrealql_{name}"), output);
    }

    tests_macros::gen_tests! { "tests/specs/surrealql/*.json", crate::surrealql::run, "surrealql" }
}
