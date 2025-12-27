pub mod auth;
pub mod blog;
pub mod ecommerce;
pub mod edge_cases;
pub mod extreme_edge_cases;

pub use auth::*;
pub use ecommerce::*;
// Note: blog types are accessed via models::blog::* to avoid name conflicts
// Note: edge_cases and extreme_edge_cases are test modules, accessed via models::edge_cases::* etc.
