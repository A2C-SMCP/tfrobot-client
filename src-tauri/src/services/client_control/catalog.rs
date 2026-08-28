use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

macro_rules! tool_ids {
    ($(($variant:ident, $value:literal)),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum ToolId {
            $($variant),+
        }

        impl ToolId {
            pub const ALL: [Self; tool_ids!(@count $($variant),+)] = [$(Self::$variant),+];

            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $value),+
                }
            }
        }

        impl FromStr for ToolId {
            type Err = UnknownToolId;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                match value {
                    $($value => Ok(Self::$variant)),+,
                    _ => Err(UnknownToolId(value.to_string())),
                }
            }
        }
    };
    (@count $($item:ident),+) => { <[()]>::len(&[$(tool_ids!(@unit $item)),+]) };
    (@unit $item:ident) => { () };
}

tool_ids!(
    (ComputerList, "computer_list"),
    (ComputerGetStatus, "computer_get_status"),
    (ComputerCreate, "computer_create"),
    (ComputerRename, "computer_rename"),
    (ComputerDuplicate, "computer_duplicate"),
    (ComputerDelete, "computer_delete"),
    (ComputerStart, "computer_start"),
    (ComputerStop, "computer_stop"),
    (ComputerRestart, "computer_restart"),
    (
        ComputerSetConnectionPolicy,
        "computer_set_connection_policy"
    ),
    (ComputerSetSkillHome, "computer_set_skill_home"),
    (ConnectionTargetList, "connection_target_list"),
    (ConnectionTargetUpsert, "connection_target_upsert"),
    (ConnectionTargetDelete, "connection_target_delete"),
    (RobotListAvailable, "robot_list_available"),
    (ComputerConnectTarget, "computer_connect_target"),
    (ComputerConnectRobot, "computer_connect_robot"),
    (ComputerDisconnect, "computer_disconnect"),
    (
        ComputerGetConnectionStatus,
        "computer_get_connection_status"
    ),
    (McpServerList, "mcp_server_list"),
    (McpConfigGetState, "mcp_config_get_state"),
    (McpServerUpsert, "mcp_server_upsert"),
    (McpServerRemove, "mcp_server_remove"),
    (McpServerStart, "mcp_server_start"),
    (McpServerStop, "mcp_server_stop"),
    (McpServerStartAll, "mcp_server_start_all"),
    (McpServerStopAll, "mcp_server_stop_all"),
    (InputDefinitionList, "input_definition_list"),
    (InputDefinitionGet, "input_definition_get"),
    (InputDefinitionUpsert, "input_definition_upsert"),
    (InputDefinitionRemove, "input_definition_remove"),
    (InputValueList, "input_value_list"),
    (InputValueGetStatus, "input_value_get_status"),
    (InputValueSet, "input_value_set"),
    (RuntimeInputValueSet, "runtime_input_value_set"),
    (InputValueRemove, "input_value_remove"),
    (InputValueClearAll, "input_value_clear_all"),
    (SkillCreate, "skill_create"),
    (SkillUpdate, "skill_update"),
    (SkillDelete, "skill_delete"),
    (MarketplaceGetCapabilities, "marketplace_get_capabilities"),
    (MarketplaceGetGovernance, "marketplace_get_governance"),
    (MarketplaceAdd, "marketplace_add"),
    (MarketplaceUpdate, "marketplace_update"),
    (MarketplaceRefresh, "marketplace_refresh"),
    (MarketplaceRemove, "marketplace_remove"),
    (PluginInstall, "plugin_install"),
    (PluginEnable, "plugin_enable"),
    (PluginDisable, "plugin_disable"),
    (PluginUninstall, "plugin_uninstall"),
    (McpToolList, "mcp_tool_list"),
    (McpToolHistory, "mcp_tool_history"),
    (McpResourceList, "mcp_resource_list"),
    (DesktopList, "desktop_list"),
    (DesktopRead, "desktop_read"),
);

