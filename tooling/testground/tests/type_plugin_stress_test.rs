//! Stress tests for the `stress_test` output-rule WASM plugin.
//!
//! These tests cover every capability `OutputRulePluginOutput` exposes: the
//! error path, type-level annotations, macroforge derives, permissions and
//! events, and field-level annotations. Field type substitution, skipping,
//! extra imports and type renaming are not part of the plugin surface; the
//! plugin emits annotation markers for them and these tests assert on the
//! markers.
//!
//! Run with: `cargo test --test type_plugin_stress_test --features wasm-plugins`

#![cfg(feature = "wasm-plugins")]

use evenframe_core::config::OutputRulePluginConfig;
use evenframe_core::error::EvenframeError;
use evenframe_core::schemasync::table::TableConfig;
use evenframe_core::types::{
    FieldType, NewtypeConfig, Pipeline, StructConfig, StructField, TaggedUnion, Variant,
};
use evenframe_core::typesync::plugin::OutputRulePluginManager;
use evenframe_core::typesync::plugin_types::{OutputRulePluginInput, OutputRulePluginOutput};
use evenframe_core::validator::{StringValidator, Validator};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[path = "support/plugins.rs"]
mod plugins;

use plugins::build_plugin;

fn testground_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn stress_manager() -> OutputRulePluginManager {
    let mut plugins = BTreeMap::new();
    plugins.insert(
        "stress".to_string(),
        OutputRulePluginConfig {
            path: build_plugin("stress_test").to_string_lossy().into_owned(),
        },
    );
    OutputRulePluginManager::new(&plugins, &testground_root())
        .expect("failed to load stress_test plugin")
}

fn field(name: &str, ty: &str) -> StructField {
    StructField {
        field_name: name.to_string(),
        wire: Default::default(),
        field_type: FieldType::Other(ty.to_string()),
        ..Default::default()
    }
}

fn field_with_annotations(name: &str, ty: &str, anns: Vec<&str>) -> StructField {
    let mut created = field(name, ty);
    created.annotations = anns.into_iter().map(|text| text.to_string()).collect();
    created
}

fn field_with_validators(name: &str, ty: &str, vals: Vec<&str>) -> StructField {
    let mut created = field(name, ty);
    created.validators = vals
        .into_iter()
        .map(|text| Validator::StringValidator(StringValidator::StringEmbedded(text.to_string())))
        .collect();
    created
}

/// Which `OutputRulePluginInput` variant the builder produces. A `Struct`
/// with a `table_name` set becomes the `Table` variant.
#[derive(Clone, Copy)]
enum BuilderKind {
    Struct,
    Enum,
    Newtype,
}

/// Fluent builder for `OutputRulePluginInput` to keep test setup readable
/// without a gigantic positional `input()` helper.
struct InputBuilder {
    name: String,
    kind: BuilderKind,
    derives: Vec<String>,
    annotations: Vec<String>,
    pipeline: String,
    generator: String,
    fields: Vec<StructField>,
    table_name: Option<String>,
}

impl InputBuilder {
    fn new(type_name: &str, kind: BuilderKind) -> Self {
        Self {
            name: type_name.to_string(),
            kind,
            derives: vec![],
            annotations: vec![],
            pipeline: "Both".to_string(),
            generator: "macroforge".to_string(),
            fields: vec![],
            table_name: None,
        }
    }

    fn derives(mut self, ds: Vec<&str>) -> Self {
        self.derives = ds.into_iter().map(|text| text.to_string()).collect();
        self
    }

    fn type_annotations(mut self, anns: Vec<&str>) -> Self {
        self.annotations = anns.into_iter().map(|text| text.to_string()).collect();
        self
    }

    fn pipeline(mut self, pipeline: &str) -> Self {
        self.pipeline = pipeline.to_string();
        self
    }

    fn generator(mut self, generator: &str) -> Self {
        self.generator = generator.to_string();
        self
    }

    fn fields(mut self, fs: Vec<StructField>) -> Self {
        self.fields = fs;
        self
    }

