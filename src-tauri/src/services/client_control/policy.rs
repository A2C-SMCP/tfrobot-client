use super::{ToolCatalog, ToolId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolScope {
    #[default]
    All,
    Custom {
        #[serde(default)]
        tools: BTreeSet<String>,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum TargetScope {
    #[default]
    SelfOnly,
    All,
    Custom {
        #[serde(default)]
        targets: BTreeSet<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct RemoteControlPolicy {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub tool_scope: ToolScope,
    #[serde(default)]
    pub target_scope: TargetScope,
}

impl RemoteControlPolicy {
    #[must_use]
    pub fn allows_tool(&self, tool: ToolId) -> bool {
        self.enabled
            && match &self.tool_scope {
                ToolScope::All => true,
                ToolScope::Custom { tools } => tools.contains(tool.as_str()),
            }
    }

    #[must_use]
    pub fn allows_target(&self, source_id: &str, target_id: &str) -> bool {
        self.enabled
            && match &self.target_scope {
                TargetScope::SelfOnly => source_id == target_id,
                TargetScope::All => true,
                TargetScope::Custom { targets } => targets.contains(target_id),
            }
    }

    pub fn validate(&self) -> Result<(), String> {
        if let ToolScope::Custom { tools } = &self.tool_scope {
            if let Some(tool) = tools.iter().find(|tool| !ToolCatalog.contains(tool)) {
                return Err(format!(
                    "unknown Client Control tool in custom scope: {tool}"
                ));
            }
        }
        if let TargetScope::Custom { targets } = &self.target_scope {
            if let Some(target) = targets
                .iter()
                .find(|target| target.trim().is_empty() || target.as_str() != target.trim())
            {
                return Err(format!(
                    "invalid Computer target in custom scope: {target:?}"
                ));
            }
        }
        Ok(())
    }

    pub fn sanitize(&mut self, known_targets: &HashSet<String>) -> bool {
        let mut changed = self.sanitize_tools();
        if let TargetScope::Custom { targets } = &mut self.target_scope {
            let before = targets.len();
            targets.retain(|target| known_targets.contains(target));
            changed |= before != targets.len();
        }
        changed
    }

    pub fn sanitize_tools(&mut self) -> bool {
        let mut changed = false;
        if let ToolScope::Custom { tools } = &mut self.tool_scope {
            let before = tools.len();
            tools.retain(|tool| ToolCatalog.contains(tool));
            changed |= before != tools.len();
        }
        changed
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvocationContext {
    pub request_id: String,
    pub source_computer_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_disabled_but_first_enable_is_all_tools_self_target() {
        let mut policy = RemoteControlPolicy::default();
        assert!(!policy.allows_tool(ToolId::ComputerList));
        policy.enabled = true;
        assert!(policy.allows_tool(ToolId::ComputerList));
        assert!(policy.allows_target("source", "source"));
        assert!(!policy.allows_target("source", "other"));
    }

    #[test]
    fn sanitization_removes_unknown_tools_and_deleted_targets() {
        let mut policy = RemoteControlPolicy {
            enabled: true,
            tool_scope: ToolScope::Custom {
                tools: ["computer_list", "mcp_tool_execute"]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
            },
            target_scope: TargetScope::Custom {
                targets: ["live", "deleted"]
                    .into_iter()
                    .map(str::to_string)
                    .collect(),
            },
        };
        assert!(policy.sanitize(&HashSet::from(["live".to_string()])));
        assert_eq!(
            policy.tool_scope,
            ToolScope::Custom {
                tools: BTreeSet::from(["computer_list".to_string()])
            }
        );
        assert_eq!(
            policy.target_scope,
            TargetScope::Custom {
                targets: BTreeSet::from(["live".to_string()])
            }
        );
    }
}
