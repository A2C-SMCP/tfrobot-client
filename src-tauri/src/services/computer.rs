use crate::commands::connection::ConnectionState;
use crate::commands::inputs::{InputDefinition, PickOption};
use crate::services::client_control::{
    client_control_server_config, ClientControlBinding, ClientControlMcpClient,
    RemoteControlPolicy, CLIENT_CONTROL_BUNDLE_ID,
};
use crate::services::computer_runtime_events::{
    ComputerRuntimeEventCause, ComputerRuntimeEventSink, ComputerRuntimeProblem,
    ComputerRuntimeSnapshot, ComputerRuntimeStatusEvent, PublicOAuthStatus,
    RuntimeDiagnosticRecord, SdkProblemObservations,
};
use crate::services::config::instance_storage_dir_name;
use crate::services::input_references::referenced_input_ids;
use crate::services::input_resolver::RuntimeInputResolver;
use crate::services::input_value_store::InputValueStore;
use crate::services::keychain::{InMemorySecretStore, SecretStore};
use crate::services::manager_context::ManagerContextKey;
use crate::services::oauth_credential_store::{effective_http_oauth, KeychainOAuthCredentialStore};
use crate::services::sdk_config::InstanceConfigContext;
use a2c_smcp::smcp_computer::computer::{Computer, ConnectOptions, Session, ToolCallRecord};
use a2c_smcp::smcp_computer::errors::{ComputerError, ComputerResult};
use a2c_smcp::smcp_computer::inputs::{env_var_name, run_command};
use a2c_smcp::smcp_computer::mcp_clients::bundle_id::resolve_bundle_id;
use a2c_smcp::smcp_computer::mcp_clients::manager::{ClientFactory, MCPServerManager};
use a2c_smcp::smcp_computer::mcp_clients::model::{
    BundleId, CallToolResult, HttpAuthenticationError, MCPServerInput, MCPServerRuntimeStatus,
    ReadResourceResult, Resource, ServerName, Tool, ToolMeta,
};
#[cfg(test)]
use a2c_smcp::smcp_computer::mcp_clients::model::{
    CommandInput, PickStringInput, PickStringOption, PromptStringInput,
};
use a2c_smcp::smcp_computer::mcp_clients::utils::client_factory;
use a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig;
use a2c_smcp::smcp_computer::settings::config::ProvenanceScope;
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
    inputs::{load_plugin_inputs, InputKind, InputResolutionError},
    inventory::{McpOwnership, McpServerWithMetadata},
    ComputerStatusSnapshot, LifecycleState,
};
use a2c_smcp::A2CSkillRef;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Weak};
use tokio::sync::{Mutex, Notify, OwnedMutexGuard, RwLock};
use tokio::task::JoinHandle;

mod connection;
mod oauth;
mod registry;
mod runtime;
pub mod runtime_lifecycle;

use connection::ClientConnectionOperationState;
pub use connection::{
    ClientConnectionActionCapabilities, ClientConnectionActionCapability,
    ClientConnectionActionDisabledReason, ClientConnectionOperation,
    ClientConnectionOperationError, ClientConnectionOperationTarget,
    ClientConnectionOperationToken, ClientConnectionStateSnapshot, ClientConnectionStatus,
    ConnectionStateSummary,
};
pub use registry::ComputerRegistry;
pub use runtime_lifecycle::{
    ComputerRuntimeAction, ComputerRuntimeActionCapabilities, ComputerRuntimeActionDisabledReason,
    ComputerRuntimeActionUnavailable, ComputerRuntimeUserState,
};

pub type ComputerInstanceId = String;
pub type ComputerRuntimeState = LifecycleState;

type SharedRuntimeEventSink = Arc<RwLock<Option<Arc<dyn ComputerRuntimeEventSink>>>>;

static NEXT_RUNTIME_INCARNATION: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RobotBindingMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_key: Option<ManagerContextKey>,
    pub state: ManagerRobotBindingState,
    pub employee_id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub robot_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "super::serde_compat::deserialize_optional_opaque_id",
        skip_serializing_if = "Option::is_none"
    )]
    pub last_resolved_robot_account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub robot_name: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ManagerRobotBindingState {
    Active,
    Dormant,
    NeedsRebind,
}

impl RobotBindingMetadata {
    pub fn active(context_key: ManagerContextKey, employee_id: u64) -> Self {
        Self {
            context_key: Some(context_key),
            state: ManagerRobotBindingState::Active,
            employee_id,
            robot_id: None,
            last_resolved_robot_account_id: None,
            namespace: None,
            robot_name: None,
        }
    }

    pub fn needs_rebind(employee_id: u64, last_resolved_robot_account_id: Option<String>) -> Self {
        Self {
            context_key: None,
            state: ManagerRobotBindingState::NeedsRebind,
            employee_id,
            robot_id: None,
            last_resolved_robot_account_id,
            namespace: None,
            robot_name: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComputerConnectionTargetType {
    ManagerRobot,
    ManualSmcp,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ComputerConnectionTarget {
    ManagerRobot {
        #[serde(rename = "contextKey")]
        context_key: ManagerContextKey,
        #[serde(rename = "employeeId")]
        employee_id: u64,
        #[serde(
            rename = "lastResolvedRobotAccountId",
            default,
            deserialize_with = "super::serde_compat::deserialize_optional_opaque_id",
            skip_serializing_if = "Option::is_none"
        )]
        last_resolved_robot_account_id: Option<String>,
    },
    ManualSmcp {
        id: String,
    },
}

impl ComputerConnectionTarget {
    pub fn manual_smcp(id: impl Into<String>) -> Self {
        Self::ManualSmcp { id: id.into() }
    }

    pub fn manager_robot(
        context_key: ManagerContextKey,
        employee_id: u64,
        last_resolved_robot_account_id: Option<String>,
    ) -> Self {
        Self::ManagerRobot {
            context_key,
            employee_id,
            last_resolved_robot_account_id,
        }
    }

    pub fn target_type(&self) -> ComputerConnectionTargetType {
        match self {
            Self::ManagerRobot { .. } => ComputerConnectionTargetType::ManagerRobot,
            Self::ManualSmcp { .. } => ComputerConnectionTargetType::ManualSmcp,
        }
    }
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
    #[serde(default)]
    pub remote_control: RemoteControlPolicy,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub robot_binding: Option<RobotBindingMetadata>,
}

pub const COMPUTER_PROFILE_SCHEMA_VERSION: u32 = 3;
pub const COMPUTER_INPUTS_SCHEMA_VERSION: u32 = 2;
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
    #[serde(default)]
    pub remote_control: RemoteControlPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub robot_binding: Option<RobotBindingMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct ComputerProfileConnectionPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<ComputerConnectionTarget>,
    #[serde(default)]
    pub auto_connect: bool,
}

impl ComputerProfile {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            schema_version: COMPUTER_PROFILE_SCHEMA_VERSION,
            id: id.into(),
            name: name.into(),
            description: None,
            connection_policy: ComputerProfileConnectionPolicy::default(),
            remote_control: RemoteControlPolicy::default(),
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
                target: instance.connection_policy.target.clone(),
                auto_connect: instance.connection_policy.auto_connect,
            },
            remote_control: instance.remote_control.clone(),
            robot_binding: instance.robot_binding.clone(),
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
                target: profile.connection_policy.target,
                auto_connect: profile.connection_policy.auto_connect,
            },
            remote_control: profile.remote_control,
            robot_binding: profile.robot_binding,
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

/// Obsolete client input sidecar retained only as a directory-transaction compatibility member.
/// New definitions are never written here; SDK `ProjectConfigDoc` is the sole source of truth.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ComputerInputsConfig {
    pub schema_version: u32,
    #[serde(default)]
    pub inputs: Vec<ComputerInputDefinition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub migration_issues: Vec<ComputerInputMigrationIssue>,
}

/// Explicit compatibility state for legacy data that cannot satisfy the v2 schema without
/// inventing a value. Normal CRUD never creates this marker.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ComputerInputMigrationIssue {
    UnresolvedPickNoOption { input_id: String },
}

