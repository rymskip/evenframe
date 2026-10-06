use evenframe::Evenframe;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Evenframe)]
pub enum Label {
    Text(#[validators(StringValidator::NonEmpty)] String),
}

fn main() {}
