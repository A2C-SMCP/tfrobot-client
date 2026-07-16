use crate::commands::connection::ConnectionState;
use crate::commands::inputs::{InputDefinition, PickOption};
use crate::services::config::instance_storage_dir_name;
use crate::services::sdk_config::InstanceConfigContext;
use a2c_smcp::smcp_computer::computer::{Computer, ConnectOptions, Session, ToolCallRecord};
use a2c_smcp::smcp_computer::errors::{ComputerError, ComputerResult};
use a2c_smcp::smcp_computer::inputs::run_command;
use a2c_smcp::smcp_computer::mcp_clients::model::{
    CallToolResult, CommandInput, MCPServerInput, PickStringInput, PromptStringInput,
    ReadResourceResult, Resource, Tool, ToolMeta,
};
use a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig;
use a2c_smcp::smcp_computer::settings::{
    resolve_policy_settings, resolve_settings, AddMarketplaceParams, DisableOptions, EnableOptions,
    InstallOptions, MarketplaceRefreshRow, MarketplaceRemoveOutcome, McpHookError, McpInstallHooks,
    PluginInstallError, RemoveMarketplaceParams, ResolveSettingsArgs, UninstallOptions,
};
use a2c_smcp::smcp_computer::settings::{GovernanceError, MarketplaceAddOutcome};
use a2c_smcp::smcp_computer::skills::{
    SkillResourceView, SkillSandboxError, MCP_INPUTS_FILENAME, MCP_SERVERS_SUBDIR,
};
use a2c_smcp::smcp_computer::{
    inputs::load_plugin_inputs,
    inventory::{McpOwnership, McpServerWithMetadata},
};
use a2c_smcp::A2CSkillRef;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpServerManagedBy {
    User,
    Plugin {
        marketplace: String,
        plugin: String,
        #[serde(rename = "pluginId", default, skip_serializing_if = "Option::is_none")]
        plugin_id: Option<String>,
    },
}

impl McpServerManagedBy {
    pub fn is_plugin_owned(&self) -> bool {
        matches!(self, Self::Plugin { .. })
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ManagedMcpServer {
    pub config: MCPServerConfig,
    #[serde(rename = "managedBy")]
    pub managed_by: McpServerManagedBy,
}

impl<'de> Deserialize<'de> for ManagedMcpServer {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum ManagedMcpServerWire {
            Managed {
                config: MCPServerConfig,
                #[serde(rename = "managedBy")]
                managed_by: McpServerManagedBy,
            },
            Legacy(MCPServerConfig),
        }

        match ManagedMcpServerWire::deserialize(deserializer)? {
            ManagedMcpServerWire::Managed { config, managed_by } => Ok(Self { config, managed_by }),
            ManagedMcpServerWire::Legacy(config) => Ok(Self::user(config)),
        }
    }
}

impl ManagedMcpServer {
    pub fn user(config: MCPServerConfig) -> Self {
        Self {
            config,
            managed_by: McpServerManagedBy::User,
        }
    }

    pub fn name(&self) -> &str {
        self.config.name()
    }

    pub fn is_plugin_owned(&self) -> bool {
        self.managed_by.is_plugin_owned()
    }
}

impl From<MCPServerConfig> for ManagedMcpServer {
    fn from(config: MCPServerConfig) -> Self {
        Self::user(config)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComputerInstance {
    pub id: ComputerInstanceId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub(crate) mcp_servers: Vec<ManagedMcpServer>,
    #[serde(default)]
    pub inputs: Vec<InputDefinition>,
    #[serde(default)]
    pub input_values: HashMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_skills_root: Option<PathBuf>,
    #[serde(default)]
    pub connection_policy: ComputerConnectionPolicy,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub robot_binding: Option<RobotBindingMetadata>,
}

pub const COMPUTER_PROFILE_SCHEMA_VERSION: u32 = 1;
pub const GLOBAL_INPUTS_SCHEMA_VERSION: u32 = 1;
pub const SDK_CONTEXT_SCHEMA_VERSION: u32 = 1;

/// Client-owned, durable metadata for one Computer instance.
///
/// This intentionally excludes SDK-owned configuration, runtime state, input
/// definitions, and resolved input values. Keeping a dedicated persistence DTO
/// prevents the legacy [`ComputerInstance`] aggregate from becoming the schema
/// of `profile.json` by accident.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ComputerProfile {
    pub schema_version: u32,
    pub id: ComputerInstanceId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub connection_policy: ComputerProfileConnectionPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub robot_binding: Option<ComputerProfileRobotBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct ComputerProfileConnectionPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<ComputerProfileConnectionTarget>,
    #[serde(default)]
    pub auto_connect: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ComputerProfileConnectionTarget {
    #[serde(rename = "type")]
    pub target_type: ComputerConnectionTargetType,
    pub id: String,
    #[serde(
        rename = "robotAccountId",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub robot_account_id: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ComputerProfileRobotBinding {
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

impl ComputerProfile {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            schema_version: COMPUTER_PROFILE_SCHEMA_VERSION,
            id: id.into(),
            name: name.into(),
            description: None,
            connection_policy: ComputerProfileConnectionPolicy::default(),
            robot_binding: None,
        }
    }
}

impl From<&ComputerInstance> for ComputerProfile {
    fn from(instance: &ComputerInstance) -> Self {
        Self {
            schema_version: COMPUTER_PROFILE_SCHEMA_VERSION,
            id: instance.id.clone(),
            name: instance.name.clone(),
            description: instance.description.clone(),
            connection_policy: ComputerProfileConnectionPolicy {
                target: instance.connection_policy.target.as_ref().map(|target| {
                    ComputerProfileConnectionTarget {
                        target_type: target.target_type.clone(),
                        id: target.id.clone(),
                        robot_account_id: target.robot_account_id,
                    }
                }),
                auto_connect: instance.connection_policy.auto_connect,
            },
            robot_binding: instance.robot_binding.as_ref().map(|binding| {
                ComputerProfileRobotBinding {
                    employee_id: binding.employee_id,
                    robot_id: binding.robot_id.clone(),
                    robot_account_id: binding.robot_account_id,
                    namespace: binding.namespace.clone(),
                    robot_name: binding.robot_name.clone(),
                }
            }),
        }
    }
}

impl From<ComputerProfile> for ComputerInstance {
    fn from(profile: ComputerProfile) -> Self {
        Self {
            id: profile.id,
            name: profile.name,
            description: profile.description,
            mcp_servers: Vec::new(),
            inputs: Vec::new(),
            input_values: HashMap::new(),
            local_skills_root: None,
            connection_policy: ComputerConnectionPolicy {
                target: profile
                    .connection_policy
                    .target
                    .map(|target| ComputerConnectionTarget {
                        target_type: target.target_type,
                        id: target.id,
                        robot_account_id: target.robot_account_id,
                    }),
                auto_connect: profile.connection_policy.auto_connect,
            },
            robot_binding: profile.robot_binding.map(|binding| RobotBindingMetadata {
                employee_id: binding.employee_id,
                robot_id: binding.robot_id,
                robot_account_id: binding.robot_account_id,
                namespace: binding.namespace,
                robot_name: binding.robot_name,
            }),
        }
    }
}

/// Client-owned injection context for the SDK runtime.
///
/// This remains separate from `profile.json`: the profile is product metadata,
/// while this sidecar only selects an SDK Skill Home for the instance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SdkContextConfig {
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_home_override: Option<PathBuf>,
}

impl Default for SdkContextConfig {
    fn default() -> Self {
        Self {
            schema_version: SDK_CONTEXT_SCHEMA_VERSION,
            skill_home_override: None,
        }
    }
}

/// Global input definitions and UI schema owned by the client.
/// Resolved values and secrets are deliberately stored through `SecretStore`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GlobalInputsConfig {
    pub schema_version: u32,
    #[serde(default)]
    pub inputs: Vec<GlobalInputDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum GlobalInputDefinition {
    PromptString {
        id: String,
        label: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        default: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        password: Option<bool>,
    },
    PickString {
        id: String,
        label: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        options: Vec<GlobalPickOption>,
        #[serde(skip_serializing_if = "Option::is_none")]
        default: Option<String>,
    },
    Command {
        id: String,
        label: String,
        command: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        args: Option<Vec<String>>,
    },
}

impl GlobalInputDefinition {
    pub fn id(&self) -> &str {
        match self {
            Self::PromptString { id, .. }
            | Self::PickString { id, .. }
            | Self::Command { id, .. } => id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GlobalPickOption {
    pub label: String,
    pub value: String,
}

impl From<&InputDefinition> for GlobalInputDefinition {
    fn from(input: &InputDefinition) -> Self {
        match input {
            InputDefinition::PromptString {
                id,
                label,
                description,
                default,
                password,
            } => Self::PromptString {
                id: id.clone(),
                label: label.clone(),
                description: description.clone(),
                default: default.clone(),
                password: *password,
            },
            InputDefinition::PickString {
                id,
                label,
                description,
                options,
                default,
            } => Self::PickString {
                id: id.clone(),
                label: label.clone(),
                description: description.clone(),
                options: options
                    .iter()
                    .map(|option| GlobalPickOption {
                        label: option.label.clone(),
                        value: option.value.clone(),
                    })
                    .collect(),
                default: default.clone(),
            },
            InputDefinition::Command {
                id,
                label,
                command,
                args,
            } => Self::Command {
                id: id.clone(),
                label: label.clone(),
                command: command.clone(),
                args: args.clone(),
            },
        }
    }
}

impl From<&GlobalInputDefinition> for InputDefinition {
    fn from(input: &GlobalInputDefinition) -> Self {
        match input {
            GlobalInputDefinition::PromptString {
                id,
                label,
                description,
                default,
                password,
            } => Self::PromptString {
                id: id.clone(),
                label: label.clone(),
                description: description.clone(),
                default: default.clone(),
                password: *password,
            },
            GlobalInputDefinition::PickString {
                id,
                label,
                description,
                options,
                default,
            } => Self::PickString {
                id: id.clone(),
                label: label.clone(),
                description: description.clone(),
                options: options
                    .iter()
                    .map(|option| PickOption {
                        label: option.label.clone(),
                        value: option.value.clone(),
                    })
                    .collect(),
                default: default.clone(),
            },
            GlobalInputDefinition::Command {
                id,
                label,
                command,
                args,
            } => Self::Command {
                id: id.clone(),
                label: label.clone(),
                command: command.clone(),
                args: args.clone(),
            },
        }
    }
}

impl Default for GlobalInputsConfig {
    fn default() -> Self {
        Self {
            schema_version: GLOBAL_INPUTS_SCHEMA_VERSION,
            inputs: Vec::new(),
        }
    }
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
            local_skills_root: None,
            connection_policy: ComputerConnectionPolicy::default(),
            robot_binding: None,
        }
    }
}

#[derive(Clone)]
pub struct InstanceSession {
    id: String,
    input_values: Arc<RwLock<HashMap<String, serde_json::Value>>>,
}

impl InstanceSession {
    fn new(id: impl Into<String>, input_values: HashMap<String, serde_json::Value>) -> Self {
        Self {
            id: id.into(),
            input_values: Arc::new(RwLock::new(input_values)),
        }
    }

