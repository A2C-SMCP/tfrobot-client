use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCallHistoryRecord {
    pub timestamp: String,
    pub req_id: String,
    pub computer_instance_id: String,
    pub server: String,
    pub tool: String,
    pub parameters: serde_json::Value,
    pub timeout: Option<f64>,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ToolCallHistoryDraft {
    pub req_id: String,
    pub computer_instance_id: String,
    pub server: String,
    pub tool: String,
    pub parameters: serde_json::Value,
    pub timeout: Option<f64>,
    pub success: bool,
    pub error: Option<String>,
}
