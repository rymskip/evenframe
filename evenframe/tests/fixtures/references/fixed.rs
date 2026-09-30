use evenframe::{Evenframe, Typesync};

#[derive(Evenframe)]
pub struct Deal {
    pub id: String,
    pub address: Address,
    pub cache: Vec<CacheRow>,
    pub party: Option<Party>,
    pub status: Status,
}

#[derive(Typesync)]
pub struct CacheRow {
    pub id: String,
}

#[derive(Evenframe)]
pub struct Party {
    pub name: String,
}

#[derive(Evenframe)]
pub enum Status {
    Open,
    Blocked(Reason),
}

#[derive(Evenframe)]
pub struct Reason {
    pub note: String,
}
