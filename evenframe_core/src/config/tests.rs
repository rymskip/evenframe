use super::{
    EvenframeConfig, EvenframeError, GeneralConfig, Path, PathBuf, env, fill, fs, placeholders,
};
use crate::typesync::config::{OutputKind, TypesyncOutput};
use tempfile::TempDir;

#[test]
fn placeholders_take_generic_parameters() {
    assert_eq!(
        fill("RecordLink<{0}>", &["Order".to_string()]),
        "RecordLink<Order>"
    );
    assert_eq!(placeholders("Pair<{0}, {1}>"), vec![0, 1]);
    assert!(placeholders("{ on: boolean }").is_empty());
}

fn foreign_types(entries: &str) -> std::result::Result<GeneralConfig, toml::de::Error> {
    toml::from_str(&format!("foreign_types = {{ {entries} }}"))
}

#[test]
fn a_record_link_entry_sets_only_its_typescript_side() {
    let owned = foreign_types(
            "RecordLink = { macroforge = { type = \"RecordLink<{0}>\", import = { from = \"./index\", name = \"RecordLink\" } } }",
        )
        .unwrap();
    assert!(owned.foreign_types.contains_key("RecordLink"));
    let error = foreign_types("RecordLink = { surrealdb = \"record\" }")
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(
        error.contains("foreign_types.RecordLink can only set arktype, effect, macroforge"),
        "{error}"
    );
}

#[test]
fn a_placeholder_needs_a_generic_parameter() {
    let error = foreign_types("Money = { macroforge = { type = \"Money<{0}>\" } }")
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(
        error.contains(
            "the macroforge type `Money<{0}>` uses `{0}`, but Money has 0 generic parameters"
        ),
        "{error}"
    );
}

// ==================== GeneralConfig Tests ====================

#[test]
fn test_general_config_default() {
    let config = GeneralConfig::default();
    assert!(config.apply_aliases.is_empty());
}

#[test]
fn test_general_config_deserialize_empty() {
    let toml_str = "";
    let config: GeneralConfig = toml::from_str(toml_str).unwrap_or_default();
    assert!(config.apply_aliases.is_empty());
}

#[test]
fn test_general_config_deserialize_with_aliases() {
    let toml_str = r#"
            apply_aliases = ["MyAlias", "AnotherAlias"]
        "#;
    let config: GeneralConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.apply_aliases.len(), 2);
    assert_eq!(config.apply_aliases[0], "MyAlias");
    assert_eq!(config.apply_aliases[1], "AnotherAlias");
}

#[test]
fn test_general_config_serialize() {
    let config = GeneralConfig {
        apply_aliases: vec!["Test".to_string()],
        env_path: None,
        ..Default::default()
    };
    let toml_str = toml::to_string(&config).unwrap();
    assert!(toml_str.contains("apply_aliases"));
    assert!(toml_str.contains("Test"));
}

// ==================== substitute_env_vars Tests ====================

#[test]
fn test_substitute_env_vars_basic() {
    temp_env::with_var("TEST_VAR_BASIC", Some("hello"), || {
        let result = EvenframeConfig::substitute_env_vars("${TEST_VAR_BASIC}").unwrap();
        assert_eq!(result, "hello");
    });
}

#[test]
fn test_substitute_env_vars_with_surrounding_text() {
    temp_env::with_var("TEST_VAR_SURROUND", Some("world"), || {
        let result = EvenframeConfig::substitute_env_vars("hello ${TEST_VAR_SURROUND}!").unwrap();
        assert_eq!(result, "hello world!");
    });
}

#[test]
fn test_substitute_env_vars_multiple() {
    temp_env::with_vars(
        [
            ("TEST_VAR_MULTI1", Some("foo")),
            ("TEST_VAR_MULTI2", Some("bar")),
        ],
        || {
            let result =
                EvenframeConfig::substitute_env_vars("${TEST_VAR_MULTI1}:${TEST_VAR_MULTI2}")
                    .unwrap();
            assert_eq!(result, "foo:bar");
        },
    );
}

