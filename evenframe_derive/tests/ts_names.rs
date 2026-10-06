use evenframe::Evenframe;
use serde::Serialize;

#[derive(Debug, PartialEq, Serialize, Evenframe)]
#[evenframe(all_ts_names = "PascalCase")]
struct Profile {
    #[validators(StringValidator::MinLength(1))]
    first_name: String,
    #[evenframe(ts_name = "kebab-case")]
    #[serde(rename = "wire_postal_code")]
    postal_code: String,
}

#[test]
fn ts_casing_does_not_change_serde_round_trips() {
    let profile = Profile {
        first_name: "Alex".to_owned(),
        postal_code: "12345".to_owned(),
    };
    let serialized = serde_json::to_string(&profile).expect("serialize profile");
    assert_eq!(
        serialized,
        r#"{"first_name":"Alex","wire_postal_code":"12345"}"#
    );
    assert_eq!(
        serde_json::from_str::<Profile>(&serialized).expect("deserialize profile"),
        profile
    );
    assert!(
        serde_json::from_str::<Profile>(r#"{"FirstName":"Alex","postal-code":"12345"}"#).is_err()
    );
    assert!(
        serde_json::from_str::<Profile>(r#"{"first_name":"","wire_postal_code":"12345"}"#).is_err()
    );
}

#[test]
fn metadata_keeps_distinct_ts_and_wire_names() {
    let config = Profile::static_struct_config();
    assert_eq!(config.fields[0].ts_name(), "FirstName");
    assert_eq!(config.fields[1].ts_name(), "postal-code");
    assert_eq!(config.fields[1].serde_name(), "wire_postal_code");
    assert_eq!(config.fields[1].db_name(), "postal_code");
}
