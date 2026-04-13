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
use std::collections::{BTreeMap, HashMap};

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
///
/// This is a tagged union matching
/// `evenframe_core::typesync::plugin_types::OutputRulePluginInput`.
/// Plugins should `match` on the variant to dispatch: free-standing
/// struct, table-backed struct, or tagged-union enum. Each variant
/// carries the full evenframe-side config(s) as `serde_json::Value` so
/// plugin authors don't have to pull `evenframe_core` into their
/// cdylibs. See the helper methods on [`TypeContext`] for ergonomic
/// accessors that cover the common lookups; anything more specialized
/// can navigate the JSON directly.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind")]
pub enum TypeContext {
    /// A standalone (non-table) Rust struct.
    Struct {
        /// Which pipeline is consuming the result: "Both", "Typesync",
        /// or "Schemasync".
        pipeline: String,
        /// Which generator is invoking the plugin ("macroforge",
        /// "arktype", etc.), or empty for the schemasync pass.
        generator: String,
        /// JSON form of `evenframe_core::types::StructConfig`.
        config: serde_json::Value,
    },
    /// A Rust struct that backs a SurrealDB table.
    Table {
        pipeline: String,
        generator: String,
        /// JSON form of `evenframe_core::types::StructConfig`.
        struct_config: serde_json::Value,
        /// JSON form of `evenframe_core::schemasync::table::TableConfig`.
        table_config: serde_json::Value,
    },
    /// A tagged-union Rust enum.
    Enum {
        pipeline: String,
        generator: String,
        /// JSON form of `evenframe_core::types::TaggedUnion`.
        config: serde_json::Value,
    },
}

impl TypeContext {
    /// The pipeline that will consume this result ("Both", "Typesync",
    /// or "Schemasync").
    pub fn pipeline(&self) -> &str {
        match self {
            TypeContext::Struct { pipeline, .. }
            | TypeContext::Table { pipeline, .. }
            | TypeContext::Enum { pipeline, .. } => pipeline,
        }
    }

    /// The generator invoking this plugin, or `""` for the schemasync pass.
    pub fn generator(&self) -> &str {
        match self {
            TypeContext::Struct { generator, .. }
            | TypeContext::Table { generator, .. }
            | TypeContext::Enum { generator, .. } => generator,
        }
    }

    /// The type's PascalCase name: `struct_name` for structs/tables,
    /// `enum_name` for enums.
    pub fn type_name(&self) -> Option<&str> {
        match self {
            TypeContext::Struct { config, .. } => {
                config.get("struct_name").and_then(|v| v.as_str())
            }
            TypeContext::Table { struct_config, .. } => {
                struct_config.get("struct_name").and_then(|v| v.as_str())
            }
            TypeContext::Enum { config, .. } => config.get("enum_name").and_then(|v| v.as_str()),
        }
    }

    /// For tables: the snake_case table name. `None` for non-table
    /// structs and enums.
    pub fn table_name(&self) -> Option<&str> {
        match self {
            TypeContext::Table { table_config, .. } => {
                table_config.get("table_name").and_then(|v| v.as_str())
            }
            _ => None,
        }
    }

    /// True when the table is a relation/edge. `false` for non-tables.
    pub fn is_relation(&self) -> bool {
        match self {
            TypeContext::Table { table_config, .. } => {
                table_config.get("relation").is_some_and(|v| !v.is_null())
            }
            _ => false,
        }
    }

    /// True when the table already has explicit `permissions` set (via
    /// `#[permissions(...)]` or the per-field `#[define_field_statement(...)]`
    /// aggregate). `false` for non-tables.
    pub fn has_explicit_permissions(&self) -> bool {
        match self {
            TypeContext::Table { table_config, .. } => table_config
                .get("permissions")
                .is_some_and(|v| !v.is_null()),
            _ => false,
        }
    }

    /// True when the table already has any `events` defined via
    /// `#[event(...)]`. `false` for non-tables.
    pub fn has_explicit_events(&self) -> bool {
        match self {
            TypeContext::Table { table_config, .. } => table_config
                .get("events")
                .and_then(|v| v.as_array())
                .is_some_and(|arr| !arr.is_empty()),
            _ => false,
        }
    }

    /// True when the table already has an explicit `mock_generation_config`.
    /// `false` for non-tables.
    pub fn has_explicit_mock_data(&self) -> bool {
        match self {
            TypeContext::Table { table_config, .. } => table_config
                .get("mock_generation_config")
                .is_some_and(|v| !v.is_null()),
            _ => false,
        }
    }