#[test]
fn a_value_is_not_substituted_again() {
    temp_env::with_vars(
        [
            ("TEST_VAR_HOLDS_REFERENCE", Some("${TEST_VAR_INNER}")),
            ("TEST_VAR_INNER", Some("inner")),
        ],
        || {
            let result = EvenframeConfig::substitute_env_vars(
                "${TEST_VAR_HOLDS_REFERENCE} ${TEST_VAR_INNER} ${TEST_VAR_INNER}",
            )
            .unwrap();
            assert_eq!(result, "${TEST_VAR_INNER} inner inner");
        },
    );
}

#[test]
fn test_substitute_env_vars_no_match() {
    let result = EvenframeConfig::substitute_env_vars("no variables here").unwrap();
    assert_eq!(result, "no variables here");
}

#[test]
fn test_substitute_env_vars_empty_string() {
    let result = EvenframeConfig::substitute_env_vars("").unwrap();
    assert_eq!(result, "");
}

#[test]
fn test_substitute_env_vars_url_pattern() {
    temp_env::with_var("TEST_DB_URL", Some("http://localhost:8000"), || {
        let result = EvenframeConfig::substitute_env_vars("${TEST_DB_URL}").unwrap();
        assert_eq!(result, "http://localhost:8000");
    });
}

#[test]
fn test_substitute_env_vars_missing_returns_error() {
    // Clear the env var to make sure it doesn't exist
    // SAFETY: This is a test environment where we control access to env vars
    unsafe {
        std::env::remove_var("DEFINITELY_NOT_SET_VAR_12345");
    }
    let result = EvenframeConfig::substitute_env_vars("${DEFINITELY_NOT_SET_VAR_12345}");
    assert!(result.is_err());
}

#[test]
fn test_substitute_env_vars_with_underscores() {
    temp_env::with_var("TEST_VAR_WITH_UNDERSCORES", Some("value"), || {
        let result = EvenframeConfig::substitute_env_vars("${TEST_VAR_WITH_UNDERSCORES}").unwrap();
        assert_eq!(result, "value");
    });
}

#[test]
fn test_substitute_env_vars_with_numbers() {
    temp_env::with_var("TEST_VAR_123", Some("num_value"), || {
        let result = EvenframeConfig::substitute_env_vars("${TEST_VAR_123}").unwrap();
        assert_eq!(result, "num_value");
    });
}

#[test]
fn test_substitute_env_vars_adjacent() {
    temp_env::with_vars([("TEST_ADJ1", Some("a")), ("TEST_ADJ2", Some("b"))], || {
        let result = EvenframeConfig::substitute_env_vars("${TEST_ADJ1}${TEST_ADJ2}").unwrap();
        assert_eq!(result, "ab");
    });
}

#[test]
fn test_substitute_env_vars_preserves_non_matching_braces() {
    let result = EvenframeConfig::substitute_env_vars("{not_a_var}").unwrap();
    assert_eq!(result, "{not_a_var}");
}

// ==================== find_config_file Tests ====================
// NOTE: These tests that use env::set_current_dir should be run with --test-threads=1
// to avoid race conditions. They are marked with #[ignore] for parallel runs.

#[test]
#[ignore = "requires --test-threads=1 due to env::set_current_dir"]
fn test_find_config_file_not_found() {
    // Create a temp directory without evenframe.toml
    let temp_dir = TempDir::new().unwrap();
    let original_dir = env::current_dir().unwrap();

    // Change to temp directory
    env::set_current_dir(temp_dir.path()).unwrap();

    let result = EvenframeConfig::find_config_file();

    // Restore original directory
    env::set_current_dir(original_dir).unwrap();

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Configuration file not found"));
}

#[test]
#[ignore = "requires --test-threads=1 due to env::set_current_dir"]
fn test_find_config_file_in_current_dir() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("evenframe.toml");
    fs::write(&config_path, "# test config").unwrap();
    // Canonicalize before changing directory
    let expected_canonical = config_path.canonicalize().unwrap();

    let original_dir = env::current_dir().unwrap();
    env::set_current_dir(temp_dir.path()).unwrap();

    let result = EvenframeConfig::find_config_file();

    env::set_current_dir(original_dir).unwrap();

    assert!(result.is_ok());
    // Use canonicalize to handle symlinks (e.g., /var -> /private/var on macOS)
    let result_canonical = result.unwrap().canonicalize().unwrap();
    assert_eq!(result_canonical, expected_canonical);
}

