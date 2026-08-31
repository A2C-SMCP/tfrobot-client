mod audit;
mod catalog;
mod dispatch;
mod error;
mod package;
mod policy;
mod provider;

pub use audit::{
    AuditOutcome, ControlAuditRecord, ControlAuditSink, NoopControlAuditSink,
    ObservabilityControlAuditSink,
};
pub use catalog::{
    TargetContract, ToolCatalog, ToolDefinition, ToolGroup, ToolId, ToolRisk, UnknownToolId,
};
pub use error::{ClientControlError, ClientControlErrorCode};
pub use package::{
    SkillContentEncoding, SkillFileChange, SkillFileInput, SkillMutationResult, SkillPackageError,
    SkillPackageErrorCode, SkillPackageService, SkillRefreshStatus,
};
pub use policy::{InvocationContext, RemoteControlPolicy, TargetScope, ToolScope};
pub use provider::{
    client_control_server_config, ClientControlBinding, ClientControlMcpClient,
    CLIENT_CONTROL_BUNDLE_ID,
};

use crate::services::chat_session::ChatSessionService;
use crate::services::computer::{ComputerInstance, ComputerRegistry};
use crate::services::config::ConfigService;
use crate::services::keychain::SecretStore;
use crate::services::manager_context::ManagerContextCoordinator;
use crate::services::observability::{Diagnostics, ObservabilityService};
use crate::services::sdk_config::SdkConfigService;
use crate::services::settings::SettingsService;
use base64::Engine as _;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct ClientControlHost {
    pub sdk_config: Arc<SdkConfigService>,
    pub secret_store: Arc<dyn SecretStore>,
    pub connection_target_reservations:
        Arc<std::sync::Mutex<std::collections::HashMap<String, String>>>,
    pub computer_lifecycle_lock: Arc<Mutex<()>>,
    pub input_mutation_lock: Arc<Mutex<()>>,
    pub observability: Arc<ObservabilityService>,
    pub diagnostics: Arc<Diagnostics>,
    pub settings_service: Arc<SettingsService>,
    pub manager_context: Arc<ManagerContextCoordinator>,
    pub chat_sessions: Arc<ChatSessionService>,
}

#[derive(Debug, Clone)]
pub struct AuthorizedInvocation {
    pub context: InvocationContext,
    pub tool: ToolDefinition,
    pub target: Option<ComputerInstance>,
}

pub struct ClientControlPlane {
    config: Arc<ConfigService>,
    computer_registry: Arc<ComputerRegistry>,
    audit: Arc<dyn ControlAuditSink>,
    catalog: ToolCatalog,
    skill_packages: SkillPackageService,
    host: Option<ClientControlHost>,
}

struct SkillMutationAudit {
    request_id: String,
    source_id: String,
    computer_id: String,
    tool: ToolId,
    summary: serde_json::Value,
}

impl ClientControlPlane {
    pub fn new(
        config: Arc<ConfigService>,
        computer_registry: Arc<ComputerRegistry>,
        audit: Arc<dyn ControlAuditSink>,
    ) -> Self {
        Self {
            config,
            computer_registry,
            audit,
            catalog: ToolCatalog,
            skill_packages: SkillPackageService::new(),
            host: None,
        }
    }

    pub fn new_with_host(
        config: Arc<ConfigService>,
        computer_registry: Arc<ComputerRegistry>,
        audit: Arc<dyn ControlAuditSink>,
        host: ClientControlHost,
    ) -> Self {
        let mut plane = Self::new(config, computer_registry, audit);
        plane.host = Some(host);
        plane
    }

    fn command_state(self: &Arc<Self>) -> Result<crate::AppState, ClientControlError> {
        let host = self.host.as_ref().ok_or_else(|| {
            ClientControlError::new(
                ClientControlErrorCode::OperationFailed,
                "Client Control command host is not available",
            )
        })?;
        Ok(crate::AppState {
            config: self.config.clone(),
            sdk_config: host.sdk_config.clone(),
            computer_registry: self.computer_registry.clone(),
            client_control: self.clone(),
            secret_store: host.secret_store.clone(),
            connection_target_reservations: host.connection_target_reservations.clone(),
            computer_lifecycle_lock: host.computer_lifecycle_lock.clone(),
            input_mutation_lock: host.input_mutation_lock.clone(),
            observability: host.observability.clone(),
            diagnostics: host.diagnostics.clone(),
            settings_service: host.settings_service.clone(),
            manager_context: host.manager_context.clone(),
            chat_sessions: host.chat_sessions.clone(),
        })
    }

    #[must_use]
    pub fn catalog(&self) -> Vec<ToolDefinition> {
        self.catalog.all()
    }

