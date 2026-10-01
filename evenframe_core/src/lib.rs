// Evenframe - Unified framework for TypeScript generation and database schema synchronization

// Common modules (always compiled)
pub mod config;
#[cfg(feature = "schemadump")]
pub mod default;
pub mod dependency;
pub mod derive;
pub mod error;
pub mod log;
/// Every derived type's metadata, found by name. Without the `metadata`
/// feature the module is empty, so `evenframe` can always re-export it.
#[cfg(feature = "metadata")]
pub mod registry;
#[cfg(not(feature = "metadata"))]
pub mod registry {}
#[cfg(any(feature = "build-typesync", feature = "build-schemadump"))]
pub mod build;
#[cfg(feature = "scan")]
pub mod scan;
pub mod traits;
pub mod types;
pub mod validator;

pub mod typesync;

// schemasync module is always declared (data types live here),
// but heavy sub-modules inside are feature-gated
pub mod schemasync;

// Re-export commonly used items for convenience
pub use error::{EvenframeError, Result};

// Validator bounds are `OrderedFloat`s in derive-generated code.
pub use ordered_float;

/// Expands the derive's metadata items when the `metadata` feature is on and
/// drops them otherwise, so this crate's feature alone decides whether a
/// deriving crate gets them.
#[cfg(feature = "metadata")]
#[doc(hidden)]
#[macro_export]
macro_rules! __metadata {
    ($($item:item)*) => { $($item)* };
}

#[cfg(not(feature = "metadata"))]
#[doc(hidden)]
#[macro_export]
macro_rules! __metadata {
    ($($item:item)*) => {};
}

// Schemasync re-exports that require surrealdb
#[cfg(feature = "surrealdb")]
pub use schemasync::{mockmake, mockmake::coordinate, mockmake::format};