#[test]
#[ignore = "requires --test-threads=1 due to env::set_current_dir"]
fn test_find_config_file_in_parent_dir() {
    let temp_dir = TempDir::new().unwrap();
    let child_dir = temp_dir.path().join("child");
    fs::create_dir(&child_dir).unwrap();

    let config_path = temp_dir.path().join("evenframe.toml");
    fs::write(&config_path, "# test config").unwrap();
    // Canonicalize before changing directory
    let expected_canonical = config_path.canonicalize().unwrap();

    let original_dir = env::current_dir().unwrap();
    env::set_current_dir(&child_dir).unwrap();

    let result = EvenframeConfig::find_config_file();

    env::set_current_dir(original_dir).unwrap();

    assert!(result.is_ok());
    // Use canonicalize to handle symlinks
    let result_canonical = result.unwrap().canonicalize().unwrap();
    assert_eq!(result_canonical, expected_canonical);
}

#[test]
#[ignore = "requires --test-threads=1 due to env::set_current_dir"]
fn test_find_config_file_in_grandparent_dir() {
    let temp_dir = TempDir::new().unwrap();
    let child_dir = temp_dir.path().join("child");
    let grandchild_dir = child_dir.join("grandchild");
    fs::create_dir_all(&grandchild_dir).unwrap();

    let config_path = temp_dir.path().join("evenframe.toml");
    fs::write(&config_path, "# test config").unwrap();
    // Canonicalize before changing directory
    let expected_canonical = config_path.canonicalize().unwrap();

    let original_dir = env::current_dir().unwrap();
    env::set_current_dir(&grandchild_dir).unwrap();

    let result = EvenframeConfig::find_config_file();

    env::set_current_dir(original_dir).unwrap();

    assert!(result.is_ok());
    // Use canonicalize to handle symlinks
    let result_canonical = result.unwrap().canonicalize().unwrap();
    assert_eq!(result_canonical, expected_canonical);
}

// ==================== .evenframe/config.toml Discovery Tests ====================

#[test]
#[ignore = "requires --test-threads=1 due to env::set_current_dir"]
fn test_find_config_dotdir_preferred_over_legacy() {
    let temp_dir = TempDir::new().unwrap();

    // Create both config files
    fs::write(temp_dir.path().join("evenframe.toml"), "# legacy").unwrap();
    let dotdir = temp_dir.path().join(".evenframe");
    fs::create_dir(&dotdir).unwrap();
    let dotdir_config = dotdir.join("config.toml");
    fs::write(&dotdir_config, "# preferred").unwrap();
    let expected_canonical = dotdir_config.canonicalize().unwrap();

    let original_dir = env::current_dir().unwrap();
    env::set_current_dir(temp_dir.path()).unwrap();

    let result = EvenframeConfig::find_config_file();

    env::set_current_dir(original_dir).unwrap();

    assert!(result.is_ok());
    let result_canonical = result.unwrap().canonicalize().unwrap();
    assert_eq!(result_canonical, expected_canonical);
}

#[test]
#[ignore = "requires --test-threads=1 due to env::set_current_dir"]
fn test_find_config_only_dotdir() {
    let temp_dir = TempDir::new().unwrap();

    let dotdir = temp_dir.path().join(".evenframe");
    fs::create_dir(&dotdir).unwrap();
    let dotdir_config = dotdir.join("config.toml");
    fs::write(&dotdir_config, "# only dotdir").unwrap();
    let expected_canonical = dotdir_config.canonicalize().unwrap();

    let original_dir = env::current_dir().unwrap();
    env::set_current_dir(temp_dir.path()).unwrap();

    let result = EvenframeConfig::find_config_file();

    env::set_current_dir(original_dir).unwrap();

    assert!(result.is_ok());
    let result_canonical = result.unwrap().canonicalize().unwrap();
    assert_eq!(result_canonical, expected_canonical);
}

