use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManualSmcpTarget {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub url: String,
    #[serde(default = "default_namespace")]
    pub namespace: String,
    pub office_id: String,
    pub computer_name: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConnectionTargetsConfig {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub manual_smcp_targets: Vec<ManualSmcpTarget>,
}

fn default_schema_version() -> u32 {
    1
}

fn default_namespace() -> String {
    "/smcp".to_string()
}

pub fn manual_target_keychain_id(target_id: &str) -> String {
    format!("manual-smcp-target:{target_id}")
}
