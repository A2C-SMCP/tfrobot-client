use crate::commands::connection::{close_smcp_connection, ConnectionState};
use crate::commands::inputs::InputDefinition;
use serde::{Deserialize, Serialize};
use smcp_computer::mcp_clients::model::{
    CommandInput, MCPServerInput, PickStringInput, PromptStringInput,
};
use smcp_computer::mcp_clients::{MCPServerConfig, MCPServerManager};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

pub type ComputerInstanceId = String;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RobotBindingMetadata {
    pub employee_id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub robot_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub robot_account_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub robot_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComputerConnectionTargetType {
    ManagerRobot,
    ManualSmcp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComputerConnectionTarget {
    #[serde(rename = "type")]
    pub target_type: ComputerConnectionTargetType,
    pub id: String,
    #[serde(
        rename = "robotAccountId",
        alias = "robot_account_id",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub robot_account_id: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct ComputerConnectionPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<ComputerConnectionTarget>,
    #[serde(default)]
    pub auto_connect: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputerInstance {
    pub id: ComputerInstanceId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub mcp_servers: Vec<MCPServerConfig>,
    #[serde(default)]
    pub inputs: Vec<InputDefinition>,
    #[serde(default)]
    pub input_values: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub connection_policy: ComputerConnectionPolicy,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub robot_binding: Option<RobotBindingMetadata>,
}

impl ComputerInstance {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: None,
            mcp_servers: Vec::new(),
            inputs: Vec::new(),
            input_values: HashMap::new(),
            connection_policy: ComputerConnectionPolicy::default(),
            robot_binding: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputerInstancesConfig {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub instances: Vec<ComputerInstance>,
}

impl Default for ComputerInstancesConfig {
    fn default() -> Self {
        Self {
            schema_version: default_schema_version(),
            instances: Vec::new(),
        }
    }
}

impl ComputerInstancesConfig {
    pub fn normalize(&mut self) {
        let mut seen = HashSet::new();
        self.instances
            .retain(|instance| !instance.id.trim().is_empty() && seen.insert(instance.id.clone()));
    }
}

fn default_schema_version() -> u32 {
    1
}

#[derive(Clone)]
pub struct ComputerInstanceRuntime {
    pub instance: ComputerInstance,
    pub manager: Arc<RwLock<Option<MCPServerManager>>>,
    pub inputs: Arc<RwLock<HashMap<String, MCPServerInput>>>,
    pub connection: Arc<RwLock<Option<ConnectionState>>>,
    running: Arc<RwLock<bool>>,
}

impl ComputerInstanceRuntime {
    /// Builds isolated runtime handles for an instance. The manager starts empty; current command
    /// surfaces initialize only the default runtime from persisted MCP server config during app
    /// startup.
    pub fn new(instance: ComputerInstance) -> Self {
        let inputs = input_definitions_to_mcp_map(&instance.inputs);
        Self {
            instance,
            manager: Arc::new(RwLock::new(Some(MCPServerManager::new()))),
            inputs: Arc::new(RwLock::new(inputs)),
            connection: Arc::new(RwLock::new(None)),
            running: Arc::new(RwLock::new(false)),
        }
    }

    fn with_instance(&self, instance: ComputerInstance) -> Self {
        Self {
            instance,
            manager: self.manager.clone(),
            inputs: self.inputs.clone(),
            connection: self.connection.clone(),
            running: self.running.clone(),
        }
    }

    pub async fn start(&self) -> Result<(), String> {
        let lock = self.manager.read().await;
        let manager = lock
            .as_ref()
            .ok_or_else(|| "MCP manager not initialized".to_string())?;
        manager
            .initialize(self.instance.mcp_servers.clone())
            .await
            .map_err(|error| error.to_string())?;
        let mut running = self.running.write().await;
        *running = true;
        Ok(())
    }

    pub async fn is_running(&self) -> bool {
        *self.running.read().await
    }

    pub async fn shutdown(&self) {
        let existing_connection = {
            let mut conn = self.connection.write().await;
            conn.take()
        };
        if let Some(connection) = existing_connection {
            close_smcp_connection(connection).await;
        }

        let lock = self.manager.read().await;
        if let Some(manager) = lock.as_ref() {
            let _ = manager.stop_all().await;
        }

        let mut running = self.running.write().await;
        *running = false;
    }

    pub async fn sync_runtime_inputs(&self) {
        let mut inputs = self.inputs.write().await;
        *inputs = input_definitions_to_mcp_map(&self.instance.inputs);
    }

    pub async fn is_connected(&self) -> bool {
        self.connection.read().await.is_some()
    }

    pub async fn connection_status(&self) -> Option<ConnectionStateSummary> {
        self.connection
            .read()
            .await
            .as_ref()
            .map(ConnectionStateSummary::from)
    }
}

fn input_definitions_to_mcp_map(
    definitions: &[InputDefinition],
) -> HashMap<String, MCPServerInput> {
    definitions
        .iter()
        .map(|definition| {
            let input = input_definition_to_mcp(definition);
            (input.id().to_string(), input)
        })
        .collect()
}

fn input_description(label: &str, description: &Option<String>) -> String {
    description
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| label.to_string())
}

fn input_definition_to_mcp(definition: &InputDefinition) -> MCPServerInput {
    match definition {
        InputDefinition::PromptString {
            id,
            label,
            description,
            default,
            password,
        } => MCPServerInput::PromptString(PromptStringInput {
            id: id.clone(),
            description: input_description(label, description),
            default: default.clone(),
            password: *password,
        }),
        InputDefinition::PickString {
            id,
            label,
            description,
            options,
            default,
        } => MCPServerInput::PickString(PickStringInput {
            id: id.clone(),
            description: input_description(label, description),
            options: options.iter().map(|option| option.value.clone()).collect(),
            default: default.clone(),
        }),
        InputDefinition::Command {
            id, label, command, ..
        } => MCPServerInput::Command(CommandInput {
            id: id.clone(),
            description: label.clone(),
            command: command.clone(),
            args: None,
        }),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionStateSummary {
    pub url: String,
    pub office_id: String,
    pub computer_name: String,
    pub connected_at: String,
    pub profile_name: String,
}

impl From<&ConnectionState> for ConnectionStateSummary {
    fn from(connection: &ConnectionState) -> Self {
        Self {
            url: connection.url.clone(),
            office_id: connection.office_id.clone(),
            computer_name: connection.computer_name.clone(),
            connected_at: connection.connected_at.to_rfc3339(),
            profile_name: connection.profile_name.clone(),
        }
    }
}

pub struct ComputerRegistry {
    runtimes: RwLock<HashMap<ComputerInstanceId, ComputerInstanceRuntime>>,
}

impl ComputerRegistry {
    pub fn from_config(config: ComputerInstancesConfig) -> Self {
        let (registry, _) = Self::from_config_with_initial_runtime(config);
        registry
    }

    pub fn from_config_with_initial_runtime(
        mut config: ComputerInstancesConfig,
    ) -> (Self, Option<ComputerInstanceRuntime>) {
        config.normalize();
        let mut runtimes = HashMap::new();

        for instance in config.instances {
            let instance_id = instance.id.clone();
            let runtime = ComputerInstanceRuntime::new(instance);
            runtimes.insert(instance_id, runtime);
        }

        let initial_runtime = runtimes.values().next().cloned();
        let registry = Self {
            runtimes: RwLock::new(runtimes),
        };

        (registry, initial_runtime)
    }

    pub async fn runtime(&self, id: &str) -> Option<ComputerInstanceRuntime> {
        let runtimes = self.runtimes.read().await;
        runtimes.get(id).cloned()
    }

    pub async fn list_runtimes(&self) -> Vec<ComputerInstanceRuntime> {
        let runtimes = self.runtimes.read().await;
        let mut values: Vec<_> = runtimes.values().cloned().collect();
        values.sort_by(|a, b| a.instance.name.cmp(&b.instance.name));
        values
    }

    pub async fn upsert_runtime(&self, instance: ComputerInstance) -> ComputerInstanceRuntime {
        let runtime = ComputerInstanceRuntime::new(instance.clone());
        let mut runtimes = self.runtimes.write().await;
        runtimes.insert(instance.id, runtime.clone());
        runtime
    }

    pub async fn update_runtime_instance(
        &self,
        instance: ComputerInstance,
    ) -> ComputerInstanceRuntime {
        let runtime = {
            let mut runtimes = self.runtimes.write().await;
            let runtime = match runtimes.get(&instance.id) {
                Some(existing) => existing.with_instance(instance.clone()),
                None => ComputerInstanceRuntime::new(instance.clone()),
            };
            runtimes.insert(instance.id, runtime.clone());
            runtime
        };
        runtime.sync_runtime_inputs().await;
        runtime
    }

    pub async fn remove_runtime(&self, id: &str) -> Option<ComputerInstanceRuntime> {
        let mut runtimes = self.runtimes.write().await;
        runtimes.remove(id)
    }

    pub async fn start_runtime(&self, id: &str) -> Result<(), String> {
        let runtime = self
            .runtime(id)
            .await
            .ok_or_else(|| format!("Computer instance not found: {id}"))?;
        runtime.start().await
    }

    pub async fn stop_runtime(&self, id: &str) -> Result<(), String> {
        let runtime = self
            .runtime(id)
            .await
            .ok_or_else(|| format!("Computer instance not found: {id}"))?;
        runtime.shutdown().await;
        Ok(())
    }

    pub async fn shutdown_all(&self) {
        let runtimes: Vec<_> = {
            let runtimes = self.runtimes.read().await;
            runtimes.values().cloned().collect()
        };

        for runtime in runtimes {
            runtime.shutdown().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instance(id: &str, name: &str) -> ComputerInstance {
        ComputerInstance::new(id, name)
    }

    fn instance_with_input(id: &str, label: &str) -> ComputerInstance {
        let mut instance = ComputerInstance::new(id, "Computer");
        instance.inputs = vec![InputDefinition::PromptString {
            id: "api-key".to_string(),
            label: label.to_string(),
            description: None,
            default: Some("default-value".to_string()),
            password: Some(true),
        }];
        instance
    }

    #[test]
    fn computer_instances_config_can_be_empty() {
        let config = ComputerInstancesConfig::default();

        assert!(config.instances.is_empty());
    }

    #[test]
    fn normalize_removes_duplicate_ids_and_empty_ids() {
        let mut config = ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![
                instance("dup", "First"),
                instance("dup", "Second"),
                instance("", "Empty"),
            ],
        };

        config.normalize();

        assert_eq!(config.instances.len(), 1);
        assert_eq!(config.instances[0].name, "First");
    }

    #[tokio::test]
    async fn registry_builds_independent_runtimes() {
        let config = ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![instance("one", "One"), instance("two", "Two")],
        };

        let registry = ComputerRegistry::from_config(config);
        let one = registry.runtime("one").await.unwrap();
        let two = registry.runtime("two").await.unwrap();

        assert_eq!(one.instance.id, "one");
        assert_eq!(two.instance.id, "two");
        assert!(!Arc::ptr_eq(&one.manager, &two.manager));
        assert!(!Arc::ptr_eq(&one.inputs, &two.inputs));
        assert!(!Arc::ptr_eq(&one.connection, &two.connection));
    }

    #[tokio::test]
    async fn runtime_seeds_smcp_input_definitions_from_instance() {
        let runtime = ComputerInstanceRuntime::new(instance_with_input("one", "API Key"));

        let inputs = runtime.inputs.read().await;
        let input = inputs.get("api-key").expect("input should be loaded");

        match input {
            MCPServerInput::PromptString(prompt) => {
                assert_eq!(prompt.description, "API Key");
                assert_eq!(prompt.default.as_deref(), Some("default-value"));
                assert_eq!(prompt.password, Some(true));
            }
            other => panic!("expected PromptString input, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn initial_runtime_is_some_when_any_instance_exists() {
        let config = ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![instance("one", "One"), instance("two", "Two")],
        };

        let (_registry, initial_runtime) =
            ComputerRegistry::from_config_with_initial_runtime(config);

        assert!(matches!(
            initial_runtime.map(|runtime| runtime.instance.id),
            Some(id) if id == "one" || id == "two"
        ));
    }

    #[tokio::test]
    async fn initial_runtime_is_none_when_registry_is_empty() {
        let (_registry, initial_runtime) =
            ComputerRegistry::from_config_with_initial_runtime(ComputerInstancesConfig::default());

        assert!(initial_runtime.is_none());
    }

    #[tokio::test]
    async fn runtime_start_stop_only_changes_target_instance() {
        let config = ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![instance("one", "One"), instance("two", "Two")],
        };
        let registry = ComputerRegistry::from_config(config);

        registry.start_runtime("one").await.unwrap();
        assert!(registry.runtime("one").await.unwrap().is_running().await);
        assert!(!registry.runtime("two").await.unwrap().is_running().await);

        registry.stop_runtime("one").await.unwrap();
        assert!(!registry.runtime("one").await.unwrap().is_running().await);
        assert!(!registry.runtime("two").await.unwrap().is_running().await);
    }

    #[tokio::test]
    async fn update_runtime_instance_preserves_runtime_handles() {
        let registry = ComputerRegistry::from_config(ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![instance_with_input("one", "Initial Label")],
        });

        let before = registry.runtime("one").await.unwrap();
        registry.start_runtime("one").await.unwrap();
        let mut updated = instance_with_input("one", "Updated Label");
        updated.name = "Renamed".to_string();
        let after = registry.update_runtime_instance(updated).await;

        assert_eq!(after.instance.name, "Renamed");
        assert!(after.is_running().await);
        assert!(Arc::ptr_eq(&before.manager, &after.manager));
        assert!(Arc::ptr_eq(&before.inputs, &after.inputs));
        assert!(Arc::ptr_eq(&before.connection, &after.connection));
        let inputs = after.inputs.read().await;
        let input = inputs.get("api-key").expect("input should be synced");
        assert!(matches!(
            input,
            MCPServerInput::PromptString(prompt) if prompt.description == "Updated Label"
        ));
    }

    #[tokio::test]
    async fn remove_runtime_allows_any_configured_instance() {
        let registry = ComputerRegistry::from_config(ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![instance("one", "One"), instance("two", "Two")],
        });

        let removed_one = registry.remove_runtime("one").await.unwrap();
        assert_eq!(removed_one.instance.id, "one");
        assert!(registry.runtime("one").await.is_none());

        let removed_two = registry.remove_runtime("two").await.unwrap();
        assert_eq!(removed_two.instance.id, "two");
        assert!(registry.runtime("two").await.is_none());
    }

    #[tokio::test]
    async fn removed_running_runtime_can_be_shutdown_without_affecting_others() {
        let registry = ComputerRegistry::from_config(ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![instance("one", "One"), instance("two", "Two")],
        });
        registry.start_runtime("one").await.unwrap();
        registry.start_runtime("two").await.unwrap();

        let removed = registry.remove_runtime("two").await.unwrap();
        removed.shutdown().await;

        assert!(!removed.is_running().await);
        assert!(registry.runtime("one").await.unwrap().is_running().await);
        assert!(registry.runtime("two").await.is_none());
    }
}