#[test]
fn a_config_may_leave_out_either_pipeline() {
    let config: EvenframeConfig = toml::from_str("[general]\napply_aliases = []\n").unwrap();
    assert!(config.schemasync.is_none());
    assert!(config.typesync.outputs.is_empty());
    let config = EvenframeConfig {
        config_file_path: PathBuf::from("/project/evenframe.toml"),
        ..config
    };
    let error = config.require_schemasync().unwrap_err().to_string();
    assert!(
        error.contains("/project/evenframe.toml has no [schemasync] section"),
        "{error}"
    );
}

#[test]
fn test_project_root_for_legacy_config() {
    let config = EvenframeConfig {
        general: GeneralConfig::default(),
        schemasync: None,
        typesync: crate::typesync::config::TypesyncConfig::default(),
        config_file_path: PathBuf::from("/project/evenframe.toml"),
    };
    assert_eq!(config.project_root(), Path::new("/project"));
}

#[test]
fn test_project_root_for_dotdir_config() {
    let config = EvenframeConfig {
        general: GeneralConfig::default(),
        schemasync: None,
        typesync: crate::typesync::config::TypesyncConfig::default(),
        config_file_path: PathBuf::from("/project/.evenframe/config.toml"),
    };
    assert_eq!(config.project_root(), Path::new("/project"));
}

// ==================== load_surql_from_path Tests ====================

#[test]
fn test_load_surql_from_file() {
    let temp_dir = TempDir::new().unwrap();
    let surql_path = temp_dir.path().join("test.surql");
    fs::write(&surql_path, "DEFINE FUNCTION fn::test() { RETURN 1; };").unwrap();

    let result = EvenframeConfig::load_surql_from_path(temp_dir.path(), "test.surql");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "DEFINE FUNCTION fn::test() { RETURN 1; };");
}

#[test]
fn test_load_surql_from_directory() {
    let temp_dir = TempDir::new().unwrap();
    let surql_dir = temp_dir.path().join("surql");
    fs::create_dir(&surql_dir).unwrap();
    fs::write(surql_dir.join("01_first.surql"), "-- first").unwrap();
    fs::write(surql_dir.join("02_second.surql"), "-- second").unwrap();
    // Non-surql file should be ignored
    fs::write(surql_dir.join("readme.txt"), "ignore me").unwrap();

    let result = EvenframeConfig::load_surql_from_path(temp_dir.path(), "surql");
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "-- first\n-- second");
}

#[test]
fn test_load_surql_nonexistent_path() {
    let temp_dir = TempDir::new().unwrap();
    let result = EvenframeConfig::load_surql_from_path(temp_dir.path(), "nonexistent.surql");
    assert!(result.is_err());
}

#[test]
fn test_load_surql_with_env_var_substitution() {
    let temp_dir = TempDir::new().unwrap();
    let surql_path = temp_dir.path().join("test.surql");
    fs::write(&surql_path, "DEFINE ACCESS test ON DATABASE TYPE JWT ALGORITHM HS256 KEY '${TEST_SURQL_KEY:-default_key}';").unwrap();

    let result = EvenframeConfig::load_surql_from_path(temp_dir.path(), "test.surql");
    assert!(result.is_ok());
    assert!(result.unwrap().contains("default_key"));
}

// ==================== EvenframeConfig Serialization Tests ====================

#[test]
fn test_evenframe_config_deserialize_minimal() {
    let toml_str = r#"
            [schemasync]
            should_generate_mocks = false

            [schemasync.database]
            provider = "surrealdb"
            url = "http://localhost:8000"
            namespace = "test"
            database = "test"

            [schemasync.mock_gen_config]
            default_record_count = 10
            default_preservation_mode = "Smart"
            full_refresh_mode = false


            [typesync]
            output = { kind = "arktype", dir = "./generated/" }
        "#;

    let config: EvenframeConfig = toml::from_str(toml_str).unwrap();
    assert!(config.general.apply_aliases.is_empty()); // Default
    assert_eq!(
        config.require_schemasync().unwrap().database.url,
        "http://localhost:8000"
    );
    assert_eq!(
        config.typesync.outputs,
        vec![TypesyncOutput::new(OutputKind::Arktype, "./generated/")]
    );
}

