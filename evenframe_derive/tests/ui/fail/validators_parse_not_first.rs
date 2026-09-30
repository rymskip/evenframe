use evenframe_derive::Evenframe;

#[derive(Debug, Clone, Evenframe)]
pub struct Reading {
    #[validators(NumberValidator::Positive, StringValidator::IntegerParse)]
    pub value: i64,
}

fn main() {}