    pub fn policy(&self, source_id: &str) -> Result<RemoteControlPolicy, ClientControlError> {
        self.config
            .get_computer_instance(source_id)
            .map(|instance| instance.remote_control)
            .map_err(|_| {
                ClientControlError::new(
                    ClientControlErrorCode::SourceNotFound,
                    format!("source Computer does not exist: {source_id}"),
                )
            })
    }

    pub async fn authorize(
        &self,
        context: InvocationContext,
        tool: ToolId,
        target_id: Option<&str>,
    ) -> Result<AuthorizedInvocation, ClientControlError> {
        let source_id = context.source_computer_id.as_str();
        let source = self.config.get_computer_instance(source_id).map_err(|_| {
            ClientControlError::invocation(
                ClientControlErrorCode::SourceNotFound,
                "source Computer does not exist",
                source_id,
                tool,
                target_id,
            )
        })?;
        let policy = source.remote_control;
        if !policy.enabled {
            return Err(ClientControlError::invocation(
                ClientControlErrorCode::RemoteControlDisabled,
                "Client Control is disabled for the source Computer",
                source_id,
                tool,
                target_id,
            ));
        }
        if !policy.allows_tool(tool) {
            return Err(ClientControlError::invocation(
                ClientControlErrorCode::ToolNotAllowed,
                "tool is outside the source Computer scope",
                source_id,
                tool,
                target_id,
            ));
        }
        let target = match target_id {
            Some(target_id) => {
                if !policy.allows_target(source_id, target_id) {
                    return Err(ClientControlError::invocation(
                        ClientControlErrorCode::TargetNotAllowed,
                        "target Computer is outside the source Computer scope",
                        source_id,
                        tool,
                        Some(target_id),
                    ));
                }
                if source_id == target_id && is_source_destructive(tool) {
                    return Err(ClientControlError::invocation(
                        ClientControlErrorCode::SourceSelfProtection,
                        "a Robot cannot stop, restart, delete, or disconnect its source Computer",
                        source_id,
                        tool,
                        Some(target_id),
                    ));
                }
                Some(self.config.get_computer_instance(target_id).map_err(|_| {
                    ClientControlError::invocation(
                        ClientControlErrorCode::TargetNotFound,
                        "target Computer does not exist",
                        source_id,
                        tool,
                        Some(target_id),
                    )
                })?)
            }
            None => None,
        };
        Ok(AuthorizedInvocation {
            context,
            tool: self.catalog.get(tool),
            target,
        })
    }

    pub async fn discover_targets(
        &self,
        source_id: &str,
        tool: ToolId,
    ) -> Result<Vec<ComputerInstance>, ClientControlError> {
        let policy = self.policy(source_id)?;
        if !policy.allows_tool(tool) {
            return Ok(Vec::new());
        }
        let mut instances = self
            .config
            .load_computer_instances()
            .map_err(|error| {
                ClientControlError::new(ClientControlErrorCode::OperationFailed, error.to_string())
            })?
            .instances;
        instances.retain(|target| {
            policy.allows_target(source_id, &target.id)
                && !(source_id == target.id && is_source_destructive(tool))
        });
        Ok(instances)
    }

    pub async fn update_policy_local(
        &self,
        source_id: &str,
        policy: RemoteControlPolicy,
    ) -> Result<RemoteControlPolicy, ClientControlError> {
        policy.validate().map_err(|message| {
            ClientControlError::new(ClientControlErrorCode::InvalidArguments, message)
        })?;
        let known_targets = self
            .config
            .load_computer_instances()
            .map_err(|error| {
                ClientControlError::new(ClientControlErrorCode::OperationFailed, error.to_string())
            })?
            .instances
            .into_iter()
            .map(|instance| instance.id)
            .collect::<HashSet<_>>();
        if let TargetScope::Custom { targets } = &policy.target_scope {
            if let Some(target) = targets
                .iter()
                .find(|target| !known_targets.contains(*target))
            {
                return Err(ClientControlError::new(
                    ClientControlErrorCode::InvalidArguments,
                    format!("custom target Computer does not exist: {target}"),
                ));
            }
        }
        let previous = self
            .config
            .get_computer_instance(source_id)
            .map_err(|error| {
                ClientControlError::new(ClientControlErrorCode::OperationFailed, error.to_string())
            })?;
        let updated = self
            .config
            .update_computer_instance(source_id, |instance| {
                instance.remote_control = policy.clone();
            })
            .map_err(|error| {
                ClientControlError::new(ClientControlErrorCode::OperationFailed, error.to_string())
            })?;
        if let Err(error) = self
            .computer_registry
            .update_runtime_instance(updated.clone())
            .await
        {
            let restored = self
                .config
                .update_computer_instance(source_id, |instance| {
                    instance.remote_control = previous.remote_control.clone();
                })
                .map_err(|restore_error| {
                    ClientControlError::new(
                        ClientControlErrorCode::OperationFailed,
                        format!(
                            "runtime refresh failed: {error}; policy rollback failed: {restore_error}"
                        ),
                    )
                })?;
            self.computer_registry
                .update_runtime_instance(restored)
                .await
                .map_err(|restore_error| {
                    ClientControlError::new(
                        ClientControlErrorCode::OperationFailed,
                        format!(
                            "runtime refresh failed: {error}; persisted policy was rolled back, but runtime rollback failed: {restore_error}"
                        ),
                    )
                })?;
            return Err(ClientControlError::new(
                ClientControlErrorCode::OperationFailed,
                format!("policy change was rolled back because runtime refresh failed: {error}"),
            ));
        }
        Ok(updated.remote_control)
    }

