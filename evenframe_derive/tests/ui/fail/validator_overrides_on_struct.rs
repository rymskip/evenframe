use evenframe_derive::Evenframe;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Evenframe)]
#[schemasync(validators(StringValidator::NonEmpty))]
pub struct Code {
    pub pin: String,
}

fn main() {}
