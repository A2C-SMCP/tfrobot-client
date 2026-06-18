use crate::commands::connection::{close_smcp_connection, ConnectionProfile, ConnectionState};
use crate::commands::inputs::InputDefinition;
use serde::{Deserialize, Serialize};
use smcp_computer::mcp_clients::model::MCPServerInput;
use smcp_computer::mcp_clients::{MCPServerConfig, MCPServerManager};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

pub const DEFAULT_COMPUTER_INSTANCE_ID: &str = "default";
pub const DEFAULT_COMPUTER_INSTANCE_NAME: &str = "Default Computer";

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputerInstance {
    pub id: ComputerInstanceId,
    pub name: String,
    #[serde(default)]
    pub mcp_servers: Vec<MCPServerConfig>,
    #[serde(default)]
    pub inputs: Vec<InputDefinition>,
    #[serde(default)]
    pub input_values: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub connection_profiles: Vec<ConnectionProfile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub robot_binding: Option<RobotBindingMetadata>,
}

impl ComputerInstance {
    pub fn default_instance() -> Self {
        Self {
            id: DEFAULT_COMPUTER_INSTANCE_ID.to_string(),
            name: DEFAULT_COMPUTER_INSTANCE_NAME.to_string(),
            mcp_servers: Vec::new(),
            inputs: Vec::new(),
            input_values: HashMap::new(),
            connection_profiles: Vec::new(),
            robot_binding: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputerInstancesConfig {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default = "default_instance_id")]
    pub default_instance_id: ComputerInstanceId,
    #[serde(default)]
    pub instances: Vec<ComputerInstance>,
}

impl Default for ComputerInstancesConfig {
    fn default() -> Self {
        Self {
            schema_version: default_schema_version(),
            default_instance_id: default_instance_id(),
            instances: vec![ComputerInstance::default_instance()],
        }
    }
}

impl ComputerInstancesConfig {
    pub fn ensure_default_instance(&mut self) {
        let mut seen = HashSet::new();
        self.instances
            .retain(|instance| !instance.id.trim().is_empty() && seen.insert(instance.id.clone()));

        if self.instances.is_empty() {
            self.instances.push(ComputerInstance::default_instance());
        }
        if !self
            .instances
            .iter()
            .any(|instance| instance.id == self.default_instance_id)
        {
            self.default_instance_id = self.instances[0].id.clone();
        }
    }

    pub fn default_instance(&self) -> Option<&ComputerInstance> {
        self.instances
            .iter()
            .find(|instance| instance.id == self.default_instance_id)
    }

    pub fn default_instance_mut(&mut self) -> Option<&mut ComputerInstance> {
        let default_id = self.default_instance_id.clone();
        self.instances
            .iter_mut()
            .find(|instance| instance.id == default_id)
    }
}

fn default_schema_version() -> u32 {
    1
}

fn default_instance_id() -> ComputerInstanceId {
    DEFAULT_COMPUTER_INSTANCE_ID.to_string()
}

#[derive(Clone)]
pub struct ComputerInstanceRuntime {
    pub instance: ComputerInstance,
    pub manager: Arc<RwLock<Option<MCPServerManager>>>,
    pub inputs: Arc<RwLock<HashMap<String, MCPServerInput>>>,
    pub connection: Arc<RwLock<Option<ConnectionState>>>,
}

impl ComputerInstanceRuntime {
    /// Builds isolated runtime handles for an instance. The manager starts empty; current command
    /// surfaces initialize only the default runtime from persisted MCP server config during app
    /// startup.
    pub fn new(instance: ComputerInstance) -> Self {
        Self {
            instance,
            manager: Arc::new(RwLock::new(Some(MCPServerManager::new()))),
            inputs: Arc::new(RwLock::new(HashMap::new())),
            connection: Arc::new(RwLock::new(None)),
        }
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
    }
}

pub struct ComputerRegistry {
    default_instance_id: ComputerInstanceId,
    runtimes: RwLock<HashMap<ComputerInstanceId, ComputerInstanceRuntime>>,
}

impl ComputerRegistry {
    pub fn from_config(config: ComputerInstancesConfig) -> Self {
        let (registry, _) = Self::from_config_with_default_runtime(config);
        registry
    }

