//! Helper crate for writing evenframe mockmake WASM plugins.
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
