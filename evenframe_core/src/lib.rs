// Evenframe - Unified framework for TypeScript generation and database schema synchronization

// Common modules
pub mod config;
pub mod default;
pub mod dependency;
pub mod derive;
pub mod error;
pub mod log;
pub mod registry;
pub mod traits;
pub mod types;
pub mod validator;
pub mod wrappers;
// TypeSync - TypeScript type generation
pub mod typesync;

// SchemaSync - Database schema synchronization
pub mod schemasync;

// Re-export commonly used items for convenience
pub use error::{EvenframeError, Result};
pub use schemasync::{
    FilterDefinition, FilterOperator, FilterPrimitive, FilterValue, compare, generate_where_clause,
    mockmake, mockmake::coordinate, mockmake::format,
};
