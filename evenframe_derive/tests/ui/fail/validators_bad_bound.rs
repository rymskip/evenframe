use evenframe_derive::Evenframe;

#[derive(Debug, Clone, Evenframe)]
pub struct Booking {
    #[validators(DurationValidator::LessThanDuration("soon"))]
    pub stay: String,
}

fn main() {}
