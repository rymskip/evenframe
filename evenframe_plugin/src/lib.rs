//! Helper crate for writing evenframe WASM plugins.
//!
//! Supports two plugin types:
//! - **Field plugins** (`define_field_plugin!`): Generate mock data for individual fields.
//! - **Type plugins** (`define_type_plugin!`): Conditionally modify type generation output.
//!
//! Eliminates all WASM boilerplate (alloc, dealloc, JSON serialization,
//! pointer packing) so you only write your generation logic.
//!
//! # Example
//!
//! ```rust,ignore
//! use evenframe_plugin::{define_field_plugin, FieldContext};
//!
//! define_field_plugin!(|ctx: &FieldContext| {
//!     if ctx.record_index != 0 { return None; }
//!     match ctx.field_name.as_str() {
//!         "email" => Some("'test@example.com'"),
//!         "password" => Some("'TestPassword123!'"),
//!         _ => None, // fall back to evenframe's default generation
//!     }
//! });
//! ```

#[doc(hidden)]
pub use serde_json;

use serde::{Deserialize, Serialize};

/// Context passed to field-level plugin functions.
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

/// Define a field-level mockmake WASM plugin.
///
/// Takes a closure `|ctx: &FieldContext| -> Option<&str>` (or `Option<String>`).
/// Return `Some("value")` to override a field, or `None` to let evenframe
/// generate the value normally.
///
/// Generates all required WASM exports: `alloc`, `dealloc`, `generate_field`, `memory`.
#[macro_export]
macro_rules! define_field_plugin {
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
// Type-transform plugin types and macro
// ============================================================

/// Context passed to type-transform plugin functions.
#[derive(Debug, Deserialize)]
pub struct TypeContext {
    pub type_name: String,
    pub kind: String,
    pub rust_derives: Vec<String>,
    pub annotations: Vec<String>,
    pub pipeline: String,
    pub generator: String,
    pub fields: Vec<TypeFieldInfo>,
}

/// Field metadata in type-transform plugin input.
#[derive(Debug, Deserialize)]
pub struct TypeFieldInfo {
    pub field_name: String,
    pub field_type: String,
    pub annotations: Vec<String>,
    pub validators: Vec<String>,
}

/// Output from a type-transform plugin.
#[derive(Serialize, Default)]
pub struct TypePluginOutput {
    #[serde(default)]
    pub field_type_overrides: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub skip_fields: Vec<String>,
    #[serde(default)]
    pub extra_imports: Vec<String>,
    #[serde(default)]
    pub field_annotations: std::collections::HashMap<String, Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_name_override: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Define a type-transform WASM plugin.
///
/// Takes a closure `|ctx: &TypeContext| -> TypePluginOutput`.
/// Inspect the struct/enum context (derives, field types, annotations) and
/// return modifications (type overrides, skipped fields, extra imports).
///
/// Generates all required WASM exports: `alloc`, `dealloc`, `transform_type`, `memory`.
///
/// # Example
///
/// ```rust,ignore
/// use evenframe_plugin::{define_type_plugin, TypeContext, TypePluginOutput};
///
/// define_type_plugin!(|ctx: &TypeContext| {
///     let mut output = TypePluginOutput::default();
///     if ctx.rust_derives.contains(&"Serialize".to_string()) {
///         for field in &ctx.fields {
///             if field.field_type == "Decimal" {
///                 output.field_type_overrides.insert(
///                     field.field_name.clone(),
///                     "BigDecimal.BigDecimal".to_string(),
///                 );
///             }
///         }
///     }
///     output
/// });
/// ```
#[macro_export]
macro_rules! define_type_plugin {
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
                    let result: $crate::TypePluginOutput = (|| $body)();
                    result
                }
                Err(e) => $crate::TypePluginOutput {
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
