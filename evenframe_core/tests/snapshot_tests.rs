use evenframe_core::config::ForeignTypeConfig;
use evenframe_core::types::{ForeignTypeRegistry, StructConfig, TaggedUnion};
use std::collections::HashMap;

#[derive(serde::Deserialize)]
struct TypesyncFixture {
    structs: HashMap<String, StructConfig>,
    enums: HashMap<String, TaggedUnion>,
    #[serde(default)]
    foreign_types: HashMap<String, ForeignTypeConfig>,
}

fn load_typesync_fixture(
    path: &str,
) -> (
    HashMap<String, StructConfig>,
    HashMap<String, TaggedUnion>,
    ForeignTypeRegistry,
) {
    let input = std::fs::read_to_string(path).unwrap();
    let fixture: TypesyncFixture = serde_json::from_str(&input).unwrap();
    let registry = ForeignTypeRegistry::from_config(&fixture.foreign_types);
    (fixture.structs, fixture.enums, registry)
}

mod arktype {
    pub fn run(
        spec_input_file: &str,
        _expected_file: &str,
        _test_directory: &str,
        _file_type: &str,
    ) {
        let (structs, enums, registry) = crate::load_typesync_fixture(spec_input_file);
        let output = evenframe_core::typesync::arktype::generate_arktype_type_string(
            &structs, &enums, true, &registry,
        );
        let name = std::path::Path::new(spec_input_file)
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap();
        insta::assert_snapshot!(format!("arktype_{name}"), output);
    }

    tests_macros::gen_tests! { "tests/specs/typesync/*.json", crate::arktype::run, "typesync" }
}

mod effect {
    pub fn run(
        spec_input_file: &str,
        _expected_file: &str,
        _test_directory: &str,
        _file_type: &str,
    ) {
        let (structs, enums, registry) = crate::load_typesync_fixture(spec_input_file);
        let output = evenframe_core::typesync::effect::generate_effect_schema_string(
            &structs, &enums, true, &registry,
        );
        let name = std::path::Path::new(spec_input_file)
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap();
        insta::assert_snapshot!(format!("effect_{name}"), output);
    }

    tests_macros::gen_tests! { "tests/specs/typesync/*.json", crate::effect::run, "typesync" }
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
        );
        let name = std::path::Path::new(spec_input_file)
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap();
        insta::assert_snapshot!(format!("protobuf_{name}"), output);
    }

    tests_macros::gen_tests! { "tests/specs/typesync/*.json", crate::protobuf::run, "typesync" }
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
        );
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
    use std::collections::HashMap;

    #[derive(serde::Deserialize)]
    struct SurrealqlFixture {
        table_name: String,
        table_config: TableConfig,
        query_details: HashMap<String, TableConfig>,
        server_only: HashMap<String, StructConfig>,
        enums: HashMap<String, TaggedUnion>,
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
                false,
                &registry,
            );
        let name = std::path::Path::new(spec_input_file)
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap();
        insta::assert_snapshot!(format!("surrealql_{name}"), output);
    }

    tests_macros::gen_tests! { "tests/specs/surrealql/*.json", crate::surrealql::run, "surrealql" }
}
