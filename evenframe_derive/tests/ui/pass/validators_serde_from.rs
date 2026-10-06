use evenframe_derive::Evenframe;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Evenframe)]
#[serde(from = "String")]
pub struct Email {
    #[validators(StringValidator::Email)]
    pub address: String,
}

impl From<String> for Email {
    fn from(address: String) -> Self {
        Self { address }
    }
}

fn main() {}
