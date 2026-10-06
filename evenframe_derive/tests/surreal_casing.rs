//! Under `#[surreal(rename_all)]`, evenframe names stored keys as the SDK's
//! `SurrealValue` derive does, except where heck 0.5 and the SDK's heck 0.4
//! split words differently. This pins that difference to the exact names.

use surrealdb::types::{SurrealValue, Value};

macro_rules! fields {
    ($derive:path, $casing:literal) => {
        #[derive($derive)]
        #[surreal(rename_all = $casing)]
        pub struct Fields {
            pub display_name: u8,
            pub version2_id: u8,
            pub a1b2c3: u8,
            pub x: u8,
            pub r#type: u8,
            pub trailing_: u8,
            pub _leading: u8,
            pub größe: u8,
            pub café_au_lait: u8,
            pub naïve_bayes: u8,
        }

        impl Fields {
            /// Each field holds its declaration index, which pairs its key
            /// with its Rust name.
            pub fn numbered() -> Self {
                Self {
                    display_name: 0,
                    version2_id: 1,
                    a1b2c3: 2,
                    x: 3,
                    r#type: 4,
                    trailing_: 5,
                    _leading: 6,
                    größe: 7,
                    café_au_lait: 8,
                    naïve_bayes: 9,
                }
            }
        }

        #[derive($derive)]
        #[surreal(rename_all = $casing)]
        pub enum Variants {
            Free,
            XmlHttpRequest,
            HTTPServer,
            Version2,
            V2Beta,
            Größe,
            CaféAuLait,
            ÜberCool,
        }

        impl Variants {
            pub const ALL: [Self; 8] = [
                Self::Free,
                Self::XmlHttpRequest,
                Self::HTTPServer,
                Self::Version2,
                Self::V2Beta,
                Self::Größe,
                Self::CaféAuLait,
                Self::ÜberCool,
            ];
        }
    };
}

macro_rules! cased {
    ($($module:ident => $casing:literal),* $(,)?) => {
        $(
            mod $module {
                pub mod sdk {
                    // The SDK's derive names the trait unqualified.
                    use surrealdb::types::SurrealValue;
                    fields!(surrealdb::types::SurrealValue, $casing);
                }
                pub mod evenframe {
                    fields!(::evenframe::SurrealValue, $casing);
                }
            }
        )*

        /// Each casing's stored names, the SDK's first.
        fn names() -> Vec<(&'static str, Vec<String>, Vec<String>)> {
            vec![$(
                (
                    $casing,
                    stored_names(
                        $module::sdk::Fields::numbered().into_value(),
                        $module::sdk::Variants::ALL.map(SurrealValue::into_value),
                    ),
                    stored_names(
                        $module::evenframe::Fields::numbered().into_value(),
                        $module::evenframe::Variants::ALL.map(SurrealValue::into_value),
                    ),
                ),
            )*]
        }
    };
}

cased! {
    lowercase => "lowercase",
    uppercase => "UPPERCASE",
    pascal => "PascalCase",
    camel => "camelCase",
    snake => "snake_case",
    screaming_snake => "SCREAMING_SNAKE_CASE",
    kebab => "kebab-case",
    screaming_kebab => "SCREAMING-KEBAB-CASE",
}

const RUST_NAMES: [&str; 18] = [
    "display_name",
    "version2_id",
    "a1b2c3",
    "x",
    "type",
    "trailing_",
    "_leading",
    "größe",
    "café_au_lait",
    "naïve_bayes",
    "Free",
    "XmlHttpRequest",
    "HTTPServer",
    "Version2",
    "V2Beta",
    "Größe",
    "CaféAuLait",
    "ÜberCool",
];

/// The struct's keys in declaration order, then each variant's name.
fn stored_names(fields: Value, variants: [Value; 8]) -> Vec<String> {
    let Value::Object(object) = fields else {
        panic!("a struct is stored as an object, got {fields:?}");
    };
    let mut keys: Vec<(i64, String)> = object
        .iter()
        .map(|(key, value)| match value {
            Value::Number(number) => (
                number.to_int().expect("an index is an integer"),
                key.clone(),
            ),
            other => panic!("`{key}` holds its index, got {other:?}"),
        })
        .collect();
    keys.sort();
    assert_eq!(keys.len(), 10, "every field keeps its own key: {keys:?}");
    keys.into_iter()
        .map(|(_, key)| key)
        .chain(variants.into_iter().map(|variant| {
            match variant {
                // evenframe stores serde's shape, the bare name; the SDK's derive
                // stores an object keyed by the name.
                Value::String(name) => name,
                Value::Object(object) if object.len() == 1 => object
                    .keys()
                    .next()
                    .cloned()
                    .expect("the object holds one key"),
                other => panic!("a unit variant is stored by its name, got {other:?}"),
            }
        }))
        .collect()
}

#[test]
fn stored_names_differ_from_the_sdk_only_where_heck_splits_unicode_words() {
    let mut divergent = Vec::new();
    let mut snake = Vec::new();
    for (casing, sdk, ours) in names() {
        for (rust, (sdk, ours)) in RUST_NAMES.into_iter().zip(sdk.into_iter().zip(ours)) {
            if sdk != ours {
                divergent.push((casing, rust));
                if casing == "snake_case" {
                    snake.push((rust, sdk, ours));
                }
            }
        }
    }

    // The SDK's heck 0.4 splits words at every non-ASCII character and drops
    // it; heck 0.5 keeps letters of any script. Plain lowercase and UPPERCASE
    // do not go through heck.
    let expected: Vec<(&str, &str)> = [
        "PascalCase",
        "camelCase",
        "snake_case",
        "SCREAMING_SNAKE_CASE",
        "kebab-case",
        "SCREAMING-KEBAB-CASE",
    ]
    .into_iter()
    .flat_map(|casing| {
        RUST_NAMES
            .into_iter()
            .filter(|name| !name.is_ascii())
            .map(move |name| (casing, name))
    })
    .collect();
    assert_eq!(divergent, expected);
    assert_eq!(
        snake,
        [
            ("größe", "gr_e", "größe"),
            ("café_au_lait", "caf_au_lait", "café_au_lait"),
            ("naïve_bayes", "na_ve_bayes", "naïve_bayes"),
            ("Größe", "gr_e", "größe"),
            ("CaféAuLait", "caf_au_lait", "café_au_lait"),
            ("ÜberCool", "ber_cool", "über_cool"),
        ]
        .map(|(rust, sdk, ours)| (rust, sdk.to_owned(), ours.to_owned()))
    );
}
