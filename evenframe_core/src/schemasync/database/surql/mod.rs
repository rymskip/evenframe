//! SurrealQL statement generation and execution.

pub mod access;
pub mod assert;
pub mod define;
pub mod execute;
pub mod mock_records;
pub mod remove;
mod type_mapper;

pub use type_mapper::SurrealdbTypeMapper;