    async fn sync_values(&self, input_values: HashMap<String, serde_json::Value>) {
        let mut values = self.input_values.write().await;
        *values = input_values;
    }
}

#[async_trait]
impl Session for InstanceSession {
    async fn resolve_input(&self, input: &MCPServerInput) -> ComputerResult<serde_json::Value> {
        let input_id = input.id();
        if let Some(value) = self.input_values.read().await.get(input_id).cloned() {
            return Ok(value);
        }

        match input {
            MCPServerInput::PromptString(input) => Ok(serde_json::Value::String(
                input.default.clone().unwrap_or_default(),
            )),
            MCPServerInput::PickString(input) => Ok(serde_json::Value::String(
                input
                    .default
                    .clone()
                    .unwrap_or_else(|| input.options.first().cloned().unwrap_or_default()),
            )),
            MCPServerInput::Command(input) => {
                let args: Vec<String> = input
                    .args
                    .as_ref()
                    .map(|args| {
                        let mut pairs: Vec<_> = args.iter().collect();
                        pairs.sort_by_key(|(index, _)| *index);
                        pairs.into_iter().map(|(_, value)| value.clone()).collect()
                    })
                    .unwrap_or_default();
                run_command(&input.command, &args).await.map_or_else(
                    |error| {
                        Err(ComputerError::RuntimeError(format!(
                            "Failed to execute command '{}': {}",
                            input.command, error
                        )))
                    },
                    |output| Ok(serde_json::Value::String(output)),
                )
            }
        }
    }

