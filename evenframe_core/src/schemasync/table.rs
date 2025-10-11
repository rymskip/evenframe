use crate::mockmake::MockGenerationConfig;
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
}
