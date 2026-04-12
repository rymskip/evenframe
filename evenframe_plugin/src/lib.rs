//! Helper crate for writing evenframe WASM plugins.
//!
//! Supports three plugin types:
//! - **Mock data plugins** (`define_mock_data_plugin!`): Generate mock data for individual fields.
//! - **Output rule plugins** (`define_output_rule_plugin!`): Provide convention-based annotations,
//!   permissions, events, derives, and field-level rules for existing structs.
//! - **Synthetic item plugins** (`define_synthetic_item_plugin!`): Emit *new*
//!   structs, tagged unions, and DB tables derived from the scanner results.
//!
//! Eliminates all WASM boilerplate (alloc, dealloc, JSON serialization,
//! pointer packing) so you only write your generation logic.

#[doc(hidden)]
pub use serde_json;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================
// Mock data plugin types and macro
// ============================================================

/// Context passed to mock data plugin functions.
#[derive(Debug, Deserialize)]
pub struct FieldContext {
    pub table_name: String,
    pub field_name: String,
    pub field_type: String,
    pub record_index: usize,
    pub total_records: usize,
    pub record_id: String,
}

#[derive(Serialize)]
#[doc(hidden)]
pub struct __FieldOutput {
    pub value: Option<String>,
    pub error: Option<String>,
}

/// Define a mock data WASM plugin for field-level value generation.
///
/// Takes a closure `|ctx: &FieldContext| -> Option<&str>`.
/// Return `Some("value")` to override a field, or `None` to let evenframe
/// generate the value normally.
///
/// Generates all required WASM exports: `alloc`, `dealloc`, `generate_field`, `memory`.
#[macro_export]
macro_rules! define_mock_data_plugin {
    (|$ctx:ident : &FieldContext| $body:expr) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn alloc(size: i32) -> i32 {
            let layout = ::std::alloc::Layout::from_size_align(size as usize, 1).unwrap();
            unsafe { ::std::alloc::alloc(layout) as i32 }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn dealloc(ptr: i32, len: i32) {
            let layout = ::std::alloc::Layout::from_size_align(len as usize, 1).unwrap();
            unsafe { ::std::alloc::dealloc(ptr as *mut u8, layout) }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn generate_field(ptr: i32, len: i32) -> i64 {
            let input_bytes =
                unsafe { ::std::slice::from_raw_parts(ptr as *const u8, len as usize) };

            let output = match $crate::serde_json::from_slice::<$crate::FieldContext>(input_bytes) {
                Ok($ctx) => {
                    let $ctx = &$ctx;
                    let result: Option<&str> = (|| $body)();
                    match result {
                        Some(val) => $crate::__FieldOutput {
                            value: Some(val.to_string()),
                            error: None,
                        },
                        None => $crate::__FieldOutput {
                            value: None,
                            error: Some("skip".into()),
                        },
                    }
                }
                Err(e) => $crate::__FieldOutput {
                    value: None,
                    error: Some(format!("parse error: {}", e)),
                },
            };

            let bytes = $crate::serde_json::to_vec(&output).unwrap();
            let out_ptr = alloc(bytes.len() as i32);
            unsafe {
                ::std::ptr::copy_nonoverlapping(bytes.as_ptr(), out_ptr as *mut u8, bytes.len());
            }
            ((out_ptr as i64) << 32) | (bytes.len() as i64)
        }
    };
}

// ============================================================
// Output rule plugin types and macro
// ============================================================

/// Context passed to output rule plugin functions.
#[derive(Debug, Serialize, Deserialize)]
pub struct TypeContext {
    pub type_name: String,
    /// "Struct" or "Enum" (matches evenframe's TypeKind serialization).
    pub kind: String,
    pub rust_derives: Vec<String>,
    /// Annotations already explicitly defined on the struct.
    pub annotations: Vec<String>,
    /// Which pipeline: "Both", "Typesync", "Schemasync".
    pub pipeline: String,
    /// The generator being invoked ("macroforge", "arktype", "effect", "surrealdb", etc.).
    pub generator: String,
    pub fields: Vec<TypeFieldInfo>,
    /// The snake_case table name (e.g., "order"). Empty for non-table types.
    #[serde(default)]
    pub table_name: String,
    /// Whether this is a relation/edge table.
    #[serde(default)]
    pub is_relation: bool,
    /// Whether `#[permissions(...)]` is explicitly defined.
    #[serde(default)]
    pub has_explicit_permissions: bool,
    /// Whether `#[event(...)]` is explicitly defined.
    #[serde(default)]
    pub has_explicit_events: bool,
    /// Whether `#[mock_data(...)]` is explicitly defined.
    #[serde(default)]
    pub has_explicit_mock_data: bool,
    /// Macroforge derives already explicitly defined.
    #[serde(default)]
    pub existing_macroforge_derives: Vec<String>,
}

/// Field metadata in output rule plugin input.
#[derive(Debug, Serialize, Deserialize)]
pub struct TypeFieldInfo {
    pub field_name: String,
    /// Canonical type string (e.g., "String", "Option<DateTime<Utc>>", "RecordLink<Site>").
    pub field_type: String,
    pub annotations: Vec<String>,
    pub validators: Vec<String>,
    #[serde(default)]
    pub is_optional: bool,
    /// If field is `RecordLink<T>`, the inner type name (e.g., "Site").
    #[serde(default)]
    pub record_link_target: Option<String>,
    /// If field is `Vec<T>`, the inner type name.
    #[serde(default)]
    pub vec_inner_type: Option<String>,
    #[serde(default)]
    pub has_explicit_format: bool,
    #[serde(default)]
    pub existing_format: Option<String>,
    #[serde(default)]
    pub has_explicit_define: bool,
}

/// Type-level override from a rule plugin.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TypeOverride {
    /// Macroforge derives for typesync.
    #[serde(default)]
    pub macroforge_derives: Vec<String>,
    /// Annotations for typesync.
    #[serde(default)]
    pub annotations: Vec<String>,
    /// Table permissions for schemasync.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<PermissionsOverride>,
    /// Event definitions for schemasync.
    #[serde(default)]
    pub events: Vec<EventOverride>,
}

