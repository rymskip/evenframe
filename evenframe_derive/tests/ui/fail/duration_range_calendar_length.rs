use evenframe::Evenframe;
use serde::Serialize;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct Subscription {
    pub id: String,
    #[format(duration_ns(min = "P1M", max = "P12M", step = "P1M"))]
    pub term: Duration,
}

fn main() {}
