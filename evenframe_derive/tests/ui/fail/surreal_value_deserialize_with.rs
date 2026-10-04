use evenframe_derive::SurrealValue;

fn read_name<'de, Deserializer: serde::Deserializer<'de>>(
    _deserializer: Deserializer,
) -> Result<String, Deserializer::Error> {
    unimplemented!()
}

#[derive(SurrealValue)]
struct Profile {
    #[serde(deserialize_with = "read_name")]
    name: String,
}

fn main() {}