    fn table_name(mut self, name: &str) -> Self {
        self.table_name = Some(name.to_string());
        self
    }

    fn build(self) -> BuiltType {
        let config = match self.kind {
            BuilderKind::Enum => {
                // The builder's `fields` carry the variant names for enums.
                let variants = self
                    .fields
                    .into_iter()
                    .map(|field| Variant {
                        name: field.field_name,
                        wire: Default::default(),
                        data: None,
                        doccom: None,
                        annotations: field.annotations,
                        output_override: None,
                        raw_attributes: Default::default(),
                        is_default: false,
                        element_validators: Vec::new(),
                        element_validator_overrides: Vec::new(),
                    })
                    .collect();
                BuiltConfig::Enum(TaggedUnion {
                    enum_name: self.name,
                    variants,
                    representation: Default::default(),
                    doccom: None,
                    macroforge_derives: vec![],
                    annotations: self.annotations,
                    pipeline: Pipeline::default(),
                    rust_derives: self.derives,
                    output_override: None,
                    resolve_only: false,
                    raw_attributes: Default::default(),
                })
            }
            BuilderKind::Newtype => BuiltConfig::Newtype(NewtypeConfig {
                name: self.name,
                inner: FieldType::String,
                annotations: self.annotations,
                rust_derives: self.derives,
                ..Default::default()
            }),
            BuilderKind::Struct => {
                let config = StructConfig {
                    struct_name: self.name,
                    fields: self.fields,
                    rust_derives: self.derives,
                    annotations: self.annotations,
                    ..Default::default()
                };
                match self.table_name {
                    // A `table_name` promotes the struct to the `Table` variant.
                    Some(table_name) => BuiltConfig::Table(Box::new(TableConfig {
                        table_name,
                        struct_config: config,
                        relation: None,
                        permissions: None,
                        mock_generation_config: None,
                        events: vec![],
                        indexes: vec![],
                        output_override: None,
                    })),
                    None => BuiltConfig::Struct(config),
                }
            }
        };
        BuiltType {
            pipeline: self.pipeline,
            generator: self.generator,
            config,
        }
    }
}

/// The config a builder made, which the plugin input borrows.
enum BuiltConfig {
    Struct(StructConfig),
    Table(Box<TableConfig>),
    Enum(TaggedUnion),
    Newtype(NewtypeConfig),
}

struct BuiltType {
    pipeline: String,
    generator: String,
    config: BuiltConfig,
}

impl BuiltType {
    fn input(&self) -> OutputRulePluginInput<'_> {
        let pipeline = self.pipeline.clone();
        let generator = self.generator.clone();
        match &self.config {
            BuiltConfig::Struct(config) => OutputRulePluginInput::Struct {
                pipeline,
                generator,
                config,
            },
            BuiltConfig::Table(table) => OutputRulePluginInput::Table {
                pipeline,
                generator,
                struct_config: &table.struct_config,
                table_config: table,
            },
            BuiltConfig::Enum(config) => OutputRulePluginInput::Enum {
                pipeline,
                generator,
                config,
            },
            BuiltConfig::Newtype(config) => OutputRulePluginInput::Newtype {
                pipeline,
                generator,
                config,
            },
        }
    }
}

/// The loaded plugin's output for `built`.
fn transform(
    manager: &mut OutputRulePluginManager,
    built: &BuiltType,
) -> Result<OutputRulePluginOutput, EvenframeError> {
    let mut outputs = manager.transform(&built.input())?;
    assert_eq!(outputs.len(), 1, "one plugin is loaded");
    Ok(outputs.remove(0).1)
}

fn struct_of(type_name: &str) -> InputBuilder {
    InputBuilder::new(type_name, BuilderKind::Struct)
}

fn enum_of(type_name: &str) -> InputBuilder {
    InputBuilder::new(type_name, BuilderKind::Enum)
}

fn newtype_of(type_name: &str) -> InputBuilder {
    InputBuilder::new(type_name, BuilderKind::Newtype)
}

