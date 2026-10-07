//! SurrealQL statement generation and execution.

pub mod access;
pub mod assert;
pub mod define;
#[cfg(feature = "schemasync")]
pub mod execute;
#[cfg(feature = "mockmake")]
pub mod mock_records;
#[cfg(feature = "schemasync")]
pub mod optional;
#[cfg(feature = "schemasync")]
pub mod remove;
pub mod shape;
mod type_mapper;

pub use type_mapper::SurrealdbTypeMapper;
