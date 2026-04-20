use crate::schemasync::mockmake::MockGenerationConfig;
use crate::schemasync::{edge::EdgeConfig, event::EventConfig, permissions::PermissionsConfig};
use crate::types::StructConfig;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TableConfig {
    pub table_name: String,
    pub struct_config: StructConfig,
    pub relation: Option<EdgeConfig>,
    pub permissions: Option<PermissionsConfig>,
    pub mock_generation_config: Option<MockGenerationConfig>,
    #[serde(default)]
    pub events: Vec<EventConfig>,
    #[serde(default)]
    pub indexes: Vec<IndexConfig>,
}

/// A struct-level composite (or single-column) index declared via
/// `#[index(fields(a, b), unique)]` on a `#[derive(Evenframe)]` struct.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IndexConfig {
    pub fields: Vec<String>,
    pub unique: bool,
}