fn field_annotations(output: &OutputRulePluginOutput, field_name: &str) -> Vec<String> {
    output
        .field_overrides
        .get(field_name)
        .map(|fo| fo.annotations.clone())
        .unwrap_or_default()
}

// ============================================================================
// Error handling
// ============================================================================

#[test]
fn panic_type_surfaces_intentional_error() {
    let mut pm = stress_manager();
    let inp = struct_of("PanicType")
        .fields(vec![field("x", "i32")])
        .build();
    let result = transform(&mut pm, &inp).unwrap();
    assert!(
        result.error.is_some(),
        "PanicType should trigger error. Got: {:?}",
        result
    );
    assert!(result.error.unwrap().contains("Intentional error"));
    assert!(
        result.field_overrides.is_empty(),
        "errored plugin must not emit field overrides"
    );
}

// ============================================================================
// Type-rename markers (formerly `type_name_override`)
// ============================================================================

#[test]
fn dto_suffix_emits_rename_annotation() {
    let mut pm = stress_manager();
    let inp = struct_of("UserDto")
        .fields(vec![field("name", "String")])
        .build();
    let result = transform(&mut pm, &inp).unwrap();
    assert!(
        result
            .type_override
            .annotations
            .iter()
            .any(|annotation| annotation == "@rename(\"UserResponse\")"),
        "expected @rename annotation; got: {:?}",
        result.type_override.annotations
    );
}

#[test]
fn a_newtype_reaches_the_plugin_by_its_name() {
    let mut pm = stress_manager();
    let result = transform(&mut pm, &newtype_of("UserIdDto").build()).unwrap();
    assert_eq!(
        result.type_override.annotations,
        vec!["@rename(\"UserIdResponse\")".to_string()]
    );
    assert!(result.field_overrides.is_empty(), "a newtype has no fields");

    let refused = transform(&mut pm, &newtype_of("PanicType").build()).unwrap();
    assert!(
        refused.error.is_some(),
        "the plugin's error reaches the host"
    );
}

#[test]
fn non_dto_types_get_no_rename_annotation() {
    let mut pm = stress_manager();
    let inp = struct_of("UserModel").build();
    let result = transform(&mut pm, &inp).unwrap();
    assert!(
        !result
            .type_override
            .annotations
            .iter()
            .any(|annotation| annotation.starts_with("@rename(")),
        "non-Dto type should not get @rename; got: {:?}",
        result.type_override.annotations
    );
}

// ============================================================================
// Derive combinations → field annotations
// ============================================================================

#[test]
fn serialize_only_annotates_decimal() {
    let mut pm = stress_manager();
    let inp = struct_of("A")
        .derives(vec!["Serialize"])
        .fields(vec![field("val", "Decimal")])
        .build();
    let result = transform(&mut pm, &inp).unwrap();
    assert!(field_annotations(&result, "val").contains(&"@bigdecimal".to_string()));
}

#[test]
fn clone_annotates_option_datetime() {
    let mut pm = stress_manager();
    let inp = struct_of("B")
        .derives(vec!["Clone"])
        .fields(vec![field("ts", "Option<DateTime>")])
        .build();
    let result = transform(&mut pm, &inp).unwrap();
    assert!(field_annotations(&result, "ts").contains(&"@datetime_nullable".to_string()));
}

#[test]
fn debug_annotates_vec_uuid() {
    let mut pm = stress_manager();
    let inp = struct_of("C")
        .derives(vec!["Debug"])
        .fields(vec![field("ids", "Vec<Uuid>")])
        .build();
    let result = transform(&mut pm, &inp).unwrap();
    assert!(field_annotations(&result, "ids").contains(&"@readonly_uuid_array".to_string()));
}