#[test]
fn unknown_keys_are_rejected() {
    let base = r#"
            [schemasync]
            should_generate_mocks = false

            [schemasync.database]
            url = "http://localhost:8000"

            [typesync]
            output = { kind = "arktype", dir = "./generated/" }
        "#;
    assert!(toml::from_str::<EvenframeConfig>(base).is_ok());

    for (unknown, key) in [
        (
            "[schemasync.mock_gen_config]\ndefault_batch_size = 10",
            "default_batch_size",
        ),
        (
            "[schemasync.performance]\ncache_duration_seconds = 60",
            "performance",
        ),
        ("[general]\napply_alias = [\"Typo\"]", "apply_alias"),
        ("[unknown_section]\nkey = 1", "unknown_section"),
    ] {
        let error = toml::from_str::<EvenframeConfig>(&format!("{base}\n{unknown}"))
            .expect_err(unknown)
            .to_string();
        assert!(error.contains(key), "{error}");
    }

    let output_typo = base.replace(
        r#"dir = "./generated/" }"#,
        r#"dir = "./generated/", barel_file = true }"#,
    );
    let error = toml::from_str::<EvenframeConfig>(&output_typo)
        .expect_err("a typo in an output")
        .to_string();
    assert!(error.contains("barel_file"), "{error}");
}

#[test]
fn test_evenframe_config_deserialize_with_general() {
    let toml_str = r#"
            [general]
            apply_aliases = ["MyAlias"]

            [schemasync]
            should_generate_mocks = true

            [schemasync.database]
            provider = "surrealdb"
            url = "http://localhost:8000"
            namespace = "test"
            database = "test"

            [schemasync.mock_gen_config]
            default_record_count = 100
            default_preservation_mode = "Smart"
            full_refresh_mode = false


            [typesync]
            outputs = [
                { kind = "arktype", dir = "./types/arktype" },
                { kind = "effect", dir = "./types/effect" },
            ]
        "#;

    let config: EvenframeConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(config.general.apply_aliases.len(), 1);
    assert_eq!(config.general.apply_aliases[0], "MyAlias");
    assert!(config.require_schemasync().unwrap().should_generate_mocks);
    let kinds: Vec<OutputKind> = config.typesync.outputs.iter().map(|o| o.kind).collect();
    assert_eq!(kinds, vec![OutputKind::Arktype, OutputKind::Effect]);
}

#[test]
fn struct_variants_default_to_named_and_accept_inline() {
    let parse = |typesync: &str| toml::from_str::<EvenframeConfig>(typesync);
    let named = parse("[typesync]\noutputs = []\n").unwrap();
    assert_eq!(
        named.typesync.struct_variants,
        crate::typesync::config::StructVariants::Named
    );
    let inline = parse("[typesync]\noutputs = []\nstruct_variants = \"inline\"\n").unwrap();
    assert_eq!(
        inline.typesync.struct_variants,
        crate::typesync::config::StructVariants::Inline
    );
    assert!(parse("[typesync]\nstruct_variants = \"flattened\"\n").is_err());
}

// ==================== EvenframeConfig::new() Integration Tests ====================
// NOTE: These tests use env::set_current_dir and should be run with --test-threads=1

#[test]
#[ignore = "requires --test-threads=1 due to env::set_current_dir"]
fn test_evenframe_config_new_with_valid_config() {
    let temp_dir = TempDir::new().unwrap();

    // Create a valid evenframe.toml
    let config_content = r#"
            [schemasync]
            should_generate_mocks = false

            [schemasync.database]
            provider = "surrealdb"
            url = "http://localhost:8000"
            namespace = "test_ns"
            database = "test_db"

            [schemasync.mock_gen_config]
            default_record_count = 10
            default_preservation_mode = "Smart"
            full_refresh_mode = false


            [typesync]
            outputs = []
        "#;
    fs::write(temp_dir.path().join("evenframe.toml"), config_content).unwrap();

    let original_dir = env::current_dir().unwrap();
    env::set_current_dir(temp_dir.path()).unwrap();

    let result = EvenframeConfig::new();

    env::set_current_dir(original_dir).unwrap();

    assert!(result.is_ok());
    let config = result.unwrap();
    assert_eq!(
        config.require_schemasync().unwrap().database.namespace,
        "test_ns"
    );
    assert_eq!(
        config.require_schemasync().unwrap().database.database,
        "test_db"
    );
}

