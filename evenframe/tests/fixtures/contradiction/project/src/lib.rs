use evenframe::Evenframe;

#[derive(Evenframe)]
pub struct Deal {
    pub id: String,
    #[validators(StringValidator::MinLength(5), StringValidator::MaxLength(2))]
    pub code: String,
}
