use evenframe_derive::Evenframe;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct Code {
    #[typesync(validators(StringValidator::RegexLiteral(Format::Custom("^[0-9]{4}$"))))]
    pub pin: String,
}

fn main() {}