#[test]
fn all_derives_combined_annotates_everything_applicable() {
    let mut pm = stress_manager();
    let inp = struct_of("D")
        .derives(vec!["Debug", "Clone", "Serialize"])
        .fields(vec![
            field("amount", "Decimal"),
            field("expires", "Option<DateTime>"),
            field("refs", "Vec<Uuid>"),
            field("name", "String"),
        ])
        .build();
    let result = transform(&mut pm, &inp).unwrap();
    assert!(field_annotations(&result, "amount").contains(&"@bigdecimal".to_string()));
    assert!(field_annotations(&result, "expires").contains(&"@datetime_nullable".to_string()));
    assert!(field_annotations(&result, "refs").contains(&"@readonly_uuid_array".to_string()));
    assert!(
        !result.field_overrides.contains_key("name"),
        "plain String field should be untouched"
    );

    // All three derives present → StressGold macroforge derive is injected
    assert!(
        result
            .type_override
            .macroforge_derives
            .contains(&"StressGold".to_string()),
        "expected StressGold macroforge derive; got: {:?}",
        result.type_override.macroforge_derives
    );
}

// ============================================================================
// HashMap and deeply nested types
// ============================================================================

#[test]
fn hashmap_field_gets_annotation() {
    let mut pm = stress_manager();
    let inp = struct_of("E")
        .fields(vec![field("scores", "HashMap<String, i64>")])
        .build();
    let result = transform(&mut pm, &inp).unwrap();
    assert!(field_annotations(&result, "scores").contains(&"@string_number_map".to_string()));
}

#[test]
fn deeply_nested_type_gets_annotation() {
    let mut pm = stress_manager();
    let inp = struct_of("F")
        .fields(vec![field("deep", "Option<Vec<HashMap<String, Decimal>>>")])
        .build();
    let result = transform(&mut pm, &inp).unwrap();
    assert!(field_annotations(&result, "deep").contains(&"@deep_nested".to_string()));
}

// ============================================================================
// Skip markers (formerly the `skip_fields` list)
// ============================================================================

#[test]
fn internal_annotated_field_gets_skip_marker() {
    let mut pm = stress_manager();
    let inp = struct_of("G")
        .fields(vec![
            field("visible", "String"),
            field_with_annotations("secret", "String", vec!["@internal"]),
        ])
        .build();
    let result = transform(&mut pm, &inp).unwrap();
    assert!(field_annotations(&result, "secret").contains(&"@skip_internal".to_string()));
    assert!(
        !result.field_overrides.contains_key("visible"),
        "visible field must not be annotated"
    );
}

#[test]
fn deprecated_annotated_field_gets_skip_marker() {
    let mut pm = stress_manager();
    let inp = struct_of("H")
        .fields(vec![field_with_annotations(
            "old_field",
            "i32",
            vec!["@deprecated"],
        )])
        .build();
    let result = transform(&mut pm, &inp).unwrap();
    assert!(field_annotations(&result, "old_field").contains(&"@skip_deprecated".to_string()));
}

#[test]
fn private_named_field_gets_skip_marker() {
    let mut pm = stress_manager();
    let inp = struct_of("I")
        .fields(vec![field("name", "String"), field("__private", "String")])
        .build();
    let result = transform(&mut pm, &inp).unwrap();
    assert!(field_annotations(&result, "__private").contains(&"@skip_private".to_string()));
}

#[test]
fn multiple_skip_reasons_stack_as_separate_annotations() {
    let mut pm = stress_manager();
    let inp = struct_of("J")
        .fields(vec![field_with_annotations(
            "__private",
            "String",
            vec!["@internal", "@deprecated"],
        )])
        .build();
    let result = transform(&mut pm, &inp).unwrap();
    let anns = field_annotations(&result, "__private");
    assert!(anns.contains(&"@skip_internal".to_string()));
    assert!(anns.contains(&"@skip_deprecated".to_string()));
    assert!(anns.contains(&"@skip_private".to_string()));
}

// ============================================================================
// Readonly / validated field annotations
// ============================================================================