    pub fn audit(&self, record: ControlAuditRecord) -> Result<(), String> {
        self.audit.record(record).inspect_err(|error| {
            log::error!("failed to persist Client Control audit record: {error}");
        })
    }

    /// Protocol-neutral entry point used by the embedded MCP adapter. The adapter is deliberately
    /// thin: policy, target authorization, mutation semantics, and audit all remain in this plane.
    pub async fn dispatch(
        self: &Arc<Self>,
        context: InvocationContext,
        tool: ToolId,
        parameters: serde_json::Value,
    ) -> Result<serde_json::Value, ClientControlError> {
        dispatch::dispatch(self, context, tool, parameters).await
    }

    pub async fn skill_create(
        &self,
        context: InvocationContext,
        computer_id: &str,
        name: String,
        files: Vec<SkillFileInput>,
    ) -> Result<SkillMutationResult, ClientControlError> {
        let request_id = context.request_id.clone();
        let source_id = context.source_computer_id.clone();
        let tool = ToolId::SkillCreate;
        self.authorize(context, tool, Some(computer_id)).await?;
        let summary = serde_json::json!({
            "computer_id": computer_id,
            "name": name,
            "files": files.iter().map(|file| serde_json::json!({
                "path": file.path,
                "encoding": file.encoding,
                "decoded_size": decoded_content_size(file.encoding, &file.content)
            })).collect::<Vec<_>>()
        });
        let lease = match self
            .target_skill_mutation_lease(computer_id, &source_id, tool)
            .await
        {
            Ok(lease) => lease,
            Err(error) => {
                self.audit_skill_mutation_failure(
                    SkillMutationAudit {
                        request_id,
                        source_id,
                        computer_id: computer_id.to_string(),
                        tool,
                        summary,
                    },
                    &error,
                );
                return Err(error);
            }
        };
        let result = self
            .skill_packages
            .create(
                lease.configured_skill_home(),
                lease.effective_skill_home().await,
                name,
                files,
            )
            .await;
        self.finish_skill_mutation(
            SkillMutationAudit {
                request_id,
                source_id,
                computer_id: computer_id.to_string(),
                tool,
                summary,
            },
            lease,
            result,
        )
        .await
    }

    pub async fn skill_update(
        &self,
        context: InvocationContext,
        computer_id: &str,
        name: String,
        changes: Vec<SkillFileChange>,
        expected_revision: Option<String>,
    ) -> Result<SkillMutationResult, ClientControlError> {
        let request_id = context.request_id.clone();
        let source_id = context.source_computer_id.clone();
        let tool = ToolId::SkillUpdate;
        self.authorize(context, tool, Some(computer_id)).await?;
        let summary = serde_json::json!({
            "computer_id": computer_id,
            "name": name,
            "expected_revision": expected_revision,
            "changes": changes.iter().map(change_audit_summary).collect::<Vec<_>>()
        });
        let lease = match self
            .target_skill_mutation_lease(computer_id, &source_id, tool)
            .await
        {
            Ok(lease) => lease,
            Err(error) => {
                self.audit_skill_mutation_failure(
                    SkillMutationAudit {
                        request_id,
                        source_id,
                        computer_id: computer_id.to_string(),
                        tool,
                        summary,
                    },
                    &error,
                );
                return Err(error);
            }
        };
        let result = self
            .skill_packages
            .update(
                lease.configured_skill_home(),
                lease.effective_skill_home().await,
                name,
                changes,
                expected_revision,
            )
            .await;
        self.finish_skill_mutation(
            SkillMutationAudit {
                request_id,
                source_id,
                computer_id: computer_id.to_string(),
                tool,
                summary,
            },
            lease,
            result,
        )
        .await
    }