impl ComputerInputMigrationIssue {
    pub fn input_id(&self) -> &str {
        match self {
            Self::UnresolvedPickNoOption { input_id } => input_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum ComputerInputDefinition {
    PromptString {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        default: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        password: Option<bool>,
    },
    PickString {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        options: Vec<GlobalPickOption>,
        #[serde(skip_serializing_if = "Option::is_none")]
        default: Option<String>,
    },
    Command {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        command: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        args: Option<Vec<String>>,
    },
}

impl ComputerInputDefinition {
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

impl From<&InputDefinition> for ComputerInputDefinition {
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

impl From<&ComputerInputDefinition> for InputDefinition {
    fn from(input: &ComputerInputDefinition) -> Self {
        match input {
            ComputerInputDefinition::PromptString {
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
            ComputerInputDefinition::PickString {
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
            ComputerInputDefinition::Command {
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

impl Default for ComputerInputsConfig {
    fn default() -> Self {
        Self {
            schema_version: COMPUTER_INPUTS_SCHEMA_VERSION,
            inputs: Vec::new(),
            migration_issues: Vec::new(),
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
            remote_control: RemoteControlPolicy::default(),
            robot_binding: None,
        }
    }
}

#[derive(Clone)]
pub struct InstanceSession {
    id: String,
}

impl InstanceSession {
    fn new(id: impl Into<String>) -> Self {
        Self { id: id.into() }
    }
}

#[async_trait]
impl Session for InstanceSession {
    async fn resolve_input(&self, input: &MCPServerInput) -> ComputerResult<serde_json::Value> {
        match input {
            MCPServerInput::PromptString(input) => Ok(serde_json::Value::String(
                input.default.clone().unwrap_or_default(),
            )),
            MCPServerInput::PickString(input) => Ok(serde_json::Value::String(
                input.default.clone().unwrap_or_else(|| {
                    input
                        .options
                        .first()
                        .map(|option| option.value.clone())
                        .unwrap_or_default()
                }),
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

#[derive(Debug, thiserror::Error)]
pub enum ComputerRuntimeStartError {
    #[error(transparent)]
    Sdk(#[from] ComputerError),
    #[error("{source}; {context}")]
    SdkWithContext {
        source: ComputerError,
        context: String,
    },
    #[error("{0}")]
    Client(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HandleReplacementConfig {
    ReloadPersisted,
    RetainCurrentGeneration,
}

struct HandleDeclarations<'a> {
    injected_inputs: &'a HashMap<String, MCPServerInput>,
    retained_inputs: Option<&'a HashMap<String, MCPServerInput>>,
    retained_mcp_servers: Option<&'a HashMap<String, MCPServerConfig>>,
}

impl ComputerRuntimeStartError {
    fn append_context(self, context: impl std::fmt::Display) -> Self {
        let context = context.to_string();
        match self {
            Self::Sdk(source) => Self::SdkWithContext { source, context },
            Self::SdkWithContext {
                source,
                context: existing,
            } => Self::SdkWithContext {
                source,
                context: format!("{existing}; {context}"),
            },
            Self::Client(message) => Self::Client(format!("{message}; {context}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmcpReconnectOutcome {
    Reconnected { expires_in: i64 },
    Stale,
}

struct RuntimeEventRelay {
    generation: u64,
    task: JoinHandle<()>,
}

struct RuntimeActivityGuard {
    active_operations: Arc<AtomicUsize>,
    activity_changed: Arc<Notify>,
}

struct OAuthAdmissionGuard(Arc<AtomicBool>);

impl Drop for OAuthAdmissionGuard {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

pub(crate) struct OAuthServerChangeAdmissionGuard(Arc<AtomicUsize>);

impl Drop for OAuthServerChangeAdmissionGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Keeps a runtime admitted for the complete multi-step SDK Skill read transaction. Deletion
/// drains these sessions before quarantining Skill Home storage or shutting down the SDK.
pub(crate) struct SdkSkillReadSession<'a> {
    runtime: &'a ComputerInstanceRuntime,
    _activity: RuntimeActivityGuard,
}

/// Holds one runtime generation stable for an entire client-owned User Skill mutation. Runtime
/// removal drains the activity guard, while policy/Skill Home replacement waits on the lifecycle
/// guard. This makes the configured/effective Home check, filesystem commit, registry refresh,
/// and response one linearized operation.
pub(crate) struct SdkSkillMutationLease {
    runtime: ComputerInstanceRuntime,
    _activity: RuntimeActivityGuard,
    _lifecycle: tokio::sync::OwnedMutexGuard<()>,
}

impl SdkSkillMutationLease {
    pub fn configured_skill_home(&self) -> PathBuf {
        self.runtime.configured_skill_home()
    }

    pub async fn effective_skill_home(&self) -> PathBuf {
        self.runtime.computer.read().await.skill_home()
    }

    pub async fn mark_skills_dirty(&self) {
        self.runtime.computer.read().await.mark_skills_dirty();
    }
}

impl SdkSkillReadSession<'_> {
    pub async fn skills(&self) -> Vec<A2CSkillRef> {
        self.runtime.computer.read().await.get_skills().await
    }

    pub async fn skill_ref(&self, name: &str) -> Option<A2CSkillRef> {
        self.runtime.computer.read().await.get_skill_ref(name).await
    }

    pub async fn read_skill_resource(
        &self,
        skill_ref: &A2CSkillRef,
        rel_path: Option<&str>,
    ) -> Result<SkillResourceView, SkillSandboxError> {
        self.runtime
            .computer
            .read()
            .await
            .read_skill_resource(skill_ref, rel_path)
    }

    pub async fn skill_home(&self) -> PathBuf {
        self.runtime.computer.read().await.skill_home()
    }
}

impl Drop for RuntimeActivityGuard {
    fn drop(&mut self) {
        if self.active_operations.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.activity_changed.notify_one();
        }
    }
}

#[derive(Clone)]
pub struct ComputerInstanceRuntime {
    pub instance: ComputerInstance,
    pub inputs: Arc<RwLock<HashMap<String, MCPServerInput>>>,
    plugin_runtime_inputs: Arc<RwLock<HashMap<String, MCPServerInput>>>,
    computer: Arc<RwLock<Computer<InstanceSession>>>,
    session: InstanceSession,
    input_resolver: Arc<RuntimeInputResolver>,
    oauth_credential_store: Arc<KeychainOAuthCredentialStore>,
    oauth_flows: Arc<Mutex<HashMap<BundleId, oauth::ActiveOAuthFlow>>>,
    oauth_required_scopes: Arc<RwLock<oauth::OAuthRequiredScopeCache>>,
    oauth_server_lifecycle_lock: Arc<Mutex<()>>,
    oauth_admission_open: Arc<AtomicBool>,
    oauth_server_change_admission_blocks: Arc<AtomicUsize>,
    skill_home_base: PathBuf,
    sdk_servers: Arc<RwLock<HashMap<BundleId, ServerName>>>,
    // Tracks only whether this client materialized a Marketplace dependency. Plugin ownership
    // itself is SDK ledger-derived and may hand off between multiple enabled plugins.
    plugin_mounted_server_ids: Arc<RwLock<HashSet<BundleId>>>,
    connection: Arc<RwLock<Option<ConnectionState>>>,
    connection_operation: Arc<RwLock<ClientConnectionOperationState>>,
    connection_authority_revision: Arc<AtomicU64>,
    lifecycle_lock: Arc<Mutex<()>>,
    refresh_task: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
    retired: Arc<AtomicBool>,
    active_operations: Arc<AtomicUsize>,
    activity_changed: Arc<Notify>,
    runtime_incarnation: u64,
    runtime_generation: Arc<AtomicU64>,
    runtime_snapshot_revision: Arc<AtomicU64>,
    runtime_snapshot_lock: Arc<Mutex<()>>,
    runtime_event_sink: SharedRuntimeEventSink,
    client_control_binding: ClientControlBinding,
    runtime_event_task: Arc<Mutex<Option<RuntimeEventRelay>>>,
    shutdown_completed: Arc<AtomicBool>,
    sdk_problem_observations: Arc<Mutex<SdkProblemObservations>>,
    client_runtime_diagnostic: Arc<RwLock<Option<RuntimeDiagnosticRecord>>>,
    mcp_start_diagnostics: Arc<RwLock<HashMap<BundleId, RuntimeDiagnosticRecord>>>,
    mcp_config_apply_diagnostics: Arc<RwLock<HashMap<BundleId, RuntimeDiagnosticRecord>>>,
    #[cfg(debug_assertions)]
    fail_prepare_shutdown_once: Arc<AtomicBool>,
    #[cfg(debug_assertions)]
    fail_sdk_shutdown_once: Arc<AtomicBool>,
    #[cfg(debug_assertions)]
    fail_smcp_disconnect_once: Arc<AtomicBool>,
    #[cfg(debug_assertions)]
    hang_smcp_disconnect_once: Arc<AtomicBool>,
}

impl ComputerInstanceRuntime {
    /// Builds isolated runtime handles for an instance. MCP runtime ownership lives in SDK
    /// Computer; the client keeps only instance-scoped state and lifecycle handles here.
    pub fn new(instance: ComputerInstance, skill_home_base: PathBuf) -> Self {
        Self::new_with_secret_store(
            instance,
            skill_home_base,
            Arc::new(InMemorySecretStore::default()),
        )
    }

    pub fn new_with_secret_store(
        instance: ComputerInstance,
        skill_home_base: PathBuf,
        secret_store: Arc<dyn SecretStore>,
    ) -> Self {
        Self::new_with_secret_store_and_event_sink(
            instance,
            skill_home_base,
            secret_store,
            Arc::new(RwLock::new(None)),
            ClientControlBinding::default(),
        )
    }

    fn new_with_secret_store_and_event_sink(
        instance: ComputerInstance,
        skill_home_base: PathBuf,
        secret_store: Arc<dyn SecretStore>,
        runtime_event_sink: SharedRuntimeEventSink,
        client_control_binding: ClientControlBinding,
    ) -> Self {
        let injected_inputs = HashMap::new();
        let session = InstanceSession::new(instance.id.clone());
        let oauth_credential_store = Arc::new(KeychainOAuthCredentialStore::new(
            instance.id.clone(),
            secret_store.clone(),
        ));
        let input_resolver = Arc::new(RuntimeInputResolver::new(
            instance.id.clone(),
            InputValueStore::from_storage_root(
                skill_home_base.join(instance_storage_dir_name(&instance.id)),
            ),
            secret_store,
        ));
        let (computer, sdk_servers, inputs) = build_sdk_computer(
            &instance,
            HandleDeclarations {
                injected_inputs: &injected_inputs,
                retained_inputs: None,
                retained_mcp_servers: None,
            },
            session.clone(),
            input_resolver.clone(),
            oauth_credential_store.clone(),
            &skill_home_base,
            client_control_binding.clone(),
        );
        Self {
            instance,
            inputs: Arc::new(RwLock::new(inputs)),
            plugin_runtime_inputs: Arc::new(RwLock::new(HashMap::new())),
            computer: Arc::new(RwLock::new(computer)),
            session,
            input_resolver,
            oauth_credential_store,
            oauth_flows: Arc::new(Mutex::new(HashMap::new())),
            oauth_required_scopes: Arc::new(RwLock::new(oauth::OAuthRequiredScopeCache::default())),
            oauth_server_lifecycle_lock: Arc::new(Mutex::new(())),
            oauth_admission_open: Arc::new(AtomicBool::new(true)),
            oauth_server_change_admission_blocks: Arc::new(AtomicUsize::new(0)),
            skill_home_base,
            sdk_servers: Arc::new(RwLock::new(sdk_servers)),
            plugin_mounted_server_ids: Arc::new(RwLock::new(HashSet::new())),
            connection: Arc::new(RwLock::new(None)),
            connection_operation: Arc::new(RwLock::new(ClientConnectionOperationState::default())),
            connection_authority_revision: Arc::new(AtomicU64::new(0)),
            lifecycle_lock: Arc::new(Mutex::new(())),
            refresh_task: Arc::new(Mutex::new(None)),
            retired: Arc::new(AtomicBool::new(false)),
            active_operations: Arc::new(AtomicUsize::new(0)),
            activity_changed: Arc::new(Notify::new()),
            runtime_incarnation: NEXT_RUNTIME_INCARNATION.fetch_add(1, Ordering::AcqRel),
            runtime_generation: Arc::new(AtomicU64::new(1)),
            runtime_snapshot_revision: Arc::new(AtomicU64::new(0)),
            runtime_snapshot_lock: Arc::new(Mutex::new(())),
            runtime_event_sink,
            client_control_binding,
            runtime_event_task: Arc::new(Mutex::new(None)),
            shutdown_completed: Arc::new(AtomicBool::new(false)),
            sdk_problem_observations: Arc::new(Mutex::new(SdkProblemObservations::default())),
            client_runtime_diagnostic: Arc::new(RwLock::new(None)),
            mcp_start_diagnostics: Arc::new(RwLock::new(HashMap::new())),
            mcp_config_apply_diagnostics: Arc::new(RwLock::new(HashMap::new())),
            #[cfg(debug_assertions)]
            fail_prepare_shutdown_once: Arc::new(AtomicBool::new(false)),
            #[cfg(debug_assertions)]
            fail_sdk_shutdown_once: Arc::new(AtomicBool::new(false)),
            #[cfg(debug_assertions)]
            fail_smcp_disconnect_once: Arc::new(AtomicBool::new(false)),
            #[cfg(debug_assertions)]
            hang_smcp_disconnect_once: Arc::new(AtomicBool::new(false)),
        }
    }

    fn with_instance(&self, instance: ComputerInstance) -> Self {
        Self {
            instance,
            inputs: self.inputs.clone(),
            plugin_runtime_inputs: self.plugin_runtime_inputs.clone(),
            computer: self.computer.clone(),
            session: self.session.clone(),
            input_resolver: self.input_resolver.clone(),
            oauth_credential_store: self.oauth_credential_store.clone(),
            oauth_flows: self.oauth_flows.clone(),
            oauth_required_scopes: self.oauth_required_scopes.clone(),
            oauth_server_lifecycle_lock: self.oauth_server_lifecycle_lock.clone(),
            oauth_admission_open: self.oauth_admission_open.clone(),
            oauth_server_change_admission_blocks: self.oauth_server_change_admission_blocks.clone(),
            skill_home_base: self.skill_home_base.clone(),
            sdk_servers: self.sdk_servers.clone(),
            plugin_mounted_server_ids: self.plugin_mounted_server_ids.clone(),
            connection: self.connection.clone(),
            connection_operation: self.connection_operation.clone(),
            connection_authority_revision: self.connection_authority_revision.clone(),
            lifecycle_lock: self.lifecycle_lock.clone(),
            refresh_task: self.refresh_task.clone(),
            retired: self.retired.clone(),
            active_operations: self.active_operations.clone(),
            activity_changed: self.activity_changed.clone(),
            runtime_incarnation: self.runtime_incarnation,
            runtime_generation: self.runtime_generation.clone(),
            runtime_snapshot_revision: self.runtime_snapshot_revision.clone(),
            runtime_snapshot_lock: self.runtime_snapshot_lock.clone(),
            runtime_event_sink: self.runtime_event_sink.clone(),
            client_control_binding: self.client_control_binding.clone(),
            runtime_event_task: self.runtime_event_task.clone(),
            shutdown_completed: self.shutdown_completed.clone(),
            sdk_problem_observations: self.sdk_problem_observations.clone(),
            client_runtime_diagnostic: self.client_runtime_diagnostic.clone(),
            mcp_start_diagnostics: self.mcp_start_diagnostics.clone(),
            mcp_config_apply_diagnostics: self.mcp_config_apply_diagnostics.clone(),
            #[cfg(debug_assertions)]
            fail_prepare_shutdown_once: self.fail_prepare_shutdown_once.clone(),
            #[cfg(debug_assertions)]
            fail_sdk_shutdown_once: self.fail_sdk_shutdown_once.clone(),
            #[cfg(debug_assertions)]
            fail_smcp_disconnect_once: self.fail_smcp_disconnect_once.clone(),
            #[cfg(debug_assertions)]
            hang_smcp_disconnect_once: self.hang_smcp_disconnect_once.clone(),
        }
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub async fn hold_activity_for_test(
        &self,
        started: tokio::sync::oneshot::Sender<()>,
        release: tokio::sync::oneshot::Receiver<()>,
    ) -> Result<(), String> {
        let _activity = self.begin_activity()?;
        let _ = started.send(());
        let _ = release.await;
        Ok(())
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn can_begin_activity_for_test(&self) -> bool {
        self.begin_activity().is_ok()
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn fail_next_smcp_disconnect_for_test(&self) {
        self.fail_smcp_disconnect_once.store(true, Ordering::SeqCst);
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn hang_next_smcp_disconnect_for_test(&self) {
        self.hang_smcp_disconnect_once.store(true, Ordering::SeqCst);
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn is_retired_for_test(&self) -> bool {
        self.is_retired()
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub async fn has_refresh_task_for_test(&self) -> bool {
        self.refresh_task
            .lock()
            .await
            .as_ref()
            .is_some_and(|task| !task.is_finished())
    }

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub fn fail_prepare_shutdown_once_for_test(&self) {
        self.fail_prepare_shutdown_once
            .store(true, Ordering::SeqCst);
    }

    pub async fn shutdown(&self) {
        if let Err(error) = self.try_shutdown().await {
            log::warn!(
                "Failed to shutdown Computer runtime for instance {}: {}",
                self.instance.id,
                error
            );
        }
    }

    pub async fn sync_runtime(&self) -> Result<(), ComputerRuntimeStartError> {
        self.sync_runtime_for_policy_change(false).await
    }

    pub(super) async fn sync_runtime_for_policy_change(
        &self,
        remote_control_policy_changed: bool,
    ) -> Result<(), ComputerRuntimeStartError> {
        let _guard = self.lifecycle_lock.lock().await;
        self.ensure_active()
            .map_err(ComputerRuntimeStartError::Client)?;

        let provider_is_mounted = self
            .computer
            .read()
            .await
            .list_mcp_servers()
            .await
            .iter()
            .any(|server| resolve_bundle_id(server).as_str() == CLIENT_CONTROL_BUNDLE_ID);
        if provider_is_mounted != self.instance.remote_control.enabled
            || (provider_is_mounted && remote_control_policy_changed)
        {
            let was_running = self.is_running().await;
            self.replace_sdk_computer(
                was_running,
                "Client Control policy changed",
                HandleReplacementConfig::RetainCurrentGeneration,
            )
            .await?;
            return Ok(());
        }

        // Persisted SDK input definitions belong to the current handle generation. Metadata
        // synchronization (status reads, rename, policy saves) must not refresh that pool: only
        // an actual start/restart rebuilds the SDK Computer from the latest raw declarations.
        // Plugin lifecycle inputs are injected explicitly by the SDK hook path and remain
        // independent from project definition CRUD.
        *self.sdk_servers.write().await = self
            .sdk_user_mcp_server_config_map()
            .await
            .into_iter()
            .map(|(bundle_id, config)| (bundle_id, config.name().to_string()))
            .collect();
        if self.is_running().await {
            self.reconcile_sdk_governance_inner()
                .await
                .map_err(ComputerRuntimeStartError::Sdk)?;
        }
        Ok(())
    }

    pub async fn add_or_update_plugin_server(&self, server: MCPServerConfig) -> ComputerResult<()> {
        let _guard = self.lifecycle_lock.lock().await;
        let _oauth_server_guard = self.oauth_server_lifecycle_lock.lock().await;
        self.ensure_active_computer()?;
        let name = server.name().to_string();
        let bundle_id = resolve_bundle_id(&server);
        if bundle_id.as_str() == CLIENT_CONTROL_BUNDLE_ID {
            return Err(ComputerError::InvalidConfiguration(
                "Plugins cannot use the reserved Client Control bundleId".to_string(),
            ));
        }
        self.cancel_oauth_before_server_lifecycle_change(&bundle_id)
            .await?;
        self.computer
            .read()
            .await
            .mount_server(normalize_mcp_server_tool_meta(server))
            .await?;
        self.sdk_servers
            .write()
            .await
            .insert(bundle_id.clone(), name);
        self.clear_mcp_start_diagnostic(&bundle_id).await;
        self.clear_mcp_config_apply_diagnostic(&bundle_id).await;
        self.plugin_mounted_server_ids
            .write()
            .await
            .insert(bundle_id);
        Ok(())
    }

    pub async fn runtime_input_kind(&self, input_id: &str) -> Option<InputKind> {
        self.plugin_runtime_inputs
            .read()
            .await
            .get(input_id)
            .and_then(runtime_stored_input_kind)
    }

    /// Applies an already-persisted user MCP declaration to this runtime without rebuilding the
    /// Computer handle or interrupting its SMCP connection. Runtime start failures are recorded
    /// per server and remain observable through the MCP status projection; they do not invalidate
    /// the durable declaration.
    pub async fn apply_user_mcp_server_config(
        &self,
        server: MCPServerConfig,
    ) -> ComputerResult<()> {
        self.apply_user_mcp_server_config_inner(server, true)
            .await
            .map(|_| ())
    }

    pub async fn restore_user_mcp_server_config_after_plugin_release(
        &self,
        server: MCPServerConfig,
    ) -> Result<(), String> {
        let bundle_id = resolve_bundle_id(&server);
        if !self
            .apply_user_mcp_server_config_inner(server, false)
            .await
            .map_err(|error| error.to_string())?
        {
            return Err(format!(
                "Failed to restore user MCP runtime after plugin release for {bundle_id}"
            ));
        }
        self.plugin_mounted_server_ids
            .write()
            .await
            .remove(&bundle_id);
        Ok(())
    }

    async fn apply_user_mcp_server_config_inner(
        &self,
        server: MCPServerConfig,
        preserve_plugin_runtime: bool,
    ) -> ComputerResult<bool> {
        let _guard = self.lifecycle_lock.lock().await;
        let _oauth_server_guard = self.oauth_server_lifecycle_lock.lock().await;
        self.ensure_active_computer()?;
        let bundle_id = resolve_bundle_id(&server);
        if bundle_id.as_str() == CLIENT_CONTROL_BUNDLE_ID {
            return Err(ComputerError::InvalidConfiguration(
                "the Client Control bundleId is reserved".to_string(),
            ));
        }
        let computer_running = self.is_running().await;

        // While a plugin owns this BundleId, its lifecycle is authoritative. Persisting a
        // disabled user fallback must not stop the plugin dependency; the durable user setting
        // is restored when the last plugin owner releases the bundle.
        if preserve_plugin_runtime
            && server.disabled()
            && self
                .plugin_mcp_server_owner_inner(&bundle_id)
                .await
                .is_some()
        {
            self.plugin_mounted_server_ids
                .write()
                .await
                .insert(bundle_id.clone());
            self.clear_mcp_start_diagnostic(&bundle_id).await;
            if computer_running {
                if let Err(error) = self.start_mcp_server_inner(&bundle_id).await {
                    log::warn!(
                        "Plugin-owned MCP server failed to remain active for instance {} after user fallback was disabled for {}: {}",
                        self.instance.id,
                        bundle_id,
                        error
                    );
                }
            }
            return Ok(true);
        }

        self.cancel_oauth_before_server_lifecycle_change(&bundle_id)
            .await?;

        if computer_running {
            if let Err(error) = self.computer.read().await.stop_mcp_client(&bundle_id).await {
                self.record_mcp_config_apply_diagnostic_for_server(
                    bundle_id.clone(),
                    server.name().to_string(),
                    format!(
                        "Configuration saved, but the active MCP process could not be stopped: {error}. Restart Runtime or inspect the logs before retrying."
                    ),
                )
                .await;
                log::warn!(
                    "Saved MCP config for instance {}, but stopping the previous runtime failed for {}: {}",
                    self.instance.id,
                    bundle_id,
                    error
                );
                return Ok(false);
            }
        }

        if let Err(error) = self
            .computer
            .read()
            .await
            .mount_server(normalize_mcp_server_tool_meta(server.clone()))
            .await
        {
            self.record_mcp_config_apply_diagnostic_for_server(
                bundle_id.clone(),
                server.name().to_string(),
                format!(
                    "Configuration saved, but it could not be applied to the active Runtime: {error}. Restart Runtime or inspect the logs before retrying."
                ),
            )
            .await;
            log::warn!(
                "Saved MCP config for instance {}, but runtime activation failed for {}: {}",
                self.instance.id,
                bundle_id,
                error
            );
            return Err(error);
        }

        self.sdk_servers
            .write()
            .await
            .insert(bundle_id.clone(), server.name().to_string());
        self.clear_mcp_config_apply_diagnostic(&bundle_id).await;
        if server.disabled() || !computer_running {
            self.clear_mcp_start_diagnostic(&bundle_id).await;
            return Ok(true);
        }

        let proactive_interactive_oauth = matches!(
            &server,
            MCPServerConfig::Http(config)
                if effective_http_oauth(config)
                    .is_some_and(|oauth| !oauth.automatic && oauth.interactive)
        );
        if proactive_interactive_oauth {
            // Persisting and mounting configuration is complete. Interactive authorization is a
            // separate lifecycle: the OAuth callback starts the server after credentials commit,
            // so an expected Unauthorized response must never become a configuration error.
            self.clear_mcp_start_diagnostic(&bundle_id).await;
            return Ok(true);
        }

        if let Err(error) = self.start_mcp_server_inner(&bundle_id).await {
            log::warn!(
                "Saved MCP config for instance {}, but server failed to start for {}: {}",
                self.instance.id,
                bundle_id,
                error
            );
            // The declaration is already persisted and mounted. Startability is runtime state,
            // not configuration validity; keep the failure in the per-server diagnostic without
            // turning a successful configuration operation into an error.
            return Ok(true);
        }
        Ok(true)
    }

    pub async fn record_mcp_config_apply_diagnostic(&self, bundle_id: BundleId, message: String) {
        let server_name = self
            .sdk_servers
            .read()
            .await
            .get(&bundle_id)
            .map(ToString::to_string);
        self.record_mcp_config_apply_diagnostic_inner(bundle_id, server_name, message)
            .await;
    }

    async fn record_mcp_config_apply_diagnostic_for_server(
        &self,
        bundle_id: BundleId,
        server_name: String,
        message: String,
    ) {
        self.record_mcp_config_apply_diagnostic_inner(bundle_id, Some(server_name), message)
            .await;
    }

    async fn record_mcp_config_apply_diagnostic_inner(
        &self,
        bundle_id: BundleId,
        server_name: Option<String>,
        message: String,
    ) {
        let operation = "apply_configuration";
        let diagnostic = match server_name {
            Some(server_name) => RuntimeDiagnosticRecord::for_mcp(operation, message, server_name),
            None => RuntimeDiagnosticRecord::new(operation, message),
        };
        let mut diagnostics = self.mcp_config_apply_diagnostics.write().await;
        let diagnostic = diagnostic.preserve_occurrence_from(diagnostics.get(&bundle_id));
        diagnostics.insert(bundle_id.clone(), diagnostic);
        drop(diagnostics);
        self.publish_runtime_status(ComputerRuntimeEventCause::McpDiagnosticChanged {
            bundle_id: bundle_id.into_string(),
            operation: operation.to_string(),
            has_error: true,
        })
        .await;
    }

    async fn clear_mcp_config_apply_diagnostic(&self, bundle_id: &BundleId) {
        if self
            .mcp_config_apply_diagnostics
            .write()
            .await
            .remove(bundle_id)
            .is_some()
        {
            self.publish_runtime_status(ComputerRuntimeEventCause::McpDiagnosticChanged {
                bundle_id: bundle_id.to_string(),
                operation: "apply_configuration".to_string(),
                has_error: false,
            })
            .await;
        }
    }

    async fn clear_mcp_start_diagnostic(&self, bundle_id: &BundleId) {
        if self
            .mcp_start_diagnostics
            .write()
            .await
            .remove(bundle_id)
            .is_some()
        {
            self.publish_runtime_status(ComputerRuntimeEventCause::McpDiagnosticChanged {
                bundle_id: bundle_id.to_string(),
                operation: "start".to_string(),
                has_error: false,
            })
            .await;
        }
    }

    pub async fn remove_user_mcp_server_config(&self, bundle_id: &BundleId) -> Result<(), String> {
        let _guard = self.lifecycle_lock.lock().await;
        let oauth_server_guard = self.oauth_server_lifecycle_lock.lock().await;
        self.ensure_active()?;
        self.clear_oauth_authorization_inner(bundle_id).await?;
        let computer_running = self.is_running().await;
        let server_started = self
            .computer
            .read()
            .await
            .get_server_runtime_statuses()
            .await
            .into_iter()
            .any(|status| status.bundle_id == *bundle_id && status.is_started());
        if server_started {
            self.computer
                .read()
                .await
                .stop_mcp_client(bundle_id)
                .await
                .map_err(|error| error.to_string())?;
        }
        self.computer
            .read()
            .await
            .unmount_server(bundle_id)
            .await
            .map_err(|error| error.to_string())?;
        self.sdk_servers.write().await.remove(bundle_id);
        self.clear_mcp_start_diagnostic(bundle_id).await;
        self.clear_mcp_config_apply_diagnostic(bundle_id).await;
        // The old SDK server no longer exists. Reconciliation hooks fence each subsequent
        // server-local mount/unmount independently, so do not retain this non-reentrant guard
        // while invoking them.
        drop(oauth_server_guard);

        self.reconcile_sdk_governance_inner()
            .await
            .map_err(|error| error.to_string())?;
        if computer_running {
            let failures = self.start_desired_mcp_servers_inner().await;
            self.log_mcp_start_failures(&failures, "user MCP removal");
        }
        Ok(())
    }

    pub async fn remove_plugin_server(&self, bundle_id: &BundleId) -> Result<(), String> {
        let _guard = self.lifecycle_lock.lock().await;
        let _oauth_server_guard = self.oauth_server_lifecycle_lock.lock().await;
        self.ensure_active()?;
        self.cancel_oauth_before_server_lifecycle_change(bundle_id)
            .await
            .map_err(|error| error.to_string())?;
        remove_tracked_plugin_server(
            bundle_id,
            &self.sdk_servers,
            &self.plugin_mounted_server_ids,
            async { self.computer.read().await.unmount_server(bundle_id).await },
            async {
                self.clear_mcp_start_diagnostic(bundle_id).await;
                self.clear_mcp_config_apply_diagnostic(bundle_id).await;
            },
        )
        .await?;
        Ok(())
    }

    pub async fn mcp_server_runtime_statuses(&self) -> Vec<MCPServerRuntimeStatus> {
        let _guard = self.lifecycle_lock.lock().await;
        self.computer
            .read()
            .await
            .get_server_runtime_statuses()
            .await
    }

    pub async fn mcp_start_diagnostics(&self) -> HashMap<BundleId, String> {
        let mut diagnostics: HashMap<_, _> = self
            .mcp_start_diagnostics
            .read()
            .await
            .iter()
            .map(|(bundle_id, diagnostic)| (bundle_id.clone(), diagnostic.message.clone()))
            .collect();
        diagnostics.extend(
            self.mcp_config_apply_diagnostics
                .read()
                .await
                .iter()
                .map(|(bundle_id, diagnostic)| (bundle_id.clone(), diagnostic.message.clone())),
        );
        diagnostics
    }

    pub async fn mcp_server_display_name(&self, bundle_id: &BundleId) -> Option<ServerName> {
        self.sdk_mcp_server_ownership()
            .await
            .into_iter()
            .find(|entry| entry.bundle_id == bundle_id.as_str())
            .map(|entry| entry.name)
    }

    pub async fn plugin_mcp_server_owner(
        &self,
        bundle_id: &BundleId,
    ) -> Option<McpServerManagedBy> {
        self.plugin_mcp_server_owner_inner(bundle_id).await
    }

    pub(crate) async fn has_tracked_plugin_mcp_server(&self, bundle_id: &BundleId) -> bool {
        self.plugin_mounted_server_ids
            .read()
            .await
            .contains(bundle_id)
    }

    pub(crate) async fn tracked_plugin_mcp_server_ids(&self) -> Vec<BundleId> {
        self.plugin_mounted_server_ids
            .read()
            .await
            .iter()
            .cloned()
            .collect()
    }

    pub async fn start_mcp_server(&self, bundle_id: &BundleId) -> ComputerResult<()> {
        if bundle_id.as_str() == CLIENT_CONTROL_BUNDLE_ID {
            return Err(ComputerError::InvalidConfiguration(
                "the reserved Client Control provider is not user-manageable".to_string(),
            ));
        }
        let _guard = self.lifecycle_lock.lock().await;
        self.start_mcp_server_inner(bundle_id).await
    }

    async fn start_mcp_server_inner(&self, bundle_id: &BundleId) -> ComputerResult<()> {
        self.ensure_active_computer()?;
        if let Some(error) = self.missing_configured_input_definition(bundle_id).await {
            self.record_mcp_start_diagnostic(bundle_id.clone(), format!("Start failed: {error}"))
                .await;
            return Err(ComputerError::InputResolution(error));
        }
        let result = self.computer.read().await.start_mcp_client(bundle_id).await;
        if Self::is_expected_oauth_required(&result) {
            // A validated OAuth challenge is an expected runtime state, not a malformed server
            // configuration or failed import. The SDK has admitted the OAuth coordinator, so the
            // runtime status can now expose authorization and the callback will retry this start.
            self.clear_mcp_start_diagnostic(bundle_id).await;
            self.clear_mcp_config_apply_diagnostic(bundle_id).await;
            return Ok(());
        }
        match &result {
            Ok(()) => {
                self.clear_mcp_start_diagnostic(bundle_id).await;
            }
            Err(error) => {
                self.record_mcp_start_diagnostic(
                    bundle_id.clone(),
                    format!("Start failed: {error}"),
                )
                .await;
            }
        }
        result
    }

    async fn missing_configured_input_definition(
        &self,
        bundle_id: &BundleId,
    ) -> Option<InputResolutionError> {
        let config = self.sdk_mcp_server_config_map().await.remove(bundle_id)?;
        let config = serde_json::to_value(config).ok()?;
        let referenced = referenced_input_ids(&config);
        if referenced.is_empty() {
            return None;
        }
        let defined = self.inputs.read().await;
        referenced
            .into_iter()
            .find(|input_id| !defined.contains_key(input_id))
            .map(|id| InputResolutionError::Missing {
                env_hint: env_var_name(&id),
                id,
                kind: InputKind::Value,
            })
    }

    /// True when a start result means "this OAuth server is awaiting authorization" rather than a
    /// genuine configuration or connectivity failure. See `start_mcp_server_inner`.
    ///
    /// The SDK surfaces this single condition through two error paths with identical semantics.
    /// A fresh connect returns the structured `HttpAuthentication(OAuthRequired)`. A restart after
    /// credentials were cleared keeps the OAuth coordinator admitted but with no stored token, so
    /// `prepare_request` fails and the SDK wraps `OAuthProtocolError::AuthorizationRequired` into a
    /// `ConnectionError` whose message ends in the canonical "OAuth authorization is required"
    /// string — the same text `HttpAuthenticationError::OAuthRequired` displays. Both must stay
    /// soft, otherwise clearing authorization and re-starting reports a hard failure for a server
    /// that is merely waiting to be authorized again.
    fn is_expected_oauth_required(result: &ComputerResult<()>) -> bool {
        match result {
            Err(ComputerError::HttpAuthentication(HttpAuthenticationError::OAuthRequired)) => true,
            Err(ComputerError::ConnectionError(message)) => {
                message.contains("OAuth authorization is required")
            }
            _ => false,
        }
    }

    async fn record_mcp_start_diagnostic(&self, bundle_id: BundleId, message: String) {
        let operation = "start";
        let mut diagnostics = self.mcp_start_diagnostics.write().await;
        let diagnostic = RuntimeDiagnosticRecord::new(operation, message)
            .preserve_occurrence_from(diagnostics.get(&bundle_id));
        diagnostics.insert(bundle_id.clone(), diagnostic);
        drop(diagnostics);
        self.publish_runtime_status(ComputerRuntimeEventCause::McpDiagnosticChanged {
            bundle_id: bundle_id.into_string(),
            operation: operation.to_string(),
            has_error: true,
        })
        .await;
    }

    pub async fn stop_mcp_server(&self, bundle_id: &BundleId) -> Result<bool, String> {
        if bundle_id.as_str() == CLIENT_CONTROL_BUNDLE_ID {
            return Err("the reserved Client Control provider is not user-manageable".to_string());
        }
        let _guard = self.lifecycle_lock.lock().await;
        self.stop_mcp_server_inner(bundle_id).await
    }

    async fn stop_mcp_server_inner(&self, bundle_id: &BundleId) -> Result<bool, String> {
        self.ensure_active()?;
        let result = self
            .computer
            .read()
            .await
            .stop_mcp_client(bundle_id)
            .await
            .map_err(|error| error.to_string());
        if result.is_ok() {
            self.clear_mcp_start_diagnostic(bundle_id).await;
        }
        result
    }

    pub async fn stop_mcp_servers_best_effort(
        &self,
        bundle_ids: Vec<BundleId>,
    ) -> Vec<(BundleId, Result<bool, String>)> {
        let _guard = self.lifecycle_lock.lock().await;
        let mut results = Vec::with_capacity(bundle_ids.len());
        for bundle_id in bundle_ids {
            if bundle_id.as_str() == CLIENT_CONTROL_BUNDLE_ID {
                results.push((
                    bundle_id,
                    Err("the reserved Client Control provider is not user-manageable".to_string()),
                ));
                continue;
            }
            let result = self.stop_mcp_server_inner(&bundle_id).await;
            results.push((bundle_id, result));
        }
        results
    }

    pub async fn start_mcp_servers_best_effort(
        &self,
        bundle_ids: Vec<BundleId>,
    ) -> Vec<(BundleId, ComputerError)> {
        let _guard = self.lifecycle_lock.lock().await;
        self.start_mcp_servers_best_effort_inner(bundle_ids).await
    }

    async fn start_mcp_servers_best_effort_inner(
        &self,
        bundle_ids: Vec<BundleId>,
    ) -> Vec<(BundleId, ComputerError)> {
        let mut failures = Vec::new();
        for bundle_id in bundle_ids {
            if let Err(error) = self.start_mcp_server_inner(&bundle_id).await {
                failures.push((bundle_id, error));
            }
        }
        failures
    }

    pub(super) async fn start_desired_mcp_servers_inner(&self) -> Vec<(BundleId, ComputerError)> {
        let bundle_ids = self
            .sdk_mcp_server_ownership_internal()
            .await
            .into_iter()
            .filter(|entry| {
                !entry.disabled || matches!(entry.managed_by, McpOwnership::Plugin { .. })
            })
            .filter_map(|entry| BundleId::try_from(entry.bundle_id.as_str()).ok())
            .collect();
        self.start_mcp_servers_best_effort_inner(bundle_ids).await
    }

    pub(super) fn log_mcp_start_failures(
        &self,
        failures: &[(BundleId, ComputerError)],
        cause: &str,
    ) {
        for (bundle_id, error) in failures {
            log::warn!(
                "Failed to start MCP server for Computer instance {} during {}: {} ({})",
                self.instance.id,
                cause,
                bundle_id,
                error
            );
        }
    }

    pub(super) async fn reconcile_governance_for_computer_start(
        &self,
        cause: &str,
    ) -> ComputerResult<()> {
        match self.reconcile_sdk_governance_inner().await {
            Ok(_) => Ok(()),
            Err(ComputerError::InputResolution(error)) => {
                log::warn!(
                    "Plugin MCP input resolution failed for Computer instance {} during {}: {}",
                    self.instance.id,
                    cause,
                    error
                );
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    pub async fn remount_enabled_plugin_servers(&self) -> ComputerResult<()> {
        let _guard = self.lifecycle_lock.lock().await;
        self.ensure_active_computer()?;
        self.reconcile_sdk_governance_inner().await.map(|_| ())
    }

    pub async fn start_all_mcp_servers(&self) -> ComputerResult<()> {
        let _guard = self.lifecycle_lock.lock().await;
        self.ensure_active_computer()?;
        let bundle_ids = self
            .sdk_mcp_server_ownership()
            .await
            .into_iter()
            .filter_map(|entry| BundleId::try_from(entry.bundle_id).ok())
            .collect();
        let failures = self.start_mcp_servers_best_effort_inner(bundle_ids).await;
        match failures.into_iter().next() {
            Some((_, error)) => Err(error),
            None => Ok(()),
        }
    }

    pub async fn stop_all_mcp_servers(&self) -> Result<(), String> {
        let _guard = self.lifecycle_lock.lock().await;
        self.ensure_active()?;
        let bundle_ids = self
            .sdk_mcp_server_ownership()
            .await
            .into_iter()
            .filter_map(|entry| BundleId::try_from(entry.bundle_id).ok())
            .collect::<Vec<_>>();
        for bundle_id in bundle_ids {
            self.stop_mcp_server_inner(&bundle_id).await?;
        }
        Ok(())
    }

    pub async fn available_tools(&self) -> Result<Vec<Tool>, String> {
        let _activity = self.begin_activity()?;
        self.computer
            .read()
            .await
            .get_available_tools()
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn resources(
        &self,
        bundle_id: &BundleId,
        cursor: Option<String>,
    ) -> Result<(Vec<Resource>, Option<String>), String> {
        let _guard = self.lifecycle_lock.lock().await;
        self.ensure_active()?;
        self.computer
            .read()
            .await
            .get_resources(bundle_id.as_str(), cursor)
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn desktop_windows(
        &self,
        window_uri: Option<&str>,
    ) -> Result<Vec<(BundleId, ServerName, Resource)>, String> {
        let _guard = self.lifecycle_lock.lock().await;
        self.ensure_active()?;
        self.computer
            .read()
            .await
            .list_windows_with_identity(window_uri)
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn window_detail(
        &self,
        bundle_id: &BundleId,
        resource: Resource,
    ) -> Result<ReadResourceResult, String> {
        let _guard = self.lifecycle_lock.lock().await;
        self.ensure_active()?;
        self.computer
            .read()
            .await
            .get_window_detail(bundle_id, resource)
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
        let _activity = self.begin_activity()?;
        self.computer
            .read()
            .await
            .execute_tool_cancellable(req_id, tool_name, parameters, timeout)
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn sdk_tool_history(&self) -> Result<Vec<ToolCallRecord>, String> {
        let _activity = self.begin_activity()?;
        self.computer
            .read()
            .await
            .get_tool_history()
            .await
            .map_err(|error| error.to_string())
    }

    pub async fn add_or_update_input(&self, input: MCPServerInput) -> Result<(), String> {
        let _guard = self.lifecycle_lock.lock().await;
        self.ensure_active()?;
        let input_id = input.id().to_string();
        self.plugin_runtime_inputs
            .write()
            .await
            .insert(input_id.clone(), input.clone());
        self.inputs.write().await.insert(input_id, input.clone());
        self.computer
            .read()
            .await
            .add_or_update_input(input)
            .await
            .map_err(|error| error.to_string())
    }

    /// Adds persisted definitions that were absent when this SDK handle was created.
    ///
    /// This is intentionally narrower than ordinary Input CRUD: callers use it only at an
    /// explicit MCP start boundary, and existing runtime definitions are never refreshed. That
    /// lets a user create a definition in response to a structured start failure and retry only
    /// the affected MCP without hot-applying unrelated Input edits.
    pub async fn materialize_missing_configured_inputs_for_retry(
        &self,
        inputs: Vec<MCPServerInput>,
    ) -> Result<(), String> {
        let _guard = self.lifecycle_lock.lock().await;
        self.ensure_active_computer()
            .map_err(|error| error.to_string())?;
        for input in inputs {
            let input_id = input.id().to_string();
            if self.inputs.read().await.contains_key(&input_id) {
                continue;
            }
            self.computer
                .read()
                .await
                .add_or_update_input(input.clone())
                .await
                .map_err(|error| error.to_string())?;
            self.inputs.write().await.insert(input_id, input);
        }
        Ok(())
    }

    pub async fn synced_sdk_servers(&self) -> HashMap<BundleId, ServerName> {
        self.sdk_servers.read().await.clone()
    }

    pub async fn sdk_mcp_server_ids(&self) -> HashSet<BundleId> {
        self.sdk_mcp_server_config_map().await.into_keys().collect()
    }

    pub async fn sdk_mcp_server_configs(&self) -> HashMap<BundleId, MCPServerConfig> {
        self.sdk_mcp_server_config_map().await
    }

    pub async fn sdk_skill_home(&self) -> PathBuf {
        self.computer.read().await.skill_home()
    }

    pub fn default_skill_home(&self) -> PathBuf {
        default_local_skills_root(&self.skill_home_base, &self.instance.id)
    }

    pub fn configured_skill_home(&self) -> PathBuf {
        instance_config_context(&self.instance, &self.skill_home_base)
            .skill_home()
            .to_path_buf()
    }

    pub async fn sdk_governance_snapshot(
        &self,
    ) -> Result<a2c_smcp::smcp_computer::GovernanceSnapshot, String> {
        let _activity = self.begin_activity()?;
        self.computer
            .read()
            .await
            .governance_snapshot()
            .await
            .map_err(|error| error.to_string())
    }

    pub(crate) fn sdk_skill_reader(&self) -> Result<SdkSkillReadSession<'_>, String> {
        Ok(SdkSkillReadSession {
            runtime: self,
            _activity: self.begin_activity()?,
        })
    }

    pub(crate) async fn acquire_skill_mutation_lease(
        &self,
    ) -> Result<SdkSkillMutationLease, String> {
        let activity = self.begin_activity()?;
        let lifecycle = self.lifecycle_lock.clone().lock_owned().await;
        self.ensure_active()?;
        Ok(SdkSkillMutationLease {
            runtime: self.clone(),
            _activity: activity,
            _lifecycle: lifecycle,
        })
    }

    pub async fn mark_sdk_skills_dirty(&self) {
        let Ok(_activity) = self.begin_activity() else {
            return;
        };
        self.computer.read().await.mark_skills_dirty();
    }

    pub async fn sdk_add_marketplace(
        &self,
        git_url: &str,
        params: AddMarketplaceParams<'_>,
    ) -> Result<MarketplaceAddOutcome, GovernanceError> {
        let _activity = self
            .begin_activity()
            .map_err(PluginInstallError::Precondition)?;
        self.computer
            .read()
            .await
            .add_marketplace(git_url, params)
            .await
    }

    pub async fn sdk_refresh_marketplace(&self, target: &str) -> Vec<MarketplaceRefreshRow> {
        let Ok(_activity) = self.begin_activity() else {
            return Vec::new();
        };
        self.computer.read().await.refresh_marketplace(target).await
    }

    pub async fn sdk_remove_marketplace(
        &self,
        name: &str,
        params: RemoveMarketplaceParams<'_>,
    ) -> Result<MarketplaceRemoveOutcome, GovernanceError> {
        let _activity = self
            .begin_activity()
            .map_err(PluginInstallError::Precondition)?;
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
        let _activity = self
            .begin_activity()
            .map_err(PluginInstallError::Precondition)?;
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
        let _activity = self
            .begin_activity()
            .map_err(PluginInstallError::Precondition)?;
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
        let _activity = self
            .begin_activity()
            .map_err(PluginInstallError::Precondition)?;
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
        let _activity = self
            .begin_activity()
            .map_err(PluginInstallError::Precondition)?;
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

    #[cfg(debug_assertions)]
    #[doc(hidden)]
    pub async fn leave_office_for_test(&self) -> Result<(), String> {
        self.computer
            .read()
            .await
            .leave_office()
            .await
            .map_err(|error| error.to_string())
    }

    async fn sdk_mcp_server_config_map(&self) -> HashMap<BundleId, MCPServerConfig> {
        self.computer
            .read()
            .await
            .list_mcp_servers()
            .await
            .iter()
            .map(|server| (resolve_bundle_id(server), server.clone()))
            .filter(|(bundle_id, _)| bundle_id.as_str() != CLIENT_CONTROL_BUNDLE_ID)
            .collect()
    }

    async fn sdk_user_mcp_server_config_map(&self) -> HashMap<BundleId, MCPServerConfig> {
        let plugin_ids: HashSet<String> = self
            .sdk_mcp_server_ownership()
            .await
            .into_iter()
            .filter(|entry| matches!(entry.managed_by, McpOwnership::Plugin { .. }))
            .map(|entry| entry.bundle_id)
            .collect();
        self.sdk_mcp_server_config_map()
            .await
            .into_iter()
            .filter(|(bundle_id, _)| !plugin_ids.contains(bundle_id.as_str()))
            .collect()
    }

    pub async fn sdk_mcp_server_ownership(&self) -> Vec<McpServerWithMetadata> {
        self.sdk_mcp_server_ownership_internal()
            .await
            .into_iter()
            .filter(|entry| entry.bundle_id != CLIENT_CONTROL_BUNDLE_ID)
            .collect()
    }

    async fn sdk_mcp_server_ownership_internal(&self) -> Vec<McpServerWithMetadata> {
        let computer = self.computer.read().await;
        let mut entries = computer.list_mcp_servers_with_metadata().await;
        drop(computer);
        entries.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.bundle_id.cmp(&right.bundle_id))
        });
        entries
    }

    /// Count the complete SDK MCP inventory for this Computer instance.
    ///
    /// The inventory includes user-configured servers and MCP servers contributed by enabled
    /// plugins. Runtime status is intentionally tracked separately from this configuration view.
    pub async fn mcp_server_inventory_count(&self) -> usize {
        self.sdk_mcp_server_ownership().await.len()
    }

    async fn plugin_mcp_server_owner_inner(
        &self,
        bundle_id: &BundleId,
    ) -> Option<McpServerManagedBy> {
        self.sdk_mcp_server_ownership()
            .await
            .into_iter()
            .find(|entry| entry.bundle_id == bundle_id.as_str())
            .and_then(|entry| sdk_managed_by_to_client(entry.managed_by))
            .filter(McpServerManagedBy::is_plugin_owned)
    }

    async fn reconcile_sdk_governance_inner(&self) -> ComputerResult<Vec<String>> {
        // Fence the complete SDK reconciliation before it can acquire SDK-internal state. Hooks
        // run synchronously inside this call and must not reacquire this non-reentrant lock; this
        // keeps the global order oauth-server fence -> SDK state identical to direct mutations and
        // OAuth flow creation.
        let _oauth_server_guard = self.oauth_server_lifecycle_lock.lock().await;
        let config_context = instance_config_context(&self.instance, &self.skill_home_base);
        let declared = resolve_instance_settings(&config_context);
        let existing_servers = self.sdk_servers.read().await.clone();
        let hooks = RuntimeMcpHooks::new(self, existing_servers)
            .await
            .map_err(ComputerError::RuntimeError)?;
        let report = self
            .computer
            .read()
            .await
            .reconcile_governance(Some(&hooks), Some(&declared))
            .await;
        for bundle_id in hooks.take_diagnostic_reset_ids().await {
            self.clear_mcp_start_diagnostic(&bundle_id).await;
            self.clear_mcp_config_apply_diagnostic(&bundle_id).await;
        }
        for marketplace in report.failed_marketplaces {
            log::warn!(
                "Marketplace '{}' degraded during SDK governance recovery for instance '{}'",
                marketplace,
                self.instance.id
            );
        }
        let plugin_owned_server_ids = self
            .sdk_mcp_server_ownership()
            .await
            .into_iter()
            .filter(|entry| matches!(entry.managed_by, McpOwnership::Plugin { .. }))
            .filter_map(|entry| BundleId::try_from(entry.bundle_id.as_str()).ok());
        self.plugin_mounted_server_ids
            .write()
            .await
            .extend(plugin_owned_server_ids);
        if let Some(error) = hooks.take_input_resolution_error().await {
            return Err(ComputerError::InputResolution(error));
        }
        Ok(hooks
            .registered_server_ids()
            .await
            .into_iter()
            .map(BundleId::into_string)
            .collect())
    }

    async fn replace_sdk_computer(
        &self,
        was_running: bool,
        reason: &str,
        config_mode: HandleReplacementConfig,
    ) -> Result<(), ComputerRuntimeStartError> {
        let injected_inputs = self.plugin_runtime_inputs.read().await.clone();
        let (retained_inputs, retained_mcp_servers) = match config_mode {
            HandleReplacementConfig::ReloadPersisted => (None, None),
            HandleReplacementConfig::RetainCurrentGeneration => {
                let inputs = self.inputs.read().await.clone();
                let servers = self
                    .sdk_user_mcp_server_config_map()
                    .await
                    .into_values()
                    .map(|config| (config.name().to_string(), config))
                    .collect();
                (Some(inputs), Some(servers))
            }
        };
        let (new_computer, sdk_servers, inputs) = build_sdk_computer(
            &self.instance,
            HandleDeclarations {
                injected_inputs: &injected_inputs,
                retained_inputs: retained_inputs.as_ref(),
                retained_mcp_servers: retained_mcp_servers.as_ref(),
            },
            self.session.clone(),
            self.input_resolver.clone(),
            self.oauth_credential_store.clone(),
            &self.skill_home_base,
            self.client_control_binding.clone(),
        );

        let oauth_admission = self.oauth_admission_open.clone();
        // Serialize the admission fence with flow reservation so replacement cannot miss a flow
        // that observed the old handle immediately before the fence closed.
        self.close_oauth_admission().await;
        let _oauth_admission_guard = OAuthAdmissionGuard(oauth_admission);
        self.cancel_all_oauth_authorizations().await;
        if self.has_smcp_transport().await {
            self.clear_smcp_connection_inner().await.map_err(|error| {
                ComputerRuntimeStartError::Client(format!(
                    "Failed to clear SMCP connection before rebuilding SDK Computer for instance {}: {}",
                    self.instance.id, error
                ))
            })?;
        }
        // Handle replacement invalidates every in-flight connection command, including a
        // Manager connect still waiting on HTTP before any transport exists.
        self.cancel_connection_operation_for_handle_replacement()
            .await;

        self.shutdown_sdk_computer_inner()
            .await
            .map_err(ComputerRuntimeStartError::Client)?;
        self.stop_runtime_event_relay().await;
        // The old relay may have consumed a queued status after cancellation began. Clear once
        // more after joining it so no scope observation crosses the SDK handle generation.
        self.oauth_required_scopes.write().await.clear();
        {
            let _snapshot_guard = self.runtime_snapshot_lock.lock().await;
            // Problem cleanup is part of the committed handle replacement. Until shutdown
            // succeeds, the old generation remains authoritative and must retain its diagnostics.
            self.clear_connection_diagnostic_for_handle_replacement_silent()
                .await;
            self.clear_client_runtime_diagnostic_silent().await;
            self.mcp_start_diagnostics.write().await.clear();
            self.mcp_config_apply_diagnostics.write().await.clear();
            let mut computer = self.computer.write().await;
            self.runtime_generation.fetch_add(1, Ordering::AcqRel);
            *computer = new_computer;
            self.shutdown_completed.store(false, Ordering::Release);
        }
        *self.inputs.write().await = inputs;
        *self.sdk_servers.write().await = sdk_servers;
        self.plugin_mounted_server_ids.write().await.clear();
        self.start_runtime_event_relay().await;
        self.publish_runtime_status(ComputerRuntimeEventCause::HandleReplaced {
            reason: reason.to_string(),
        })
        .await;

        if was_running {
            self.computer
                .read()
                .await
                .boot_up()
                .await
                .map_err(ComputerRuntimeStartError::Sdk)?;
            self.reconcile_governance_for_computer_start(reason)
                .await
                .map_err(ComputerRuntimeStartError::Sdk)?;
            let failures = self.start_desired_mcp_servers_inner().await;
            self.log_mcp_start_failures(&failures, reason);
        }
        Ok(())
    }
}

async fn remove_tracked_plugin_server<F, C>(
    bundle_id: &BundleId,
    sdk_servers: &Arc<RwLock<HashMap<BundleId, ServerName>>>,
    plugin_mounted_server_ids: &Arc<RwLock<HashSet<BundleId>>>,
    unmount: F,
    on_removed: C,
) -> Result<bool, String>
where
    F: std::future::Future<Output = ComputerResult<bool>>,
    C: std::future::Future<Output = ()>,
{
    if !plugin_mounted_server_ids.read().await.contains(bundle_id) {
        return Ok(false);
    }

    // Preserve ownership until the SDK confirms the runtime side was removed.
    // A failed unmount must remain observable and retryable by the caller.
    unmount.await.map_err(|error| error.to_string())?;
    plugin_mounted_server_ids.write().await.remove(bundle_id);
    sdk_servers.write().await.remove(bundle_id);
    on_removed.await;
    Ok(true)
}

fn build_sdk_computer(
    instance: &ComputerInstance,
    declarations: HandleDeclarations<'_>,
    session: InstanceSession,
    input_resolver: Arc<RuntimeInputResolver>,
    oauth_credential_store: Arc<KeychainOAuthCredentialStore>,
    skill_home_base: &Path,
    client_control_binding: ClientControlBinding,
) -> (
    Computer<InstanceSession>,
    HashMap<BundleId, ServerName>,
    HashMap<String, MCPServerInput>,
) {
    let instance_storage_root = skill_home_base.join(instance_storage_dir_name(&instance.id));
    let config_context = instance_config_context(instance, skill_home_base);
    let skill_home = config_context.skill_home().to_path_buf();
    let snapshot = config_context.load();
    let mut inputs = declarations.retained_inputs.cloned().unwrap_or_else(|| {
        snapshot
            .inputs
            .inputs
            .into_iter()
            .map(|input| (input.id().to_string(), input))
            .collect::<HashMap<_, _>>()
    });
    inputs.extend(declarations.injected_inputs.clone());
    let mut mcp_servers: HashMap<String, MCPServerConfig> = declarations
        .retained_mcp_servers
        .cloned()
        .unwrap_or_else(|| {
            snapshot
                .mcp
                .servers
                .into_iter()
                // Plugin servers are a read-side projection derived from the governance ledger,
                // not durable declarations. Feeding them back into a fresh Computer would make
                // governance reconciliation treat them as pre-existing and skip input
                // injection/remount.
                .filter(|server| server.origin != ProvenanceScope::Plugin)
                .filter(|server| {
                    let reserved = resolve_bundle_id(&server.config).as_str()
                        == CLIENT_CONTROL_BUNDLE_ID;
                    if reserved {
                        log::warn!(
                            "Ignoring durable MCP declaration with reserved bundleId '{}' for Computer {}",
                            CLIENT_CONTROL_BUNDLE_ID,
                            instance.id
                        );
                    }
                    !reserved
                })
                .map(|server| (server.name, normalize_mcp_server_tool_meta(server.config)))
                .collect()
        });
    if instance.remote_control.enabled {
        mcp_servers.insert(
            CLIENT_CONTROL_BUNDLE_ID.to_string(),
            client_control_server_config(),
        );
    }
    let sdk_servers = mcp_servers
        .values()
        .map(|config| (resolve_bundle_id(config), config.name().to_string()))
        .collect();
    let source_id = instance.id.clone();
    let factory: ClientFactory = Arc::new(move |config, notify| {
        if config.bundle_id().map(BundleId::as_str) == Some(CLIENT_CONTROL_BUNDLE_ID) {
            Arc::new(ClientControlMcpClient::new(
                source_id.clone(),
                client_control_binding.clone(),
                notify,
            ))
        } else {
            client_factory(config, notify)
        }
    });
    let computer = Computer::new(
        instance.name.clone(),
        session,
        Some(inputs.clone()),
        Some(mcp_servers),
        instance.connection_policy.auto_connect,
        true,
    )
    .with_client_factory(factory);

    let computer = computer
        .with_input_resolver(input_resolver.clone())
        .with_secret_resolver(input_resolver)
        .with_oauth_credential_store(oauth_credential_store)
        .with_skill_home(skill_home)
        .with_config_dir(config_context.project_anchor())
        .with_config_env(config_context.env().clone())
        .with_blob_cache_root(instance_storage_root.join("blob"));
    (computer, sdk_servers, inputs)
}

struct RuntimeMcpHooks {
    runtime: ComputerInstanceRuntime,
    computer: Arc<RwLock<Computer<InstanceSession>>>,
    inputs: Arc<RwLock<HashMap<String, MCPServerInput>>>,
    plugin_runtime_inputs: Arc<RwLock<HashMap<String, MCPServerInput>>>,
    sdk_servers: Arc<RwLock<HashMap<BundleId, ServerName>>>,
    plugin_mounted_server_ids: Arc<RwLock<HashSet<BundleId>>>,
    existing_servers: HashMap<BundleId, ServerName>,
    bundled_server_ids: HashSet<BundleId>,
    disabled_independent_server_ids: HashSet<BundleId>,
    root_ownership: HashMap<PathBuf, (String, String)>,
    registered_server_ids: Arc<Mutex<Vec<BundleId>>>,
    diagnostic_reset_ids: Arc<Mutex<Vec<BundleId>>>,
    preserved_registration_counts: Arc<Mutex<HashMap<BundleId, usize>>>,
    first_input_resolution_error: Arc<Mutex<Option<InputResolutionError>>>,
}

impl RuntimeMcpHooks {
    async fn new(
        runtime: &ComputerInstanceRuntime,
        existing_servers: HashMap<BundleId, ServerName>,
    ) -> Result<Self, String> {
        let snapshot = runtime
            .computer
            .read()
            .await
            .governance_snapshot()
            .await
            .map_err(|error| format!("Failed to load SDK governance snapshot: {error}"))?;
        let mut root_ownership = HashMap::new();
        let mut bundled_server_ids = HashSet::new();
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
            for raw_bundle_id in plugin.bundled_mcp_servers {
                let bundle_id = BundleId::try_from(raw_bundle_id.as_str()).map_err(|error| {
                    format!("Invalid bundled MCP server id '{raw_bundle_id}' in governance snapshot: {error}")
                })?;
                bundled_server_ids.insert(bundle_id);
            }
        }
        let disabled_independent_server_ids =
            instance_config_context(&runtime.instance, &runtime.skill_home_base)
                .load()
                .mcp
                .servers
                .into_iter()
                .filter(|server| {
                    server.origin != ProvenanceScope::Plugin && server.config.disabled()
                })
                .map(|server| resolve_bundle_id(&server.config))
                .collect::<HashSet<_>>();
        let existing_servers = existing_servers
            .into_iter()
            .filter(|(bundle_id, _)| {
                !bundled_server_ids.contains(bundle_id)
                    || !disabled_independent_server_ids.contains(bundle_id)
            })
            .collect();
        Ok(Self {
            runtime: runtime.clone(),
            computer: runtime.computer.clone(),
            inputs: runtime.inputs.clone(),
            plugin_runtime_inputs: runtime.plugin_runtime_inputs.clone(),
            sdk_servers: runtime.sdk_servers.clone(),
            plugin_mounted_server_ids: runtime.plugin_mounted_server_ids.clone(),
            existing_servers,
            bundled_server_ids,
            disabled_independent_server_ids,
            root_ownership,
            registered_server_ids: Arc::new(Mutex::new(Vec::new())),
            diagnostic_reset_ids: Arc::new(Mutex::new(Vec::new())),
            preserved_registration_counts: Arc::new(Mutex::new(HashMap::new())),
            first_input_resolution_error: Arc::new(Mutex::new(None)),
        })
    }

    async fn registered_server_ids(&self) -> Vec<BundleId> {
        self.registered_server_ids.lock().await.clone()
    }

    async fn take_diagnostic_reset_ids(&self) -> Vec<BundleId> {
        std::mem::take(&mut *self.diagnostic_reset_ids.lock().await)
    }

    async fn take_input_resolution_error(&self) -> Option<InputResolutionError> {
        self.first_input_resolution_error.lock().await.take()
    }
}

#[async_trait]
impl McpInstallHooks for RuntimeMcpHooks {
    fn existing_servers(&self) -> HashMap<BundleId, ServerName> {
        self.existing_servers.clone()
    }

    async fn register_server(&self, cfg: MCPServerConfig) -> Result<(), McpHookError> {
        let name = cfg.name().to_string();
        let bundle_id = resolve_bundle_id(&cfg);
        if bundle_id.as_str() == CLIENT_CONTROL_BUNDLE_ID {
            return Err(McpHookError(
                "Plugin MCP server uses reserved bundleId 'client_control'".to_string(),
            ));
        }
        if !self.bundled_server_ids.contains(&bundle_id) {
            return Err(McpHookError(format!(
                "Missing plugin ownership metadata for bundled MCP server '{bundle_id}'"
            )));
        }
        if self.sdk_servers.read().await.contains_key(&bundle_id)
            && !self.disabled_independent_server_ids.contains(&bundle_id)
        {
            *self
                .preserved_registration_counts
                .lock()
                .await
                .entry(bundle_id.clone())
                .or_default() += 1;
            self.diagnostic_reset_ids
                .lock()
                .await
                .push(bundle_id.clone());
            self.registered_server_ids.lock().await.push(bundle_id);
            return Ok(());
        }
        let cfg = if self.disabled_independent_server_ids.contains(&bundle_id) {
            force_mcp_server_enabled(cfg)
        } else {
            cfg
        };
        self.runtime
            .cancel_oauth_before_server_lifecycle_change(&bundle_id)
            .await
            .map_err(|error| McpHookError(error.to_string()))?;
        let mount_result = self
            .computer
            .read()
            .await
            .mount_server(normalize_mcp_server_tool_meta(cfg))
            .await;
        if let Err(ComputerError::InputResolution(error)) = &mount_result {
            let mut first_error = self.first_input_resolution_error.lock().await;
            if first_error.is_none() {
                *first_error = Some(error.clone());
            }
        }
        mount_result.map_err(|error| McpHookError(error.to_string()))?;
        self.sdk_servers
            .write()
            .await
            .insert(bundle_id.clone(), name);
        self.plugin_mounted_server_ids
            .write()
            .await
            .insert(bundle_id.clone());
        self.diagnostic_reset_ids
            .lock()
            .await
            .push(bundle_id.clone());
        self.registered_server_ids.lock().await.push(bundle_id);
        Ok(())
    }

    async fn remove_server(&self, bundle_id: &BundleId) -> Result<(), McpHookError> {
        let mut preserved = self.preserved_registration_counts.lock().await;
        if let Some(count) = preserved.get_mut(bundle_id) {
            *count -= 1;
            if *count == 0 {
                preserved.remove(bundle_id);
            }
            drop(preserved);
            self.diagnostic_reset_ids
                .lock()
                .await
                .push(bundle_id.clone());
            return Ok(());
        }
        drop(preserved);
        self.runtime
            .cancel_oauth_before_server_lifecycle_change(bundle_id)
            .await
            .map_err(|error| McpHookError(error.to_string()))?;
        remove_tracked_plugin_server(
            bundle_id,
            &self.sdk_servers,
            &self.plugin_mounted_server_ids,
            async { self.computer.read().await.unmount_server(bundle_id).await },
            async {},
        )
        .await
        .map_err(McpHookError)?;
        self.diagnostic_reset_ids
            .lock()
            .await
            .push(bundle_id.clone());
        Ok(())
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

        for input in inputs {
            self.plugin_runtime_inputs
                .write()
                .await
                .insert(input.id().to_string(), input.clone());
            self.inputs
                .write()
                .await
                .insert(input.id().to_string(), input.clone());
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

pub(crate) fn force_mcp_server_enabled(mut config: MCPServerConfig) -> MCPServerConfig {
    match &mut config {
        MCPServerConfig::Stdio(server) => server.disabled = false,
        MCPServerConfig::Sse(server) => server.disabled = false,
        MCPServerConfig::Http(server) => server.disabled = false,
    }
    config
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

pub(crate) fn configured_skill_home_for_instance(
    instance: &ComputerInstance,
    skill_home_base: &Path,
) -> PathBuf {
    instance_config_context(instance, skill_home_base)
        .skill_home()
        .to_path_buf()
}

fn resolve_instance_settings(
    context: &InstanceConfigContext,
) -> serde_json::Map<String, serde_json::Value> {
    context.with_read(|| {
        let policy = resolve_policy_settings(Some(context.env()), None, None);
        resolve_settings(ResolveSettingsArgs {
            cwd: Some(context.project_anchor()),
            env: Some(context.env()),
            flag_settings_path: None,
            policy_settings: Some(&policy),
        })
        .settings
    })
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

fn runtime_stored_input_kind(input: &MCPServerInput) -> Option<InputKind> {
    match input {
        MCPServerInput::PromptString(input) => Some(if input.password == Some(true) {
            InputKind::Secret
        } else {
            InputKind::Value
        }),
        MCPServerInput::PickString(_) => Some(InputKind::Value),
        MCPServerInput::Command(_) => None,
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
    use crate::commands::connection::{settle_refresh_terminal, RefreshTerminalOutcome};

    #[test]
    fn is_expected_oauth_required_classifies_both_sdk_error_paths() {
        // Path A — a fresh connect surfaces the structured OAuth challenge, which the SDK models as
        // HttpAuthentication(OAuthRequired).
        let fresh_challenge: ComputerResult<()> = Err(ComputerError::HttpAuthentication(
            HttpAuthenticationError::OAuthRequired,
        ));
        assert!(ComputerInstanceRuntime::is_expected_oauth_required(
            &fresh_challenge
        ));

        // Path B — a restart after credentials were cleared keeps the OAuth coordinator admitted
        // but with no stored token, so prepare_request fails and the SDK wraps the same
        // "OAuth authorization is required" condition into a ConnectionError.
        let cleared_restart: ComputerResult<()> = Err(ComputerError::ConnectionError(
            "Failed to connect to svc: Connection error: OAuth request preparation failed: \
             OAuth protocol error: OAuth authorization is required"
                .to_string(),
        ));
        assert!(ComputerInstanceRuntime::is_expected_oauth_required(
            &cleared_restart
        ));

        // A genuine connection failure must not be misclassified as a soft "needs authorization"
        // state and silently swallowed.
        let connectivity_failure: ComputerResult<()> = Err(ComputerError::ConnectionError(
            "Failed to connect to svc: Connection error: DNS resolution failed".to_string(),
        ));
        assert!(!ComputerInstanceRuntime::is_expected_oauth_required(
            &connectivity_failure
        ));

        // A successful start is not an "awaiting authorization" state.
        assert!(!ComputerInstanceRuntime::is_expected_oauth_required(
            &Ok(())
        ));
    }

    #[derive(Default)]
    struct RecordingRuntimeEventSink {
        events: std::sync::Mutex<Vec<ComputerRuntimeStatusEvent>>,
        changed: tokio::sync::Notify,
    }

    impl ComputerRuntimeEventSink for RecordingRuntimeEventSink {
        fn emit(&self, event: &ComputerRuntimeStatusEvent) -> Result<(), String> {
            self.events.lock().unwrap().push(event.clone());
            self.changed.notify_waiters();
            Ok(())
        }
    }

    impl RecordingRuntimeEventSink {
        fn event_count(&self) -> usize {
            self.events.lock().unwrap().len()
        }

        async fn wait_for(
            &self,
            predicate: impl Fn(&ComputerRuntimeStatusEvent) -> bool,
        ) -> ComputerRuntimeStatusEvent {
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    let changed = self.changed.notified();
                    if let Some(event) = self
                        .events
                        .lock()
                        .unwrap()
                        .iter()
                        .find(|event| predicate(event))
                        .cloned()
                    {
                        return event;
                    }
                    changed.await;
                }
            })
            .await
            .expect("runtime event was not emitted")
        }
    }

    fn instance(id: &str, name: &str) -> ComputerInstance {
        ComputerInstance::new(id, name)
    }

    #[test]
    fn computer_profile_v2_preserves_opaque_numeric_robot_account_snapshots() {
        let profile: ComputerProfile = serde_json::from_value(serde_json::json!({
            "schema_version": COMPUTER_PROFILE_SCHEMA_VERSION,
            "id": "computer-1",
            "name": "Computer",
            "connection_policy": {
                "target": {
                    "type": "manager_robot",
                    "contextKey": {
                        "environment": "staging",
                        "accountId": "account-a",
                        "organizationId": "org-a"
                    },
                    "employeeId": 11,
                    "lastResolvedRobotAccountId": 4200
                },
                "auto_connect": true
            },
            "robot_binding": {
                "context_key": {
                    "environment": "staging",
                    "accountId": "account-a",
                    "organizationId": "org-a"
                },
                "state": "active",
                "employee_id": 11,
                "last_resolved_robot_account_id": 4200
            }
        }))
        .expect("numeric opaque snapshots should remain readable");

        let Some(ComputerConnectionTarget::ManagerRobot {
            last_resolved_robot_account_id,
            ..
        }) = profile.connection_policy.target.as_ref()
        else {
            panic!("expected Manager Robot target");
        };
        assert_eq!(last_resolved_robot_account_id.as_deref(), Some("4200"));
        assert_eq!(
            profile
                .robot_binding
                .as_ref()
                .and_then(|binding| binding.last_resolved_robot_account_id.as_deref()),
            Some("4200")
        );

        let serialized = serde_json::to_value(profile).unwrap();
        assert_eq!(
            serialized["connection_policy"]["target"]["lastResolvedRobotAccountId"],
            serde_json::json!("4200")
        );
        assert_eq!(
            serialized["robot_binding"]["last_resolved_robot_account_id"],
            serde_json::json!("4200")
        );
    }

    fn instance_with_input(id: &str, label: &str) -> ComputerInstance {
        let mut instance = ComputerInstance::new(id, "Computer");
        instance.inputs = vec![InputDefinition::PromptString {
            id: "api-key".to_string(),
            label: Some(label.to_string()),
            description: None,
            default: None,
            password: Some(true),
        }];
        instance
    }

    fn seed_sdk_input(instance: &ComputerInstance, skill_home_base: &Path, label: &str) {
        let context = instance_config_context(instance, skill_home_base);
        let definition = MCPServerInput::PromptString(PromptStringInput {
            id: "api-key".to_string(),
            description: label.to_string(),
            default: None,
            password: Some(true),
        });
        let document = a2c_smcp::smcp_computer::settings::config::ProjectConfigDoc {
            mcp: Some(
                serde_json::json!({ "inputs": [definition] })
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
            ..Default::default()
        };
        a2c_smcp::smcp_computer::settings::config::save_config(context.project_anchor(), &document)
            .unwrap();
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

    #[tokio::test]
    async fn failed_plugin_server_unmount_preserves_tracking_for_retry() {
        let bundle_id = BundleId::try_from("plugin-mcp").unwrap();
        let sdk_servers = Arc::new(RwLock::new(HashMap::from([(
            bundle_id.clone(),
            "Plugin MCP".to_string(),
        )])));
        let plugin_mounted_server_ids = Arc::new(RwLock::new(HashSet::from([bundle_id.clone()])));
        let diagnostic_cleared = Arc::new(AtomicBool::new(false));

        let error = remove_tracked_plugin_server(
            &bundle_id,
            &sdk_servers,
            &plugin_mounted_server_ids,
            async {
                Err(ComputerError::RuntimeError(
                    "injected unmount failure".to_string(),
                ))
            },
            {
                let diagnostic_cleared = diagnostic_cleared.clone();
                async move {
                    diagnostic_cleared.store(true, Ordering::SeqCst);
                }
            },
        )
        .await
        .unwrap_err();

        assert_eq!(error, "Runtime error: injected unmount failure");
        assert!(sdk_servers.read().await.contains_key(&bundle_id));
        assert!(plugin_mounted_server_ids.read().await.contains(&bundle_id));
        assert!(
            !diagnostic_cleared.load(Ordering::SeqCst),
            "failed unmount must retain owner diagnostics"
        );

        remove_tracked_plugin_server(
            &bundle_id,
            &sdk_servers,
            &plugin_mounted_server_ids,
            async { Ok(true) },
            {
                let diagnostic_cleared = diagnostic_cleared.clone();
                async move {
                    diagnostic_cleared.store(true, Ordering::SeqCst);
                }
            },
        )
        .await
        .unwrap();

        assert!(!sdk_servers.read().await.contains_key(&bundle_id));
        assert!(!plugin_mounted_server_ids.read().await.contains(&bundle_id));
        assert!(diagnostic_cleared.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn successful_plugin_server_removal_clears_current_diagnostics() {
        let runtime = ComputerInstanceRuntime::new(
            instance("one", "One"),
            std::env::temp_dir().join("tfrobot-client-test-skill-home"),
        );
        runtime.start().await.unwrap();
        let server = server_config("plugin-mcp");
        let bundle_id = resolve_bundle_id(&server);
        runtime.add_or_update_plugin_server(server).await.unwrap();
        runtime.mcp_start_diagnostics.write().await.insert(
            bundle_id.clone(),
            RuntimeDiagnosticRecord::new("start", "plugin start failed"),
        );
        runtime
            .record_mcp_config_apply_diagnostic(
                bundle_id.clone(),
                "plugin configuration failed".to_string(),
            )
            .await;
        assert_eq!(runtime.runtime_snapshot().await.problems.len(), 2);

        runtime.remove_plugin_server(&bundle_id).await.unwrap();

        assert!(!runtime.sdk_servers.read().await.contains_key(&bundle_id));
        assert!(!runtime
            .plugin_mounted_server_ids
            .read()
            .await
            .contains(&bundle_id));
        assert!(!runtime
            .mcp_start_diagnostics
            .read()
            .await
            .contains_key(&bundle_id));
        assert!(!runtime
            .mcp_config_apply_diagnostics
            .read()
            .await
            .contains_key(&bundle_id));
        assert!(runtime.runtime_snapshot().await.problems.is_empty());
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn preserved_plugin_owner_transitions_request_published_diagnostic_cleanup() {
        let runtime = ComputerInstanceRuntime::new(
            instance("one", "One"),
            std::env::temp_dir().join("tfrobot-client-test-skill-home"),
        );
        runtime.start().await.unwrap();
        let server = server_config("shared-mcp");
        let bundle_id = resolve_bundle_id(&server);
        runtime
            .sdk_servers
            .write()
            .await
            .insert(bundle_id.clone(), server.name().to_string());
        runtime.mcp_start_diagnostics.write().await.insert(
            bundle_id.clone(),
            RuntimeDiagnosticRecord::new("start", "previous owner failed"),
        );

        let hooks = RuntimeMcpHooks {
            runtime: runtime.clone(),
            computer: runtime.computer.clone(),
            inputs: runtime.inputs.clone(),
            plugin_runtime_inputs: runtime.plugin_runtime_inputs.clone(),
            sdk_servers: runtime.sdk_servers.clone(),
            plugin_mounted_server_ids: runtime.plugin_mounted_server_ids.clone(),
            existing_servers: runtime.sdk_servers.read().await.clone(),
            bundled_server_ids: HashSet::from([bundle_id.clone()]),
            disabled_independent_server_ids: HashSet::new(),
            root_ownership: HashMap::new(),
            registered_server_ids: Arc::new(Mutex::new(Vec::new())),
            diagnostic_reset_ids: Arc::new(Mutex::new(Vec::new())),
            preserved_registration_counts: Arc::new(Mutex::new(HashMap::new())),
            first_input_resolution_error: Arc::new(Mutex::new(None)),
        };

        hooks.register_server(server).await.unwrap();
        for reset_id in hooks.take_diagnostic_reset_ids().await {
            runtime.clear_mcp_start_diagnostic(&reset_id).await;
            runtime.clear_mcp_config_apply_diagnostic(&reset_id).await;
        }
        assert!(
            runtime.runtime_snapshot().await.problems.is_empty(),
            "user-to-plugin owner transition must clear the previous owner's problem"
        );

        runtime
            .record_mcp_config_apply_diagnostic(
                bundle_id.clone(),
                "plugin owner failed".to_string(),
            )
            .await;
        hooks.remove_server(&bundle_id).await.unwrap();
        for reset_id in hooks.take_diagnostic_reset_ids().await {
            runtime.clear_mcp_start_diagnostic(&reset_id).await;
            runtime.clear_mcp_config_apply_diagnostic(&reset_id).await;
        }
        assert!(
            runtime.runtime_snapshot().await.problems.is_empty(),
            "plugin-to-user owner transition must clear the previous owner's problem"
        );
        runtime.shutdown().await;
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
        assert!(!Arc::ptr_eq(
            &one.runtime_generation,
            &two.runtime_generation
        ));
        assert_ne!(one.runtime_incarnation, two.runtime_incarnation);
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
    async fn runtime_seeds_smcp_input_definitions_from_sdk_project_config() {
        let directory = tempfile::tempdir().unwrap();
        let skill_home_base = directory.path().to_path_buf();
        let instance = instance("one", "One");
        seed_sdk_input(&instance, &skill_home_base, "API Key");
        let runtime = ComputerInstanceRuntime::new(instance, skill_home_base);

        let inputs = runtime.inputs.read().await;
        let input = inputs.get("api-key").expect("input should be loaded");

        match input {
            MCPServerInput::PromptString(prompt) => {
                assert_eq!(prompt.description, "API Key");
                assert_eq!(prompt.default, None);
                assert_eq!(prompt.password, Some(true));
            }
            other => panic!("expected PromptString input, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn instance_session_does_not_receive_transient_resolved_values() {
        let directory = tempfile::tempdir().unwrap();
        let skill_home_base = directory.path().to_path_buf();
        let instance = instance_with_input_value("one", serde_json::json!("persisted-secret"));
        seed_sdk_input(&instance, &skill_home_base, "API Key");
        let runtime = ComputerInstanceRuntime::new(instance, skill_home_base);
        let inputs = runtime.inputs.read().await;
        let input = inputs.get("api-key").unwrap();

        assert_eq!(
            runtime.session.resolve_input(input).await.unwrap(),
            serde_json::json!("")
        );
    }

    #[tokio::test]
    async fn instance_session_falls_back_to_sdk_input_semantics() {
        let session = InstanceSession::new("one");
        let pick = MCPServerInput::PickString(PickStringInput {
            id: "runtime".to_string(),
            description: "Runtime".to_string(),
            options: vec![
                PickStringOption {
                    label: "Node".to_string(),
                    value: "node".to_string(),
                },
                PickStringOption {
                    label: "Python".to_string(),
                    value: "python".to_string(),
                },
            ],
            default: None,
        });
        let command = MCPServerInput::Command(CommandInput {
            id: "command".to_string(),
            description: "Command".to_string(),
            command: "echo".to_string(),
            args: Some(HashMap::from([
                ("000000".to_string(), "hello".to_string()),
                ("000001".to_string(), "world".to_string()),
            ])),
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
        assert_eq!(one.runtime_state().await, ComputerRuntimeState::Started);
        assert!(!registry.runtime("two").await.unwrap().is_running().await);

        registry.stop_runtime("one").await.unwrap();
        let one = registry.runtime("one").await.unwrap();
        assert!(!one.is_running().await);
        assert!(!one.sdk_is_mcp_manager_initialized().await);
        assert_eq!(one.runtime_state().await, ComputerRuntimeState::Shutdown);
        assert!(!registry.runtime("two").await.unwrap().is_running().await);
    }

    #[tokio::test]
    async fn unconfirmed_sdk_shutdown_is_not_treated_as_successful_teardown() {
        let runtime = ComputerInstanceRuntime::new(
            instance("one", "One"),
            std::env::temp_dir().join("tfrobot-client-test-skill-home"),
        );

        runtime.computer.read().await.shutdown().await.unwrap();
        let error = runtime.try_shutdown().await.unwrap_err();

        assert!(error.contains("reached Shutdown without client teardown confirmation"));
        assert!(!runtime.shutdown_completed.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn client_runtime_diagnostic_events_are_deduplicated_and_clearable() {
        let registry = ComputerRegistry::from_config(ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![instance("one", "One")],
        });
        let sink = Arc::new(RecordingRuntimeEventSink::default());
        registry.set_runtime_event_sink(sink.clone()).await;
        let runtime = registry.runtime("one").await.unwrap();

        runtime
            .set_client_runtime_diagnostic(
                "connect",
                Some("SMCP connection failed; see logs for details".to_string()),
            )
            .await;
        runtime
            .set_client_runtime_diagnostic(
                "connect",
                Some("SMCP connection failed; see logs for details".to_string()),
            )
            .await;

        let events = sink.events.lock().unwrap().clone();
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(
                    &event.cause,
                    ComputerRuntimeEventCause::ClientDiagnosticChanged {
                        has_error: true,
                        ..
                    }
                ))
                .count(),
            1
        );
        let snapshot = runtime.runtime_snapshot().await;
        assert_eq!(snapshot.problems.len(), 1);
        assert_eq!(snapshot.problems[0].operation, "connect");
        assert_eq!(
            snapshot.problems[0].technical_detail.as_deref(),
            Some("SMCP connection failed; see logs for details")
        );

        runtime
            .client_runtime_diagnostic
            .write()
            .await
            .as_mut()
            .unwrap()
            .occurred_at = "first-occurrence".to_string();
        runtime
            .set_client_runtime_diagnostic(
                "connect",
                Some("SMCP connection still unavailable; see logs for details".to_string()),
            )
            .await;
        runtime
            .set_client_runtime_diagnostic("disconnect", None)
            .await;
        let snapshot = runtime.runtime_snapshot().await;
        assert_eq!(snapshot.problems.len(), 1);
        assert_eq!(snapshot.problems[0].operation, "connect");
        assert_eq!(snapshot.problems[0].occurred_at, "first-occurrence");
        assert_eq!(
            snapshot.problems[0].technical_detail.as_deref(),
            Some("SMCP connection still unavailable; see logs for details")
        );
        let events = sink.events.lock().unwrap().clone();
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(
                    &event.cause,
                    ComputerRuntimeEventCause::ClientDiagnosticChanged {
                        operation,
                        has_error: false,
                    } if operation == "disconnect"
                ))
                .count(),
            0
        );

        runtime.set_client_runtime_diagnostic("connect", None).await;
        let events = sink.events.lock().unwrap().clone();
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(
                    &event.cause,
                    ComputerRuntimeEventCause::ClientDiagnosticChanged {
                        has_error: false,
                        ..
                    }
                ))
                .count(),
            1
        );
        assert!(runtime.runtime_snapshot().await.problems.is_empty());
    }

    #[tokio::test]
    async fn repeated_mcp_failures_preserve_each_problem_occurrence() {
        let runtime = ComputerInstanceRuntime::new(
            instance("one", "One"),
            std::env::temp_dir().join("tfrobot-client-test-skill-home"),
        );
        runtime.start().await.unwrap();
        let bundle_id = BundleId::try_from("server-a").unwrap();
        runtime
            .sdk_servers
            .write()
            .await
            .insert(bundle_id.clone(), "Server A".to_string());

        runtime
            .record_mcp_start_diagnostic(bundle_id.clone(), "first start failure".to_string())
            .await;
        runtime
            .mcp_start_diagnostics
            .write()
            .await
            .get_mut(&bundle_id)
            .unwrap()
            .occurred_at = "first-start-occurrence".to_string();
        runtime
            .record_mcp_start_diagnostic(bundle_id.clone(), "updated start failure".to_string())
            .await;

        runtime
            .record_mcp_config_apply_diagnostic(
                bundle_id.clone(),
                "first configuration failure".to_string(),
            )
            .await;
        runtime
            .mcp_config_apply_diagnostics
            .write()
            .await
            .get_mut(&bundle_id)
            .unwrap()
            .occurred_at = "first-configuration-occurrence".to_string();
        runtime
            .record_mcp_config_apply_diagnostic(
                bundle_id.clone(),
                "updated configuration failure".to_string(),
            )
            .await;

        let problems = runtime.runtime_snapshot().await.problems;
        let start = problems
            .iter()
            .find(|problem| problem.operation == "start")
            .unwrap();
        assert_eq!(start.occurred_at, "first-start-occurrence");
        assert_eq!(
            start.technical_detail.as_deref(),
            Some("updated start failure")
        );
        let configuration = problems
            .iter()
            .find(|problem| problem.operation == "apply_configuration")
            .unwrap();
        assert_eq!(configuration.occurred_at, "first-configuration-occurrence");
        assert_eq!(
            configuration.technical_detail.as_deref(),
            Some("updated configuration failure")
        );
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn failed_first_mcp_mount_is_structured_before_runtime_inventory_admission() {
        let registry = ComputerRegistry::from_config(ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![instance("one", "One")],
        });
        let sink = Arc::new(RecordingRuntimeEventSink::default());
        registry.set_runtime_event_sink(sink.clone()).await;
        registry.start_runtime("one").await.unwrap();
        let runtime = registry.runtime("one").await.unwrap();
        let bundle_id = BundleId::try_from("server-a").unwrap();

        runtime
            .record_mcp_config_apply_diagnostic_for_server(
                bundle_id.clone(),
                "Server A".to_string(),
                "active process rejected the saved configuration".to_string(),
            )
            .await;

        assert!(!runtime.sdk_servers.read().await.contains_key(&bundle_id));
        let problem = runtime.runtime_snapshot().await.problems.pop().unwrap();
        assert_eq!(problem.operation, "apply_configuration");
        assert!(matches!(
            problem.affected_capabilities.as_slice(),
            [crate::services::computer_runtime_events::ComputerRuntimeAffectedCapability::McpServer {
                bundle_id: affected_bundle_id,
                name: Some(name),
            }] if affected_bundle_id == "server-a" && name == "Server A"
        ));
        assert!(problem
            .technical_detail
            .as_deref()
            .is_some_and(|detail| detail.contains("rejected")));
        runtime.clear_mcp_config_apply_diagnostic(&bundle_id).await;
        assert!(runtime.runtime_snapshot().await.problems.is_empty());
        assert!(sink.events.lock().unwrap().iter().any(|event| matches!(
            &event.cause,
            ComputerRuntimeEventCause::McpDiagnosticChanged {
                has_error: false,
                ..
            }
        )));
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn connection_state_changes_publish_versioned_runtime_events() {
        let registry = ComputerRegistry::from_config(ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![instance("one", "One")],
        });
        let sink = Arc::new(RecordingRuntimeEventSink::default());
        registry.set_runtime_event_sink(sink.clone()).await;
        let runtime = registry.runtime("one").await.unwrap();

        let initial_connected_at = chrono::Utc::now() - chrono::Duration::hours(1);
        runtime
            .install_connection_state(ConnectionState {
                profile_name: "manual".to_string(),
                url: "https://smcp.example.com".to_string(),
                office_id: "office-a".to_string(),
                computer_name: "One".to_string(),
                connected_at: initial_connected_at,
                source_type: "manual_smcp".to_string(),
                target_id: Some("target-a".to_string()),
                target_name: Some("Target A".to_string()),
                employee_id: None,
                generation: 7,
            })
            .await
            .unwrap();

        let installed = sink
            .wait_for(|event| {
                matches!(
                    &event.cause,
                    ComputerRuntimeEventCause::ClientConnectionStateChanged {
                        revision: 1,
                        status: ClientConnectionStatus::Connected,
                    }
                )
            })
            .await;
        assert!(installed.connection.present);
        assert_eq!(installed.connection.revision, 1);
        assert_eq!(
            installed
                .connection
                .context
                .as_ref()
                .and_then(|context| context.target_id.as_deref()),
            Some("target-a")
        );

        assert!(runtime.refresh_connection_timestamp_for_generation(7).await);
        let refreshed = sink
            .wait_for(|event| {
                matches!(
                    &event.cause,
                    ComputerRuntimeEventCause::ClientConnectionStateChanged {
                        revision: 2,
                        status: ClientConnectionStatus::Connected,
                    }
                )
            })
            .await;
        assert_eq!(refreshed.connection.revision, 2);
        assert!(refreshed.connection.present);
        let refreshed_at = chrono::DateTime::parse_from_rfc3339(
            &refreshed.connection.context.as_ref().unwrap().connected_at,
        )
        .unwrap()
        .with_timezone(&chrono::Utc);
        assert!(refreshed_at > initial_connected_at);

        let event_count = sink.event_count();
        assert!(
            !runtime
                .refresh_connection_timestamp_for_generation(99)
                .await
        );
        assert_eq!(sink.event_count(), event_count);
        assert_eq!(runtime.connection_snapshot().await.revision, 2);

        runtime.take_connection_state().await;
        let removed = sink
            .wait_for(|event| {
                matches!(
                    &event.cause,
                    ComputerRuntimeEventCause::ClientConnectionStateChanged {
                        revision: 3,
                        status: ClientConnectionStatus::Disconnected,
                    }
                )
            })
            .await;
        assert!(!removed.connection.present);
        assert_eq!(removed.connection.revision, 3);
        assert!(removed.connection.context.is_none());

        let event_count = sink.event_count();
        assert!(!runtime.refresh_connection_timestamp_for_generation(7).await);
        assert_eq!(sink.event_count(), event_count);
        assert_eq!(runtime.connection_snapshot().await.revision, 3);
    }

    #[tokio::test]
    async fn connection_operation_state_is_versioned_and_isolated_per_computer() {
        let registry = ComputerRegistry::from_config(ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![instance("one", "One"), instance("two", "Two")],
        });
        let first = registry.runtime("one").await.unwrap();
        let second = registry.runtime("two").await.unwrap();

        first
            .begin_connection_operation(
                ClientConnectionOperation::Connect,
                Some(ClientConnectionOperationTarget {
                    source_type: "manager_robot".to_string(),
                    target_id: Some("employee:11".to_string()),
                    employee_id: Some(11),
                }),
            )
            .await
            .unwrap();
        let connecting = first.connection_snapshot().await;
        assert_eq!(connecting.status, ClientConnectionStatus::Connecting);
        assert_eq!(
            connecting.operation,
            Some(ClientConnectionOperation::Connect)
        );
        assert_eq!(
            connecting
                .operation_target
                .as_ref()
                .and_then(|target| target.employee_id),
            Some(11)
        );
        assert_eq!(connecting.revision, 1);
        assert!(!connecting.actions.connect.enabled);
        assert!(!connecting.actions.disconnect.enabled);

        let unaffected = second.connection_snapshot().await;
        assert_eq!(unaffected.status, ClientConnectionStatus::Disconnected);
        assert_eq!(unaffected.revision, 0);

        first
            .install_connection_state(ConnectionState {
                profile_name: "manual".to_string(),
                url: "https://smcp.example.com".to_string(),
                office_id: "office-a".to_string(),
                computer_name: "One".to_string(),
                connected_at: chrono::Utc::now(),
                source_type: "manual_smcp".to_string(),
                target_id: Some("target-a".to_string()),
                target_name: Some("Target A".to_string()),
                employee_id: None,
                generation: 7,
            })
            .await
            .unwrap();
        assert_eq!(
            first.connection_snapshot().await.status,
            ClientConnectionStatus::Connecting
        );

        first.complete_connection_operation().await;
        let connected = first.connection_snapshot().await;
        assert_eq!(connected.status, ClientConnectionStatus::Connected);
        assert_eq!(connected.revision, 3);
        assert!(connected.operation_target.is_none());
        assert!(connected.actions.disconnect.enabled);

        first
            .begin_connection_operation(
                ClientConnectionOperation::Disconnect,
                connected
                    .context
                    .as_ref()
                    .map(|context| ClientConnectionOperationTarget {
                        source_type: context.source_type.clone(),
                        target_id: context.target_id.clone(),
                        employee_id: context.employee_id,
                    }),
            )
            .await
            .unwrap();
        assert_eq!(
            first.connection_snapshot().await.status,
            ClientConnectionStatus::Disconnecting
        );
        first
            .reconcile_disconnect_failure("transport no longer valid".to_string())
            .await;
        let failed = first.connection_snapshot().await;
        assert_eq!(failed.status, ClientConnectionStatus::Disconnected);
        assert!(!failed.present);
        assert!(failed.operation_target.is_none());
        assert_eq!(
            failed.last_error.as_ref().map(|error| error.operation),
            Some(ClientConnectionOperation::Disconnect)
        );
        assert!(failed
            .last_error
            .as_ref()
            .is_some_and(|error| error.retryable));

        let orphan_transport_projection = ClientConnectionStateSnapshot::from_parts(
            failed.revision + 1,
            None,
            &ClientConnectionOperationState::default(),
            ComputerRuntimeState::Connected,
        );
        assert_eq!(
            orphan_transport_projection.status,
            ClientConnectionStatus::Disconnected
        );
        assert!(!orphan_transport_projection.actions.connect.enabled);
        assert!(orphan_transport_projection.actions.disconnect.enabled);
    }

    #[tokio::test]
    async fn aborted_reconnect_settles_only_its_generation() {
        let registry = ComputerRegistry::from_config(ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![instance("one", "One")],
        });
        let runtime = registry.runtime("one").await.unwrap();
        runtime
            .install_connection_state(ConnectionState {
                profile_name: "manager:1".to_string(),
                url: "https://smcp.example.com".to_string(),
                office_id: "office-a".to_string(),
                computer_name: "One".to_string(),
                connected_at: chrono::Utc::now(),
                source_type: "manager_robot".to_string(),
                target_id: Some("manager:1".to_string()),
                target_name: Some("Robot One".to_string()),
                employee_id: Some(1),
                generation: 7,
            })
            .await
            .unwrap();
        assert!(runtime.begin_reconnect_for_generation(7).await);
        assert!(!runtime.abort_reconnect_for_generation(6).await);
        assert_eq!(
            runtime.connection_snapshot().await.status,
            ClientConnectionStatus::Connecting
        );

        runtime.take_connection_state().await;
        runtime
            .install_connection_state(ConnectionState {
                profile_name: "manual".to_string(),
                url: "https://other.example.com".to_string(),
                office_id: "office-b".to_string(),
                computer_name: "One".to_string(),
                connected_at: chrono::Utc::now(),
                source_type: "manual_smcp".to_string(),
                target_id: Some("target-b".to_string()),
                target_name: Some("Target B".to_string()),
                employee_id: None,
                generation: 8,
            })
            .await
            .unwrap();

        assert!(runtime.abort_reconnect_for_generation(7).await);
        let settled = runtime.connection_snapshot().await;
        assert_eq!(settled.status, ClientConnectionStatus::Connected);
        assert_eq!(
            settled.context.and_then(|context| context.target_id),
            Some("target-b".to_string())
        );

        assert!(runtime.begin_reconnect_for_generation(8).await);
        assert!(
            runtime
                .fail_reconnect_for_generation(8, "Manager session expired".to_string(), false,)
                .await
        );
        let unauthorized = runtime.connection_snapshot().await;
        assert_eq!(unauthorized.status, ClientConnectionStatus::Disconnected);
        assert!(!unauthorized.present);
        assert_eq!(
            unauthorized
                .last_error
                .as_ref()
                .map(|error| error.retryable),
            Some(false)
        );
    }

    #[tokio::test]
    async fn reconnect_retries_preserve_the_first_problem_occurrence() {
        let registry = ComputerRegistry::from_config(ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![instance("one", "One")],
        });
        let runtime = registry.runtime("one").await.unwrap();
        runtime
            .install_connection_state(ConnectionState {
                profile_name: "manager:1".to_string(),
                url: "https://smcp.example.com".to_string(),
                office_id: "office-a".to_string(),
                computer_name: "One".to_string(),
                connected_at: chrono::Utc::now(),
                source_type: "manager_robot".to_string(),
                target_id: Some("manager:1".to_string()),
                target_name: Some("Robot One".to_string()),
                employee_id: Some(1),
                generation: 9,
            })
            .await
            .unwrap();
        assert!(runtime.begin_reconnect_for_generation(9).await);
        assert!(
            runtime
                .record_reconnect_retry(9, "first reconnect failure".to_string())
                .await
        );
        let first_occurrence = runtime
            .connection_snapshot()
            .await
            .last_error
            .unwrap()
            .occurred_at;
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;

        assert!(
            runtime
                .record_reconnect_retry(9, "updated reconnect failure".to_string())
                .await
        );
        let retry = runtime.connection_snapshot().await.last_error.unwrap();
        assert_eq!(retry.occurred_at, first_occurrence);
        assert_eq!(retry.message, "updated reconnect failure");

        assert!(runtime.complete_reconnect_for_generation(9).await);
        assert!(runtime.begin_reconnect_for_generation(9).await);
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        assert!(
            runtime
                .record_reconnect_retry(9, "new reconnect failure".to_string())
                .await
        );
        let second_occurrence = runtime
            .connection_snapshot()
            .await
            .last_error
            .unwrap()
            .occurred_at;
        assert_ne!(second_occurrence, first_occurrence);
        assert!(
            runtime
                .fail_reconnect_for_generation(9, "terminal reconnect failure".to_string(), false,)
                .await
        );
        let terminal = runtime.connection_snapshot().await.last_error.unwrap();
        assert_eq!(terminal.occurred_at, second_occurrence);
        assert_eq!(terminal.message, "terminal reconnect failure");
        assert!(!terminal.retryable);
    }

    #[tokio::test]
    async fn reconnect_problem_preserves_occurrence_across_diagnostic_sources() {
        let registry = ComputerRegistry::from_config(ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![instance("one", "One")],
        });
        let sink = Arc::new(RecordingRuntimeEventSink::default());
        registry.set_runtime_event_sink(sink.clone()).await;
        let runtime = registry.runtime("one").await.unwrap();
        runtime
            .install_connection_state(ConnectionState {
                profile_name: "manager:1".to_string(),
                url: "https://smcp.example.com".to_string(),
                office_id: "office-a".to_string(),
                computer_name: "One".to_string(),
                connected_at: chrono::Utc::now(),
                source_type: "manager_robot".to_string(),
                target_id: Some("manager:1".to_string()),
                target_name: Some("Robot One".to_string()),
                employee_id: Some(1),
                generation: 10,
            })
            .await
            .unwrap();
        assert!(runtime.begin_reconnect_for_generation(10).await);
        runtime
            .set_client_runtime_diagnostic(
                "reconnect",
                Some("SMCP reconnect failed; retrying".to_string()),
            )
            .await;
        runtime
            .client_runtime_diagnostic
            .write()
            .await
            .as_mut()
            .unwrap()
            .occurred_at = "2026-01-01T00:00:00+00:00".to_string();
        let first = runtime
            .runtime_snapshot()
            .await
            .problems
            .into_iter()
            .find(|problem| problem.operation == "reconnect")
            .unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        assert!(
            runtime
                .record_reconnect_retry(10, "retry 1/3".to_string())
                .await
        );
        let projected = runtime
            .runtime_snapshot()
            .await
            .problems
            .into_iter()
            .find(|problem| problem.operation == "reconnect")
            .unwrap();
        assert_eq!(projected.id, first.id);
        assert_eq!(projected.occurred_at, first.occurred_at);
        assert_eq!(projected.technical_detail.as_deref(), Some("retry 1/3"));

        let event_count_before_recovery = sink.events.lock().unwrap().len();
        assert!(runtime.complete_reconnect_for_generation(10).await);
        assert!(runtime
            .runtime_snapshot()
            .await
            .problems
            .iter()
            .all(|problem| problem.operation != "reconnect"));
        let recovery_events = sink.events.lock().unwrap().clone();
        assert!(recovery_events[event_count_before_recovery..]
            .iter()
            .all(|event| event
                .snapshot
                .problems
                .iter()
                .all(|problem| problem.operation != "reconnect")));
        assert!(recovery_events[event_count_before_recovery..]
            .iter()
            .any(|event| matches!(
                &event.cause,
                ComputerRuntimeEventCause::ClientDiagnosticChanged {
                    operation,
                    has_error: false,
                } if operation == "reconnect"
            )));
    }

    #[tokio::test]
    async fn manual_connect_recovery_clears_a_terminal_reconnect_problem() {
        let registry = ComputerRegistry::from_config(ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![instance("one", "One")],
        });
        let sink = Arc::new(RecordingRuntimeEventSink::default());
        registry.set_runtime_event_sink(sink.clone()).await;
        let runtime = registry.runtime("one").await.unwrap();
        runtime
            .install_connection_state(ConnectionState {
                profile_name: "manager:1".to_string(),
                url: "https://smcp.example.com".to_string(),
                office_id: "office-a".to_string(),
                computer_name: "One".to_string(),
                connected_at: chrono::Utc::now(),
                source_type: "manager_robot".to_string(),
                target_id: Some("manager:1".to_string()),
                target_name: Some("Robot One".to_string()),
                employee_id: Some(1),
                generation: 11,
            })
            .await
            .unwrap();
        assert!(runtime.begin_reconnect_for_generation(11).await);
        runtime
            .set_client_runtime_diagnostic(
                "reconnect",
                Some("SMCP reconnect failed; retrying".to_string()),
            )
            .await;
        assert!(
            runtime
                .record_reconnect_retry(11, "retry limit exhausted".to_string())
                .await
        );
        assert!(
            runtime
                .fail_reconnect_for_generation(
                    11,
                    "SMCP reconnect retry limit exhausted".to_string(),
                    true,
                )
                .await
        );
        assert!(runtime.runtime_snapshot().await.problems.iter().any(|problem| {
            problem.operation == "reconnect"
                && problem.source
                    == crate::services::computer_runtime_events::ComputerRuntimeProblemSource::ClientConnection
        }));

        let token = runtime
            .begin_connection_operation(ClientConnectionOperation::Connect, None)
            .await
            .unwrap();
        let event_count_before_recovery = sink.events.lock().unwrap().len();
        runtime
            .install_connection_state(ConnectionState {
                profile_name: "manual".to_string(),
                url: "https://smcp.example.com".to_string(),
                office_id: "office-a".to_string(),
                computer_name: "One".to_string(),
                connected_at: chrono::Utc::now(),
                source_type: "manual_smcp".to_string(),
                target_id: Some("manual-target".to_string()),
                target_name: Some("Manual target".to_string()),
                employee_id: None,
                generation: 12,
            })
            .await
            .unwrap();
        assert!(runtime.complete_connection_operation_for_token(token).await);

        assert!(runtime
            .runtime_snapshot()
            .await
            .problems
            .iter()
            .all(|problem| problem.operation != "reconnect"));
        let recovery_events = sink.events.lock().unwrap().clone();
        assert!(recovery_events[event_count_before_recovery..]
            .iter()
            .all(|event| event
                .snapshot
                .problems
                .iter()
                .all(|problem| problem.operation != "reconnect")));
        assert!(recovery_events[event_count_before_recovery..]
            .iter()
            .any(|event| matches!(
                &event.cause,
                ComputerRuntimeEventCause::ClientDiagnosticChanged {
                    operation,
                    has_error: false,
                } if operation == "reconnect"
            )));
    }

    #[tokio::test]
    async fn manual_connect_keeps_an_owner_only_terminal_problem_until_recovery() {
        let registry = ComputerRegistry::from_config(ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![instance("one", "One")],
        });
        let sink = Arc::new(RecordingRuntimeEventSink::default());
        registry.set_runtime_event_sink(sink.clone()).await;
        let runtime = registry.runtime("one").await.unwrap();
        runtime
            .install_connection_state(ConnectionState {
                profile_name: "manager:1".to_string(),
                url: "https://smcp.example.com".to_string(),
                office_id: "office-a".to_string(),
                computer_name: "One".to_string(),
                connected_at: chrono::Utc::now(),
                source_type: "manager_robot".to_string(),
                target_id: Some("manager:1".to_string()),
                target_name: Some("Robot One".to_string()),
                employee_id: Some(1),
                generation: 13,
            })
            .await
            .unwrap();
        assert!(runtime.begin_reconnect_for_generation(13).await);
        assert!(
            runtime
                .fail_reconnect_for_generation(
                    13,
                    "Manager session expired during SMCP token refresh".to_string(),
                    false,
                )
                .await
        );
        assert!(runtime.client_runtime_diagnostic.read().await.is_none());
        let terminal = runtime
            .runtime_snapshot()
            .await
            .problems
            .into_iter()
            .find(|problem| problem.operation == "reconnect")
            .unwrap();

        let event_count_before_retry = sink.events.lock().unwrap().len();
        let token = runtime
            .begin_connection_operation(ClientConnectionOperation::Connect, None)
            .await
            .unwrap();
        let retrying = runtime
            .runtime_snapshot()
            .await
            .problems
            .into_iter()
            .find(|problem| problem.operation == "reconnect")
            .unwrap();
        assert_eq!(retrying.id, terminal.id);
        assert_eq!(retrying.occurred_at, terminal.occurred_at);
        let retry_events = sink.events.lock().unwrap().clone();
        assert!(retry_events[event_count_before_retry..]
            .iter()
            .all(|event| {
                event.snapshot.problems.iter().any(|problem| {
                    problem.id == terminal.id && problem.occurred_at == terminal.occurred_at
                })
            }));

        let event_count_before_recovery = retry_events.len();
        runtime
            .install_connection_state(ConnectionState {
                profile_name: "manual".to_string(),
                url: "https://smcp.example.com".to_string(),
                office_id: "office-a".to_string(),
                computer_name: "One".to_string(),
                connected_at: chrono::Utc::now(),
                source_type: "manual_smcp".to_string(),
                target_id: Some("manual-target".to_string()),
                target_name: Some("Manual target".to_string()),
                employee_id: None,
                generation: 14,
            })
            .await
            .unwrap();
        assert!(runtime.complete_connection_operation_for_token(token).await);

        assert!(runtime.runtime_snapshot().await.problems.is_empty());
        let recovery_events = sink.events.lock().unwrap().clone();
        assert!(recovery_events[event_count_before_recovery..]
            .iter()
            .all(|event| event.snapshot.problems.is_empty()));
    }

    #[tokio::test]
    async fn handle_replacement_invalidates_connect_operation_without_transport() {
        let registry = ComputerRegistry::from_config(ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![instance("one", "One")],
        });
        let runtime = registry.runtime("one").await.unwrap();
        runtime.start().await.unwrap();
        let token = runtime
            .begin_connection_operation(ClientConnectionOperation::Connect, None)
            .await
            .unwrap();

        runtime.restart().await.unwrap();

        assert!(runtime.ensure_connection_operation(token).await.is_err());
        let snapshot = runtime.connection_snapshot().await;
        assert_eq!(snapshot.status, ClientConnectionStatus::Disconnected);
        assert!(snapshot.operation.is_none());
        assert!(!snapshot.present);
    }

    #[tokio::test]
    async fn every_refresh_terminal_outcome_leaves_a_non_transitional_snapshot() {
        let cases = [
            (RefreshTerminalOutcome::Gone, None),
            (RefreshTerminalOutcome::Unauthorized, Some(false)),
            (RefreshTerminalOutcome::Stop, Some(false)),
            (RefreshTerminalOutcome::Exhausted, Some(true)),
        ];
        for (index, (outcome, expected_retryable)) in cases.into_iter().enumerate() {
            let id = format!("terminal-{index}");
            let registry = ComputerRegistry::from_config(ComputerInstancesConfig {
                schema_version: 1,
                instances: vec![instance(&id, "Terminal")],
            });
            let runtime = registry.runtime(&id).await.unwrap();
            let generation = 100 + index as u64;
            runtime
                .install_connection_state(ConnectionState {
                    profile_name: "manager:1".to_string(),
                    url: "https://smcp.example.com".to_string(),
                    office_id: "office-a".to_string(),
                    computer_name: "Terminal".to_string(),
                    connected_at: chrono::Utc::now(),
                    source_type: "manager_robot".to_string(),
                    target_id: Some("manager:1".to_string()),
                    target_name: Some("Robot One".to_string()),
                    employee_id: Some(1),
                    generation,
                })
                .await
                .unwrap();
            assert!(runtime.begin_reconnect_for_generation(generation).await);
            assert!(
                settle_refresh_terminal(&runtime, generation, outcome).await,
                "terminal outcome {outcome:?} did not settle"
            );

            let snapshot = runtime.connection_snapshot().await;
            assert_ne!(snapshot.status, ClientConnectionStatus::Connecting);
            assert_ne!(snapshot.status, ClientConnectionStatus::Disconnecting);
            assert_eq!(
                snapshot.last_error.as_ref().map(|error| error.retryable),
                expected_retryable
            );
        }
    }

    #[tokio::test]
    async fn start_after_shutdown_replaces_handle_and_rebinds_sdk_events() {
        let registry = ComputerRegistry::from_config(ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![instance("one", "One")],
        });
        let sink = Arc::new(RecordingRuntimeEventSink::default());
        registry.set_runtime_event_sink(sink.clone()).await;
        let runtime = registry.runtime("one").await.unwrap();

        registry.start_runtime("one").await.unwrap();
        let first_generation = runtime.runtime_generation();
        sink.wait_for(|event| {
            event.snapshot.generation == first_generation
                && event.snapshot.lifecycle == LifecycleState::Started
        })
        .await;
        runtime
            .record_mcp_config_apply_diagnostic(
                {
                    let bundle_id = BundleId::try_from("server-a").unwrap();
                    runtime
                        .sdk_servers
                        .write()
                        .await
                        .insert(bundle_id.clone(), "Server A".to_string());
                    bundle_id
                },
                "configuration apply failed".to_string(),
            )
            .await;
        runtime
            .fail_connection_operation(
                ClientConnectionOperation::Connect,
                "connection failed".to_string(),
                true,
            )
            .await;
        assert_eq!(runtime.runtime_snapshot().await.problems.len(), 2);
        registry.stop_runtime("one").await.unwrap();
        sink.wait_for(|event| {
            event.snapshot.generation == first_generation
                && event.snapshot.lifecycle == LifecycleState::Shutdown
        })
        .await;

        registry.start_runtime("one").await.unwrap();
        let second_generation = runtime.runtime_generation();
        let replaced = sink
            .wait_for(|event| {
                event.snapshot.generation == second_generation
                    && matches!(
                        &event.cause,
                        ComputerRuntimeEventCause::HandleReplaced { .. }
                    )
            })
            .await;
        let restarted = sink
            .wait_for(|event| {
                event.snapshot.generation == second_generation
                    && event.snapshot.lifecycle == LifecycleState::Started
            })
            .await;

        assert_eq!(second_generation, first_generation + 1);
        assert_eq!(replaced.snapshot.lifecycle, LifecycleState::Created);
        assert!(
            replaced.snapshot.problems.is_empty(),
            "handle replacement must not carry diagnostics from the retired generation"
        );
        assert!(restarted.snapshot.capability_revision > 0);
        assert!(runtime.is_running().await);
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn failed_handle_replacement_preserves_current_generation_problems() {
        let registry = ComputerRegistry::from_config(ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![instance("one", "One")],
        });
        let runtime = registry.runtime("one").await.unwrap();
        runtime.start().await.unwrap();
        let bundle_id = BundleId::try_from("server-a").unwrap();
        runtime
            .sdk_servers
            .write()
            .await
            .insert(bundle_id.clone(), "Server A".to_string());
        runtime
            .record_mcp_config_apply_diagnostic(bundle_id, "configuration apply failed".to_string())
            .await;
        runtime
            .fail_connection_operation(
                ClientConnectionOperation::Reconnect,
                "connection failed".to_string(),
                true,
            )
            .await;
        let generation = runtime.runtime_generation();
        let before = runtime.runtime_snapshot().await.problems;
        assert_eq!(before.len(), 2);

        runtime.fail_sdk_shutdown_once.store(true, Ordering::SeqCst);
        let error = runtime.restart().await.unwrap_err().to_string();

        assert!(error.contains("Injected SDK shutdown failure"));
        assert_eq!(runtime.runtime_generation(), generation);
        assert_eq!(
            runtime.runtime_snapshot().await.problems,
            before,
            "a failed replacement must preserve the authoritative generation's problems"
        );
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn handle_generation_advances_atomically_with_sdk_swap() {
        let registry = ComputerRegistry::from_config(ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![instance("one", "One")],
        });
        let runtime = registry.runtime("one").await.unwrap();
        runtime.start().await.unwrap();
        let first_generation = runtime.runtime_generation();
        runtime
            .oauth_required_scopes
            .write()
            .await
            .apply_public_status(
                &BundleId::try_from("protected").unwrap(),
                &PublicOAuthStatus::ReauthorizationRequired {
                    required_scope: "tools.write".to_string(),
                },
            );

        let snapshot_guard = runtime.runtime_snapshot_lock.lock().await;
        let restart_runtime = runtime.clone();
        let restart_task = tokio::spawn(async move { restart_runtime.restart().await });

        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while runtime.runtime_state().await != LifecycleState::Shutdown {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("old SDK handle did not reach shutdown");
        assert_eq!(
            runtime.runtime_generation(),
            first_generation,
            "generation must not advance while the SDK handle swap is blocked"
        );

        drop(snapshot_guard);
        restart_task.await.unwrap().unwrap();
        assert_eq!(runtime.runtime_generation(), first_generation + 1);
        assert!(runtime.oauth_required_scopes.read().await.is_empty());
        assert!(runtime.is_running().await);
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn concurrent_event_sink_install_and_restart_rebinds_current_generation() {
        let registry = Arc::new(ComputerRegistry::from_config(ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![instance("one", "One")],
        }));
        let runtime = registry.runtime("one").await.unwrap();
        runtime.start().await.unwrap();
        let sink = Arc::new(RecordingRuntimeEventSink::default());

        let (_, restart_result) = tokio::join!(
            registry.set_runtime_event_sink(sink.clone()),
            runtime.restart()
        );
        restart_result.unwrap();
        let generation = runtime.runtime_generation();

        runtime.try_shutdown().await.unwrap();
        sink.wait_for(|event| {
            event.snapshot.generation == generation
                && event.snapshot.lifecycle == LifecycleState::Shutdown
        })
        .await;
    }

    #[tokio::test]
    async fn update_runtime_instance_saves_profile_without_restarting_active_handle() {
        let registry = ComputerRegistry::from_config(ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![instance("one", "One")],
        });

        let before = registry.runtime("one").await.unwrap();
        registry.start_runtime("one").await.unwrap();
        let generation_before_update = before.runtime_generation();
        let mut updated = instance("one", "One");
        updated.name = "Renamed".to_string();
        let after = registry.update_runtime_instance(updated).await.unwrap();

        assert_eq!(after.instance.name, "Renamed");
        assert!(after.is_running().await);
        assert!(Arc::ptr_eq(&before.inputs, &after.inputs));
        assert!(Arc::ptr_eq(&before.computer, &after.computer));
        assert!(Arc::ptr_eq(&before.connection, &after.connection));
        assert!(Arc::ptr_eq(
            &before.runtime_generation,
            &after.runtime_generation
        ));
        assert_eq!(before.runtime_incarnation, after.runtime_incarnation);
        assert!(Arc::ptr_eq(&before.lifecycle_lock, &after.lifecycle_lock));
        assert_eq!(after.runtime_generation(), generation_before_update);
        assert_eq!(after.computer.read().await.name(), "One");
        let inputs = after.inputs.read().await;
        assert!(inputs.is_empty());
        assert_eq!(
            after.sdk_skill_home().await,
            after.skill_home_base.join("one").join("skill_home")
        );
        drop(inputs);

        after.restart().await.unwrap();
        assert_eq!(after.runtime_generation(), generation_before_update + 1);
        assert_eq!(after.computer.read().await.name(), "Renamed");
    }

    #[tokio::test]
    async fn update_runtime_instance_does_not_recreate_a_removed_runtime() {
        let registry = ComputerRegistry::from_config(ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![instance("one", "Original")],
        });
        registry
            .remove_runtime("one")
            .await
            .unwrap()
            .expect("runtime should be removed");

        let error = registry
            .update_runtime_instance(instance("one", "Stale Update"))
            .await
            .err()
            .expect("update must not recreate a removed runtime");

        assert!(error.contains("does not exist"));
        assert!(registry.runtime("one").await.is_none());
    }

    #[tokio::test]
    async fn start_reads_saved_skill_home_without_rebuilding_during_save() {
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
            PathBuf::from("/tmp/custom-skill-home")
        );
        runtime.start().await.unwrap();
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

        let reader = runtime.sdk_skill_reader().unwrap();
        let skills = reader.skills().await;
        let skill_ref = skills
            .iter()
            .find(|skill| skill.name == "example-skill")
            .cloned()
            .expect("example skill should be staged by SDK Computer");
        assert_eq!(skill_ref.source, "user");
        assert_eq!(
            reader.skill_ref("example-skill").await,
            Some(skill_ref.clone())
        );
        let view = reader.read_skill_resource(&skill_ref, None).await.unwrap();
        assert_eq!(
            String::from_utf8(view.read_all().unwrap()).unwrap(),
            "Body\n"
        );

        drop(reader);
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
        assert!(before.sdk_mcp_server_ids().await.is_empty());

        let mut updated = instance("one", "Updated");
        updated.mcp_servers = vec![server_config("updated-server").into()];
        updated.connection_policy.auto_connect = true;
        let after = registry.update_runtime_instance(updated).await.unwrap();

        assert!(Arc::ptr_eq(&before.computer, &after.computer));
        assert!(after.is_running().await);
        assert!(after.sdk_is_mcp_manager_initialized().await);
        assert_eq!(after.computer.read().await.name(), "Initial");
        assert!(after.sdk_mcp_server_ids().await.is_empty());
    }

    #[tokio::test]
    async fn upsert_same_id_retires_the_previous_runtime_before_replacement() {
        let registry = ComputerRegistry::from_config(ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![instance("one", "Original")],
        });
        let sink = Arc::new(RecordingRuntimeEventSink::default());
        registry.set_runtime_event_sink(sink).await;
        let previous = registry.runtime("one").await.unwrap();
        previous.start().await.unwrap();
        let previous_incarnation = previous.runtime_incarnation;

        let replacement = registry
            .upsert_runtime(instance("one", "Replacement"))
            .await
            .unwrap();

        assert_eq!(previous.runtime_state().await, LifecycleState::Shutdown);
        assert!(previous.is_retired());
        assert!(previous.runtime_event_task.lock().await.is_none());
        assert!(replacement.runtime_incarnation > previous_incarnation);
        assert_eq!(replacement.instance.name, "Replacement");
        assert!(Arc::ptr_eq(
            &registry.runtime("one").await.unwrap().computer,
            &replacement.computer
        ));
        assert!(matches!(
            previous.start().await,
            Err(ComputerRuntimeStartError::Client(error)) if error.contains("has been retired")
        ));
        assert!(matches!(
            previous.restart().await,
            Err(ComputerRuntimeStartError::Client(error)) if error.contains("has been retired")
        ));
        assert!(previous
            .stop_all_mcp_servers()
            .await
            .unwrap_err()
            .contains("has been retired"));
        assert!(previous
            .connect_smcp_socketio(
                "http://127.0.0.1:1",
                None,
                HashMap::new(),
                None,
                "retired-office",
                "retired-computer",
            )
            .await
            .unwrap_err()
            .contains("has been retired"));
        assert!(previous
            .available_tools()
            .await
            .unwrap_err()
            .contains("has been retired"));
        let retired_server = BundleId::try_from("retired-server").unwrap();
        assert!(previous
            .resources(&retired_server, None)
            .await
            .unwrap_err()
            .contains("has been retired"));
    }

    #[tokio::test]
    async fn upsert_waits_for_registered_runtime_activity_before_teardown() {
        let registry = Arc::new(ComputerRegistry::from_config(ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![instance("one", "Original")],
        }));
        let previous = registry.runtime("one").await.unwrap();
        previous.start().await.unwrap();
        let activity = previous.begin_activity().unwrap();
        let replacement_registry = registry.clone();
        let replacement_task = tokio::spawn(async move {
            replacement_registry
                .upsert_runtime(instance("one", "Replacement"))
                .await
        });

        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !previous.is_retired() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("replacement did not begin retirement");
        assert!(!replacement_task.is_finished());
        assert!(previous.begin_activity().is_err());

        drop(activity);
        let replacement = replacement_task.await.unwrap().unwrap();
        assert_eq!(previous.runtime_state().await, LifecycleState::Shutdown);
        assert_eq!(replacement.instance.name, "Replacement");
    }

    #[tokio::test]
    async fn committed_retirement_diagnostic_does_not_reopen_terminal_runtime() {
        let registry = ComputerRegistry::from_config(ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![instance("one", "Original")],
        });
        let previous = registry.runtime("one").await.unwrap();
        previous.start().await.unwrap();
        previous.fail_prepare_shutdown_once_for_test();

        let replacement = registry
            .upsert_runtime(instance("one", "Replacement"))
            .await
            .expect("committed cleanup diagnostics must not rollback retirement");

        assert!(previous.is_retired());
        assert_eq!(previous.runtime_state().await, LifecycleState::Shutdown);
        assert_eq!(replacement.instance.name, "Replacement");
        assert!(!replacement.is_retired());
    }

    #[tokio::test]
    async fn upsert_retirement_does_not_block_lookup_for_another_instance() {
        let registry = Arc::new(ComputerRegistry::from_config(ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![instance("one", "Original"), instance("two", "Independent")],
        }));
        let previous = registry.runtime("one").await.unwrap();
        let activity = previous.begin_activity().unwrap();
        let replacement_registry = registry.clone();
        let replacement_task = tokio::spawn(async move {
            replacement_registry
                .upsert_runtime(instance("one", "Replacement"))
                .await
        });

        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !previous.is_retired() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("replacement did not begin retirement");

        let independent = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            registry.runtime("two"),
        )
        .await
        .expect("instance one retirement blocked instance two lookup")
        .expect("instance two runtime should remain registered");
        assert_eq!(independent.instance.name, "Independent");

        drop(activity);
        replacement_task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn update_sync_does_not_block_lookup_for_another_instance() {
        let registry = Arc::new(ComputerRegistry::from_config(ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![instance("one", "Original"), instance("two", "Independent")],
        }));
        let runtime = registry.runtime("one").await.unwrap();
        let lifecycle_guard = runtime.lifecycle_lock.lock().await;
        let coordinator = registry.runtime_mutation_coordinator("one");
        let update_registry = registry.clone();
        let update_task = tokio::spawn(async move {
            update_registry
                .update_runtime_instance(instance("one", "Updated"))
                .await
        });

        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if coordinator.try_lock().is_err() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("update did not enter the instance mutation coordinator");

        let independent = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            registry.runtime("two"),
        )
        .await
        .expect("instance one update blocked instance two lookup")
        .expect("instance two runtime should remain registered");
        assert_eq!(independent.instance.name, "Independent");

        drop(lifecycle_guard);
        assert_eq!(update_task.await.unwrap().unwrap().instance.name, "Updated");
    }

    #[tokio::test]
    async fn removal_retirement_does_not_block_lookup_for_another_instance() {
        let registry = Arc::new(ComputerRegistry::from_config(ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![instance("one", "Removed"), instance("two", "Independent")],
        }));
        let removed = registry.runtime("one").await.unwrap();
        let activity = removed.begin_activity().unwrap();
        let removal_registry = registry.clone();
        let removal_task =
            tokio::spawn(async move { removal_registry.remove_runtime("one").await });

        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !removed.is_retired() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("removal did not begin retirement");

        let independent = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            registry.runtime("two"),
        )
        .await
        .expect("instance one removal blocked instance two lookup")
        .expect("instance two runtime should remain registered");
        assert_eq!(independent.instance.name, "Independent");

        drop(activity);
        assert!(removal_task.await.unwrap().unwrap().is_some());
    }

    #[tokio::test]
    async fn removal_drains_sdk_skill_read_sessions_before_shutdown() {
        let registry = Arc::new(ComputerRegistry::from_config(ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![instance("one", "Removed")],
        }));
        let runtime = registry.runtime("one").await.unwrap();
        let reader = runtime.sdk_skill_reader().unwrap();
        let removal_registry = registry.clone();
        let removal_task =
            tokio::spawn(async move { removal_registry.remove_runtime("one").await });

        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !runtime.is_retired() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("removal did not close Skill read admission");
        assert!(
            !removal_task.is_finished(),
            "removal raced an admitted SDK Skill read session"
        );

        drop(reader);
        assert!(removal_task.await.unwrap().unwrap().is_some());
    }

    #[tokio::test]
    async fn remove_runtime_allows_any_configured_instance() {
        let registry = ComputerRegistry::from_config(ComputerInstancesConfig {
            schema_version: 1,
            instances: vec![instance("one", "One"), instance("two", "Two")],
        });

        let removed_one = registry.remove_runtime("one").await.unwrap().unwrap();
        assert_eq!(removed_one.instance.id, "one");
        assert!(removed_one.is_retired());
        assert!(registry.runtime("one").await.is_none());

        let removed_two = registry.remove_runtime("two").await.unwrap().unwrap();
        assert_eq!(removed_two.instance.id, "two");
        assert!(removed_two.is_retired());
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

        let removed = registry.remove_runtime("two").await.unwrap().unwrap();

        assert!(!removed.is_running().await);
        assert!(matches!(
            removed.start().await,
            Err(ComputerRuntimeStartError::Client(error)) if error.contains("has been retired")
        ));
        assert!(registry.runtime("one").await.unwrap().is_running().await);
        assert!(registry.runtime("two").await.is_none());
    }
}