fn config_with_env_refs(url: &str, output_path: &str) -> String {
    format!(
        r#"
            [schemasync]
            should_generate_mocks = false

            [schemasync.database]
            provider = "surrealdb"
            url = "{url}"
            namespace = "${{EF_TEST_OFFLINE_UNSET_NS}}"
            database = "${{EF_TEST_OFFLINE_UNSET_DB:-fallback_db}}"

            [schemasync.mock_gen_config]
            default_record_count = 10
            default_preservation_mode = "Smart"
            full_refresh_mode = false


            [typesync]
            output = {{ kind = "arktype", dir = "{output_path}" }}
            "#
    )
}

/// A configuration path whose directory holds no `.env`.
fn config_path() -> PathBuf {
    PathBuf::from("/nonexistent-evenframe-project/evenframe.toml")
}

#[test]
fn offline_substitution_leaves_unset_connection_vars_unresolved() {
    let content = config_with_env_refs("${EF_TEST_OFFLINE_UNSET_URL}", "./output/");

    let strict = EvenframeConfig::parse(&content, config_path(), true);
    assert!(
        matches!(strict, Err(EvenframeError::EnvVarNotSet(_))),
        "online loading must still require the connection variables"
    );

    let config = EvenframeConfig::parse(&content, config_path(), false).unwrap();
    let database = &config.require_schemasync().unwrap().database;
    assert_eq!(database.url, "${EF_TEST_OFFLINE_UNSET_URL}");
    assert_eq!(database.namespace, "${EF_TEST_OFFLINE_UNSET_NS}");
    assert_eq!(database.database, "fallback_db");
}

#[test]
fn offline_substitution_still_requires_other_vars() {
    let content = config_with_env_refs(
        "${EF_TEST_OFFLINE_UNSET_URL}",
        "${EF_TEST_OFFLINE_UNSET_OUTPUT}",
    );
    let result = EvenframeConfig::parse(&content, config_path(), false);
    assert!(
        matches!(&result, Err(EvenframeError::EnvVarNotSet(name)) if name == "EF_TEST_OFFLINE_UNSET_OUTPUT"),
        "unexpected result: {result:?}"
    );
}

#[test]
fn a_value_with_quotes_and_backslashes_is_substituted_as_data() {
    temp_env::with_var("EF_TEST_QUOTED_DIR", Some(r#"out "quoted" \ dir"#), || {
        let content = config_with_env_refs("http://localhost:8000", "${EF_TEST_QUOTED_DIR}");
        let config = EvenframeConfig::parse(&content, config_path(), false).unwrap();
        assert_eq!(
            config.typesync.outputs[0].dir,
            r#"/nonexistent-evenframe-project/out "quoted" \ dir"#
        );
    });
}

#[test]
#[ignore = "requires --test-threads=1 due to env::set_current_dir"]
fn test_evenframe_config_new_invalid_toml() {
    let temp_dir = TempDir::new().unwrap();

    // Create invalid TOML
    fs::write(
        temp_dir.path().join("evenframe.toml"),
        "invalid toml content {{{",
    )
    .unwrap();

    let original_dir = env::current_dir().unwrap();
    env::set_current_dir(temp_dir.path()).unwrap();

    let result = EvenframeConfig::new();

    env::set_current_dir(original_dir).unwrap();

    assert!(result.is_err());
}

#[test]
fn a_config_with_only_general_settings_loads() {
    let config =
        EvenframeConfig::parse("[general]\napply_aliases = []\n", config_path(), true).unwrap();
    assert!(config.schemasync.is_none());
    assert!(config.typesync.outputs.is_empty());
}
