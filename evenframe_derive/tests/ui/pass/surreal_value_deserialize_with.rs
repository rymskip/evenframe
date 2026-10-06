use evenframe_derive::SurrealValue;

fn read_name<'de, Deserializer: serde::Deserializer<'de>>(
    deserializer: Deserializer,
) -> Result<String, Deserializer::Error> {
    let name: String = serde::Deserialize::deserialize(deserializer)?;
    Ok(name.trim().to_owned())
}

#[derive(SurrealValue)]
struct Profile {
    #[serde(deserialize_with = "read_name")]
    name: String,
}

fn main() {}