impl fmt::Display for ToolId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown Client Control tool id: {0}")]
pub struct UnknownToolId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolGroup {
    ComputerFleet,
    ComputerConnection,
    McpServer,
    McpInputs,
    Skills,
    MarketplacePlugins,
    DebugResources,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRisk {
    ReadOnly,
    Mutating,
    SensitiveWrite,
    Destructive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetContract {
    Discovery,
    Optional,
    Required,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolDefinition {
    pub id: ToolId,
    pub group: ToolGroup,
    pub risk: ToolRisk,
    pub target: TargetContract,
}

impl ToolDefinition {
    #[must_use]
    pub fn input_schema(self) -> serde_json::Value {
        let (required, properties): (Vec<&str>, Vec<&str>) = match self.id {
            ToolId::ComputerList | ToolId::ConnectionTargetList | ToolId::RobotListAvailable => {
                (vec![], vec![])
            }
            ToolId::ComputerCreate => (vec!["name"], vec!["name", "description"]),
            ToolId::ComputerRename => (
                vec!["computer_id", "name"],
                vec!["computer_id", "name", "description"],
            ),
            ToolId::ComputerDuplicate => (
                vec!["computer_id", "name"],
                vec![
                    "computer_id",
                    "name",
                    "description",
                    "copy_robot_binding",
                    "connection_target_id",
                    "skill_home_mode",
                ],
            ),
            ToolId::ComputerSetConnectionPolicy => (
                vec!["computer_id"],
                vec!["computer_id", "target", "auto_connect"],
            ),
            ToolId::ComputerSetSkillHome => (
                vec!["computer_id"],
                vec!["computer_id", "local_skills_root"],
            ),
            ToolId::ConnectionTargetUpsert => (vec!["target"], vec!["target", "api_key_action"]),
            ToolId::ConnectionTargetDelete => (vec!["target_id"], vec!["target_id"]),
            ToolId::ComputerConnectTarget => (
                vec!["computer_id", "target_id"],
                vec!["computer_id", "target_id"],
            ),
            ToolId::ComputerConnectRobot => (
                vec!["computer_id", "employee_id"],
                vec!["computer_id", "employee_id", "scope"],
            ),
            ToolId::McpServerUpsert => {
                (vec!["computer_id", "server"], vec!["computer_id", "server"])
            }
            ToolId::McpServerRemove | ToolId::McpServerStart | ToolId::McpServerStop => (
                vec!["computer_id", "bundle_id"],
                vec!["computer_id", "bundle_id"],
            ),
            ToolId::InputDefinitionGet
            | ToolId::InputDefinitionRemove
            | ToolId::InputValueGetStatus
            | ToolId::InputValueRemove => (
                vec!["computer_id", "input_id"],
                vec!["computer_id", "input_id"],
            ),
            ToolId::InputDefinitionUpsert => (
                vec!["computer_id", "definition"],
                vec!["computer_id", "definition"],
            ),
            ToolId::InputValueSet | ToolId::RuntimeInputValueSet => (
                vec!["computer_id", "input_id", "value"],
                vec!["computer_id", "input_id", "value"],
            ),
            ToolId::SkillCreate => (
                vec!["computer_id", "name", "files"],
                vec!["computer_id", "name", "files"],
            ),
            ToolId::SkillUpdate => (
                vec!["computer_id", "name", "changes"],
                vec!["computer_id", "name", "changes", "expected_revision"],
            ),
            ToolId::SkillDelete => (
                vec!["computer_id", "name"],
                vec!["computer_id", "name", "expected_revision"],
            ),
            ToolId::MarketplaceAdd | ToolId::MarketplaceUpdate => (
                vec!["computer_id", "name", "git_url"],
                vec!["computer_id", "name", "git_url"],
            ),
            ToolId::MarketplaceRefresh | ToolId::MarketplaceRemove => (
                vec!["computer_id", "marketplace"],
                vec!["computer_id", "marketplace"],
            ),
            ToolId::PluginInstall
            | ToolId::PluginEnable
            | ToolId::PluginDisable
            | ToolId::PluginUninstall => (
                vec!["computer_id", "marketplace", "plugin"],
                vec!["computer_id", "marketplace", "plugin"],
            ),
            ToolId::McpResourceList => (
                vec!["computer_id", "bundle_id"],
                vec!["computer_id", "bundle_id", "cursor"],
            ),
            ToolId::DesktopList => (vec!["computer_id"], vec!["computer_id", "uri"]),
            ToolId::DesktopRead => (
                vec!["computer_id", "bundle_id", "uri"],
                vec!["computer_id", "bundle_id", "uri"],
            ),
            _ => (vec!["computer_id"], vec!["computer_id"]),
        };
        let properties = properties
            .into_iter()
            .map(|name| (name.to_string(), property_schema(self.id, name)))
            .collect::<serde_json::Map<_, _>>();
        serde_json::json!({
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false
        })
    }
}

fn property_schema(tool: ToolId, name: &str) -> serde_json::Value {
    match name {
        "copy_robot_binding" | "auto_connect" => serde_json::json!({"type": "boolean"}),
        "employee_id" => serde_json::json!({"type": "integer", "minimum": 0}),
        "value" => serde_json::json!({"description": "Any JSON value"}),
        "skill_home_mode" => serde_json::json!({"type": "string", "enum": ["empty", "copy"]}),
        "files" => serde_json::json!({
            "type": "array",
            "maxItems": 256,
            "items": skill_file_schema()
        }),
        "changes" => serde_json::json!({
            "type": "array",
            "items": {
                "oneOf": [
                    {
                        "type": "object",
                        "properties": {
                            "action": {"const": "upsert"},
                            "path": {"type": "string", "minLength": 1},
                            "encoding": {"type": "string", "enum": ["utf8", "base64"]},
                            "content": {"type": "string"}
                        },
                        "required": ["action", "path", "encoding", "content"],
                        "additionalProperties": false
                    },
                    {
                        "type": "object",
                        "properties": {
                            "action": {"const": "delete"},
                            "path": {"type": "string", "minLength": 1}
                        },
                        "required": ["action", "path"],
                        "additionalProperties": false
                    }
                ]
            }
        }),
        "target" if tool == ToolId::ConnectionTargetUpsert => serde_json::json!({
            "type": "object",
            "properties": {
                "id": {"type": "string"},
                "name": {"type": "string", "minLength": 1},
                "url": {"type": "string", "minLength": 1},
                "namespace": {"type": "string"},
                "office_id": {"type": "string", "minLength": 1},
                "headers": {"type": "object", "additionalProperties": {"type": "string"}}
            },
            "required": ["name", "url", "office_id"]
        }),
        "target" => serde_json::json!({
            "oneOf": [
                {
                    "type": "object",
                    "properties": {"type": {"const": "manual_smcp"}, "id": {"type": "string"}},
                    "required": ["type", "id"],
                    "additionalProperties": false
                },
                {
                    "type": "object",
                    "properties": {
                        "type": {"const": "manager_robot"},
                        "contextKey": {"type": "object"},
                        "employeeId": {"type": "integer", "minimum": 0},
                        "lastResolvedRobotAccountId": {"type": ["string", "null"]}
                    },
                    "required": ["type", "contextKey", "employeeId"],
                    "additionalProperties": false
                },
                {"type": "null"}
            ]
        }),
        "api_key_action" => serde_json::json!({
            "oneOf": [
                {
                    "type": "object",
                    "properties": {"kind": {"const": "unchanged"}},
                    "required": ["kind"],
                    "additionalProperties": false
                },
                {
                    "type": "object",
                    "properties": {"kind": {"const": "clear"}},
                    "required": ["kind"],
                    "additionalProperties": false
                },
                {
                    "type": "object",
                    "properties": {"kind": {"const": "set"}, "value": {"type": "string"}},
                    "required": ["kind", "value"],
                    "additionalProperties": false
                },
                {"type": "null"}
            ]
        }),
        "server" => serde_json::json!({"type": "object", "description": "One SDK MCPServerConfig"}),
        "definition" => input_definition_schema(),
        "description"
        | "connection_target_id"
        | "local_skills_root"
        | "expected_revision"
        | "scope"
        | "cursor" => serde_json::json!({"type": ["string", "null"]}),
        "uri" if tool == ToolId::DesktopList => serde_json::json!({"type": ["string", "null"]}),
        "computer_id" | "name" | "target_id" | "input_id" | "bundle_id" | "marketplace"
        | "git_url" | "plugin" | "uri" => {
            serde_json::json!({"type": "string", "minLength": 1})
        }
        _ => serde_json::json!({"type": "string"}),
    }
}

fn skill_file_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": {"type": "string", "minLength": 1},
            "encoding": {"type": "string", "enum": ["utf8", "base64"]},
            "content": {"type": "string"}
        },
        "required": ["path", "encoding", "content"],
        "additionalProperties": false
    })
}

