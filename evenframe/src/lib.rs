pub use evenframe_core::{
    __metadata, config,
    error::{self, EvenframeError, Result},
    registry, schemasync, traits, types, typesync, validator,
};

#[cfg(any(feature = "build-typesync", feature = "build-schemadump"))]
pub use evenframe_core::{build, scan};

pub use evenframe_derive::{Evenframe, EvenframeUnion, Schemasync, Typesync};
pub use linkme;

pub mod prelude {
    pub use convert_case::{Case, Casing};
    pub use evenframe_core::ordered_float;
    pub use linkme;
    pub use regex;
    pub use url;
    pub use uuid;
}
