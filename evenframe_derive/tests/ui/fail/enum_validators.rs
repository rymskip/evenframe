use evenframe_derive::Evenframe;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Evenframe)]
#[validators(StringValidator::NonEmpty)]
pub enum Status {
    Active,
    Archived,
}

fn main() {}
