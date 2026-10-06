//! Pins what `#[derive(Evenframe)]` builds for each kind of enum variant.
//!
//! A struct variant carries a `StructConfig` literal, which must set every
//! field (including `resolve_only`) or any crate deriving such an enum fails to
//! compile with E0063. A proc-macro's output is only type-checked at the use
//! site, so this enum is the use site. Tuple variants carry what serde writes:
//! a newtype variant its one field, any other count an array.

use evenframe::Evenframe;
use evenframe::traits::EvenframeTaggedUnion;
use evenframe::types::{FieldType, VariantData};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Evenframe)]
pub enum Shape {
    Empty,
    Cleared(),
    Newtype(i64),
    Pair(i64, String),
    Labeled { width: f64, label: String },
}

#[test]
fn inline_struct_variants_build_full_struct_config() {
    let tagged_union = Shape::variants();
    assert_eq!(tagged_union.enum_name, "Shape");
    assert!(
        !tagged_union.resolve_only,
        "enum resolve_only must default to false"
    );

    let data = |name: &str| {
        tagged_union
            .variants
            .iter()
            .find(|variant| variant.name == name)
            .unwrap_or_else(|| panic!("missing variant {name}"))
            .data
            .clone()
    };

    match data("Labeled") {
        Some(VariantData::InlineStruct(inline)) => {
            assert_eq!(inline.struct_name, "Shape_Labeled");
            assert_eq!(inline.fields.len(), 2);
            assert!(
                !inline.resolve_only,
                "inline-struct StructConfig.resolve_only must be false"
            );
        }
        other => panic!("expected InlineStruct for Labeled, got {other:?}"),
    }

    assert_eq!(
        data("Pair"),
        Some(VariantData::DataStructureRef(FieldType::Tuple(vec![
            FieldType::I64,
            FieldType::String
        ])))
    );
    assert_eq!(
        data("Cleared"),
        Some(VariantData::DataStructureRef(FieldType::Tuple(Vec::new())))
    );
    assert_eq!(
        data("Newtype"),
        Some(VariantData::DataStructureRef(FieldType::I64))
    );
    assert_eq!(data("Empty"), None);
}

#[test]
fn tuple_variants_serialize_as_the_derive_models_them() {
    let written = [
        Shape::Cleared(),
        Shape::Pair(1, "x".to_string()),
        Shape::Newtype(2),
    ]
    .map(|shape| serde_json::to_string(&shape).map_err(|error| error.to_string()));
    assert_eq!(
        written,
        [
            Ok(r#"{"Cleared":[]}"#.to_string()),
            Ok(r#"{"Pair":[1,"x"]}"#.to_string()),
            Ok(r#"{"Newtype":2}"#.to_string()),
        ]
    );
}
