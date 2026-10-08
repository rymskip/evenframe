use evenframe::Evenframe;

#[derive(Evenframe)]
pub struct Account {
    pub id: String,
    pub home: Address,
}
