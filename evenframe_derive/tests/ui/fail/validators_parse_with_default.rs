use evenframe_derive::Evenframe;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct Reading {
    #[validators(StringValidator::IntegerParse)]
    #[serde(default)]
    pub value: i64,
}

fn main() {}