fn input_definition_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "type": {"type": "string", "enum": ["PromptString", "PickString", "Command"]},
            "id": {"type": "string", "minLength": 1},
            "label": {"type": "string", "minLength": 1},
            "description": {"type": ["string", "null"]},
            "default": {"type": ["string", "null"]},
            "password": {"type": ["boolean", "null"]},
            "options": {"type": "array", "items": {"type": "object"}},
            "command": {"type": "string"},
            "args": {"type": ["array", "null"], "items": {"type": "string"}}
        },
        "required": ["type", "id", "label"]
    })
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ToolCatalog;

impl ToolCatalog {
    #[must_use]
    pub fn all(self) -> Vec<ToolDefinition> {
        ToolId::ALL.into_iter().map(Self::definition).collect()
    }

    #[must_use]
    pub fn get(self, id: ToolId) -> ToolDefinition {
        Self::definition(id)
    }

    #[must_use]
    pub fn contains(self, value: &str) -> bool {
        value.parse::<ToolId>().is_ok()
    }

    fn definition(id: ToolId) -> ToolDefinition {
        use ToolGroup as Group;
        use ToolId as Id;
        let group = match id {
            Id::ComputerList
            | Id::ComputerGetStatus
            | Id::ComputerCreate
            | Id::ComputerRename
            | Id::ComputerDuplicate
            | Id::ComputerDelete
            | Id::ComputerStart
            | Id::ComputerStop
            | Id::ComputerRestart => Group::ComputerFleet,
            Id::ComputerSetConnectionPolicy
            | Id::ComputerSetSkillHome
            | Id::ConnectionTargetList
            | Id::ConnectionTargetUpsert
            | Id::ConnectionTargetDelete
            | Id::RobotListAvailable
            | Id::ComputerConnectTarget
            | Id::ComputerConnectRobot
            | Id::ComputerDisconnect
            | Id::ComputerGetConnectionStatus => Group::ComputerConnection,
            Id::McpServerList
            | Id::McpConfigGetState
            | Id::McpServerUpsert
            | Id::McpServerRemove
            | Id::McpServerStart
            | Id::McpServerStop
            | Id::McpServerStartAll
            | Id::McpServerStopAll => Group::McpServer,
            Id::InputDefinitionList
            | Id::InputDefinitionGet
            | Id::InputDefinitionUpsert
            | Id::InputDefinitionRemove
            | Id::InputValueList
            | Id::InputValueGetStatus
            | Id::InputValueSet
            | Id::RuntimeInputValueSet
            | Id::InputValueRemove
            | Id::InputValueClearAll => Group::McpInputs,
            Id::SkillCreate | Id::SkillUpdate | Id::SkillDelete => Group::Skills,
            Id::MarketplaceGetCapabilities
            | Id::MarketplaceGetGovernance
            | Id::MarketplaceAdd
            | Id::MarketplaceUpdate
            | Id::MarketplaceRefresh
            | Id::MarketplaceRemove
            | Id::PluginInstall
            | Id::PluginEnable
            | Id::PluginDisable
            | Id::PluginUninstall => Group::MarketplacePlugins,
            Id::McpToolList
            | Id::McpToolHistory
            | Id::McpResourceList
            | Id::DesktopList
            | Id::DesktopRead => Group::DebugResources,
        };
        let risk = match id {
            Id::ComputerDelete
            | Id::ComputerStop
            | Id::ComputerRestart
            | Id::ComputerDisconnect
            | Id::McpServerRemove
            | Id::InputDefinitionRemove
            | Id::InputValueRemove
            | Id::InputValueClearAll
            | Id::SkillDelete
            | Id::MarketplaceRemove
            | Id::PluginUninstall => ToolRisk::Destructive,
            Id::InputValueSet | Id::RuntimeInputValueSet | Id::ConnectionTargetUpsert => {
                ToolRisk::SensitiveWrite
            }
            Id::ComputerList
            | Id::ComputerGetStatus
            | Id::ConnectionTargetList
            | Id::RobotListAvailable
            | Id::ComputerGetConnectionStatus
            | Id::McpServerList
            | Id::McpConfigGetState
            | Id::InputDefinitionList
            | Id::InputDefinitionGet
            | Id::InputValueList
            | Id::InputValueGetStatus
            | Id::MarketplaceGetCapabilities
            | Id::MarketplaceGetGovernance
            | Id::McpToolList
            | Id::McpToolHistory
            | Id::McpResourceList
            | Id::DesktopList
            | Id::DesktopRead => ToolRisk::ReadOnly,
            _ => ToolRisk::Mutating,
        };
        let target = match id {
            Id::ComputerList
            | Id::ComputerCreate
            | Id::ConnectionTargetList
            | Id::ConnectionTargetUpsert
            | Id::ConnectionTargetDelete
            | Id::RobotListAvailable => TargetContract::Discovery,
            _ => TargetContract::Required,
        };
        ToolDefinition {
            id,
            group,
            risk,
            target,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn catalog_is_exactly_the_confirmed_55_tool_set() {
        let catalog = ToolCatalog.all();
        assert_eq!(catalog.len(), 55);
        let ids = catalog
            .iter()
            .map(|item| item.id.as_str())
            .collect::<HashSet<_>>();
        assert_eq!(ids.len(), 55);
        assert!(!ids.contains("mcp_tool_execute"));
        assert!(!ids.contains("skill_list"));
        assert!(!ids.contains("skill_read"));
        assert!(!ids.contains("skill_refresh"));
    }

    #[test]
    fn every_group_has_the_confirmed_owner_count_and_schema() {
        let catalog = ToolCatalog.all();
        let count = |group| catalog.iter().filter(|item| item.group == group).count();
        assert_eq!(count(ToolGroup::ComputerFleet), 9);
        assert_eq!(count(ToolGroup::ComputerConnection), 10);
        assert_eq!(count(ToolGroup::McpServer), 8);
        assert_eq!(count(ToolGroup::McpInputs), 10);
        assert_eq!(count(ToolGroup::Skills), 3);
        assert_eq!(count(ToolGroup::MarketplacePlugins), 10);
        assert_eq!(count(ToolGroup::DebugResources), 5);
        assert!(catalog
            .iter()
            .all(|item| item.input_schema()["type"] == "object"));
        assert!(catalog.iter().all(|item| item.input_schema()["properties"]
            .as_object()
            .is_some_and(|properties| properties
                .values()
                .all(|schema| schema != &serde_json::json!({})))
            || item.input_schema()["properties"]
                .as_object()
                .is_some_and(serde_json::Map::is_empty)));

        let create_schema = ToolCatalog.get(ToolId::SkillCreate).input_schema();
        assert_eq!(create_schema["properties"]["files"]["type"], "array");
        assert_eq!(
            create_schema["properties"]["files"]["items"]["additionalProperties"],
            false
        );
        let update_schema = ToolCatalog.get(ToolId::SkillUpdate).input_schema();
        assert_eq!(
            update_schema["properties"]["changes"]["items"]["oneOf"]
                .as_array()
                .unwrap()
                .len(),
            2
        );

        let target_schema = ToolCatalog
            .get(ToolId::ConnectionTargetUpsert)
            .input_schema();
        assert_eq!(
            target_schema["properties"]["api_key_action"]["oneOf"][2]["properties"]["kind"]
                ["const"],
            "set"
        );
        for value in [
            serde_json::json!({"kind": "unchanged"}),
            serde_json::json!({"kind": "clear"}),
            serde_json::json!({"kind": "set", "value": "write-only"}),
        ] {
            serde_json::from_value::<crate::commands::connection::ManualSmcpApiKeyAction>(value)
                .expect("published API-key action variants must match the command decoder");
        }
    }
}
