//! `#[typesync(...)]` reaches the registered metadata at every position it
//! applies to, as the scan reads it.

use evenframe::Evenframe;
use evenframe::registry::all_configs;
use evenframe::types::VariantData;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Evenframe)]
#[typesync(macroforge(derives = [Default, Encode]), annotation("@form"))]
pub struct Profile {
    pub id: String,
    #[typesync(macroforge(attributes = [endec(rename = "full_name"), hidden]))]
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Evenframe)]
#[typesync(macroforge(attributes = [endec(tag = "kind")]))]
pub enum Shape {
    #[typesync(macroforge(derives = [Decode]), annotation("@circle"))]
    Circle { radius: f64 },
    #[typesync(annotation("@dot"))]
    Dot,
}

#[derive(Debug, Clone, Serialize, Evenframe)]
#[typesync(macroforge(derives = [Encode]))]
pub struct Slug(String);

#[test]
fn typesync_attributes_reach_the_registered_metadata() {
    let configs = all_configs();
    let profile = &configs.objects["Profile"];
    assert_eq!(profile.macroforge_derives, ["Default", "Encode"]);
    assert_eq!(profile.annotations, ["@form"]);
    let name = profile
        .fields
        .iter()
        .find(|field| field.field_name == "name")
        .expect("the name field");
    assert_eq!(
        name.annotations,
        [r#"@endec({ rename: "full_name" })"#, "@hidden"]
    );

    let shape = &configs.enums["Shape"];
    assert_eq!(shape.annotations, [r#"@endec({ tag: "kind" })"#]);
    assert_eq!(shape.variants[0].annotations, ["@circle"]);
    match &shape.variants[0].data {
        Some(VariantData::InlineStruct(circle)) => {
            assert_eq!(circle.macroforge_derives, ["Decode"]);
        }
        other => panic!("expected the circle's fields, got {other:?}"),
    }
    assert_eq!(shape.variants[1].annotations, ["@dot"]);
    assert_eq!(configs.newtypes["Slug"].macroforge_derives, ["Encode"]);
}
