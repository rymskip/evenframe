//! Regression test for the derive macro's inline-struct enum-variant codegen.
//!
//! `#[derive(Evenframe)]` builds `StructConfig { .. }` literals for enum variants
//! that carry struct/tuple data:
//!   - a multi-field tuple variant  -> `VariantData::InlineStruct` (enum_impl.rs:117)
//!   - a named-field struct variant -> `VariantData::InlineStruct` (enum_impl.rs:162)
//!
//! Those literals must set every `StructConfig` field — including `resolve_only`
//! — or any downstream crate that derives such an enum fails to compile with
//! E0063 "missing field `resolve_only`". A proc-macro's output is only
//! type-checked at the use site, and until now no in-repo `#[derive(Evenframe)]`
//! enum had a struct/tuple variant (all were C-style, unit-only), so this codegen
//! path was never compiled and the missing field slipped through `cargo
//! check`/`cargo test`. This test pins both inline-struct paths.

use evenframe::Evenframe;
use evenframe::traits::EvenframeTaggedUnion;
use evenframe::types::VariantData;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Evenframe)]
pub enum Shape {
    /// Fields::Unit -> no variant data.
    Empty,
    /// Fields::Unnamed, len == 1 -> DataStructureRef (newtype), not a StructConfig.
    Newtype(i64),
    /// Fields::Unnamed, len > 1 -> InlineStruct(StructConfig) (enum_impl.rs:117).
    Pair(i64, String),
    /// Fields::Named -> InlineStruct(StructConfig) (enum_impl.rs:162).
    Labeled { width: f64, label: String },
}

#[test]
fn inline_struct_variants_build_full_struct_config() {
    let tu = Shape::variants();
    assert_eq!(tu.enum_name, "Shape");
    assert!(!tu.resolve_only, "enum resolve_only must default to false");

    let by_name = |n: &str| {
        tu.variants
            .iter()
            .find(|v| v.name == n)
            .unwrap_or_else(|| panic!("missing variant {n}"))
    };

    // Multi-field tuple variant -> InlineStruct with a fully-built StructConfig.
    match by_name("Pair").data.as_ref().expect("Pair has data") {
        VariantData::InlineStruct(sc) => {
            assert_eq!(sc.struct_name, "Shape_Pair");
            assert_eq!(sc.fields.len(), 2);
            assert!(!sc.resolve_only, "inline-struct StructConfig.resolve_only must be false");
        }
        _ => panic!("expected InlineStruct for Pair"),
    }

    // Named-field variant -> InlineStruct with a fully-built StructConfig.
    match by_name("Labeled").data.as_ref().expect("Labeled has data") {
        VariantData::InlineStruct(sc) => {
            assert_eq!(sc.struct_name, "Shape_Labeled");
            assert_eq!(sc.fields.len(), 2);
            assert!(!sc.resolve_only, "inline-struct StructConfig.resolve_only must be false");
        }
        _ => panic!("expected InlineStruct for Labeled"),
    }

    // Newtype variant -> DataStructureRef, not an inline struct.
    assert!(matches!(
        by_name("Newtype").data.as_ref().expect("Newtype has data"),
        VariantData::DataStructureRef(_)
    ));

    // Unit variant -> no data.
    assert!(by_name("Empty").data.is_none());
}