    pub async fn skill_delete(
        &self,
        context: InvocationContext,
        computer_id: &str,
        name: String,
        expected_revision: Option<String>,
    ) -> Result<SkillMutationResult, ClientControlError> {
        let request_id = context.request_id.clone();
        let source_id = context.source_computer_id.clone();
        let tool = ToolId::SkillDelete;
        self.authorize(context, tool, Some(computer_id)).await?;
        let summary = serde_json::json!({
            "computer_id": computer_id,
            "name": name,
            "expected_revision": expected_revision
        });
        let lease = match self
            .target_skill_mutation_lease(computer_id, &source_id, tool)
            .await
        {
            Ok(lease) => lease,
            Err(error) => {
                self.audit_skill_mutation_failure(
                    SkillMutationAudit {
                        request_id,
                        source_id,
                        computer_id: computer_id.to_string(),
                        tool,
                        summary,
                    },
                    &error,
                );
                return Err(error);
            }
        };
        let result = self
            .skill_packages
            .delete(
                lease.configured_skill_home(),
                lease.effective_skill_home().await,
                name,
                expected_revision,
            )
            .await;
        self.finish_skill_mutation(
            SkillMutationAudit {
                request_id,
                source_id,
                computer_id: computer_id.to_string(),
                tool,
                summary,
            },
            lease,
            result,
        )
        .await
    }

    async fn target_runtime(
        &self,
        computer_id: &str,
        source_id: &str,
        tool: ToolId,
    ) -> Result<crate::services::computer::ComputerInstanceRuntime, ClientControlError> {
        self.computer_registry
            .runtime(computer_id)
            .await
            .ok_or_else(|| {
                ClientControlError::invocation(
                    ClientControlErrorCode::TargetNotFound,
                    "target Computer runtime does not exist",
                    source_id,
                    tool,
                    Some(computer_id),
                )
            })
    }

    async fn target_skill_mutation_lease(
        &self,
        computer_id: &str,
        source_id: &str,
        tool: ToolId,
    ) -> Result<crate::services::computer::SdkSkillMutationLease, ClientControlError> {
        self.target_runtime(computer_id, source_id, tool)
            .await?
            .acquire_skill_mutation_lease()
            .await
            .map_err(|error| {
                ClientControlError::invocation(
                    ClientControlErrorCode::OperationFailed,
                    error,
                    source_id,
                    tool,
                    Some(computer_id),
                )
            })
    }

    async fn finish_skill_mutation(
        &self,
        audit: SkillMutationAudit,
        lease: crate::services::computer::SdkSkillMutationLease,
        result: Result<SkillMutationResult, SkillPackageError>,
    ) -> Result<SkillMutationResult, ClientControlError> {
        match result {
            Ok(mut mutation) => {
                lease.mark_skills_dirty().await;
                mutation.mark_notified();
                let _ = self.audit(ControlAuditRecord {
                    request_id: audit.request_id,
                    source_computer_id: audit.source_id,
                    target_computer_id: Some(audit.computer_id),
                    tool: audit.tool,
                    parameters: audit.summary,
                    outcome: AuditOutcome::Succeeded,
                    error: None,
                });
                Ok(mutation)
            }
            Err(error) => {
                let mapped = map_skill_package_error(error);
                let _ = self.audit(ControlAuditRecord {
                    request_id: audit.request_id,
                    source_computer_id: audit.source_id,
                    target_computer_id: Some(audit.computer_id),
                    tool: audit.tool,
                    parameters: audit.summary,
                    outcome: AuditOutcome::Failed,
                    error: Some(mapped.message.clone()),
                });
                Err(mapped)
            }
        }
    }

    fn audit_skill_mutation_failure(&self, audit: SkillMutationAudit, error: &ClientControlError) {
        let _ = self.audit(ControlAuditRecord {
            request_id: audit.request_id,
            source_computer_id: audit.source_id,
            target_computer_id: Some(audit.computer_id),
            tool: audit.tool,
            parameters: audit.summary,
            outcome: AuditOutcome::Failed,
            error: Some(error.message.clone()),
        });
    }
}

fn change_audit_summary(change: &SkillFileChange) -> serde_json::Value {
    match change {
        SkillFileChange::Upsert {
            path,
            encoding,
            content,
        } => serde_json::json!({
            "action": "upsert",
            "path": path,
            "encoding": encoding,
            "decoded_size": decoded_content_size(*encoding, content)
        }),
        SkillFileChange::Delete { path } => {
            serde_json::json!({"action": "delete", "path": path})
        }
    }
}

fn decoded_content_size(encoding: SkillContentEncoding, content: &str) -> Option<usize> {
    match encoding {
        SkillContentEncoding::Utf8 => Some(content.len()),
        SkillContentEncoding::Base64 => base64::engine::general_purpose::STANDARD
            .decode(content)
            .ok()
            .map(|bytes| bytes.len()),
    }
}