/// Field-level override from a rule plugin.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FieldOverride {
    /// Annotations for typesync.
    #[serde(default)]
    pub annotations: Vec<String>,
}

/// Permissions for schemasync DEFINE TABLE.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PermissionsOverride {
    pub select: String,
    pub create: String,
    pub update: String,
    pub delete: String,
}

/// Event definition for schemasync.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EventOverride {
    pub name: String,
    pub statement: String,
}

/// Output from an output rule plugin.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct OutputRulePluginOutput {
    /// Type-level overrides.
    #[serde(default)]
    pub type_override: TypeOverride,
    /// Per-field overrides: field_name -> FieldOverride.
    #[serde(default)]
    pub field_overrides: HashMap<String, FieldOverride>,
    /// Error message — if set, this plugin's output is skipped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Define an output rule WASM plugin.
///
/// Takes a closure `|ctx: &TypeContext| -> OutputRulePluginOutput`.
/// Inspect the struct/enum context and return convention-based defaults for
/// permissions, events, derives, annotations, and field-level rules.
///
/// Generates all required WASM exports: `alloc`, `dealloc`, `transform_type`, `memory`.
#[macro_export]
macro_rules! define_output_rule_plugin {
    (|$ctx:ident : &TypeContext| $body:expr) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn alloc(size: i32) -> i32 {
            let layout = ::std::alloc::Layout::from_size_align(size as usize, 1).unwrap();
            unsafe { ::std::alloc::alloc(layout) as i32 }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn dealloc(ptr: i32, len: i32) {
            let layout = ::std::alloc::Layout::from_size_align(len as usize, 1).unwrap();
            unsafe { ::std::alloc::dealloc(ptr as *mut u8, layout) }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn transform_type(ptr: i32, len: i32) -> i64 {
            let input_bytes =
                unsafe { ::std::slice::from_raw_parts(ptr as *const u8, len as usize) };

            let output = match $crate::serde_json::from_slice::<$crate::TypeContext>(input_bytes) {
                Ok($ctx) => {
                    let $ctx = &$ctx;
                    let result: $crate::OutputRulePluginOutput = (|| $body)();
                    result
                }
                Err(e) => $crate::OutputRulePluginOutput {
                    error: Some(format!("parse error: {}", e)),
                    ..Default::default()
                },
            };

            let bytes = $crate::serde_json::to_vec(&output).unwrap();
            let out_ptr = alloc(bytes.len() as i32);
            unsafe {
                ::std::ptr::copy_nonoverlapping(bytes.as_ptr(), out_ptr as *mut u8, bytes.len());
            }
            ((out_ptr as i64) << 32) | (bytes.len() as i64)
        }
    };
}

// ============================================================
// Testing utilities
// ============================================================

/// Builder for constructing `TypeContext` in tests.
pub struct TypeContextBuilder {
    ctx: TypeContext,
}

impl TypeContextBuilder {
    /// Create a table struct context with the given name (has table_name set).
    pub fn handler(name: &str) -> Self {
        Self {
            ctx: TypeContext {
                type_name: name.to_string(),
                kind: "Struct".to_string(),
                rust_derives: vec![],
                annotations: vec![],
                pipeline: "Both".to_string(),
                generator: String::new(),
                fields: vec![],
                table_name: to_snake(name),
                is_relation: false,
                has_explicit_permissions: false,
                has_explicit_events: false,
                has_explicit_mock_data: false,
                existing_macroforge_derives: vec![],
            },
        }
    }

    /// Create a non-table struct context (no table_name).
    pub fn standard(name: &str) -> Self {
        let mut b = Self::handler(name);
        b.ctx.table_name = String::new();
        b
    }

    /// Create an enum context.
    pub fn tagged_union(name: &str) -> Self {
        let mut b = Self::handler(name);
        b.ctx.kind = "Enum".to_string();
        b.ctx.table_name = String::new();
        b
    }

    /// Add a field.
    pub fn field(mut self, name: &str, field_type: &str) -> Self {
        self.ctx.fields.push(TypeFieldInfo {
            field_name: name.to_string(),
            field_type: field_type.to_string(),
            annotations: vec![],
            validators: vec![],
            is_optional: field_type.starts_with("Option"),
            record_link_target: extract_record_link_target(field_type),
            vec_inner_type: extract_vec_inner(field_type),
            has_explicit_format: false,
            existing_format: None,
            has_explicit_define: false,
        });
        self
    }

    /// Add a field with an existing annotation.
    pub fn field_with_annotation(mut self, name: &str, field_type: &str, annotation: &str) -> Self {
        self.ctx.fields.push(TypeFieldInfo {
            field_name: name.to_string(),
            field_type: field_type.to_string(),
            annotations: vec![annotation.to_string()],
            validators: vec![],
            is_optional: field_type.starts_with("Option"),
            record_link_target: extract_record_link_target(field_type),
            vec_inner_type: extract_vec_inner(field_type),
            has_explicit_format: false,
            existing_format: None,
            has_explicit_define: false,
        });
        self
    }

    pub fn with_explicit_permissions(mut self) -> Self {
        self.ctx.has_explicit_permissions = true;
        self
    }

    pub fn with_explicit_events(mut self) -> Self {
        self.ctx.has_explicit_events = true;
        self
    }

    pub fn with_explicit_mock_data(mut self) -> Self {
        self.ctx.has_explicit_mock_data = true;
        self
    }

    pub fn with_macroforge_derives(mut self, derives: &[&str]) -> Self {
        self.ctx.existing_macroforge_derives = derives.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn build(self) -> TypeContext {
        self.ctx
    }
}

/// Run a rule plugin function with full validation.
///
/// 1. Validates the TypeContext matches evenframe's conventions (kind values, etc.)
/// 2. JSON roundtrips input and output (simulates WASM IPC)
/// 3. Validates the output overrides are well-formed
///
/// Panics with clear messages if anything is wrong.
pub fn test_plugin(
    plugin_fn: impl Fn(&TypeContext) -> OutputRulePluginOutput,
    ctx: TypeContext,
) -> OutputRulePluginOutput {
    // Validate kind matches evenframe's TypeKind enum
    assert!(
        ctx.kind == "Struct" || ctx.kind == "Enum",
        "TypeContext.kind must be \"Struct\" or \"Enum\" (evenframe's TypeKind values), got \"{}\".\n\
         Note: application-specific aliases like \"handler\", \"standard\", \"tagged_union\" are NOT \
         what evenframe sends. Use table_name.is_empty() to distinguish tables from nested structs.",
        ctx.kind
    );

    // Validate table_name consistency
    if ctx.kind == "Enum" {
        assert!(
            ctx.table_name.is_empty(),
            "Enums should not have a table_name, got \"{}\"",
            ctx.table_name
        );
    }

    // JSON roundtrip input (simulates WASM IPC)
    let json = serde_json::to_vec(&ctx)
        .expect("Failed to serialize TypeContext — builder produced invalid data");
    let ctx: TypeContext = serde_json::from_slice(&json)
        .expect("Failed to deserialize TypeContext — serialization roundtrip failed");

    let output = plugin_fn(&ctx);

    // JSON roundtrip output (simulates WASM IPC)
    let json = serde_json::to_vec(&output).expect("Failed to serialize OutputRulePluginOutput");
    let output: OutputRulePluginOutput = serde_json::from_slice(&json)
        .expect("Failed to deserialize OutputRulePluginOutput — roundtrip failed");

    // Validate output overrides are well-formed
    for (field_name, ov) in &output.field_overrides {
        assert!(
            !ov.annotations.is_empty(),
            "field_overrides[\"{}\"].annotations is empty — don't add an override with no annotations",
            field_name
        );
    }

    output
}

/// Print a summary of what the plugin produced.
pub fn print_plugin_output(type_name: &str, output: &OutputRulePluginOutput) {
    println!("=== Output for '{}' ===", type_name);
    if !output.type_override.macroforge_derives.is_empty() {
        println!(
            "  macroforge_derives: {:?}",
            output.type_override.macroforge_derives
        );
    }
    if !output.type_override.annotations.is_empty() {
        println!("  annotations: {:?}", output.type_override.annotations);
    }
    if let Some(ref perms) = output.type_override.permissions {
        println!(
            "  permissions: select={}, create={}, update={}, delete={}",
            perms.select, perms.create, perms.update, perms.delete
        );
    }
    for event in &output.type_override.events {
        println!(
            "  event '{}': {}",
            event.name,
            &event.statement[..event.statement.len().min(80)]
        );
    }
    for (field, ov) in &output.field_overrides {
        println!("  field '{}' annotations: {:?}", field, ov.annotations);
    }
    if let Some(ref err) = output.error {
        println!("  ERROR: {}", err);
    }
    println!();
}

fn to_snake(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }
    result
}

fn extract_record_link_target(t: &str) -> Option<String> {
    let s = t.strip_prefix("Option<").unwrap_or(t);
    if let Some(rest) = s.strip_prefix("RecordLink<") {
        rest.strip_suffix('>')
            .or_else(|| rest.strip_suffix(">>"))
            .map(|s| s.to_string())
    } else {
        None
    }
}

fn extract_vec_inner(t: &str) -> Option<String> {
    let s = t.strip_prefix("Option<").unwrap_or(t);
    if let Some(rest) = s.strip_prefix("Vec<") {
        rest.strip_suffix('>')
            .or_else(|| rest.strip_suffix(">>"))
            .map(|s| s.to_string())
    } else {
        None
    }
}

// ============================================================
// Synthetic item plugin types and macro
// ============================================================
//
// Unlike output-rule plugins, synthetic plugins *add* new structs/enums/
// tables derived from the scanner results rather than overriding existing
// items. The host sends the FULL set of configs (not lossy summaries) so
// plugins can make system-wide decisions and copy field types verbatim
// from existing types into new ones.
//
// Both the input and output carry items as `serde_json::Value` so this
// crate stays tiny (no `evenframe_core` dep). The host serializes real
// `StructConfig` / `TaggedUnion` / `TableConfig` values and the plugin
// navigates the raw JSON or deserializes it into a locally-defined
// mirror type tailored to its needs.

/// Full snapshot of everything evenframe knows about so far, handed to
/// each synthetic plugin call.
///
/// All three maps hold items as `serde_json::Value`:
///
/// - `structs`:  keyed by struct name, values are the JSON form of
///               `evenframe_core::types::StructConfig`.
/// - `enums`:    keyed by enum name, values are the JSON form of
///               `evenframe_core::types::TaggedUnion`.
/// - `tables`:   keyed by snake_case table name, values are the JSON form of
///               `evenframe_core::schemasync::table::TableConfig`.
///
/// Each plugin sees the cumulative state after all previous synthetic
/// plugins have run, so chained plugins can build on each other.
#[derive(Debug, Deserialize)]
pub struct SyntheticContext {
    #[serde(default)]
    pub structs: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub enums: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub tables: HashMap<String, serde_json::Value>,
}

impl SyntheticContext {
    /// Returns the serialized `FieldType` of a field inside a given struct,
    /// if that struct exists and has such a field. The returned value is
    /// ready to be splatted into a new struct via [`struct_item`] or
    /// [`struct_item_with_raw`].
    ///
    /// This is the primary ergonomic helper for partial/projection-style
    /// plugins that need to copy an existing field's type verbatim into
    /// a newly-generated struct.
    pub fn struct_field_type(
        &self,
        struct_name: &str,
        field_name: &str,
    ) -> Option<serde_json::Value> {
        let sc = self.structs.get(struct_name)?;
        let fields = sc.get("fields")?.as_array()?;
        for f in fields {
            if f.get("field_name")?.as_str()? == field_name {
                return f.get("field_type").cloned();
            }
        }
        None
    }

    /// Same as [`Self::struct_field_type`] but searches a *table*'s
    /// inner `struct_config.fields`.
    pub fn table_field_type(
        &self,
        table_name: &str,
        field_name: &str,
    ) -> Option<serde_json::Value> {
        let tc = self.tables.get(table_name)?;
        let fields = tc.get("struct_config")?.get("fields")?.as_array()?;
        for f in fields {
            if f.get("field_name")?.as_str()? == field_name {
                return f.get("field_type").cloned();
            }
        }
        None
    }

    /// Returns every Rust derive on a struct (e.g. `["Debug","Clone","Serialize"]`).
    pub fn struct_rust_derives(&self, struct_name: &str) -> Vec<String> {
        self.structs
            .get(struct_name)
            .and_then(|sc| sc.get("rust_derives"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Returns every `#[annotation("...")]` string on a struct.
    pub fn struct_annotations(&self, struct_name: &str) -> Vec<String> {
        self.structs
            .get(struct_name)
            .and_then(|sc| sc.get("annotations"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Output from a synthetic-item plugin.
///
/// Each item is carried as an opaque `serde_json::Value` whose shape must
/// match evenframe_core's `StructConfig` / `TaggedUnion` / `TableConfig`
/// on the host side. See the built-in helpers [`struct_item`],
/// [`enum_item`], and [`table_item`] for ergonomic constructors.
#[derive(Debug, Serialize, Default)]
pub struct SyntheticPluginOutput {
    #[serde(default)]
    pub new_structs: Vec<serde_json::Value>,
    #[serde(default)]
    pub new_enums: Vec<serde_json::Value>,
    #[serde(default)]
    pub new_tables: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Build a minimal `StructConfig`-shaped JSON value.
///
/// `fields` is a slice of `(field_name, field_type_json)` pairs. The
/// `field_type_json` is an already-built `FieldType` JSON value — use
/// [`string_type`], [`bool_type`], etc. for primitives.
pub fn struct_item(name: &str, fields: &[(&str, serde_json::Value)]) -> serde_json::Value {
    let field_objs: Vec<serde_json::Value> = fields
        .iter()
        .map(|(fname, ftype)| {
            serde_json::json!({
                "field_name": fname,
                "field_type": ftype,
                "edge_config": serde_json::Value::Null,
                "define_config": serde_json::Value::Null,
                "format": serde_json::Value::Null,
                "validators": [],
                "always_regenerate": false,
            })
        })
        .collect();

    serde_json::json!({
        "struct_name": name,
        "fields": field_objs,
        "validators": [],
    })
}

/// Build an enum-as-tagged-union JSON value. Variants are given as
/// name-only strings (unit variants).
pub fn enum_item(name: &str, unit_variants: &[&str]) -> serde_json::Value {
    let variants: Vec<serde_json::Value> = unit_variants
        .iter()
        .map(|v| {
            serde_json::json!({
                "name": v,
                "data": serde_json::Value::Null,
                "annotations": [],
            })
        })
        .collect();

    serde_json::json!({
        "enum_name": name,
        "variants": variants,
        "representation": "ExternallyTagged",
    })
}

/// Build a `TableConfig`-shaped JSON value wrapping a synthetic struct.
pub fn table_item(
    table_name: &str,
    struct_name: &str,
    fields: &[(&str, serde_json::Value)],
) -> serde_json::Value {
    serde_json::json!({
        "table_name": table_name,
        "struct_config": struct_item(struct_name, fields),
        "relation": serde_json::Value::Null,
        "permissions": serde_json::Value::Null,
        "mock_generation_config": serde_json::Value::Null,
    })
}

// Convenience field-type constructors — match the `FieldType` serde tags.

pub fn string_type() -> serde_json::Value {
    serde_json::json!("String")
}
pub fn bool_type() -> serde_json::Value {
    serde_json::json!("Bool")
}
pub fn i64_type() -> serde_json::Value {
    serde_json::json!("I64")
}
pub fn f64_type() -> serde_json::Value {
    serde_json::json!("F64")
}
pub fn option_of(inner: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "Option": inner })
}
pub fn vec_of(inner: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "Vec": inner })
}

/// Define a synthetic-item WASM plugin.
///
/// Takes a closure `|ctx: &SyntheticContext| -> SyntheticPluginOutput`.
/// Inspect the scanner summary and return new structs/enums/tables.
///
/// Generates all required WASM exports: `alloc`, `dealloc`, `generate_items`, `memory`.
#[macro_export]
macro_rules! define_synthetic_item_plugin {
    (|$ctx:ident : &SyntheticContext| $body:expr) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn alloc(size: i32) -> i32 {
            let layout = ::std::alloc::Layout::from_size_align(size as usize, 1).unwrap();
            unsafe { ::std::alloc::alloc(layout) as i32 }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn dealloc(ptr: i32, len: i32) {
            let layout = ::std::alloc::Layout::from_size_align(len as usize, 1).unwrap();
            unsafe { ::std::alloc::dealloc(ptr as *mut u8, layout) }
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn generate_items(ptr: i32, len: i32) -> i64 {
            let input_bytes =
                unsafe { ::std::slice::from_raw_parts(ptr as *const u8, len as usize) };

            let output =
                match $crate::serde_json::from_slice::<$crate::SyntheticContext>(input_bytes) {
                    Ok($ctx) => {
                        let $ctx = &$ctx;
                        let result: $crate::SyntheticPluginOutput = (|| $body)();
                        result
                    }
                    Err(e) => $crate::SyntheticPluginOutput {
                        error: Some(format!("parse error: {}", e)),
                        ..Default::default()
                    },
                };

            let bytes = $crate::serde_json::to_vec(&output).unwrap();
            let out_ptr = alloc(bytes.len() as i32);
            unsafe {
                ::std::ptr::copy_nonoverlapping(bytes.as_ptr(), out_ptr as *mut u8, bytes.len());
            }
            ((out_ptr as i64) << 32) | (bytes.len() as i64)
        }
    };
}
