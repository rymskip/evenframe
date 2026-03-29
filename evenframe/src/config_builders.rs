//! Configuration builders for processing Evenframe types.
//!
//! This module re-exports functionality from `evenframe_core::tooling`.

pub use evenframe_core::tooling::{
    BuildConfig, build_all_configs, filter_for_schemasync, filter_for_typesync,
    merge_tables_and_objects,
};
