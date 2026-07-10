use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const MANUAL_TARGETS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GlobalManualTargetsConfig {
    pub schema_version: u32,
    #[serde(default)]
    pub manual_smcp_targets: Vec<GlobalManualSmcpTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GlobalManualSmcpTarget {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub url: String,
    #[serde(default = "default_namespace")]
    pub namespace: String,
    pub office_id: String,
    #[serde(default)]
    pub routing_headers: HashMap<String, String>,
}

impl From<&ManualSmcpTarget> for GlobalManualSmcpTarget {
    fn from(target: &ManualSmcpTarget) -> Self {
        Self {
            id: target.id.clone(),
            name: target.name.clone(),
            url: target.url.clone(),
            namespace: target.namespace.clone(),
            office_id: target.office_id.clone(),
            routing_headers: target.headers.clone(),
        }
    }
}

impl Default for GlobalManualTargetsConfig {
    fn default() -> Self {
        Self {
            schema_version: MANUAL_TARGETS_SCHEMA_VERSION,
            manual_smcp_targets: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManualSmcpTarget {
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub url: String,
    #[serde(default = "default_namespace")]
    pub namespace: String,
    pub office_id: String,
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
    MANUAL_TARGETS_SCHEMA_VERSION
}

fn default_namespace() -> String {
    "/smcp".to_string()
}

pub fn manual_target_keychain_id(target_id: &str) -> String {
    format!("manual-smcp-target:{target_id}")
}