    fn session_id(&self) -> &str {
        &self.id
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ComputerRuntimeState {
    Created,
    Booting,
    Booted,
    Connecting,
    Connected,
    JoinedOffice,
    Stopping,
    Stopped,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmcpReconnectOutcome {
    Reconnected { expires_in: i64 },
    Stale,
}

#[derive(Clone)]
pub struct ComputerInstanceRuntime {
    pub instance: ComputerInstance,
    pub inputs: Arc<RwLock<HashMap<String, MCPServerInput>>>,
    computer: Arc<RwLock<Computer<InstanceSession>>>,
    session: InstanceSession,
    skill_home_base: PathBuf,
    sdk_auto_connect: Arc<RwLock<bool>>,
    sdk_server_names: Arc<RwLock<HashSet<String>>>,
    plugin_server_owners: Arc<RwLock<HashMap<String, McpServerManagedBy>>>,
    pub connection: Arc<RwLock<Option<ConnectionState>>>,
    state: Arc<RwLock<ComputerRuntimeState>>,
    lifecycle_lock: Arc<Mutex<()>>,
    refresh_task: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    running: Arc<RwLock<bool>>,
}

impl ComputerInstanceRuntime {
    /// Builds isolated runtime handles for an instance. MCP runtime ownership lives in SDK
    /// Computer; the client keeps only instance-scoped state and lifecycle handles here.
    pub fn new(instance: ComputerInstance, skill_home_base: PathBuf) -> Self {
        let inputs = input_definitions_to_mcp_map(&instance.inputs);
        let session = InstanceSession::new(instance.id.clone(), instance.input_values.clone());
        let (computer, sdk_server_names) =
            build_sdk_computer(&instance, &inputs, session.clone(), &skill_home_base);
        let auto_connect = instance.connection_policy.auto_connect;
        Self {
            instance,
            inputs: Arc::new(RwLock::new(inputs)),
            computer: Arc::new(RwLock::new(computer)),
            session,
            skill_home_base,
            sdk_auto_connect: Arc::new(RwLock::new(auto_connect)),
            sdk_server_names: Arc::new(RwLock::new(sdk_server_names)),
            plugin_server_owners: Arc::new(RwLock::new(HashMap::new())),
            connection: Arc::new(RwLock::new(None)),
            state: Arc::new(RwLock::new(ComputerRuntimeState::Created)),
            lifecycle_lock: Arc::new(Mutex::new(())),
            refresh_task: Arc::new(Mutex::new(None)),
            running: Arc::new(RwLock::new(false)),
        }
    }

    fn with_instance(&self, instance: ComputerInstance) -> Self {
        Self {
            instance,
            inputs: self.inputs.clone(),
            computer: self.computer.clone(),
            session: self.session.clone(),
            skill_home_base: self.skill_home_base.clone(),
            sdk_auto_connect: self.sdk_auto_connect.clone(),
            sdk_server_names: self.sdk_server_names.clone(),
            plugin_server_owners: self.plugin_server_owners.clone(),
            connection: self.connection.clone(),
            state: self.state.clone(),
            lifecycle_lock: self.lifecycle_lock.clone(),
            refresh_task: self.refresh_task.clone(),
            running: self.running.clone(),
        }
    }

    pub async fn start(&self) -> Result<(), String> {
        let _guard = self.lifecycle_lock.lock().await;
        self.set_state(ComputerRuntimeState::Booting).await;

        if let Err(error) = self.computer.read().await.boot_up().await {
            self.set_state(ComputerRuntimeState::Error).await;
            return Err(error.to_string());
        }
        self.reconcile_sdk_governance_inner().await?;

        let mut running = self.running.write().await;
        *running = true;
        drop(running);
        self.set_state(ComputerRuntimeState::Booted).await;
        Ok(())
    }

    pub async fn is_running(&self) -> bool {
        *self.running.read().await
    }

    pub async fn runtime_state(&self) -> ComputerRuntimeState {
        *self.state.read().await
    }

    pub async fn shutdown(&self) {
        let _guard = self.lifecycle_lock.lock().await;
        self.set_state(ComputerRuntimeState::Stopping).await;

        let existing_connection = {
            let mut conn = self.connection.write().await;
            conn.take()
        };
        if existing_connection.is_some() {
            self.abort_refresh_task().await;
            if let Err(error) = self.disconnect_smcp_socketio_inner().await {
                log::warn!(
                    "Failed to disconnect SMCP socket during shutdown for instance {}: {}",
                    self.instance.id,
                    error
                );
            }
        }

        if let Err(error) = self.computer.read().await.shutdown().await {
            log::warn!(
                "Failed to shutdown SDK Computer for instance {}: {}",
                self.instance.id,
                error
            );
        }

        let mut running = self.running.write().await;
        *running = false;
        drop(running);
        self.set_state(ComputerRuntimeState::Stopped).await;
    }

    pub async fn sync_runtime(&self) -> Result<(), String> {
        let _guard = self.lifecycle_lock.lock().await;
        let was_running = *self.running.read().await;

        let rebuilt = if self.sdk_requires_rebuild().await {
            self.replace_sdk_computer(was_running).await?;
            true
        } else {
            false
        };

        let mut inputs = self.inputs.write().await;
        *inputs = input_definitions_to_mcp_map(&self.instance.inputs);
        self.session
            .sync_values(self.instance.input_values.clone())
            .await;
        self.computer
            .read()
            .await
            .update_inputs(inputs.clone())
            .await
            .map_err(|error| {
                format!(
                    "Failed to sync SDK Computer inputs for instance {}: {}",
                    self.instance.id, error
                )
            })?;
        let _ = rebuilt;
        *self.sdk_server_names.write().await = self
            .sdk_user_mcp_server_config_map()
            .await
            .keys()
            .cloned()
            .collect();
        if was_running {
            self.reconcile_sdk_governance_inner().await?;
        }
        Ok(())
    }

    pub async fn add_or_update_server(&self, server: MCPServerConfig) -> Result<(), String> {
        let _guard = self.lifecycle_lock.lock().await;
        let name = server.name().to_string();
        self.computer
            .read()
            .await
            .add_or_update_server(normalize_mcp_server_tool_meta(server))
            .await
            .map_err(|error| error.to_string())?;
        self.sdk_server_names.write().await.insert(name);
        Ok(())
    }

    pub async fn add_or_update_plugin_server(
        &self,
        server: MCPServerConfig,
        managed_by: McpServerManagedBy,
    ) -> Result<(), String> {
        if !managed_by.is_plugin_owned() {
            return Err("Plugin MCP server must have plugin ownership metadata".to_string());
        }
        let _guard = self.lifecycle_lock.lock().await;
        let name = server.name().to_string();
        self.computer
            .read()
            .await
            .mount_server(normalize_mcp_server_tool_meta(server))
            .await
            .map_err(|error| error.to_string())?;
        self.sdk_server_names.write().await.insert(name.clone());
        self.plugin_server_owners
            .write()
            .await
            .insert(name, managed_by);
        Ok(())
    }

    pub async fn remove_server(&self, name: &str) -> Result<(), String> {
        let _guard = self.lifecycle_lock.lock().await;
        let was_running = *self.running.read().await;
        let computer = self.computer.read().await;
        let bundle_id = computer
            .list_mcp_servers_with_metadata()
            .await
            .into_iter()
            .find(|server| server.name == name)
            .map(|server| server.bundle_id)
            .ok_or_else(|| format!("MCP server not found: {name}"))?;
        computer
            .remove_server(&bundle_id)
            .await
            .map_err(|error| error.to_string())?;
        drop(computer);
        self.sdk_server_names.write().await.remove(name);
        self.plugin_server_owners.write().await.remove(name);
        if was_running {
            let remounted_server_names = self.reconcile_sdk_governance_inner().await?;
            self.start_mcp_servers_inner(&remounted_server_names)
                .await?;
        }
        Ok(())
    }

    pub async fn remove_plugin_server(&self, name: &str) -> Result<(), String> {
        let _guard = self.lifecycle_lock.lock().await;
        remove_tracked_plugin_server(
            name,
            &self.sdk_server_names,
            &self.plugin_server_owners,
            async {
                self.computer
                    .read()
                    .await
                    .unmount_server(name)
                    .await
                    .map_err(|error| error.to_string())
            },
        )
        .await
    }

    pub async fn mcp_server_statuses(&self) -> Vec<(String, bool, String)> {
        let _guard = self.lifecycle_lock.lock().await;
        self.computer.read().await.get_server_status().await
    }

    pub async fn plugin_mcp_server_owner(&self, name: &str) -> Option<McpServerManagedBy> {
        self.plugin_mcp_server_owner_inner(name).await
    }

    pub async fn start_mcp_server(&self, name: &str) -> Result<(), String> {
        let _guard = self.lifecycle_lock.lock().await;
        self.computer
            .read()
            .await
            .start_mcp_client(name)
            .await
            .map_err(|error| error.to_string())?;
        self.emit_sdk_tool_list_update_if_connected().await;
        Ok(())
    }

    pub async fn stop_mcp_server(&self, name: &str) -> Result<(), String> {
        let _guard = self.lifecycle_lock.lock().await;
        self.computer
            .read()
            .await
            .stop_mcp_client(name)
            .await
            .map_err(|error| error.to_string())?;
        self.emit_sdk_tool_list_update_if_connected().await;
        Ok(())
    }

    pub async fn remount_enabled_plugin_servers(&self) -> Result<(), String> {
        let _guard = self.lifecycle_lock.lock().await;
        self.reconcile_sdk_governance_inner().await.map(|_| ())
    }

    pub async fn start_all_mcp_servers(&self) -> Result<(), String> {
        self.start_mcp_server("all").await
    }

    pub async fn stop_all_mcp_servers(&self) -> Result<(), String> {
        self.stop_mcp_server("all").await
    }

    pub async fn connect_smcp_socketio(
        &self,
        url: &str,
        auth_payload: Option<serde_json::Value>,
        headers: HashMap<String, String>,
        namespace: Option<String>,
        office_id: &str,
        computer_name: &str,
    ) -> Result<(), String> {
        let _guard = self.lifecycle_lock.lock().await;
        self.set_state(ComputerRuntimeState::Connecting).await;
        if let Err(error) = self
            .connect_smcp_socketio_inner(
                url,
                auth_payload,
                headers,
                namespace,
                office_id,
                computer_name,
            )
            .await
        {
            self.set_state(ComputerRuntimeState::Error).await;
            return Err(error);
        }
        Ok(())
    }

    pub async fn disconnect_smcp_socketio(&self) -> Result<(), String> {
        let _guard = self.lifecycle_lock.lock().await;
        self.disconnect_smcp_socketio_inner().await
    }

    pub async fn clear_smcp_connection(&self) -> Result<(), String> {
        let _guard = self.lifecycle_lock.lock().await;
        self.clear_smcp_connection_inner().await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn reconnect_smcp_socketio_for_generation(
        &self,
        connection: &Arc<RwLock<Option<ConnectionState>>>,
        generation: u64,
        url: &str,
        auth_payload: Option<serde_json::Value>,
        headers: HashMap<String, String>,
        namespace: Option<String>,
        office_id: &str,
        computer_name: &str,
        expires_in: i64,
    ) -> Result<SmcpReconnectOutcome, String> {
        let _guard = self.lifecycle_lock.lock().await;
        {
            let guard = connection.read().await;
            match guard.as_ref() {
                Some(connection) if connection.generation == generation => {}
                _ => return Ok(SmcpReconnectOutcome::Stale),
            }
        }

        if let Err(error) = self.disconnect_smcp_socketio_inner().await {
            self.set_state(ComputerRuntimeState::Error).await;
            return Err(error);
        }

        self.set_state(ComputerRuntimeState::Connecting).await;
        if let Err(error) = self
            .connect_smcp_socketio_inner(
                url,
                auth_payload,
                headers,
                namespace,
                office_id,
                computer_name,
            )
            .await
        {
            self.set_state(ComputerRuntimeState::Error).await;
            return Err(error);
        }

        let mut guard = connection.write().await;
        match guard.as_mut() {
            Some(connection) if connection.generation == generation => {
                connection.connected_at = chrono::Utc::now();
                Ok(SmcpReconnectOutcome::Reconnected { expires_in })
            }
            _ => {
                if let Err(error) = self.disconnect_smcp_socketio_inner().await {
                    self.set_state(ComputerRuntimeState::Error).await;
                    return Err(error);
                }
                Ok(SmcpReconnectOutcome::Stale)
            }
        }
    }

    pub async fn set_refresh_task(&self, task: tokio::task::JoinHandle<()>) {
        let mut lock = self.refresh_task.lock().await;
        if let Some(previous) = lock.replace(task) {
            previous.abort();
        }
    }

    pub async fn abort_refresh_task(&self) {
        if let Some(task) = self.refresh_task.lock().await.take() {
            task.abort();
        }
    }

    pub async fn available_tools(&self) -> Result<Vec<Tool>, String> {
        self.computer
            .read()
            .await
            .get_available_tools()
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn resources(
        &self,
        server_name: &str,
        cursor: Option<String>,
    ) -> Result<(Vec<Resource>, Option<String>), String> {
        let _guard = self.lifecycle_lock.lock().await;
        self.computer
            .read()
            .await
            .get_resources(server_name, cursor)
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn desktop_windows(
        &self,
        window_uri: Option<&str>,
    ) -> Result<Vec<(String, Resource)>, String> {
        let _guard = self.lifecycle_lock.lock().await;
        self.computer
            .read()
            .await
            .list_all_windows(window_uri)
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn window_detail(
        &self,
        server_name: &str,
        resource: Resource,
    ) -> Result<ReadResourceResult, String> {
        let _guard = self.lifecycle_lock.lock().await;
        self.computer
            .read()
            .await
            .get_window_detail(server_name, resource)
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn execute_tool_cancellable(
        &self,
        req_id: &str,
        tool_name: &str,
        parameters: serde_json::Value,
        timeout: Option<f64>,
    ) -> Result<CallToolResult, String> {
        self.computer
            .read()
            .await
            .execute_tool_cancellable(req_id, tool_name, parameters, timeout)
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn sdk_tool_history(&self) -> Result<Vec<ToolCallRecord>, String> {
        self.computer
            .read()
            .await
            .get_tool_history()
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn resolve_input_value(&self, input_id: &str) -> Result<serde_json::Value, String> {
        let inputs = self.inputs.read().await;
        let input = inputs
            .get(input_id)
            .ok_or_else(|| format!("Input not found: {input_id}"))?;
        self.session
            .resolve_input(input)
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn add_or_update_input(&self, input: MCPServerInput) -> Result<(), String> {
        let _guard = self.lifecycle_lock.lock().await;
        let input_id = input.id().to_string();
        self.inputs.write().await.insert(input_id, input.clone());
        self.computer
            .read()
            .await
            .add_or_update_input(input)
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn synced_sdk_server_names(&self) -> HashSet<String> {
        self.sdk_server_names.read().await.clone()
    }

    pub async fn sdk_mcp_server_names(&self) -> HashSet<String> {
        self.sdk_mcp_server_config_map().await.into_keys().collect()
    }

    pub async fn sdk_mcp_server_configs(&self) -> HashMap<String, MCPServerConfig> {
        self.sdk_mcp_server_config_map().await
    }

    pub async fn sdk_skill_home(&self) -> PathBuf {
        self.computer.read().await.skill_home()
    }

    pub async fn sdk_governance_snapshot(
        &self,
    ) -> Result<a2c_smcp::smcp_computer::GovernanceSnapshot, String> {
        self.computer
            .read()
            .await
            .governance_snapshot()
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn sdk_skills(&self) -> Vec<A2CSkillRef> {
        self.computer.read().await.get_skills().await
    }

    pub async fn sdk_skill_ref(&self, name: &str) -> Option<A2CSkillRef> {
        self.computer.read().await.get_skill_ref(name).await
    }

    pub async fn sdk_read_skill_resource(
        &self,
        skill_ref: &A2CSkillRef,
        rel_path: Option<&str>,
    ) -> Result<SkillResourceView, SkillSandboxError> {
        self.computer
            .read()
            .await
            .read_skill_resource(skill_ref, rel_path)
    }

    pub async fn mark_sdk_skills_dirty(&self) {
        self.computer.read().await.mark_skills_dirty();
    }

    pub async fn sdk_add_marketplace(
        &self,
        git_url: &str,
        params: AddMarketplaceParams<'_>,
    ) -> Result<MarketplaceAddOutcome, GovernanceError> {
        self.computer
            .read()
            .await
            .add_marketplace(git_url, params)
            .await
    }

    pub async fn sdk_refresh_marketplace(&self, target: &str) -> Vec<MarketplaceRefreshRow> {
        self.computer.read().await.refresh_marketplace(target).await
    }

    pub async fn sdk_remove_marketplace(
        &self,
        name: &str,
        params: RemoveMarketplaceParams<'_>,
    ) -> Result<MarketplaceRemoveOutcome, GovernanceError> {
        self.computer
            .read()
            .await
            .remove_marketplace(name, params)
            .await
    }

    pub async fn sdk_install_plugin(
        &self,
        plugin_id: &str,
        options: InstallOptions<'_>,
        hooks: Option<&dyn McpInstallHooks>,
    ) -> Result<a2c_smcp::smcp_computer::settings::InstalledPluginRecord, PluginInstallError> {
        self.computer
            .read()
            .await
            .install_plugin(plugin_id, options, hooks)
            .await
    }

    pub async fn sdk_enable_plugin(
        &self,
        plugin_id: &str,
        options: EnableOptions<'_>,
        hooks: Option<&dyn McpInstallHooks>,
    ) -> Result<(), PluginInstallError> {
        self.computer
            .read()
            .await
            .enable_plugin(plugin_id, options, hooks)
            .await
    }

    pub async fn sdk_disable_plugin(
        &self,
        plugin_id: &str,
        options: DisableOptions<'_>,
        hooks: Option<&dyn McpInstallHooks>,
    ) -> Result<(), PluginInstallError> {
        self.computer
            .read()
            .await
            .disable_plugin(plugin_id, options, hooks)
            .await
    }

    pub async fn sdk_uninstall_plugin(
        &self,
        plugin_id: &str,
        options: UninstallOptions<'_>,
        hooks: Option<&dyn McpInstallHooks>,
    ) -> Result<bool, PluginInstallError> {
        self.computer
            .read()
            .await
            .uninstall_plugin(plugin_id, options, hooks)
            .await
    }

    pub async fn sdk_is_mcp_manager_initialized(&self) -> bool {
        self.computer
            .read()
            .await
            .is_mcp_manager_initialized()
            .await
    }

    async fn emit_sdk_tool_list_update_if_connected(&self) {
        let socketio_ref = self.computer.read().await.get_socketio_client();
        let client = socketio_ref.read().await.clone();
        if let Some(client) = client {
            if let Err(error) = client.emit_update_tool_list().await {
                log::warn!(
                    "Failed to emit MCP tool list update for instance {}: {}",
                    self.instance.id,
                    error
                );
            }
        }
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub async fn clone_sdk_socketio_client_for_test(
        &self,
    ) -> Option<Arc<a2c_smcp::smcp_computer::socketio_client::SmcpComputerClient>> {
        let socketio_ref = self.computer.read().await.get_socketio_client();
        let client = socketio_ref.read().await.clone();
        client
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub async fn register_sdk_skill_ref_for_test(&self, skill_ref: A2CSkillRef) {
        let registry = self.computer.read().await.skill_registry_arc();
        registry.write().await.register(skill_ref);
    }

    pub async fn is_connected(&self) -> bool {
        self.runtime_state().await == ComputerRuntimeState::JoinedOffice
            && self.connection.read().await.is_some()
    }

    pub async fn connection_status(&self) -> Option<ConnectionStateSummary> {
        if self.runtime_state().await != ComputerRuntimeState::JoinedOffice {
            return None;
        }
        self.connection
            .read()
            .await
            .as_ref()
            .map(ConnectionStateSummary::from)
    }

    async fn sdk_requires_rebuild(&self) -> bool {
        let computer = self.computer.read().await;
        if computer.name() != self.instance.name.as_str() {
            return true;
        }
        if *self.sdk_auto_connect.read().await != self.instance.connection_policy.auto_connect {
            return true;
        }
        if computer.skill_home()
            != self.instance.local_skills_root.clone().unwrap_or_else(|| {
                default_local_skills_root(&self.skill_home_base, &self.instance.id)
            })
        {
            return true;
        }

        false
    }

    async fn connect_smcp_socketio_inner(
        &self,
        url: &str,
        auth_payload: Option<serde_json::Value>,
        headers: HashMap<String, String>,
        namespace: Option<String>,
        office_id: &str,
        computer_name: &str,
    ) -> Result<(), String> {
        let options = ConnectOptions {
            auth_payload,
            headers: headers_to_connect_options(headers),
            namespace: namespace.unwrap_or_default(),
        };
        let options = if options.namespace.trim().is_empty() {
            ConnectOptions {
                namespace: ConnectOptions::default().namespace,
                ..options
            }
        } else {
            options
        };

        if let Err(error) = self
            .computer
            .read()
            .await
            .connect_socketio(url, options)
            .await
        {
            return Err(error.to_string());
        }
        self.set_state(ComputerRuntimeState::Connected).await;

        if let Err(error) = self
            .computer
            .read()
            .await
            .join_office(office_id, computer_name)
            .await
        {
            if let Err(disconnect_error) = self.disconnect_smcp_socketio_inner().await {
                log::warn!(
                    "Failed to disconnect Socket.IO after join_office failure for instance {}: {}",
                    self.instance.id,
                    disconnect_error
                );
            }
            return Err(error.to_string());
        }
        self.set_state(ComputerRuntimeState::JoinedOffice).await;
        Ok(())
    }

    async fn disconnect_smcp_socketio_inner(&self) -> Result<(), String> {
        if let Err(error) = self.computer.read().await.leave_office().await {
            log::warn!(
                "Failed to leave SMCP office for instance {}: {}",
                self.instance.id,
                error
            );
        }
        let socketio_ref = self.computer.read().await.get_socketio_client();
        let client = {
            let mut guard = socketio_ref.write().await;
            guard.take()
        };
        if let Some(client) = client {
            if Arc::strong_count(&client) > 1 {
                let mut guard = socketio_ref.write().await;
                *guard = Some(client);
                return Err(format!(
                    "SMCP socket for instance {} still has shared references; cannot confirm disconnect",
                    self.instance.id
                ));
            }
            match Arc::try_unwrap(client) {
                Ok(client) => {
                    client
                        .disconnect()
                        .await
                        .map_err(|error| error.to_string())?;
                }
                Err(_) => {
                    return Err(format!(
                        "SMCP socket for instance {} still has shared references; cannot disconnect",
                        self.instance.id
                    ));
                }
            }
        }
        self.set_state(ComputerRuntimeState::Booted).await;
        Ok(())
    }

    async fn clear_smcp_connection_inner(&self) -> Result<(), String> {
        match self.disconnect_smcp_socketio_inner().await {
            Ok(()) => {
                self.abort_refresh_task().await;
                let mut connection = self.connection.write().await;
                connection.take();
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    async fn sdk_mcp_server_config_map(&self) -> HashMap<String, MCPServerConfig> {
        self.computer
            .read()
            .await
            .list_mcp_servers()
            .await
            .iter()
            .map(|server| (server.name().to_string(), server.clone()))
            .collect()
    }

    async fn sdk_user_mcp_server_config_map(&self) -> HashMap<String, MCPServerConfig> {
        let plugin_names: HashSet<String> = self
            .sdk_mcp_server_ownership()
            .await
            .into_iter()
            .filter(|entry| matches!(entry.managed_by, McpOwnership::Plugin { .. }))
            .map(|entry| entry.name)
            .collect();
        self.sdk_mcp_server_config_map()
            .await
            .into_iter()
            .filter(|(name, _)| !plugin_names.contains(name))
            .collect()
    }

    pub async fn sdk_mcp_server_ownership(&self) -> Vec<McpServerWithMetadata> {
        let tracked_owners = self.plugin_server_owners.read().await.clone();
        let computer = self.computer.read().await;
        let materialized_names: HashSet<String> = computer
            .list_mcp_servers()
            .await
            .into_iter()
            .map(|server| server.name().to_string())
            .collect();
        let mut entries = computer.list_mcp_servers_with_metadata().await;
        drop(computer);
        for entry in &mut entries {
            if let Some(managed_by) = tracked_owners.get(&entry.name) {
                entry.managed_by = client_managed_by_to_sdk(managed_by);
            } else if materialized_names.contains(&entry.name) {
                // SDK inventory intentionally joins plugin ownership by server name. The client
                // tracks actual runtime mounts so a user server that wins an enable conflict is
                // not mislabeled as plugin-owned merely because the plugin ledger is enabled.
                entry.managed_by = McpOwnership::User;
            }
        }
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        entries
    }

    /// Count the complete SDK MCP inventory for this Computer instance.
    ///
    /// The inventory includes user-configured servers and MCP servers contributed by enabled
    /// plugins. Runtime status is intentionally tracked separately from this configuration view.
    pub async fn mcp_server_inventory_count(&self) -> usize {
        let config_context = instance_config_context(&self.instance, &self.skill_home_base);
        let mut server_names: HashSet<String> = config_context
            .load()
            .mcp
            .servers
            .into_iter()
            .map(|server| server.name)
            .collect();
        server_names.extend(
            self.sdk_mcp_server_ownership()
                .await
                .into_iter()
                .filter(|server| matches!(server.managed_by, McpOwnership::Plugin { .. }))
                .map(|server| server.name),
        );
        server_names.len()
    }

    async fn plugin_mcp_server_owner_inner(&self, name: &str) -> Option<McpServerManagedBy> {
        self.sdk_mcp_server_ownership()
            .await
            .into_iter()
            .find(|entry| entry.name == name)
            .and_then(|entry| sdk_managed_by_to_client(entry.managed_by))
            .filter(McpServerManagedBy::is_plugin_owned)
    }

    async fn reconcile_sdk_governance_inner(&self) -> Result<Vec<String>, String> {
        let config_context = instance_config_context(&self.instance, &self.skill_home_base);
        let declared = resolve_instance_settings(&config_context);
        let existing_server_names = self.sdk_server_names.read().await.clone();
        let hooks = RuntimeMcpHooks::new(self, existing_server_names).await?;
        let report = self
            .computer
            .read()
            .await
            .reconcile_governance(Some(&hooks), Some(&declared))
            .await;
        for marketplace in report.failed_marketplaces {
            log::warn!(
                "Marketplace '{}' degraded during SDK governance recovery for instance '{}'",
                marketplace,
                self.instance.id
            );
        }
        Ok(hooks.registered_server_names().await)
    }

    async fn start_mcp_servers_inner(&self, names: &[String]) -> Result<(), String> {
        for name in names {
            if let Err(error) = self.computer.read().await.start_mcp_client(name).await {
                self.emit_sdk_tool_list_update_if_connected().await;
                return Err(error.to_string());
            }
        }
        if !names.is_empty() {
            self.emit_sdk_tool_list_update_if_connected().await;
        }
        Ok(())
    }

    async fn replace_sdk_computer(&self, was_running: bool) -> Result<(), String> {
        let inputs = input_definitions_to_mcp_map(&self.instance.inputs);
        let (new_computer, sdk_server_names) = build_sdk_computer(
            &self.instance,
            &inputs,
            self.session.clone(),
            &self.skill_home_base,
        );

        if was_running {
            if let Err(error) = new_computer.boot_up().await {
                self.set_state(ComputerRuntimeState::Error).await;
                return Err(format!(
                    "Failed to boot replacement SDK Computer for instance {}: {}",
                    self.instance.id, error
                ));
            }
        }

        if self.connection.read().await.is_some() {
            if let Err(error) = self.clear_smcp_connection_inner().await {
                log::warn!(
                    "Failed to clear SMCP connection before rebuilding SDK Computer for instance {}: {}",
                    self.instance.id,
                    error
                );
            }
        }

        let mut computer = self.computer.write().await;
        if let Err(error) = computer.shutdown().await {
            log::warn!(
                "Failed to shutdown stale SDK Computer for instance {}: {}",
                self.instance.id,
                error
            );
        }
        *computer = new_computer;
        *self.sdk_server_names.write().await = sdk_server_names;
        self.plugin_server_owners.write().await.clear();
        *self.sdk_auto_connect.write().await = self.instance.connection_policy.auto_connect;

        if was_running {
            self.set_state(ComputerRuntimeState::Booted).await;
        }
        Ok(())
    }

    async fn set_state(&self, state: ComputerRuntimeState) {
        *self.state.write().await = state;
    }
}

async fn remove_tracked_plugin_server<F>(
    name: &str,
    sdk_server_names: &Arc<RwLock<HashSet<String>>>,
    plugin_server_owners: &Arc<RwLock<HashMap<String, McpServerManagedBy>>>,
    unmount: F,
) -> Result<(), String>
where
    F: Future<Output = Result<(), String>>,
{
    if !plugin_server_owners.read().await.contains_key(name) {
        return Ok(());
    }

    // Preserve ownership until the SDK confirms the runtime side was removed.
    // A failed unmount must remain observable and retryable by the caller.
    unmount.await?;
    plugin_server_owners.write().await.remove(name);
    sdk_server_names.write().await.remove(name);
    Ok(())
}

fn build_sdk_computer(
    instance: &ComputerInstance,
    inputs: &HashMap<String, MCPServerInput>,
    session: InstanceSession,
    skill_home_base: &Path,
) -> (Computer<InstanceSession>, HashSet<String>) {
    let instance_storage_root = skill_home_base.join(instance_storage_dir_name(&instance.id));
    let config_context = instance_config_context(instance, skill_home_base);
    let skill_home = config_context.skill_home().to_path_buf();
    let mcp_servers: HashMap<String, MCPServerConfig> = config_context
        .load()
        .mcp
        .servers
        .into_iter()
        .map(|server| (server.name, normalize_mcp_server_tool_meta(server.config)))
        .collect();
    let sdk_server_names = mcp_servers.keys().cloned().collect();
    let computer = Computer::new(
        instance.name.clone(),
        session,
        Some(inputs.clone()),
        Some(mcp_servers),
        instance.connection_policy.auto_connect,
        true,
    );

    let computer = computer
        .with_skill_home(skill_home)
        .with_config_dir(config_context.project_anchor())
        .with_config_env(config_context.env().clone())
        .with_blob_cache_root(instance_storage_root.join("blob"));
    (computer, sdk_server_names)
}

struct RuntimeMcpHooks {
    computer: Arc<RwLock<Computer<InstanceSession>>>,
    inputs: Arc<RwLock<HashMap<String, MCPServerInput>>>,
    sdk_server_names: Arc<RwLock<HashSet<String>>>,
    plugin_server_owners: Arc<RwLock<HashMap<String, McpServerManagedBy>>>,
    existing_server_names: HashSet<String>,
    bundled_owners: HashMap<String, McpServerManagedBy>,
    root_ownership: HashMap<PathBuf, (String, String)>,
    registered_server_names: Arc<Mutex<Vec<String>>>,
}

impl RuntimeMcpHooks {
    async fn new(
        runtime: &ComputerInstanceRuntime,
        existing_server_names: HashSet<String>,
    ) -> Result<Self, String> {
        let snapshot = runtime
            .computer
            .read()
            .await
            .governance_snapshot()
            .await
            .map_err(|error| format!("Failed to load SDK governance snapshot: {error}"))?;
        let mut root_ownership = HashMap::new();
        let mut bundled_owners = HashMap::new();
        for plugin in snapshot
            .plugins
            .into_iter()
            .filter(|plugin| plugin.installed && plugin.enabled)
        {
            if let Some(install_path) = plugin.install_path.as_ref() {
                root_ownership.insert(
                    PathBuf::from(install_path),
                    (plugin.plugin.clone(), plugin.marketplace.clone()),
                );
            }
            for server_name in plugin.bundled_mcp_servers {
                bundled_owners.insert(
                    server_name,
                    McpServerManagedBy::Plugin {
                        marketplace: plugin.marketplace.clone(),
                        plugin: plugin.plugin.clone(),
                        plugin_id: Some(plugin.id.clone()),
                    },
                );
            }
        }
        Ok(Self {
            computer: runtime.computer.clone(),
            inputs: runtime.inputs.clone(),
            sdk_server_names: runtime.sdk_server_names.clone(),
            plugin_server_owners: runtime.plugin_server_owners.clone(),
            existing_server_names,
            bundled_owners,
            root_ownership,
            registered_server_names: Arc::new(Mutex::new(Vec::new())),
        })
    }

    async fn registered_server_names(&self) -> Vec<String> {
        self.registered_server_names.lock().await.clone()
    }
}

#[async_trait]
impl McpInstallHooks for RuntimeMcpHooks {
    fn existing_server_names(&self) -> HashSet<String> {
        self.existing_server_names.clone()
    }

    async fn register_server(&self, cfg: MCPServerConfig) -> Result<(), McpHookError> {
        let name = cfg.name().to_string();
        let owner = self.bundled_owners.get(&name).cloned().ok_or_else(|| {
            McpHookError(format!(
                "Missing plugin ownership metadata for bundled MCP server '{name}'"
            ))
        })?;
        self.computer
            .read()
            .await
            .mount_server(normalize_mcp_server_tool_meta(cfg))
            .await
            .map_err(|error| McpHookError(error.to_string()))?;
        self.sdk_server_names.write().await.insert(name.clone());
        self.plugin_server_owners
            .write()
            .await
            .insert(name.clone(), owner);
        self.registered_server_names.lock().await.push(name);
        Ok(())
    }

    async fn remove_server(&self, name: &str) -> Result<(), McpHookError> {
        remove_tracked_plugin_server(
            name,
            &self.sdk_server_names,
            &self.plugin_server_owners,
            async {
                self.computer
                    .read()
                    .await
                    .unmount_server(name)
                    .await
                    .map_err(|error| error.to_string())
            },
        )
        .await
        .map_err(McpHookError)
    }

    async fn inject_inputs(&self, plugin_root: &Path) -> Result<(), McpHookError> {
        let Some((plugin, marketplace)) = self.root_ownership.get(plugin_root) else {
            return Ok(());
        };
        let inputs_json = plugin_root
            .join(MCP_SERVERS_SUBDIR)
            .join(MCP_INPUTS_FILENAME);
        let inputs = load_plugin_inputs(&inputs_json, plugin, marketplace);
        if inputs.is_empty() {
            return Ok(());
        }

        let mut current = self.inputs.write().await;
        for input in inputs {
            current.insert(input.id().to_string(), input.clone());
            self.computer
                .read()
                .await
                .add_or_update_input(input)
                .await
                .map_err(|error| McpHookError(error.to_string()))?;
        }
        Ok(())
    }
}

fn instance_config_context(
    instance: &ComputerInstance,
    skill_home_base: &Path,
) -> InstanceConfigContext {
    let instance_storage_root = skill_home_base.join(instance_storage_dir_name(&instance.id));
    let skill_home = instance
        .local_skills_root
        .clone()
        .unwrap_or_else(|| default_local_skills_root(skill_home_base, &instance.id));
    InstanceConfigContext::new(instance_storage_root.join("sdk_config"), skill_home)
}

fn resolve_instance_settings(
    context: &InstanceConfigContext,
) -> serde_json::Map<String, serde_json::Value> {
    let policy = resolve_policy_settings(Some(context.env()), None, None);
    resolve_settings(ResolveSettingsArgs {
        cwd: Some(context.project_anchor()),
        env: Some(context.env()),
        flag_settings_path: None,
        policy_settings: Some(&policy),
    })
    .settings
}

pub(crate) fn sdk_managed_by_to_client(managed_by: McpOwnership) -> Option<McpServerManagedBy> {
    match managed_by {
        McpOwnership::User => Some(McpServerManagedBy::User),
        McpOwnership::Plugin {
            marketplace,
            plugin,
            plugin_id,
        } => Some(McpServerManagedBy::Plugin {
            marketplace,
            plugin,
            plugin_id: Some(plugin_id),
        }),
    }
}

fn client_managed_by_to_sdk(managed_by: &McpServerManagedBy) -> McpOwnership {
    match managed_by {
        McpServerManagedBy::User => McpOwnership::User,
        McpServerManagedBy::Plugin {
            marketplace,
            plugin,
            plugin_id,
        } => McpOwnership::Plugin {
            marketplace: marketplace.clone(),
            plugin: plugin.clone(),
            plugin_id: plugin_id
                .clone()
                .unwrap_or_else(|| format!("{plugin}@{marketplace}")),
        },
    }
}

fn default_local_skills_root(skill_home_base: &Path, instance_id: &str) -> PathBuf {
    skill_home_base
        .join(instance_storage_dir_name(instance_id))
        .join("skill_home")
}

fn normalize_mcp_server_tool_meta(mut config: MCPServerConfig) -> MCPServerConfig {
    match &mut config {
        MCPServerConfig::Stdio(server) => {
            normalize_default_tool_meta(&mut server.default_tool_meta);
            normalize_tool_meta_map(&mut server.tool_meta);
        }
        MCPServerConfig::Http(server) => {
            normalize_default_tool_meta(&mut server.default_tool_meta);
            normalize_tool_meta_map(&mut server.tool_meta);
        }
        MCPServerConfig::Sse(server) => {
            normalize_default_tool_meta(&mut server.default_tool_meta);
            normalize_tool_meta_map(&mut server.tool_meta);
        }
    }
    config
}

fn normalize_default_tool_meta(meta: &mut Option<ToolMeta>) {
    if let Some(value) = meta {
        normalize_tool_meta(value);
        if is_empty_tool_meta(value) {
            *meta = None;
        }
    }
}

fn normalize_tool_meta_map(tool_meta: &mut HashMap<String, ToolMeta>) {
    tool_meta.retain(|_, meta| {
        normalize_tool_meta(meta);
        !is_empty_tool_meta(meta)
    });
}

fn normalize_tool_meta(meta: &mut ToolMeta) {
    if meta
        .alias
        .as_ref()
        .is_some_and(|alias| alias.trim().is_empty())
    {
        meta.alias = None;
    }
    if let Some(tags) = &mut meta.tags {
        tags.retain(|tag| !tag.trim().is_empty());
        if tags.is_empty() {
            meta.tags = None;
        }
    }
}

fn is_empty_tool_meta(meta: &ToolMeta) -> bool {
    meta.auto_apply.is_none()
        && meta.alias.is_none()
        && meta.tags.is_none()
        && meta.ret_object_mapper.is_none()
}

fn headers_to_connect_options(headers: HashMap<String, String>) -> Option<String> {
    if headers.is_empty() {
        return None;
    }
    let mut pairs = headers.into_iter().collect::<Vec<_>>();
    pairs.sort_by(|(left, _), (right, _)| left.cmp(right));
    Some(
        pairs
            .into_iter()
            .map(|(key, value)| format!("{key}:{value}"))
            .collect::<Vec<_>>()
            .join(","),
    )
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
            id,
            label,
            command,
            args,
        } => MCPServerInput::Command(CommandInput {
            id: id.clone(),
            description: label.clone(),
            command: command.clone(),
            args: command_args_to_mcp(args),
        }),
    }
}

fn command_args_to_mcp(args: &Option<Vec<String>>) -> Option<HashMap<String, String>> {
    args.as_ref().map(|args| {
        args.iter()
            .enumerate()
            .map(|(index, value)| (format!("{index:06}"), value.clone()))
            .collect()
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConnectionStateSummary {
    pub url: String,
    pub office_id: String,
    pub computer_name: String,
    pub connected_at: String,
    pub profile_name: String,
    pub source_type: String,
    pub target_id: Option<String>,
    pub target_name: Option<String>,
    pub employee_id: Option<u64>,
}

impl From<&ConnectionState> for ConnectionStateSummary {
    fn from(connection: &ConnectionState) -> Self {
        Self {
            url: connection.url.clone(),
            office_id: connection.office_id.clone(),
            computer_name: connection.computer_name.clone(),
            connected_at: connection.connected_at.to_rfc3339(),
            profile_name: connection.profile_name.clone(),
            source_type: connection.source_type.clone(),
            target_id: connection.target_id.clone(),
            target_name: connection.target_name.clone(),
            employee_id: connection.employee_id,
        }
    }
}

pub struct ComputerRegistry {
    runtimes: RwLock<HashMap<ComputerInstanceId, ComputerInstanceRuntime>>,
    skill_home_base: PathBuf,
}

impl ComputerRegistry {
    pub fn from_config(config: ComputerInstancesConfig) -> Self {
        Self::from_config_with_skill_home_base(config, default_skill_home_base())
    }

    pub fn from_config_with_skill_home_base(
        config: ComputerInstancesConfig,
        skill_home_base: PathBuf,
    ) -> Self {
        let (registry, _) =
            Self::from_config_with_initial_runtime_and_skill_home_base(config, skill_home_base);
        registry
    }

    pub fn from_config_with_initial_runtime(
        config: ComputerInstancesConfig,
    ) -> (Self, Option<ComputerInstanceRuntime>) {
        Self::from_config_with_initial_runtime_and_skill_home_base(
            config,
            default_skill_home_base(),
        )
    }

    pub fn from_config_with_initial_runtime_and_skill_home_base(
        mut config: ComputerInstancesConfig,
        skill_home_base: PathBuf,
    ) -> (Self, Option<ComputerInstanceRuntime>) {
        config.normalize();
        let mut runtimes = HashMap::new();

        for instance in config.instances {
            let instance_id = instance.id.clone();
            let runtime = ComputerInstanceRuntime::new(instance, skill_home_base.clone());
            runtimes.insert(instance_id, runtime);
        }

        let initial_runtime = runtimes.values().next().cloned();
        let registry = Self {
            runtimes: RwLock::new(runtimes),
            skill_home_base,
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
        let runtime = ComputerInstanceRuntime::new(instance.clone(), self.skill_home_base.clone());
        let mut runtimes = self.runtimes.write().await;
        runtimes.insert(instance.id, runtime.clone());
        runtime
    }

    pub async fn update_runtime_instance(
        &self,
        instance: ComputerInstance,
    ) -> Result<ComputerInstanceRuntime, String> {
        let previous = self.runtime(&instance.id).await;
        let runtime = {
            let mut runtimes = self.runtimes.write().await;
            let runtime = match runtimes.get(&instance.id) {
                Some(existing) => existing.with_instance(instance.clone()),
                None => {
                    ComputerInstanceRuntime::new(instance.clone(), self.skill_home_base.clone())
                }
            };
            runtimes.insert(instance.id, runtime.clone());
            runtime
        };
        if let Err(error) = runtime.sync_runtime().await {
            if let Some(previous_runtime) = previous {
                let restore_runtime =
                    previous_runtime.with_instance(previous_runtime.instance.clone());
                let mut runtimes = self.runtimes.write().await;
                runtimes.insert(restore_runtime.instance.id.clone(), restore_runtime.clone());
                drop(runtimes);
                if let Err(restore_error) = restore_runtime.sync_runtime().await {
                    return Err(format!(
                        "{error}; additionally failed to restore previous runtime: {restore_error}"
                    ));
                }
            } else {
                let mut runtimes = self.runtimes.write().await;
                runtimes.remove(&runtime.instance.id);
            }
            return Err(error);
        }
        Ok(runtime)
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

fn default_skill_home_base() -> PathBuf {
    std::env::temp_dir()
        .join("tfrobot-client")
        .join("computer_instances")
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

    fn instance_with_input_value(id: &str, value: serde_json::Value) -> ComputerInstance {
        let mut instance = instance_with_input(id, "API Key");
        instance.input_values.insert("api-key".to_string(), value);
        instance
    }

    fn server_config(name: &str) -> MCPServerConfig {
        serde_json::from_value(serde_json::json!({
            "type": "Stdio",
            "name": name,
            "server_parameters": {
                "command": "node",
                "args": ["server.js"],
                "env": {}
            }
        }))
        .unwrap()
    }

    fn disabled_server_config(name: &str) -> MCPServerConfig {
        serde_json::from_value(serde_json::json!({
            "type": "Stdio",
            "name": name,
            "disabled": true,
            "server_parameters": {
                "command": "node",
                "args": ["server.js"],
                "env": {}
            }
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn failed_plugin_server_unmount_preserves_tracking_for_retry() {
        let sdk_server_names = Arc::new(RwLock::new(HashSet::from(["plugin-mcp".to_string()])));
        let plugin_server_owners = Arc::new(RwLock::new(HashMap::from([(
            "plugin-mcp".to_string(),
            McpServerManagedBy::Plugin {
                marketplace: "official".to_string(),
                plugin: "audit".to_string(),
                plugin_id: Some("official:audit".to_string()),
            },
        )])));

        let error = remove_tracked_plugin_server(
            "plugin-mcp",
            &sdk_server_names,
            &plugin_server_owners,
            async { Err("injected unmount failure".to_string()) },
        )
        .await
        .unwrap_err();

        assert_eq!(error, "injected unmount failure");
        assert!(sdk_server_names.read().await.contains("plugin-mcp"));
        assert!(plugin_server_owners.read().await.contains_key("plugin-mcp"));

        remove_tracked_plugin_server(
            "plugin-mcp",
            &sdk_server_names,
            &plugin_server_owners,
            async { Ok(()) },
        )
        .await
        .unwrap();

        assert!(!sdk_server_names.read().await.contains("plugin-mcp"));
        assert!(!plugin_server_owners.read().await.contains_key("plugin-mcp"));
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
        assert!(!Arc::ptr_eq(&one.inputs, &two.inputs));
        assert!(!Arc::ptr_eq(&one.computer, &two.computer));
        assert!(!Arc::ptr_eq(&one.connection, &two.connection));
        assert!(!Arc::ptr_eq(&one.state, &two.state));
        assert!(!Arc::ptr_eq(&one.lifecycle_lock, &two.lifecycle_lock));
        assert_eq!(one.runtime_state().await, ComputerRuntimeState::Created);
        assert_eq!(two.runtime_state().await, ComputerRuntimeState::Created);
    }

    #[tokio::test]
    async fn runtime_builds_sdk_computer_with_instance_skill_home() {
        let skill_home_base = std::env::temp_dir().join("tfrobot-client-test-skill-home");
        let runtime =
            ComputerInstanceRuntime::new(instance("instance/one", "One"), skill_home_base.clone());

        assert_eq!(
            runtime.sdk_skill_home().await,
            default_local_skills_root(&skill_home_base, "instance/one")
        );
    }

    #[tokio::test]
    async fn sdk_config_writes_are_scoped_to_the_computer_instance() {
        let temp = tempfile::tempdir().unwrap();
        let skill_home_base = temp.path().join("computer_instances");
        let runtime =
            ComputerInstanceRuntime::new(instance("computer-a", "One"), skill_home_base.clone());

        runtime
            .add_or_update_server(disabled_server_config("isolated-server"))
            .await
            .unwrap();

        let instance_config = skill_home_base
            .join("computer-a")
            .join("sdk_config")
            .join(".tfrobot")
            .join("mcp.local.json");
        assert!(instance_config.exists());
        let persisted = std::fs::read_to_string(instance_config).unwrap();
        assert!(persisted.contains("isolated-server"));
        assert!(!temp.path().join(".tfrobot").exists());

        let restarted =
            ComputerInstanceRuntime::new(instance("computer-a", "One"), skill_home_base);
        assert!(restarted
            .sdk_mcp_server_names()
            .await
            .contains("isolated-server"));
        restarted.start().await.unwrap();
        assert!(restarted
            .mcp_server_statuses()
            .await
            .iter()
            .any(|(name, _, _)| name == "isolated-server"));
    }

    #[tokio::test]
    async fn remove_server_resolves_explicit_bundle_id_from_sdk_inventory() {
        let temp = tempfile::tempdir().unwrap();
        let skill_home_base = temp.path().join("computer_instances");
        let runtime =
            ComputerInstanceRuntime::new(instance("computer-a", "One"), skill_home_base.clone());
        let config: MCPServerConfig = serde_json::from_value(serde_json::json!({
            "type": "Stdio",
            "name": "display-name",
            "bundle_id": "stable-bundle-id",
            "server_parameters": {
                "command": "node",
                "args": ["server.js"],
                "env": {}
            }
        }))
        .unwrap();

        runtime.add_or_update_server(config).await.unwrap();
        runtime.remove_server("display-name").await.unwrap();

        assert!(!runtime
            .sdk_mcp_server_names()
            .await
            .contains("display-name"));
        let restarted =
            ComputerInstanceRuntime::new(instance("computer-a", "One"), skill_home_base);
        assert!(!restarted
            .sdk_mcp_server_names()
            .await
            .contains("display-name"));
    }

    #[tokio::test]
    async fn runtime_uses_custom_local_skills_root_when_configured() {
        let custom_root = std::env::temp_dir().join("tfrobot-client-custom-skills");
        let mut instance = instance("one", "One");
        instance.local_skills_root = Some(custom_root.clone());

        let runtime = ComputerInstanceRuntime::new(
            instance,
            std::env::temp_dir().join("tfrobot-client-test-skill-home"),
        );

        assert_eq!(runtime.sdk_skill_home().await, custom_root);
    }

    #[tokio::test]
    async fn runtime_seeds_smcp_input_definitions_from_instance() {
        let runtime = ComputerInstanceRuntime::new(
            instance_with_input("one", "API Key"),
            std::env::temp_dir().join("tfrobot-client-test-skill-home"),
        );

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
    async fn instance_session_resolves_values_from_current_instance() {
        let runtime = ComputerInstanceRuntime::new(
            instance_with_input_value("one", serde_json::json!("persisted-secret")),
            std::env::temp_dir().join("tfrobot-client-test-skill-home"),
        );
        let inputs = runtime.inputs.read().await;
        let input = inputs.get("api-key").unwrap();

        assert_eq!(
            runtime.session.resolve_input(input).await.unwrap(),
            serde_json::json!("persisted-secret")
        );
    }

    #[tokio::test]
    async fn instance_session_falls_back_to_sdk_input_semantics() {
        let session = InstanceSession::new("one", HashMap::new());
        let pick = MCPServerInput::PickString(PickStringInput {
            id: "runtime".to_string(),
            description: "Runtime".to_string(),
            options: vec!["node".to_string(), "python".to_string()],
            default: None,
        });
        let command = MCPServerInput::Command(CommandInput {
            id: "command".to_string(),
            description: "Command".to_string(),
            command: "echo".to_string(),
            args: command_args_to_mcp(&Some(vec!["hello".to_string(), "world".to_string()])),
        });

        assert_eq!(
            session.resolve_input(&pick).await.unwrap(),
            serde_json::json!("node")
        );
        assert_eq!(
            session.resolve_input(&command).await.unwrap(),
            serde_json::json!("hello world")
        );
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
        let one = registry.runtime("one").await.unwrap();
        assert!(one.is_running().await);
        assert!(one.sdk_is_mcp_manager_initialized().await);
        assert_eq!(one.runtime_state().await, ComputerRuntimeState::Booted);
        assert!(!registry.runtime("two").await.unwrap().is_running().await);

        registry.stop_runtime("one").await.unwrap();
        let one = registry.runtime("one").await.unwrap();
        assert!(!one.is_running().await);
        assert!(!one.sdk_is_mcp_manager_initialized().await);
        assert_eq!(one.runtime_state().await, ComputerRuntimeState::Stopped);
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
        let after = registry.update_runtime_instance(updated).await.unwrap();

        assert_eq!(after.instance.name, "Renamed");
        assert!(after.is_running().await);
        assert!(Arc::ptr_eq(&before.inputs, &after.inputs));
        assert!(Arc::ptr_eq(&before.computer, &after.computer));
        assert!(Arc::ptr_eq(&before.connection, &after.connection));
        assert!(Arc::ptr_eq(&before.state, &after.state));
        assert!(Arc::ptr_eq(&before.lifecycle_lock, &after.lifecycle_lock));
        let inputs = after.inputs.read().await;
        let input = inputs.get("api-key").expect("input should be synced");
        assert!(matches!(
            input,
            MCPServerInput::PromptString(prompt) if prompt.description == "Updated Label"
        ));
        assert_eq!(
            after.session.resolve_input(input).await.unwrap(),
            serde_json::json!("default-value")
        );
        assert_eq!(
            after.sdk_skill_home().await,
            after.skill_home_base.join("one").join("skill_home")
        );
    }

    #[tokio::test]
    async fn runtime_rebuild_restores_default_skill_home_when_override_is_cleared() {
        let registry = ComputerRegistry::from_config_with_skill_home_base(
            ComputerInstancesConfig {
                schema_version: 1,
                instances: vec![{
                    let mut instance = instance("one", "One");
                    instance.local_skills_root = Some(PathBuf::from("/tmp/custom-skill-home"));
                    instance
                }],
            },
            std::env::temp_dir().join("tfrobot-client-test-skill-home-clear"),
        );

        let mut updated = instance("one", "One");
        updated.local_skills_root = None;
        let runtime = registry.update_runtime_instance(updated).await.unwrap();

        assert_eq!(
            runtime.sdk_skill_home().await,
            runtime.skill_home_base.join("one").join("skill_home")
        );
    }

    #[tokio::test]
    async fn runtime_exposes_sdk_skill_registry_and_resource_access() {
        let skill_home_base = tempfile::TempDir::new().unwrap();
        let runtime =
            ComputerInstanceRuntime::new(instance("one", "One"), skill_home_base.path().into());
        let skill_home = runtime.sdk_skill_home().await;
        let skill_dir = skill_home.join("user").join("example-skill");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: example-skill\ndescription: Example skill\n---\nBody\n",
        )
        .unwrap();

        runtime.start().await.unwrap();

        let skills = runtime.sdk_skills().await;
        let skill_ref = skills
            .iter()
            .find(|skill| skill.name == "example-skill")
            .cloned()
            .expect("example skill should be staged by SDK Computer");
        assert_eq!(skill_ref.source, "user");
        assert_eq!(
            runtime.sdk_skill_ref("example-skill").await,
            Some(skill_ref.clone())
        );
        let view = runtime
            .sdk_read_skill_resource(&skill_ref, None)
            .await
            .unwrap();
        assert_eq!(
            String::from_utf8(view.read_all().unwrap()).unwrap(),
            "Body\n"
        );

        runtime.mark_sdk_skills_dirty().await;
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn update_runtime_instance_does_not_import_legacy_profile_mcp_servers() {
        let registry = ComputerRegistry::from_config(ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![instance("one", "Initial")],
        });

        registry.start_runtime("one").await.unwrap();
        let before = registry.runtime("one").await.unwrap();
        assert!(before.sdk_mcp_server_names().await.is_empty());

        let mut updated = instance("one", "Updated");
        updated.mcp_servers = vec![server_config("updated-server").into()];
        updated.connection_policy.auto_connect = true;
        let after = registry.update_runtime_instance(updated).await.unwrap();

        assert!(Arc::ptr_eq(&before.computer, &after.computer));
        assert!(after.is_running().await);
        assert!(after.sdk_is_mcp_manager_initialized().await);
        assert_eq!(after.computer.read().await.name(), "Updated");
        assert!(after.sdk_mcp_server_names().await.is_empty());
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
