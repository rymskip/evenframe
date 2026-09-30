use evenframe::{Evenframe, Schemasync};

#[derive(Evenframe)]
pub struct Deal {
    pub id: String,
    pub address: Address,
    pub cache: Vec<CacheRow>,
    pub party: Option<Party>,
    pub status: Status,
}

#[derive(Schemasync)]
pub struct CacheRow {
    pub id: String,
}

pub struct Party {
    pub name: String,
}

#[derive(Evenframe)]
pub enum Status {
    Open,
    Blocked(Reason),
}

pub struct Reason {
    pub note: String,
}
