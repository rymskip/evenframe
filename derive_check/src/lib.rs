//! Every kind of type the derive handles, in a crate that depends only on
//! `evenframe` with no default features. Verify builds and tests it both
//! without features and with `metadata`.

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
}
