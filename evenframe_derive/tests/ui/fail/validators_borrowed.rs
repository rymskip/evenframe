use evenframe_derive::Evenframe;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct Note<'text> {
    #[validators(StringValidator::NonEmpty)]
    pub body: &'text str,
}

fn main() {}
