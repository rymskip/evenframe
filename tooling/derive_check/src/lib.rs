//! Every kind of type the derive handles, in a crate that depends only on
//! `evenframe` with no default features. Verify builds and tests it without
//! features, with `metadata` and with `surrealdb-types`.

use evenframe::{Evenframe, EvenframeUnion};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Evenframe)]
#[mock_data(n = 5)]
pub struct Author {
    pub id: String,
    #[validators(StringValidator::Email)]
    pub email: String,
    pub address: Address,
    pub status: Status,
}

#[derive(Debug, Clone, Serialize, Deserialize, Evenframe)]
pub struct Editor {
    pub id: String,
    pub name: Name,
}

#[derive(Debug, Clone, PartialEq, Serialize, Evenframe)]
#[validators(StringValidator::NonEmpty)]
pub struct Name(String);

/// A newtype over a type parameter. Core's blanket impl already covers its
/// `TryFrom<T>`, and a registry entry names one concrete type, so only a build
/// without `metadata` compiles it.
#[cfg(not(feature = "metadata"))]
#[derive(Debug, Clone, PartialEq, Serialize, Evenframe)]
#[validators(StringValidator::NonEmpty)]
pub struct Tagged<T: evenframe::validator::runtime::StringValue>(T);

#[derive(Debug, Clone, Serialize, Deserialize, Evenframe)]
pub struct Address {
    pub street: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Evenframe)]
pub enum Status {
    Draft,
    Published { at: String },
}

#[derive(Debug, Clone, Serialize, EvenframeUnion)]
pub enum Contributor {
    Author(Author),
    Editor(Editor),
}

#[cfg(test)]
mod tests {
    use super::{Author, Contributor, Editor, Name};
    use evenframe::traits::EvenframeTable;
    use serde_json::json;

    fn table<T: EvenframeTable>() {}

    #[test]
    fn tables_and_unions_of_tables_are_tables() {
        table::<Author>();
        table::<Editor>();
        table::<Contributor>();
    }

    #[test]
    fn validators_run_on_deserialize() {
        let author = |email: &str| {
            json!({
                "id": "author:1",
                "email": email,
                "address": { "street": "Main" },
                "status": "Draft",
            })
        };
        assert!(serde_json::from_value::<Author>(author("ada@example.com")).is_ok());
        assert!(serde_json::from_value::<Author>(author("not an email")).is_err());
    }

    #[test]
    fn a_newtype_checks_its_value_wherever_it_is_read() {
        let editor = |name: &str| json!({ "id": "editor:1", "name": name });
        let read = serde_json::from_value::<Editor>(editor("Ada")).expect("reads");
        assert_eq!(read.name, Name("Ada".to_owned()));
        let error = serde_json::from_value::<Editor>(editor("")).expect_err("empty name");
        assert!(error.to_string().contains("non-empty"), "{error}");
    }

    #[test]
    fn a_newtype_is_built_from_its_value_through_its_validators() {
        assert_eq!(
            Name::try_from("Ada").map(|name| name.as_str().to_owned()),
            Ok("Ada".to_owned())
        );
        assert!(Name::try_from("").is_err());
    }

    #[cfg(not(feature = "metadata"))]
    #[test]
    fn a_generic_newtype_reads_through_its_validators() {
        use super::Tagged;

        assert_eq!(
            serde_json::from_value::<Tagged<String>>(json!("a")).expect("reads"),
            Tagged("a".to_owned())
        );
        assert!(serde_json::from_value::<Tagged<String>>(json!("")).is_err());
    }

    #[cfg(feature = "metadata")]
    #[test]
    fn metadata_finds_every_derived_type_by_name() {
        use evenframe::registry::{
            get_newtype_config, get_struct_config, get_table_config, get_tagged_union,
            get_union_of_tables,
        };
        use evenframe::traits::EvenframePersistableStruct;

        let author = get_table_config("Author").expect("Author is a registered table");
        assert_eq!(author.table_name, "author");
        assert_eq!(Author::static_table_config().table_name, "author");
        assert!(get_table_config("Editor").is_some());
        assert!(get_struct_config("Address").is_some());
        assert!(get_tagged_union("Status").is_some());
        assert!(get_newtype_config("Name").is_some());
        assert_eq!(
            get_union_of_tables("Contributor"),
            Some(["Author", "Editor"].as_slice())
        );
    }

    #[cfg(feature = "surrealdb-types")]
    #[test]
    fn surreal_value_reads_the_database_shape_and_validates() {
        use super::{Address, Status};
        use evenframe::surreal_value::__private::{SurrealValue, Value};

        let author = Author {
            id: "author:ada".to_owned(),
            email: "ada@example.com".to_owned(),
            address: Address {
                street: "Main".to_owned(),
            },
            status: Status::Draft,
        };
        let read = Author::from_value(author.clone().into_value()).expect("reads");
        assert_eq!(read.email, author.email);
        assert_eq!(read.address.street, "Main");
        assert!(matches!(read.status, Status::Draft));
        let error = Author::from_value(Value::Object(
            [
                ("id".to_owned(), Value::String("author:ada".to_owned())),
                ("email".to_owned(), Value::String("not an email".to_owned())),
                (
                    "address".to_owned(),
                    Value::Object(
                        [("street".to_owned(), Value::String("Main".to_owned()))]
                            .into_iter()
                            .collect(),
                    ),
                ),
                ("status".to_owned(), Value::String("Draft".to_owned())),
            ]
            .into_iter()
            .collect(),
        ))
        .expect_err("email fails validation");
        assert!(error.to_string().contains("email"), "{error}");
    }

    #[cfg(feature = "surrealdb-types")]
    #[test]
    fn a_newtype_is_stored_as_its_value() {
        use evenframe::surreal_value::__private::{SurrealValue, Value};

        assert_eq!(
            Name("Ada".to_owned()).into_value(),
            Value::String("Ada".to_owned())
        );
        assert!(Name::from_value(Value::String(String::new())).is_err());
    }
}
