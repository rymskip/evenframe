use evenframe::Evenframe;
use evenframe::types::FromText;

#[derive(Debug, Clone, Evenframe)]
pub struct Reading {
    pub label: FromText<String>,
}

fn main() {}
