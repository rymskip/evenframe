use evenframe::Evenframe;
use serde::Serialize;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Evenframe)]
pub struct Appointment {
    pub id: String,
    #[format(duration_ns(min = "PT1H", max = "PT2H", step = "PT25M"))]
    pub length: Duration,
}

fn main() {}
