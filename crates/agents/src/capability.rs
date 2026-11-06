//! Local capability metadata. Remote MCP annotations never grant authority.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    #[default]
    ReadOnly,
    WorkspaceWrite,
    ExternalWrite,
    Destructive,
}

#[derive(Debug, Clone, Serialize)]
pub struct CapabilityCard {
    pub capability_id: String,
    pub tool_id: String,
    pub description: String,
    pub provider: String,
    pub effect: Effect,
    pub input_schema: Value,
    pub output_schema: Option<Value>,
    pub input_types: Vec<String>,
    pub output_types: Vec<String>,
}
