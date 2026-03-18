//! A minimal evenframe mockmake WASM plugin for testing.
//!
//! Exports: alloc, dealloc, generate_field, memory
//! Generates deterministic values based on field name and record index.

use serde::{Deserialize, Serialize};
use std::alloc::Layout;

#[derive(Deserialize)]
struct FieldInput {
    #[allow(dead_code)]
    table_name: String,
    field_name: String,
    field_type: String,
    record_index: usize,
    #[allow(dead_code)]
    total_records: usize,
    #[allow(dead_code)]
    record_id: String,
}

#[derive(Serialize)]
struct FieldOutput {
    value: String,
}

/// Allocate memory for the host to write input data.
#[unsafe(no_mangle)]
pub extern "C" fn alloc(size: i32) -> i32 {
    let layout = Layout::from_size_align(size as usize, 1).unwrap();
    unsafe { std::alloc::alloc(layout) as i32 }
}

/// Free memory after the host reads output data.
#[unsafe(no_mangle)]
pub extern "C" fn dealloc(ptr: i32, len: i32) {
    let layout = Layout::from_size_align(len as usize, 1).unwrap();
    unsafe { std::alloc::dealloc(ptr as *mut u8, layout) }
}

/// Generate a field value. Returns packed (ptr << 32 | len) as i64.
#[unsafe(no_mangle)]
pub extern "C" fn generate_field(ptr: i32, len: i32) -> i64 {
    let input_bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };

    let output = match serde_json::from_slice::<FieldInput>(input_bytes) {
        Ok(input) => {
            // Generate a deterministic value based on field name and index
            let value = match input.field_type.as_str() {
                "String" | "Char" => {
                    format!("'plugin_{}_{}'", input.field_name, input.record_index)
                }
                "Bool" => (input.record_index % 2 == 0).to_string(),
                "I32" | "I64" | "U32" | "U64" | "Isize" | "Usize" => {
                    format!("{}", input.record_index * 100)
                }
                "F32" | "F64" => format!("{:.2}", input.record_index as f64 * 1.5),
                _ => format!("'plugin_unknown_{}'", input.record_index),
            };
            serde_json::to_vec(&FieldOutput { value }).unwrap()
        }
        Err(e) => {
            let err = serde_json::json!({ "error": format!("parse error: {}", e) });
            serde_json::to_vec(&err).unwrap()
        }
    };

    let out_ptr = alloc(output.len() as i32);
    unsafe {
        std::ptr::copy_nonoverlapping(output.as_ptr(), out_ptr as *mut u8, output.len());
    }

    // Pack pointer and length into i64
    ((out_ptr as i64) << 32) | (output.len() as i64)
}
