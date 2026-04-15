pub use evenframe_core::error::{EvenframeError, Result};
pub use evenframe_core::{config, error, registry, traits, types, validator, wrappers};

#[cfg(feature = "schemasync")]
pub use evenframe_core::schemasync;

pub use evenframe_derive::{Evenframe, EvenframeUnion, Schemasync, Typesync};
pub use linkme;

pub mod prelude {
    pub use convert_case::{Case, Casing};
    pub use linkme;
    pub use regex;
    // Re-exported so the validator-generator in `evenframe_core` can emit
    // `::evenframe::prelude::url::Url::parse(...)` /
    // `::evenframe::prelude::uuid::Uuid::parse_str(...)` without forcing
    // consumer crates to list `url`/`uuid` as direct deps.
    pub use url;
    pub use uuid;
}
