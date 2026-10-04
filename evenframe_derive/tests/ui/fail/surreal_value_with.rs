use evenframe_derive::SurrealValue;

#[derive(SurrealValue)]
struct Profile {
    #[serde(with = "name_format")]
    name: String,
}

mod name_format {
    pub fn serialize<Serializer: serde::Serializer>(
        value: &String,
        serializer: Serializer,
    ) -> Result<Serializer::Ok, Serializer::Error> {
        serializer.serialize_str(value)
    }

    pub fn deserialize<'de, Deserializer: serde::Deserializer<'de>>(
        deserializer: Deserializer,
    ) -> Result<String, Deserializer::Error> {
        serde::Deserialize::deserialize(deserializer)
    }
}

fn main() {}
