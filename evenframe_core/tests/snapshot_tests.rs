#![cfg(feature = "typesync")]

/// Typesync spec files and loading them.
#[cfg(any(
    feature = "arktype",
    feature = "effect",
    feature = "macroforge",
    feature = "protobuf",
    feature = "flatbuffers"
))]
mod fixture {
    use evenframe_core::config::ForeignTypeConfig;
    use evenframe_core::types::{
        AllConfigs, ForeignTypeRegistry, NewtypeConfig, StructConfig, TaggedUnion,
    };
    use evenframe_core::typesync::config::StructVariants;
    use evenframe_core::typesync::struct_variants::declare_payloads;
    use std::collections::BTreeMap;

    #[derive(serde::Deserialize)]
    struct TypesyncFixture {
        structs: BTreeMap<String, StructConfig>,
        enums: BTreeMap<String, TaggedUnion>,
        #[serde(default)]
        newtypes: BTreeMap<String, NewtypeConfig>,
        #[serde(default)]
        foreign_types: BTreeMap<String, ForeignTypeConfig>,
        #[serde(default)]
        struct_variants: StructVariants,
    }

    /// A spec's types, ready for the generators.
    pub struct Loaded {
        structs: BTreeMap<String, StructConfig>,
        enums: BTreeMap<String, TaggedUnion>,
        newtypes: BTreeMap<String, NewtypeConfig>,
        pub registry: ForeignTypeRegistry,
    }

    impl Loaded {
        #[cfg(any(feature = "arktype", feature = "effect", feature = "macroforge"))]
        pub fn index(&self) -> evenframe_core::typesync::type_index::TypeIndex<'_> {
            evenframe_core::typesync::type_index::TypeIndex::with_newtypes(
                &self.structs,
                &self.enums,
                &self.newtypes,
            )
            .unwrap()
        }

        /// The structs and enums with every newtype stored as its inner type, as
        /// the binary schema outputs describe them.
        #[cfg(any(feature = "protobuf", feature = "flatbuffers"))]
        pub fn stored(
            &self,
        ) -> (
            BTreeMap<String, StructConfig>,
            BTreeMap<String, TaggedUnion>,
        ) {
            let mut structs = self.structs.clone();
            let mut enums = self.enums.clone();
            evenframe_core::types::desugar_newtypes(
                &self.newtypes,
                &mut enums,
                &mut BTreeMap::new(),
                &mut structs,
            )
            .unwrap();
            (structs, enums)
        }
    }

    /// The spec's types as the typesync pipeline gives them to the outputs.
    pub fn load(path: &str) -> Loaded {
        let input = std::fs::read_to_string(path).unwrap();
        let mut fixture: TypesyncFixture = serde_json::from_str(&input).unwrap();
        if fixture.struct_variants == StructVariants::Named {
            declare_payloads(&mut fixture.structs, &mut fixture.enums).unwrap();
        }
        let registry = ForeignTypeRegistry::from_config(&fixture.foreign_types);
        let typesync = AllConfigs {
            enums: fixture.enums,
            tables: BTreeMap::new(),
            objects: fixture.structs,
            newtypes: fixture.newtypes,
        }
        .for_typesync()
        .unwrap();
        Loaded {
            structs: typesync.objects,
            enums: typesync.enums,
            newtypes: typesync.newtypes,
            registry,
        }
    }
}

#[cfg(feature = "arktype")]
mod arktype {
    pub fn run(
        spec_input_file: &str,
        _expected_file: &str,
        _test_directory: &str,
        _file_type: &str,
    ) {
        let loaded = crate::fixture::load(spec_input_file);
        let output = evenframe_core::typesync::arktype::generate_arktype_type_string(
            &loaded.index(),
            &loaded.registry,
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
        let loaded = crate::fixture::load(spec_input_file);
        let output = evenframe_core::typesync::effect::generate_effect_schema_string(
            &loaded.index(),
            true,
            &loaded.registry,
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
        let loaded = crate::fixture::load(spec_input_file);
        let mut helpers =
            evenframe_core::typesync::macroforge::HelperModule::new("./helpers".to_owned());
        let interfaces = evenframe_core::typesync::macroforge::generate_macroforge_type_string(
            &loaded.index(),
            Default::default(),
            &loaded.registry,
            &mut helpers,
            None,
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
        let loaded = crate::fixture::load(spec_input_file);
        let (structs, enums) = loaded.stored();
        let output = evenframe_core::typesync::protobuf::generate_protobuf_schema_string(
            &structs,
            &enums,
            None,
            false,
            &loaded.registry,
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
        let loaded = crate::fixture::load(spec_input_file);
        let (structs, enums) = loaded.stored();
        let output = evenframe_core::typesync::protobuf::generate_protobuf_schema_string(
            &structs,
            &enums,
            None,
            true,
            &loaded.registry,
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
        let loaded = crate::fixture::load(spec_input_file);
        let (structs, enums) = loaded.stored();
        let output = evenframe_core::typesync::flatbuffers::generate_flatbuffers_schema_string(
            &structs,
            &enums,
            None,
            &loaded.registry,
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