fn map_skill_package_error(error: SkillPackageError) -> ClientControlError {
    let code = match error.code {
        SkillPackageErrorCode::SkillAlreadyExists => ClientControlErrorCode::SkillAlreadyExists,
        SkillPackageErrorCode::SkillNotFound => ClientControlErrorCode::SkillNotFound,
        SkillPackageErrorCode::RevisionConflict => ClientControlErrorCode::RevisionConflict,
        SkillPackageErrorCode::RestartRequired => ClientControlErrorCode::RestartRequired,
        SkillPackageErrorCode::InvalidSkillName
        | SkillPackageErrorCode::InvalidPath
        | SkillPackageErrorCode::ForbiddenFile
        | SkillPackageErrorCode::InvalidEncoding
        | SkillPackageErrorCode::InvalidPackage
        | SkillPackageErrorCode::PackageLimitExceeded
        | SkillPackageErrorCode::UnsafeExistingPackage => {
            ClientControlErrorCode::InvalidSkillPackage
        }
        SkillPackageErrorCode::MutationFailed => ClientControlErrorCode::OperationFailed,
    };
    ClientControlError::new(code, error.message)
}

#[must_use]
pub fn is_source_destructive(tool: ToolId) -> bool {
    matches!(
        tool,
        ToolId::ComputerStop
            | ToolId::ComputerRestart
            | ToolId::ComputerDelete
            | ToolId::ComputerDisconnect
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::computer::ComputerInstance;
    use crate::services::config::ConfigService;
    use crate::services::keychain::InMemorySecretStore;
    use crate::services::observability::ObservabilityService;
    use crate::services::settings::SettingsService;
    use a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig;
    use std::collections::BTreeSet;
    use tempfile::TempDir;

    #[derive(Default)]
    struct RecordingAuditSink {
        records: std::sync::Mutex<Vec<(ToolId, AuditOutcome)>>,
    }

    impl ControlAuditSink for RecordingAuditSink {
        fn record(&self, record: ControlAuditRecord) -> Result<(), String> {
            self.records
                .lock()
                .unwrap()
                .push((record.tool, record.outcome));
            Ok(())
        }
    }

    fn test_plane() -> (Arc<ClientControlPlane>, Arc<ConfigService>, TempDir) {
        let temp = TempDir::new().unwrap();
        let config = Arc::new(ConfigService::new(temp.path().to_path_buf()).unwrap());
        for id in ["source", "target-b", "target-c"] {
            config
                .add_computer_instance(ComputerInstance::new(id, id))
                .unwrap();
        }
        let registry = Arc::new(ComputerRegistry::from_config_with_skill_home_base(
            config.load_computer_instances().unwrap(),
            config.computer_skill_home_base(),
        ));
        let plane = Arc::new(ClientControlPlane::new(
            config.clone(),
            registry,
            Arc::new(NoopControlAuditSink),
        ));
        (plane, config, temp)
    }

    fn context(request: &str) -> InvocationContext {
        InvocationContext {
            request_id: request.to_string(),
            source_computer_id: "source".to_string(),
        }
    }

    #[test]
    fn self_protection_is_exactly_the_four_confirmed_operations() {
        let protected = ToolId::ALL
            .into_iter()
            .filter(|tool| is_source_destructive(*tool))
            .map(ToolId::as_str)
            .collect::<Vec<_>>();
        assert_eq!(
            protected,
            vec![
                "computer_delete",
                "computer_stop",
                "computer_restart",
                "computer_disconnect"
            ]
        );
    }

    #[tokio::test]
    async fn discovery_and_call_time_authorization_enforce_tool_target_and_self_protection() {
        let (plane, config, _temp) = test_plane();
        config
            .update_computer_instance("source", |source| {
                source.remote_control = RemoteControlPolicy {
                    enabled: true,
                    tool_scope: ToolScope::Custom {
                        tools: BTreeSet::from([
                            "computer_get_status".to_string(),
                            "computer_stop".to_string(),
                        ]),
                    },
                    target_scope: TargetScope::Custom {
                        targets: BTreeSet::from(["source".to_string(), "target-b".to_string()]),
                    },
                };
            })
            .unwrap();

        let discovered = plane
            .discover_targets("source", ToolId::ComputerGetStatus)
            .await
            .unwrap();
        assert_eq!(
            discovered
                .into_iter()
                .map(|instance| instance.id)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["source".to_string(), "target-b".to_string()])
        );
        assert_eq!(
            plane
                .authorize(
                    context("forged-target"),
                    ToolId::ComputerGetStatus,
                    Some("target-c"),
                )
                .await
                .unwrap_err()
                .code,
            ClientControlErrorCode::TargetNotAllowed
        );
        assert_eq!(
            plane
                .authorize(context("self-stop"), ToolId::ComputerStop, Some("source"),)
                .await
                .unwrap_err()
                .code,
            ClientControlErrorCode::SourceSelfProtection
        );
        assert_eq!(
            plane
                .authorize(
                    context("forged-tool"),
                    ToolId::McpToolList,
                    Some("target-b"),
                )
                .await
                .unwrap_err()
                .code,
            ClientControlErrorCode::ToolNotAllowed
        );
    }

    #[tokio::test]
    async fn control_plane_skill_create_uses_target_home_and_notifies_sdk_refresh() {
        let (plane, config, _temp) = test_plane();
        config
            .update_computer_instance("source", |source| {
                source.remote_control = RemoteControlPolicy {
                    enabled: true,
                    tool_scope: ToolScope::Custom {
                        tools: BTreeSet::from(["skill_create".to_string()]),
                    },
                    target_scope: TargetScope::Custom {
                        targets: BTreeSet::from(["target-b".to_string()]),
                    },
                };
            })
            .unwrap();

        let result = plane
            .skill_create(
                context("skill-create"),
                "target-b",
                "managed-skill".to_string(),
                vec![SkillFileInput {
                    path: "SKILL.md".to_string(),
                    encoding: SkillContentEncoding::Utf8,
                    content: "---\nname: managed-skill\ndescription: Managed\n---\nbody\n"
                        .to_string(),
                }],
            )
            .await
            .unwrap();

        assert_eq!(result.refresh_status, SkillRefreshStatus::Notified);
        assert!(config
            .default_local_skills_root("target-b")
            .join("user/managed-skill/SKILL.md")
            .is_file());
        assert!(!config
            .default_local_skills_root("source")
            .join("user/managed-skill")
            .exists());
    }

    #[tokio::test]
    async fn enabled_provider_exposes_exact_catalog_but_stays_out_of_mcp_management() {
        let temp = TempDir::new().unwrap();
        let config = Arc::new(ConfigService::new(temp.path().to_path_buf()).unwrap());
        let mut source = ComputerInstance::new("source", "Source");
        source.remote_control.enabled = true;
        config.add_computer_instance(source).unwrap();
        let registry = Arc::new(ComputerRegistry::from_config_with_skill_home_base(
            config.load_computer_instances().unwrap(),
            config.computer_skill_home_base(),
        ));
        let plane = Arc::new(ClientControlPlane::new(
            config,
            registry.clone(),
            Arc::new(NoopControlAuditSink),
        ));
        registry.bind_client_control(&plane);
        let runtime = registry.runtime("source").await.unwrap();
        runtime.start().await.unwrap();

        assert!(runtime.sdk_mcp_server_ownership().await.is_empty());
        let provider = runtime
            .mcp_server_runtime_statuses()
            .await
            .into_iter()
            .find(|status| status.bundle_id.as_str() == CLIENT_CONTROL_BUNDLE_ID)
            .expect("reserved provider must be mounted");
        assert!(
            provider.is_connected(),
            "reserved provider must be connected"
        );

        let tools = runtime.available_tools().await.unwrap();
        let control_tools = tools
            .iter()
            .filter(|tool| tool.name.starts_with("client_control__"))
            .collect::<Vec<_>>();
        assert_eq!(control_tools.len(), 55);
        assert!(control_tools
            .iter()
            .all(|tool| tool.name.as_ref() != "client_control__mcp_tool_execute"));

        runtime.stop_all_mcp_servers().await.unwrap();
        let provider_after_batch_stop = runtime
            .mcp_server_runtime_statuses()
            .await
            .into_iter()
            .find(|status| status.bundle_id.as_str() == CLIENT_CONTROL_BUNDLE_ID)
            .unwrap();
        assert!(provider_after_batch_stop.is_connected());
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn policy_toggle_hot_swaps_only_the_provider_without_persisting_an_mcp_server() {
        let temp = TempDir::new().unwrap();
        let config = Arc::new(ConfigService::new(temp.path().to_path_buf()).unwrap());
        config
            .add_computer_instance(ComputerInstance::new("source", "Source"))
            .unwrap();
        let registry = Arc::new(ComputerRegistry::from_config_with_skill_home_base(
            config.load_computer_instances().unwrap(),
            config.computer_skill_home_base(),
        ));
        let plane = Arc::new(ClientControlPlane::new(
            config.clone(),
            registry.clone(),
            Arc::new(NoopControlAuditSink),
        ));
        registry.bind_client_control(&plane);
        let runtime = registry.runtime("source").await.unwrap();
        runtime.start().await.unwrap();
        let generation = runtime.runtime_snapshot().await.generation;

        plane
            .update_policy_local(
                "source",
                RemoteControlPolicy {
                    enabled: true,
                    ..RemoteControlPolicy::default()
                },
            )
            .await
            .unwrap();
        let enabled = registry.runtime("source").await.unwrap();
        assert_eq!(enabled.runtime_snapshot().await.generation, generation);
        assert!(enabled
            .mcp_server_runtime_statuses()
            .await
            .iter()
            .any(|status| {
                status.bundle_id.as_str() == CLIENT_CONTROL_BUNDLE_ID && status.is_connected()
            }));
        assert!(enabled.sdk_mcp_server_ownership().await.is_empty());
        plane
            .update_policy_local(
                "source",
                RemoteControlPolicy {
                    enabled: true,
                    tool_scope: ToolScope::Custom {
                        tools: std::collections::BTreeSet::from([ToolId::ComputerList
                            .as_str()
                            .to_string()]),
                    },
                    target_scope: TargetScope::SelfOnly,
                },
            )
            .await
            .unwrap();
        let narrowed = registry.runtime("source").await.unwrap();
        assert_eq!(narrowed.runtime_snapshot().await.generation, generation);
        let narrowed_tools = narrowed
            .available_tools()
            .await
            .unwrap()
            .into_iter()
            .filter(|tool| tool.name.starts_with("client_control__"))
            .collect::<Vec<_>>();
        assert_eq!(narrowed_tools.len(), 1);
        assert_eq!(
            narrowed_tools[0].name.as_ref(),
            "client_control__computer_list"
        );

        plane
            .update_policy_local(
                "source",
                RemoteControlPolicy {
                    enabled: true,
                    ..RemoteControlPolicy::default()
                },
            )
            .await
            .unwrap();
        let expanded = registry.runtime("source").await.unwrap();
        assert_eq!(expanded.runtime_snapshot().await.generation, generation);
        assert_eq!(
            expanded
                .available_tools()
                .await
                .unwrap()
                .into_iter()
                .filter(|tool| tool.name.starts_with("client_control__"))
                .count(),
            55
        );
        plane
            .update_policy_local("source", RemoteControlPolicy::default())
            .await
            .unwrap();
        let disabled = registry.runtime("source").await.unwrap();
        assert_eq!(disabled.runtime_snapshot().await.generation, generation);
        assert!(disabled
            .mcp_server_runtime_statuses()
            .await
            .iter()
            .all(|status| status.bundle_id.as_str() != CLIENT_CONTROL_BUNDLE_ID));
        disabled.shutdown().await;
    }

    #[tokio::test]
    async fn skill_mutation_lease_blocks_target_runtime_removal_until_refresh_is_queued() {
        let temp = TempDir::new().unwrap();
        let registry = Arc::new(ComputerRegistry::from_config_with_skill_home_base(
            crate::services::computer::ComputerInstancesConfig {
                schema_version: 1,
                instances: vec![ComputerInstance::new("target", "Target")],
            },
            temp.path().join("skill-homes"),
        ));
        let runtime = registry.runtime("target").await.unwrap();
        let lease = runtime.acquire_skill_mutation_lease().await.unwrap();
        let removal_registry = registry.clone();
        let mut removal =
            tokio::spawn(async move { removal_registry.remove_runtime("target").await.unwrap() });

        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), &mut removal)
                .await
                .is_err()
        );
        lease.mark_skills_dirty().await;
        drop(lease);

        let removed = tokio::time::timeout(std::time::Duration::from_secs(2), removal)
            .await
            .expect("runtime removal should continue after mutation lease release")
            .unwrap();
        assert!(removed.is_some());
        assert!(registry.runtime("target").await.is_none());
    }

    #[tokio::test]
    async fn disabled_and_invalid_calls_are_audited_as_denied() {
        let temp = TempDir::new().unwrap();
        let config = Arc::new(ConfigService::new(temp.path().to_path_buf()).unwrap());
        config
            .add_computer_instance(ComputerInstance::new("source", "Source"))
            .unwrap();
        let registry = Arc::new(ComputerRegistry::from_config_with_skill_home_base(
            config.load_computer_instances().unwrap(),
            config.computer_skill_home_base(),
        ));
        let sink = Arc::new(RecordingAuditSink::default());
        let plane = Arc::new(ClientControlPlane::new(
            config.clone(),
            registry,
            sink.clone(),
        ));

        let disabled = plane
            .dispatch(
                context("disabled"),
                ToolId::ComputerList,
                serde_json::json!({}),
            )
            .await
            .unwrap_err();
        assert_eq!(disabled.code, ClientControlErrorCode::RemoteControlDisabled);
        config
            .update_computer_instance("source", |source| source.remote_control.enabled = true)
            .unwrap();
        let invalid = plane
            .dispatch(
                context("invalid"),
                ToolId::ComputerGetStatus,
                serde_json::json!({}),
            )
            .await
            .unwrap_err();
        assert_eq!(invalid.code, ClientControlErrorCode::InvalidArguments);
        assert_eq!(
            *sink.records.lock().unwrap(),
            vec![
                (ToolId::ComputerList, AuditOutcome::Denied),
                (ToolId::ComputerGetStatus, AuditOutcome::Denied),
            ]
        );
    }

    #[tokio::test]
    async fn host_dispatch_reuses_input_core_and_keeps_secret_values_write_only() {
        let temp = TempDir::new().unwrap();
        let config = ConfigService::new(temp.path().to_path_buf()).unwrap();
        config
            .add_computer_instance(ComputerInstance::new("source", "Source"))
            .unwrap();
        config
            .add_computer_instance(ComputerInstance::new("target", "Target"))
            .unwrap();
        let state = crate::AppState::new_with_secret_store(
            config,
            ObservabilityService::new(temp.path()).unwrap(),
            SettingsService::new(temp.path().to_path_buf()),
            InMemorySecretStore::shared(),
        );
        state
            .client_control
            .update_policy_local(
                "source",
                RemoteControlPolicy {
                    enabled: true,
                    tool_scope: ToolScope::All,
                    target_scope: TargetScope::All,
                },
            )
            .await
            .unwrap();

        state
            .client_control
            .dispatch(
                context("definition"),
                ToolId::InputDefinitionUpsert,
                serde_json::json!({
                    "computer_id": "target",
                    "definition": {
                        "type": "PromptString",
                        "id": "api-key",
                        "label": "API key",
                        "password": true
                    }
                }),
            )
            .await
            .unwrap();
        state
            .client_control
            .dispatch(
                context("value"),
                ToolId::InputValueSet,
                serde_json::json!({
                    "computer_id": "target",
                    "input_id": "api-key",
                    "value": "top-secret"
                }),
            )
            .await
            .unwrap();
        let status = state
            .client_control
            .dispatch(
                context("status"),
                ToolId::InputValueGetStatus,
                serde_json::json!({
                    "computer_id": "target",
                    "input_id": "api-key"
                }),
            )
            .await
            .unwrap();
        assert_eq!(status["configured"], true);
        assert!(status.get("value").is_none());
    }

    #[tokio::test]
    async fn client_control_config_state_redacts_constants_but_local_state_round_trips_them() {
        let temp = TempDir::new().unwrap();
        let config = ConfigService::new(temp.path().to_path_buf()).unwrap();
        config
            .add_computer_instance(ComputerInstance::new("source", "Source"))
            .unwrap();
        config
            .add_computer_instance(ComputerInstance::new("target", "Target"))
            .unwrap();
        let state = crate::AppState::new_with_secret_store(
            config,
            ObservabilityService::new(temp.path()).unwrap(),
            SettingsService::new(temp.path().to_path_buf()),
            InMemorySecretStore::shared(),
        );
        state
            .client_control
            .update_policy_local(
                "source",
                RemoteControlPolicy {
                    enabled: true,
                    tool_scope: ToolScope::All,
                    target_scope: TargetScope::All,
                },
            )
            .await
            .unwrap();

        let stdio: MCPServerConfig = serde_json::from_value(serde_json::json!({
            "type": "stdio",
            "name": "remote-secret-stdio",
            "server_parameters": {
                "command": "helper",
                "args": [],
                "env": {
                    "TOKEN": "remote-env-secret",
                    "REGION": "${input:REGION}"
                }
            }
        }))
        .unwrap();
        let http: MCPServerConfig = serde_json::from_value(serde_json::json!({
            "type": "http",
            "name": "remote-secret-http",
            "server_parameters": {
                "url": "https://remote-user:remote-password@example.com/mcp",
                "headers": { "Authorization": "Bearer remote-header-secret" }
            }
        }))
        .unwrap();
        state
            .sdk_config
            .upsert_mcp_configs("target", &[stdio, http])
            .unwrap();

        let local = crate::commands::sdk_config::get_computer_config_state_core(&state, "target")
            .await
            .unwrap();
        let local_json = serde_json::to_string(&local).unwrap();
        assert!(local_json.contains("remote-env-secret"));
        assert!(local_json.contains("remote-header-secret"));
        assert!(local_json.contains("remote-user:remote-password"));
        assert!(local_json.contains("${input:REGION}"));

        let remote = state
            .client_control
            .dispatch(
                context("redacted-config-state"),
                ToolId::McpConfigGetState,
                serde_json::json!({ "computer_id": "target" }),
            )
            .await
            .unwrap();
        let remote_json = serde_json::to_string(&remote).unwrap();
        assert!(!remote_json.contains("remote-env-secret"));
        assert!(!remote_json.contains("remote-header-secret"));
        assert!(!remote_json.contains("remote-user:remote-password"));
        assert!(remote_json.contains("${REDACTED}"));
        assert!(remote_json.contains("${input:REGION}"));
    }
}
