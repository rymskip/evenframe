use evenframe::Evenframe;

#[derive(Evenframe)]
pub struct Deal {
    pub id: String,
    #[validators(StringValidator::MinLength(2), StringValidator::MaxLength(5))]
    pub code: String,
}