    /// Macroforge derives already declared in the Rust source via
    /// `#[macroforge_derive(...)]`. Empty if none are present.
    pub fn existing_macroforge_derives(&self) -> Vec<String> {
        let node = match self {
            TypeContext::Struct { config, .. } => config.get("macroforge_derives"),
            TypeContext::Table { struct_config, .. } => struct_config.get("macroforge_derives"),
            TypeContext::Enum { config, .. } => config.get("macroforge_derives"),
        };
        node.and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Type-level `#[annotation("...")]` strings as written in the
    /// Rust source.
    pub fn annotations(&self) -> Vec<String> {
        let node = match self {
            TypeContext::Struct { config, .. } => config.get("annotations"),
            TypeContext::Table { struct_config, .. } => struct_config.get("annotations"),
            TypeContext::Enum { config, .. } => config.get("annotations"),
        };
        node.and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Type-level raw attribute stubs — attributes that evenframe doesn't
    /// parse natively, keyed by attribute name with each value being the
    /// parenthesized body (or `""` for bare path attributes like
    /// `#[overview]`). Multi-occurrence attributes preserve order.
    ///
    /// Returned as a `BTreeMap` so iteration is in sorted attribute-name
    /// order, which keeps downstream annotation emission deterministic.
    pub fn raw_attributes(&self) -> BTreeMap<String, Vec<String>> {
        let node = match self {
            TypeContext::Struct { config, .. } => config.get("raw_attributes"),
            TypeContext::Table { struct_config, .. } => struct_config.get("raw_attributes"),
            TypeContext::Enum { config, .. } => config.get("raw_attributes"),
        };
        node.and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .filter_map(|(k, v)| {
                        let bodies: Vec<String> = v
                            .as_array()?
                            .iter()
                            .filter_map(|x| x.as_str().map(str::to_string))
                            .collect();
                        Some((k.clone(), bodies))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Iterate the struct's fields (or enum's variants). Each entry is
    /// the raw JSON node so callers can introspect anything — use the
    /// [`TypeFieldInfo`] accessors for the common fields.
    pub fn fields(&self) -> Vec<TypeFieldInfo> {
        let arr = match self {
            TypeContext::Struct { config, .. } => config.get("fields"),
            TypeContext::Table { struct_config, .. } => struct_config.get("fields"),
            TypeContext::Enum { config, .. } => config.get("variants"),
        };
        arr.and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .map(|v| TypeFieldInfo { node: v.clone() })
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// A single field (for structs/tables) or variant (for enums). Wraps the
/// raw JSON node so callers can read any evenframe-side metadata; the
/// common-case getters below cover stub parsing, annotations, and
/// validators.
#[derive(Debug, Clone)]
pub struct TypeFieldInfo {
    /// JSON form of `evenframe_core::types::StructField` for struct
    /// fields, or `VariantInfo` for enum variants.
    pub node: serde_json::Value,
}

impl TypeFieldInfo {
    /// Field or variant name (e.g., `"customer_name"` or `"Viewer"`).
    pub fn field_name(&self) -> Option<&str> {
        // Variants use `name`, struct fields use `field_name`.
        self.node
            .get("field_name")
            .and_then(|v| v.as_str())
            .or_else(|| self.node.get("name").and_then(|v| v.as_str()))
    }

    /// Natively-parsed `#[annotation("...")]` strings on this field/variant.
    pub fn annotations(&self) -> Vec<String> {
        self.node
            .get("annotations")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Field-level raw attribute stubs (e.g., `#[text(label = "...")]`).
    /// See [`TypeContext::raw_attributes`] for shape details.
    pub fn raw_attributes(&self) -> BTreeMap<String, Vec<String>> {
        self.node
            .get("raw_attributes")
            .and_then(|v| v.as_object())
            .map(|obj| {
                obj.iter()
                    .filter_map(|(k, v)| {
                        let bodies: Vec<String> = v
                            .as_array()?
                            .iter()
                            .filter_map(|x| x.as_str().map(str::to_string))
                            .collect();
                        Some((k.clone(), bodies))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Documentation comment on this field/variant, if any.
    pub fn doccom(&self) -> Option<&str> {
        self.node.get("doccom").and_then(|v| v.as_str())
    }
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

/// Builder for constructing a [`TypeContext`] in tests.
///
/// The builder emits the same JSON shape evenframe_core would send at
/// runtime, so plugin tests exercise the exact deserialization path the
/// production WASM plugin will. See the three entry points
/// [`Self::struct_`], [`Self::table`], and [`Self::enum_`].
pub struct TypeContextBuilder {
    variant: BuilderVariant,
    pipeline: String,
    generator: String,
    // Shared "struct" config state (used by Struct and Table variants).
    struct_name: String,
    struct_annotations: Vec<String>,
    struct_raw_attributes: BTreeMap<String, Vec<serde_json::Value>>,
    struct_macroforge_derives: Vec<String>,
    struct_rust_derives: Vec<String>,
    fields: Vec<serde_json::Value>,
    // Enum-only state.
    enum_name: String,
    enum_annotations: Vec<String>,
    enum_raw_attributes: BTreeMap<String, Vec<serde_json::Value>>,
    enum_macroforge_derives: Vec<String>,
    variants: Vec<serde_json::Value>,
    // Table-only state.
    table_name: String,
    table_permissions: Option<serde_json::Value>,
    table_events: Vec<serde_json::Value>,
    table_relation: Option<serde_json::Value>,
}

enum BuilderVariant {
    Struct,
    Table,
    Enum,
}

impl TypeContextBuilder {
    /// Build a context for a standalone (non-table) struct.
    pub fn struct_(name: &str) -> Self {
        Self::new(BuilderVariant::Struct, name)
    }

    /// Build a context for a table-backed struct. The table name defaults
    /// to snake_case(`name`); override with [`Self::table_name_override`].
    pub fn table(name: &str) -> Self {
        let mut b = Self::new(BuilderVariant::Table, name);
        b.table_name = to_snake(name);
        b
    }

    /// Build a context for a tagged-union enum.
    pub fn enum_(name: &str) -> Self {
        let mut b = Self::new(BuilderVariant::Enum, name);
        b.enum_name = name.to_string();
        b
    }

    fn new(variant: BuilderVariant, name: &str) -> Self {
        Self {
            variant,
            pipeline: "Both".to_string(),
            generator: String::new(),
            struct_name: name.to_string(),
            struct_annotations: Vec::new(),
            struct_raw_attributes: BTreeMap::new(),
            struct_macroforge_derives: Vec::new(),
            struct_rust_derives: Vec::new(),
            fields: Vec::new(),
            enum_name: String::new(),
            enum_annotations: Vec::new(),
            enum_raw_attributes: BTreeMap::new(),
            enum_macroforge_derives: Vec::new(),
            variants: Vec::new(),
            table_name: String::new(),
            table_permissions: None,
            table_events: Vec::new(),
            table_relation: None,
        }
    }

    /// Add a struct field. `field_type` is stored as a canonical string
    /// (same shape evenframe produces).
    pub fn field(mut self, name: &str, field_type: &str) -> Self {
        self.fields.push(serde_json::json!({
            "field_name": name,
            "field_type": field_type,
            "annotations": Vec::<String>::new(),
            "validators": Vec::<String>::new(),
            "raw_attributes": serde_json::Map::new(),
        }));
        self
    }

    /// Add a struct field with a pre-existing annotation (from a native
    /// `#[annotation("...")]` attribute in Rust).
    pub fn field_with_annotation(mut self, name: &str, field_type: &str, annotation: &str) -> Self {
        self.fields.push(serde_json::json!({
            "field_name": name,
            "field_type": field_type,
            "annotations": vec![annotation.to_string()],
            "validators": Vec::<String>::new(),
            "raw_attributes": serde_json::Map::new(),
        }));
        self
    }

    /// Add an enum variant (no data payload).
    pub fn variant(mut self, name: &str) -> Self {
        self.variants.push(serde_json::json!({
            "name": name,
            "data": serde_json::Value::Null,
            "annotations": Vec::<String>::new(),
            "raw_attributes": serde_json::Map::new(),
        }));
        self
    }

    /// Add a type-level raw attribute stub (e.g. `#[overview]` →
    /// `("overview", "")`). Appends bodies so multi-occurrence attributes
    /// accumulate.
    pub fn with_raw_attr(mut self, name: &str, body: &str) -> Self {
        let target = match self.variant {
            BuilderVariant::Enum => &mut self.enum_raw_attributes,
            _ => &mut self.struct_raw_attributes,
        };
        target
            .entry(name.to_string())
            .or_default()
            .push(serde_json::Value::String(body.to_string()));
        self
    }

    /// Add a field-level raw attribute stub to the last-added field or
    /// variant.
    pub fn with_field_raw_attr(mut self, name: &str, body: &str) -> Self {
        let target = match self.variant {
            BuilderVariant::Enum => self.variants.last_mut(),
            _ => self.fields.last_mut(),
        };
        if let Some(node) = target
            && let Some(map) = node
                .get_mut("raw_attributes")
                .and_then(|v| v.as_object_mut())
        {
            let entry = map
                .entry(name.to_string())
                .or_insert_with(|| serde_json::Value::Array(Vec::new()));
            if let Some(arr) = entry.as_array_mut() {
                arr.push(serde_json::Value::String(body.to_string()));
            }
        }
        self
    }

    /// Seed `macroforge_derives` on the underlying struct/enum config as
    /// if the Rust source had `#[macroforge_derive(...)]`.
    pub fn with_macroforge_derives(mut self, derives: &[&str]) -> Self {
        let derives: Vec<String> = derives.iter().map(|s| s.to_string()).collect();
        match self.variant {
            BuilderVariant::Enum => self.enum_macroforge_derives = derives,
            _ => self.struct_macroforge_derives = derives,
        }
        self
    }

    /// Override the snake_case table name (`Table` variant only).
    pub fn table_name_override(mut self, name: &str) -> Self {
        self.table_name = name.to_string();
        self
    }

    /// Mark the table as already having explicit permissions.
    pub fn with_explicit_permissions(mut self) -> Self {
        self.table_permissions = Some(serde_json::json!({
            "all_permissions": null,
            "select_permissions": "FULL",
            "update_permissions": "FULL",
            "delete_permissions": "FULL",
            "create_permissions": "FULL"
        }));
        self
    }

    /// Mark the table as already having explicit events.
    pub fn with_explicit_events(mut self) -> Self {
        self.table_events
            .push(serde_json::json!({ "statement": "DEFINE EVENT stub ON TABLE x ..." }));
        self
    }

    /// Mark the table as a relation/edge.
    pub fn as_relation(mut self) -> Self {
        self.table_relation = Some(serde_json::json!({
            "edge_name": self.table_name,
            "from": Vec::<String>::new(),
            "to": Vec::<String>::new(),
            "direction": null
        }));
        self
    }

    pub fn build(self) -> TypeContext {
        let struct_config = serde_json::json!({
            "struct_name": self.struct_name,
            "fields": self.fields,
            "validators": Vec::<String>::new(),
            "doccom": serde_json::Value::Null,
            "macroforge_derives": self.struct_macroforge_derives,
            "annotations": self.struct_annotations,
            "pipeline": "Both",
            "rust_derives": self.struct_rust_derives,
            "raw_attributes": self.struct_raw_attributes,
        });

        match self.variant {
            BuilderVariant::Struct => TypeContext::Struct {
                pipeline: self.pipeline,
                generator: self.generator,
                config: struct_config,
            },
            BuilderVariant::Table => {
                let table_config = serde_json::json!({
                    "table_name": self.table_name,
                    "struct_config": struct_config.clone(),
                    "relation": self.table_relation,
                    "permissions": self.table_permissions,
                    "mock_generation_config": serde_json::Value::Null,
                    "events": self.table_events,
                });
                TypeContext::Table {
                    pipeline: self.pipeline,
                    generator: self.generator,
                    struct_config,
                    table_config,
                }
            }
            BuilderVariant::Enum => {
                let enum_config = serde_json::json!({
                    "enum_name": self.enum_name,
                    "variants": self.variants,
                    "representation": { "ExternallyTagged": null },
                    "doccom": serde_json::Value::Null,
                    "macroforge_derives": self.enum_macroforge_derives,
                    "annotations": self.enum_annotations,
                    "pipeline": "Both",
                    "rust_derives": Vec::<String>::new(),
                    "raw_attributes": self.enum_raw_attributes,
                });
                TypeContext::Enum {
                    pipeline: self.pipeline,
                    generator: self.generator,
                    config: enum_config,
                }
            }
        }
    }
}

/// Run a rule plugin function with a JSON roundtrip (simulates WASM IPC).
///
/// Panics with clear messages if the roundtrip fails.
pub fn test_plugin(
    plugin_fn: impl Fn(&TypeContext) -> OutputRulePluginOutput,
    ctx: TypeContext,
) -> OutputRulePluginOutput {
    // We only serialize TypeContext via its Deserialize impl (from the
    // JSON evenframe_core emits). Here we reconstruct that JSON shape
    // from the in-memory TypeContext by matching on the variant and
    // reassembling it so the roundtrip exercises real deserialization.
    let json = match &ctx {
        TypeContext::Struct {
            pipeline,
            generator,
            config,
        } => serde_json::json!({
            "kind": "Struct",
            "pipeline": pipeline,
            "generator": generator,
            "config": config,
        }),
        TypeContext::Table {
            pipeline,
            generator,
            struct_config,
            table_config,
        } => serde_json::json!({
            "kind": "Table",
            "pipeline": pipeline,
            "generator": generator,
            "struct_config": struct_config,
            "table_config": table_config,
        }),
        TypeContext::Enum {
            pipeline,
            generator,
            config,
        } => serde_json::json!({
            "kind": "Enum",
            "pipeline": pipeline,
            "generator": generator,
            "config": config,
        }),
    };

    let bytes = serde_json::to_vec(&json).expect("Failed to serialize TypeContext test JSON");
    let ctx: TypeContext = serde_json::from_slice(&bytes)
        .expect("Failed to deserialize TypeContext — roundtrip failed");

    let output = plugin_fn(&ctx);

    let bytes = serde_json::to_vec(&output).expect("Failed to serialize OutputRulePluginOutput");
    let output: OutputRulePluginOutput = serde_json::from_slice(&bytes)
        .expect("Failed to deserialize OutputRulePluginOutput — roundtrip failed");

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
