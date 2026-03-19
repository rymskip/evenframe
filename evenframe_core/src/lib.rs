// Evenframe - Unified framework for TypeScript generation and database schema synchronization

// Common modules (always compiled)
pub mod config;
pub mod default;
pub mod dependency;
pub mod derive;
pub mod error;
pub mod log;
pub mod registry;
pub mod tooling;
pub mod traits;
pub mod types;
pub mod validator;
pub mod wrappers;

pub mod typesync;

// schemasync module is always declared (data types live here),
// but heavy sub-modules inside are feature-gated
pub mod schemasync;

// Re-export commonly used items for convenience
pub use error::{EvenframeError, Result};

// Schemasync re-exports that require surrealdb
#[cfg(feature = "surrealdb")]
pub use schemasync::{mockmake, mockmake::coordinate, mockmake::format};