#[test]
fn readonly_annotation_on_timestamp_fields() {
    let mut pm = stress_manager();
    let inp = struct_of("K")
        .fields(vec![
            field("created_at", "String"),
            field("updated_at", "String"),
            field("name", "String"),
        ])
        .build();
    let result = transform(&mut pm, &inp).unwrap();
    assert!(field_annotations(&result, "created_at").contains(&"@readonly".to_string()));
    assert!(field_annotations(&result, "updated_at").contains(&"@readonly".to_string()));
    assert!(
        !result.field_overrides.contains_key("name"),
        "name should not be annotated"
    );
}

#[test]
fn validated_annotation_on_fields_with_validators() {
    let mut pm = stress_manager();
    let inp = struct_of("L")
        .fields(vec![
            field_with_validators("email", "String", vec!["email"]),
            field("name", "String"),
        ])
        .build();
    let result = transform(&mut pm, &inp).unwrap();
    assert!(field_annotations(&result, "email").contains(&"@validated".to_string()));
}

// ============================================================================
// Generator-specific
// ============================================================================

#[test]
fn arktype_generator_adds_type_level_annotation() {
    let mut pm = stress_manager();
    let inp = struct_of("M")
        .derives(vec!["Serialize"])
        .generator("arktype")
        .fields(vec![field("val", "Decimal")])
        .build();
    let result = transform(&mut pm, &inp).unwrap();
    assert!(
        result
            .type_override
            .annotations
            .contains(&"@arktype_generator".to_string()),
        "expected @arktype_generator; got: {:?}",
        result.type_override.annotations
    );
}

#[test]
fn macroforge_generator_does_not_add_arktype_annotation() {
    let mut pm = stress_manager();
    let inp = struct_of("N")
        .derives(vec!["Serialize"])
        .fields(vec![field("val", "Decimal")])
        .build();
    let result = transform(&mut pm, &inp).unwrap();
    assert!(
        !result
            .type_override
            .annotations
            .contains(&"@arktype_generator".to_string())
    );
}

// ============================================================================
// Pipeline-specific
// ============================================================================

#[test]
fn schemasync_annotates_option_fields() {
    let mut pm = stress_manager();
    let inp = struct_of("O")
        .pipeline("Schemasync")
        .generator("surrealdb")
        .fields(vec![
            field("name", "String"),
            field("bio", "Option<String>"),
            field("avatar", "Option<Url>"),
        ])
        .build();
    let result = transform(&mut pm, &inp).unwrap();
    assert!(field_annotations(&result, "bio").contains(&"@schemasync_option".to_string()));
    assert!(field_annotations(&result, "avatar").contains(&"@schemasync_option".to_string()));
    assert!(!result.field_overrides.contains_key("name"));
}

#[test]
fn typesync_does_not_annotate_option_fields() {
    let mut pm = stress_manager();
    let inp = struct_of("P")
        .pipeline("Typesync")
        .fields(vec![
            field("name", "String"),
            field("bio", "Option<String>"),
        ])
        .build();
    let result = transform(&mut pm, &inp).unwrap();
    assert!(!field_annotations(&result, "bio").contains(&"@schemasync_option".to_string()));
}

// ============================================================================
// Enum handling
// ============================================================================

#[test]
fn enum_gets_tracked_annotation() {
    let mut pm = stress_manager();
    let inp = enum_of("Status")
        .fields(vec![field("Active", "Unit"), field("Inactive", "Unit")])
        .build();
    let result = transform(&mut pm, &inp).unwrap();
    assert!(
        result
            .type_override
            .annotations
            .contains(&"@tracked_enum(\"Status\")".to_string()),
        "expected @tracked_enum; got: {:?}",
        result.type_override.annotations
    );
}

// ============================================================================
// Permissions + events for Dto-named tables
// ============================================================================

#[test]
fn dto_table_gets_permissions_and_events() {
    let mut pm = stress_manager();
    let inp = struct_of("OrderDto")
        .derives(vec!["Serialize"])
        .fields(vec![field("id", "String")])
        .table_name("order_dto")
        .build();
    let result = transform(&mut pm, &inp).unwrap();

    let perms = result
        .type_override
        .permissions
        .as_ref()
        .expect("Dto table should receive permissions");
    assert_eq!(perms.select, "FULL");
    assert!(perms.create.contains("$auth"));
    assert!(perms.delete.contains("admin"));

    assert_eq!(
        result.type_override.events.len(),
        1,
        "Dto table should receive one event"
    );
    assert_eq!(result.type_override.events[0].name, "dto_audit");
    assert!(result.type_override.events[0].statement.contains("audit"));
}

