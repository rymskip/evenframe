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

#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct Editor {
    pub id: String,
    pub name: String,
}

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
    use super::{Author, Contributor, Editor};
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

    #[cfg(feature = "metadata")]
    #[test]
    fn metadata_finds_every_derived_type_by_name() {
        use evenframe::registry::{
            get_struct_config, get_table_config, get_tagged_union, get_union_of_tables,
        };
        use evenframe::traits::EvenframePersistableStruct;

        let author = get_table_config("Author").expect("Author is a registered table");
        assert_eq!(author.table_name, "author");
        assert_eq!(Author::static_table_config().table_name, "author");
        assert!(get_table_config("Editor").is_some());
        assert!(get_struct_config("Address").is_some());
        assert!(get_tagged_union("Status").is_some());
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
}