    pub fn from_config_with_default_runtime(
        mut config: ComputerInstancesConfig,
    ) -> (Self, ComputerInstanceRuntime) {
        config.ensure_default_instance();
        let default_instance_id = config.default_instance_id.clone();
        let mut runtimes = HashMap::new();

        for instance in config.instances {
            let instance_id = instance.id.clone();
            let runtime = ComputerInstanceRuntime::new(instance);
            runtimes.insert(instance_id, runtime);
        }

        let default_runtime = runtimes
            .get(&default_instance_id)
            .expect("default runtime should exist")
            .clone();
        let registry = Self {
            default_instance_id,
            runtimes: RwLock::new(runtimes),
        };

        (registry, default_runtime)
    }

    pub async fn default_runtime(&self) -> Option<ComputerInstanceRuntime> {
        self.runtime(&self.default_instance_id).await
    }

    pub async fn runtime(&self, id: &str) -> Option<ComputerInstanceRuntime> {
        let runtimes = self.runtimes.read().await;
        runtimes.get(id).cloned()
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

    #[test]
    fn computer_instances_config_has_default_instance() {
        let config = ComputerInstancesConfig::default();

        assert_eq!(config.default_instance_id, DEFAULT_COMPUTER_INSTANCE_ID);
        assert_eq!(config.instances.len(), 1);
        assert_eq!(
            config.default_instance().unwrap().id,
            DEFAULT_COMPUTER_INSTANCE_ID
        );
    }

    #[test]
    fn ensure_default_instance_removes_duplicate_ids() {
        let mut config = ComputerInstancesConfig {
            schema_version: 1,
            default_instance_id: "dup".to_string(),
            instances: vec![
                ComputerInstance {
                    id: "dup".to_string(),
                    name: "First".to_string(),
                    ..ComputerInstance::default_instance()
                },
                ComputerInstance {
                    id: "dup".to_string(),
                    name: "Second".to_string(),
                    ..ComputerInstance::default_instance()
                },
            ],
        };

        config.ensure_default_instance();

        assert_eq!(config.instances.len(), 1);
        assert_eq!(config.instances[0].name, "First");
    }

    #[tokio::test]
    async fn registry_builds_independent_runtimes() {
        let config = ComputerInstancesConfig {
            schema_version: 1,
            default_instance_id: "one".to_string(),
            instances: vec![
                ComputerInstance {
                    id: "one".to_string(),
                    name: "One".to_string(),
                    ..ComputerInstance::default_instance()
                },
                ComputerInstance {
                    id: "two".to_string(),
                    name: "Two".to_string(),
                    ..ComputerInstance::default_instance()
                },
            ],
        };

        let registry = ComputerRegistry::from_config(config);
        let one = registry.default_runtime().await.unwrap();
        let two = registry.runtime("two").await.unwrap();

        assert_eq!(one.instance.id, "one");
        assert_eq!(two.instance.id, "two");
        assert!(!Arc::ptr_eq(&one.manager, &two.manager));
        assert!(!Arc::ptr_eq(&one.inputs, &two.inputs));
        assert!(!Arc::ptr_eq(&one.connection, &two.connection));
    }

    #[tokio::test]
    async fn returned_default_runtime_matches_registry_runtime_with_duplicate_ids() {
        let config = ComputerInstancesConfig {
            schema_version: 1,
            default_instance_id: "dup".to_string(),
            instances: vec![
                ComputerInstance {
                    id: "dup".to_string(),
                    name: "First".to_string(),
                    ..ComputerInstance::default_instance()
                },
                ComputerInstance {
                    id: "dup".to_string(),
                    name: "Second".to_string(),
                    ..ComputerInstance::default_instance()
                },
            ],
        };

        let (registry, default_runtime) =
            ComputerRegistry::from_config_with_default_runtime(config);
        let registry_default = registry.default_runtime().await.unwrap();

        assert_eq!(default_runtime.instance.name, "First");
        assert!(Arc::ptr_eq(
            &default_runtime.manager,
            &registry_default.manager
        ));
        assert!(Arc::ptr_eq(
            &default_runtime.inputs,
            &registry_default.inputs
        ));
        assert!(Arc::ptr_eq(
            &default_runtime.connection,
            &registry_default.connection
        ));
    }
}