#[test]
fn non_dto_table_does_not_get_permissions() {
    let mut pm = stress_manager();
    let inp = struct_of("OrderModel")
        .derives(vec!["Serialize"])
        .fields(vec![field("id", "String")])
        .table_name("order_model")
        .build();
    let result = transform(&mut pm, &inp).unwrap();
    assert!(result.type_override.permissions.is_none());
    assert!(result.type_override.events.is_empty());
}

// ============================================================================
// JSON special characters round-trip
// ============================================================================

#[test]
fn json_tricky_annotation_round_trips() {
    let mut pm = stress_manager();
    let inp = struct_of("Q")
        .fields(vec![field("json_tricky", "String")])
        .build();
    let result = transform(&mut pm, &inp).unwrap();
    let anns = field_annotations(&result, "json_tricky");
    assert!(
        anns.iter()
            .any(|annotation| annotation.contains("@tricky(")),
        "expected @tricky annotation with escapes to survive JSON round-trip; got: {:?}",
        anns
    );
}

// ============================================================================
// Edge inputs
// ============================================================================

#[test]
fn empty_struct_returns_empty_output() {
    let mut pm = stress_manager();
    let inp = struct_of("Empty").build();
    let result = transform(&mut pm, &inp).unwrap();
    assert!(result.error.is_none());
    assert!(result.field_overrides.is_empty());
    assert!(result.type_override.annotations.is_empty());
    assert!(result.type_override.macroforge_derives.is_empty());
    assert!(result.type_override.permissions.is_none());
    assert!(result.type_override.events.is_empty());
}

#[test]
fn single_decimal_field_struct() {
    let mut pm = stress_manager();
    let inp = struct_of("Single")
        .derives(vec!["Serialize"])
        .fields(vec![field("x", "Decimal")])
        .build();
    let result = transform(&mut pm, &inp).unwrap();
    assert_eq!(result.field_overrides.len(), 1);
    assert!(field_annotations(&result, "x").contains(&"@bigdecimal".to_string()));
}

// ============================================================================
// Massive field counts
// ============================================================================

#[test]
fn plugin_handles_100_fields() {
    let mut pm = stress_manager();
    let fields: Vec<StructField> = (0..100)
        .map(|index| {
            if index % 3 == 0 {
                field(&format!("decimal_{}", index), "Decimal")
            } else {
                field(&format!("string_{}", index), "String")
            }
        })
        .collect();

    let inp = struct_of("BigStruct")
        .derives(vec!["Serialize"])
        .fields(fields)
        .build();
    let result = transform(&mut pm, &inp).unwrap();

    let decimal_count = (0..100).filter(|index| index % 3 == 0).count();
    let annotated_decimals: Vec<_> = result
        .field_overrides
        .keys()
        .filter(|key| key.starts_with("decimal_"))
        .collect();
    assert_eq!(
        annotated_decimals.len(),
        decimal_count,
        "expected all {} Decimal fields annotated",
        decimal_count
    );
}

#[test]
fn plugin_handles_500_fields() {
    let mut pm = stress_manager();
    let fields: Vec<StructField> = (0..500)
        .map(|index| field(&format!("f_{}", index), "String"))
        .collect();

    let inp = struct_of("HugeStruct").fields(fields).build();
    let result = transform(&mut pm, &inp);
    assert!(result.is_ok(), "500 fields should not crash the plugin");
}

// ============================================================================
// Rapid sequential calls (WASM memory stability)
// ============================================================================

