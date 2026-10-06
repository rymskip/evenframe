use evenframe_derive::Evenframe;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct Code {
    #[validators(StringValidator::RegexLiteral(Format::Custom(r"^\d{4}$")))]
    pub pin: String,
}

fn main() {}