#[test]
fn plugin_survives_rapid_sequential_calls() {
    let mut pm = stress_manager();

    for index in 0..50 {
        let inp = struct_of(&format!("Rapid{}", index))
            .derives(vec!["Serialize", "Clone", "Debug"])
            .fields(vec![
                field("amount", "Decimal"),
                field("expires", "Option<DateTime>"),
                field("ids", "Vec<Uuid>"),
            ])
            .build();
        let result = transform(&mut pm, &inp);
        assert!(result.is_ok(), "call {} failed: {:?}", index, result.err());
        let output = result.unwrap();
        assert!(field_annotations(&output, "amount").contains(&"@bigdecimal".to_string()));
        assert!(field_annotations(&output, "expires").contains(&"@datetime_nullable".to_string()));
        assert!(field_annotations(&output, "ids").contains(&"@readonly_uuid_array".to_string()));
    }
}

#[test]
fn plugin_survives_alternating_error_and_success() {
    let mut pm = stress_manager();
    for index in 0..20 {
        let type_name = if index % 2 == 0 {
            "PanicType"
        } else {
            "NormalType"
        };
        let inp = struct_of(type_name).fields(vec![field("x", "i32")]).build();
        let result = transform(&mut pm, &inp);
        assert!(
            result.is_ok(),
            "call {} should not crash: {:?}",
            index,
            result
        );
        if index % 2 == 0 {
            assert!(result.unwrap().error.is_some());
        } else {
            assert!(result.unwrap().error.is_none());
        }
    }
}

// ============================================================================
// Everything combined
// ============================================================================

#[test]
fn everything_combined_kitchen_sink() {
    let mut pm = stress_manager();
    let inp = struct_of("KitchenSinkDto")
        .derives(vec!["Debug", "Clone", "Serialize"])
        .type_annotations(vec!["@audit"])
        .generator("arktype")
        .fields(vec![
            field("amount", "Decimal"),                              // @bigdecimal
            field("expires", "Option<DateTime>"),                    // @datetime_nullable
            field("ids", "Vec<Uuid>"),                               // @readonly_uuid_array
            field("scores", "HashMap<String, i64>"),                 // @string_number_map
            field("created_at", "String"),                           // @readonly
            field_with_validators("email", "String", vec!["email"]), // @validated
            field_with_annotations("secret", "String", vec!["@internal"]), // @skip_internal
            field("__private", "i32"),                               // @skip_private
            field("normal", "bool"),                                 // untouched
            field("json_tricky", "String"),                          // @tricky(...)
        ])
        .table_name("kitchen_sink_dto")
        .build();

    let result = transform(&mut pm, &inp).unwrap();
    assert!(result.error.is_none());

    // Type-level markers
    assert!(
        result
            .type_override
            .annotations
            .iter()
            .any(|annotation| annotation == "@rename(\"KitchenSinkResponse\")")
    );
    assert!(
        result
            .type_override
            .annotations
            .contains(&"@arktype_generator".to_string())
    );
    assert!(
        result
            .type_override
            .macroforge_derives
            .contains(&"StressGold".to_string())
    );
    assert!(result.type_override.permissions.is_some());
    assert_eq!(result.type_override.events.len(), 1);

    // Field-level annotation markers
    assert!(field_annotations(&result, "amount").contains(&"@bigdecimal".to_string()));
    assert!(field_annotations(&result, "expires").contains(&"@datetime_nullable".to_string()));
    assert!(field_annotations(&result, "ids").contains(&"@readonly_uuid_array".to_string()));
    assert!(field_annotations(&result, "scores").contains(&"@string_number_map".to_string()));
    assert!(field_annotations(&result, "created_at").contains(&"@readonly".to_string()));
    assert!(field_annotations(&result, "email").contains(&"@validated".to_string()));
    assert!(field_annotations(&result, "secret").contains(&"@skip_internal".to_string()));
    assert!(field_annotations(&result, "__private").contains(&"@skip_private".to_string()));
    assert!(
        !result.field_overrides.contains_key("normal"),
        "normal field should be untouched"
    );
    assert!(
        field_annotations(&result, "json_tricky")
            .iter()
            .any(|annotation| annotation.contains("@tricky("))
    );
}
