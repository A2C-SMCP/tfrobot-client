//! TFRSManager Tauri commands.
//!
//! Authentication and authenticated requests are delegated to the backend-owned
//! [`ManagerContextCoordinator`]. The only credential-bearing webview boundary is the dedicated,
//! generation-bound `@turingfocus/tfrs-auth` token bridge below.

use std::sync::Arc;

use futures_util::future::join_all;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::services::chat_session::ChatSessionService;
use crate::services::computer::{
    ComputerConnectionTarget, ComputerInstance, ComputerInstanceRuntime, ComputerRegistry,
    ManagerRobotBindingState, RobotBindingMetadata,
};
use crate::services::config::ConfigService;
use crate::services::manager_client::{
    DigitalEmployeeBrief, LoginResult, ManagerAccountSummary, ManagerError, UserInfo,
};
use crate::services::manager_context::{
    ManagerContextEventSink, ManagerContextKey, ManagerContextLifecycleSink,
    ManagerContextSnapshot, RestoredManagerSession, MANAGER_AUTH_EXPIRED_EVENT,
    MANAGER_CONTEXT_CHANGED_EVENT,
};
use crate::services::manager_environment::ManagerEnvironment;
use crate::services::manager_token_bridge::{
    ManagerTokenBridgeCompletion, ManagerTokenBridgeRequest, ManagerTokenBridgeSink,
    ManagerTokenHttpResponse,
};
use crate::services::observability::{
    redact_text, ActivityEventDraft, ActivityLevel, ActivityOutcome, ObservabilityService,
};
use crate::AppState;

pub(crate) struct TauriManagerContextEventSink {
    app: AppHandle,
    observability: Arc<ObservabilityService>,
}

impl TauriManagerContextEventSink {
    pub(crate) fn new(app: AppHandle, observability: Arc<ObservabilityService>) -> Self {
        Self { app, observability }
    }
}

impl ManagerContextEventSink for TauriManagerContextEventSink {
    fn emit_context_changed(&self, snapshot: &ManagerContextSnapshot) -> Result<(), String> {
        self.app
            .emit(MANAGER_CONTEXT_CHANGED_EVENT, snapshot)
            .map_err(|error| error.to_string())
    }

    fn emit_auth_expired(&self) -> Result<(), String> {
        let mut activity = ActivityEventDraft::client(
            ActivityLevel::Warn,
            "auth",
            "manager_session",
            "session_expired",
            ActivityOutcome::Failed,
            "Manager session expired",
        );
        activity.fields = Some(serde_json::json!({
            "app_version": env!("CARGO_PKG_VERSION"),
            "trigger": "system",
            "error_code": "unauthorized",
        }));
        activity.correlation_id = Some(uuid::Uuid::new_v4().to_string());
        if let Err(error) = self.observability.record_activity(&activity) {
            log::error!("failed to persist Manager session expiry activity: {error}");
        }
        self.app
            .emit(MANAGER_AUTH_EXPIRED_EVENT, ())
            .map_err(|error| error.to_string())
    }
}

pub(crate) struct TauriManagerTokenBridgeSink {
    app: AppHandle,
}

impl TauriManagerTokenBridgeSink {
    pub(crate) fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

#[async_trait::async_trait]
impl ManagerTokenBridgeSink for TauriManagerTokenBridgeSink {
    async fn emit_token_request(&self, request: &ManagerTokenBridgeRequest) -> Result<(), String> {
        let window = self
            .app
            .get_webview_window("main")
            .ok_or_else(|| "main webview is unavailable".to_string())?;
        window
            .emit(
                crate::services::manager_token_bridge::MANAGER_TOKEN_REQUEST_EVENT,
                request,
            )
            .map_err(|error| error.to_string())
    }
}

/// Bridges a Manager identity transaction to all client-owned Computer runtimes without retaining
/// `AppState` (and therefore without creating an `AppState -> coordinator -> sink` Arc cycle).
#[derive(Clone)]
pub(crate) struct TauriManagerContextLifecycleSink {
    config: Arc<ConfigService>,
    computer_registry: Arc<ComputerRegistry>,
    chat_sessions: Arc<ChatSessionService>,
}

impl TauriManagerContextLifecycleSink {
    pub(crate) fn new(
        config: Arc<ConfigService>,
        computer_registry: Arc<ComputerRegistry>,
        chat_sessions: Arc<ChatSessionService>,
    ) -> Self {
        Self {
            config,
            computer_registry,
            chat_sessions,
        }
    }

    async fn persist_dormant_profile(
        &self,
        instance_id: &str,
        departing_context: &ManagerContextKey,
    ) -> Result<(), String> {
        // The caller holds this Computer's operation gate. ConfigService provides the short
        // atomic filesystem commit; runtime replacement remains outside every cross-Computer
        // coordinator so a slow teardown cannot convoy unrelated Computers.
        let previous = self
            .config
            .get_computer_instance(instance_id)
            .map_err(|error| error.to_string())?;
        let mut candidate = previous.clone();
        if !make_departing_manager_binding_dormant(&mut candidate, departing_context) {
            return Ok(());
        }
        let updated = self
            .config
            .update_computer_instance(instance_id, |instance| {
                make_departing_manager_binding_dormant(instance, departing_context);
            })
            .map_err(|error| error.to_string())?;
        if let Err(runtime_error) = self
            .computer_registry
            .update_runtime_instance(updated)
            .await
        {
            let restored = self
                .config
                .update_computer_instance(instance_id, |instance| {
                    *instance = previous.clone();
                })
                .map_err(|restore_error| {
                    format!(
                        "failed to mark Manager binding dormant in runtime: {runtime_error}; additionally failed to restore persisted profile: {restore_error}"
                    )
                })?;
            if let Err(restore_runtime_error) = self
                .computer_registry
                .update_runtime_instance(restored)
                .await
            {
                return Err(format!(
                    "failed to mark Manager binding dormant in runtime: {runtime_error}; persisted profile was restored, but runtime restore failed: {restore_runtime_error}"
                ));
            }
            return Err(format!(
                "failed to mark Manager binding dormant in runtime; restored previous profile: {runtime_error}"
            ));
        }
        Ok(())
    }

    async fn cleanup_runtime_for_context(
        &self,
        runtime: ComputerInstanceRuntime,
        departing_context: Option<&ManagerContextKey>,
    ) -> Vec<String> {
        let instance_id = runtime.instance.id.clone();
        let _operation_guard = self.computer_registry.operation_lease(&instance_id).await;
        if self
            .computer_registry
            .ensure_current_runtime(&runtime)
            .await
            .is_err()
        {
            return Vec::new();
        }
        let mut diagnostics = Vec::new();
        if let Err(error) = runtime
            .clear_manager_connection_for_context_transaction()
            .await
        {
            diagnostics.push(format!(
                "Computer {instance_id} Manager connection teardown: {error}"
            ));
        }
        if let Some(context) = departing_context {
            if let Err(error) = self.persist_dormant_profile(&instance_id, context).await {
                diagnostics.push(format!(
                    "Computer {instance_id} Manager binding cleanup: {error}"
                ));
            }
        }
        diagnostics
    }

    async fn cleanup_manager_context_transaction(
        &self,
        departing_context: Option<ManagerContextKey>,
    ) -> Vec<String> {
        // Chat is single-current-Context state. Clear every lease fail-closed so an abnormal
        // historical lease can never survive an account, organization, environment, or auth
        // transition.
        self.chat_sessions.close_for_context(None).await;
        // Publish a fail-closed Context tombstone and take the runtime snapshot in one short
        // membership transaction. Duplicate publication checks the tombstone, so long teardown
        // work can proceed without holding a cross-Computer guard.
        let runtimes = {
            let _membership_guard = self.computer_registry.membership_lease().await;
            if let Some(context) = departing_context.as_ref() {
                self.computer_registry
                    .mark_manager_context_departing(context.clone());
            }
            self.computer_registry.list_runtimes().await
        };
        let diagnostics: Vec<String> =
            join_all(runtimes.into_iter().map(|runtime| {
                self.cleanup_runtime_for_context(runtime, departing_context.as_ref())
            }))
            .await
            .into_iter()
            .flatten()
            .collect();
        if diagnostics.is_empty() {
            if let Some(context) = departing_context.as_ref() {
                let _membership_guard = self.computer_registry.membership_lease().await;
                self.computer_registry
                    .clear_departing_manager_context(context);
            }
        }
        diagnostics
    }
}

#[async_trait::async_trait]
impl ManagerContextLifecycleSink for TauriManagerContextLifecycleSink {
    async fn cleanup_manager_context(
        &self,
        departing_context: Option<&ManagerContextKey>,
    ) -> Vec<String> {
        let sink = self.clone();
        let departing_context = departing_context.cloned();
        tokio::spawn(async move {
            sink.cleanup_manager_context_transaction(departing_context)
                .await
        })
        .await
        .unwrap_or_else(|error| vec![format!("Manager Context cleanup task failed: {error}")])
    }
}

pub(crate) fn make_departing_manager_binding_dormant(
    instance: &mut ComputerInstance,
    departing_context: &ManagerContextKey,
) -> bool {
    let target = match instance.connection_policy.target.as_ref() {
        Some(ComputerConnectionTarget::ManagerRobot {
            context_key,
            employee_id,
            last_resolved_robot_account_id,
        }) if context_key == departing_context => {
            Some((*employee_id, last_resolved_robot_account_id.clone()))
        }
        _ => None,
    };
    let binding_matches = instance
        .robot_binding
        .as_ref()
        .and_then(|binding| binding.context_key.as_ref())
        == Some(departing_context);
    if target.is_none() && !binding_matches {
        return false;
    }

    if let Some((employee_id, last_resolved_robot_account_id)) = target {
        instance.connection_policy.auto_connect = false;
        let binding = instance
            .robot_binding
            .get_or_insert_with(|| RobotBindingMetadata {
                context_key: Some(departing_context.clone()),
                state: ManagerRobotBindingState::Dormant,
                employee_id,
                robot_id: None,
                last_resolved_robot_account_id: last_resolved_robot_account_id.clone(),
                namespace: None,
                robot_name: None,
            });
        if binding.context_key.as_ref() != Some(departing_context) {
            *binding = RobotBindingMetadata {
                context_key: Some(departing_context.clone()),
                state: ManagerRobotBindingState::Dormant,
                employee_id,
                robot_id: None,
                last_resolved_robot_account_id,
                namespace: None,
                robot_name: None,
            };
        } else {
            binding.state = ManagerRobotBindingState::Dormant;
        }
    } else if let Some(binding) = instance.robot_binding.as_mut() {
        binding.state = ManagerRobotBindingState::Dormant;
    }
    true
}

#[tauri::command]
pub async fn manager_get_context(
    state: State<'_, AppState>,
) -> Result<ManagerContextSnapshot, ManagerError> {
    Ok(state.manager_context.snapshot().await)
}

#[tauri::command]
pub async fn manager_token_bridge_ready(
    state: State<'_, AppState>,
    lease_id: String,
    ready: bool,
) -> Result<(), ManagerError> {
    state
        .manager_context
        .set_token_bridge_ready(&lease_id, ready)
        .await;
    Ok(())
}

#[tauri::command]
pub async fn manager_token_bridge_http_request(
    state: State<'_, AppState>,
    request_id: String,
    generation: u64,
    body: String,
) -> Result<ManagerTokenHttpResponse, ManagerError> {
    if body.len() > 1024 * 1024 {
        return Err(ManagerError::InvalidResponse(
            "Manager token request exceeded the 1 MiB bridge limit".to_string(),
        ));
    }
    let (status, body, content_type) = state
        .manager_context
        .token_bridge_http_request(&request_id, generation, body)
        .await?;
    Ok(ManagerTokenHttpResponse {
        status,
        body,
        content_type,
    })
}

#[tauri::command]
pub async fn manager_token_bridge_complete(
    state: State<'_, AppState>,
    request_id: String,
    generation: u64,
    completion: ManagerTokenBridgeCompletion,
) -> Result<(), ManagerError> {
    state
        .manager_context
        .complete_token_bridge_request(&request_id, generation, completion)
        .await
}

#[tauri::command]
pub async fn manager_login(
    state: State<'_, AppState>,
    environment: ManagerEnvironment,
    identifier: String,
    password: String,
) -> Result<LoginResult, ManagerError> {
    manager_login_core(&state, environment, identifier, password).await
}

async fn manager_login_core(
    state: &AppState,
    environment: ManagerEnvironment,
    identifier: String,
    password: String,
) -> Result<LoginResult, ManagerError> {
    let started = std::time::Instant::now();
    log::info!("manager_login: environment={environment:?}");
    let result = state
        .manager_context
        .login(environment, &identifier, &password)
        .await;
    record_manager_activity(
        state,
        ManagerActivityContext {
            operation: "login",
            trigger: "user",
            started,
            requested_environment: Some(environment),
            target_account_id: None,
        },
        &result,
    )
    .await;
    result
}

#[tauri::command]
pub async fn manager_select_account(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<UserInfo, ManagerError> {
    manager_select_account_core(&state, account_id).await
}

async fn manager_select_account_core(
    state: &AppState,
    account_id: String,
) -> Result<UserInfo, ManagerError> {
    let started = std::time::Instant::now();
    log::info!("manager_select_account: account_id={account_id}");
    let result = state.manager_context.select_account(&account_id).await;
    record_manager_activity(
        state,
        ManagerActivityContext {
            operation: "select_account",
            trigger: "user",
            started,
            requested_environment: None,
            target_account_id: Some(&account_id),
        },
        &result,
    )
    .await;
    result
}

#[tauri::command]
pub async fn manager_restore_session(
    state: State<'_, AppState>,
) -> Result<Option<RestoredManagerSession>, ManagerError> {
    manager_restore_session_core(&state).await
}

async fn manager_restore_session_core(
    state: &AppState,
) -> Result<Option<RestoredManagerSession>, ManagerError> {
    let started = std::time::Instant::now();
    let result = state.manager_context.restore_session().await;
    record_manager_activity(
        state,
        ManagerActivityContext {
            operation: "restore_session",
            trigger: "system",
            started,
            requested_environment: None,
            target_account_id: None,
        },
        &result,
    )
    .await;
    result
}

#[tauri::command]
pub async fn manager_list_digital_employees(
    state: State<'_, AppState>,
) -> Result<Vec<DigitalEmployeeBrief>, ManagerError> {
    state.manager_context.list_digital_employees().await
}

#[tauri::command]
pub async fn manager_list_accounts(
    state: State<'_, AppState>,
) -> Result<Vec<ManagerAccountSummary>, ManagerError> {
    state.manager_context.list_accounts().await
}

#[tauri::command]
pub async fn manager_switch_account(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<(), ManagerError> {
    manager_switch_account_core(&state, account_id).await
}

async fn manager_switch_account_core(
    state: &AppState,
    account_id: String,
) -> Result<(), ManagerError> {
    let started = std::time::Instant::now();
    log::info!("manager_switch_account: account_id={account_id}");
    let result = state.manager_context.switch_account(&account_id).await;
    record_manager_activity(
        state,
        ManagerActivityContext {
            operation: "switch_account",
            trigger: "user",
            started,
            requested_environment: None,
            target_account_id: Some(&account_id),
        },
        &result,
    )
    .await;
    result
}

#[tauri::command]
pub async fn manager_logout(state: State<'_, AppState>) -> Result<(), ManagerError> {
    manager_logout_core(&state).await
}

async fn manager_logout_core(state: &AppState) -> Result<(), ManagerError> {
    let started = std::time::Instant::now();
    log::info!("manager_logout");
    let result = state.manager_context.logout().await;
    record_manager_activity(
        state,
        ManagerActivityContext {
            operation: "logout",
            trigger: "user",
            started,
            requested_environment: None,
            target_account_id: None,
        },
        &result,
    )
    .await;
    result
}

struct ManagerActivityContext<'a> {
    operation: &'a str,
    trigger: &'a str,
    started: std::time::Instant,
    requested_environment: Option<ManagerEnvironment>,
    target_account_id: Option<&'a str>,
}

async fn record_manager_activity<T>(
    state: &AppState,
    context: ManagerActivityContext<'_>,
    result: &Result<T, ManagerError>,
) {
    let snapshot = state.manager_context.snapshot().await;
    let (level, outcome, message, error_code, error) = match result {
        Ok(_) => (
            ActivityLevel::Info,
            ActivityOutcome::Succeeded,
            format!("Manager {} succeeded", context.operation),
            None,
            None,
        ),
        Err(error) => {
            let serialized = serde_json::to_value(error).unwrap_or_default();
            (
                ActivityLevel::Warn,
                ActivityOutcome::Failed,
                format!("Manager {} failed", context.operation),
                serialized
                    .get("kind")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
                Some(redact_text(&error.to_string())),
            )
        }
    };
    let mut activity = ActivityEventDraft::client(
        level,
        "auth",
        "manager_session",
        context.operation,
        outcome,
        message,
    );
    activity.fields = Some(serde_json::json!({
        "app_version": env!("CARGO_PKG_VERSION"),
        "trigger": context.trigger,
        "environment": context.requested_environment.or(snapshot.environment),
        "target_id": context.target_account_id,
        "account_id": snapshot.context_key.as_ref().map(|context| &context.account_id),
        "organization_id": snapshot.context_key.as_ref().map(|context| &context.organization_id),
        "auth_state": snapshot.auth_state,
        "duration_ms": context.started.elapsed().as_millis(),
        "error_code": error_code,
        "error": error,
    }));
    activity.correlation_id = Some(uuid::Uuid::new_v4().to_string());
    if let Err(error) = state.observability.record_activity_async(activity).await {
        log::error!(
            "failed to persist Manager {} activity: {error}",
            context.operation
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::config::ConfigService;
    use crate::services::keychain::InMemorySecretStore;
    use crate::services::observability::{
        ActivityQuery, ActivityScopeFilter, ObservabilityService,
    };
    use crate::services::settings::SettingsService;
    use tempfile::tempdir;
    fn context(account_id: &str, organization_id: &str) -> ManagerContextKey {
        ManagerContextKey {
            environment: ManagerEnvironment::Staging,
            account_id: account_id.to_string(),
            organization_id: organization_id.to_string(),
        }
    }

    #[tokio::test]
    async fn manager_activity_records_stable_result_and_redacts_failure_details() {
        let dir = tempdir().unwrap();
        let state = AppState::new_with_secret_store(
            ConfigService::new(dir.path().to_path_buf()).unwrap(),
            ObservabilityService::new(dir.path()).unwrap(),
            SettingsService::new(dir.path().to_path_buf()),
            InMemorySecretStore::shared(),
        );

        record_manager_activity(
            &state,
            ManagerActivityContext {
                operation: "login",
                trigger: "user",
                started: std::time::Instant::now(),
                requested_environment: Some(ManagerEnvironment::Staging),
                target_account_id: None,
            },
            &Ok::<(), ManagerError>(()),
        )
        .await;
        record_manager_activity(
            &state,
            ManagerActivityContext {
                operation: "login",
                trigger: "user",
                started: std::time::Instant::now(),
                requested_environment: Some(ManagerEnvironment::Staging),
                target_account_id: None,
            },
            &Err::<(), ManagerError>(ManagerError::InvalidCredentials {
                message: "password=super-secret".to_string(),
            }),
        )
        .await;

        let page = state
            .observability
            .query_activity(&ActivityQuery {
                scope: ActivityScopeFilter::ClientOnly,
                ..ActivityQuery::default()
            })
            .unwrap();
        assert_eq!(page.total, 2);
        assert_eq!(page.items[0].category, "auth");
        assert_eq!(page.items[0].outcome, ActivityOutcome::Failed);
        assert_eq!(
            page.items[0]
                .fields
                .as_ref()
                .and_then(|fields| fields["error_code"].as_str()),
            Some("invalid_credentials")
        );
        let serialized = serde_json::to_string(&page.items[0].fields).unwrap();
        assert!(!serialized.contains("super-secret"));
        assert_eq!(page.items[1].outcome, ActivityOutcome::Succeeded);
    }

    #[tokio::test]
    async fn manager_command_cores_audit_restore_switch_and_logout_results() {
        let dir = tempdir().unwrap();
        let state = AppState::new_with_secret_store(
            ConfigService::new(dir.path().to_path_buf()).unwrap(),
            ObservabilityService::new(dir.path()).unwrap(),
            SettingsService::new(dir.path().to_path_buf()),
            InMemorySecretStore::shared(),
        );

        assert!(manager_restore_session_core(&state)
            .await
            .unwrap()
            .is_none());
        assert!(
            manager_switch_account_core(&state, "missing-account".to_string())
                .await
                .is_err()
        );
        manager_logout_core(&state).await.unwrap();

        let page = state
            .observability
            .query_activity(&ActivityQuery {
                scope: ActivityScopeFilter::ClientOnly,
                ..ActivityQuery::default()
            })
            .unwrap();
        assert_eq!(page.total, 3);
        let outcomes = page
            .items
            .iter()
            .map(|event| (event.operation.as_str(), event.outcome.clone()))
            .collect::<std::collections::HashMap<_, _>>();
        assert_eq!(
            outcomes.get("restore_session"),
            Some(&ActivityOutcome::Succeeded)
        );
        assert_eq!(
            outcomes.get("switch_account"),
            Some(&ActivityOutcome::Failed)
        );
        assert_eq!(outcomes.get("logout"), Some(&ActivityOutcome::Succeeded));
    }

    #[test]
    fn departing_manager_target_becomes_dormant_and_disables_auto_connect() {
        let departing = context("account-a", "organization-a");
        let mut instance = ComputerInstance::new("computer-a", "Computer A");
        instance.connection_policy.target = Some(ComputerConnectionTarget::manager_robot(
            departing.clone(),
            42,
            Some("robot-account-42".to_string()),
        ));
        instance.connection_policy.auto_connect = true;
        instance.robot_binding = Some(RobotBindingMetadata::active(departing.clone(), 42));

        assert!(make_departing_manager_binding_dormant(
            &mut instance,
            &departing
        ));
        assert!(!instance.connection_policy.auto_connect);
        assert_eq!(
            instance.robot_binding.as_ref().map(|binding| binding.state),
            Some(ManagerRobotBindingState::Dormant)
        );
    }

    #[test]
    fn manual_target_remains_auto_connectable_while_historical_manager_binding_dormants() {
        let departing = context("account-a", "organization-a");
        let mut instance = ComputerInstance::new("computer-a", "Computer A");
        instance.connection_policy.target = Some(ComputerConnectionTarget::manual_smcp("manual-a"));
        instance.connection_policy.auto_connect = true;
        instance.robot_binding = Some(RobotBindingMetadata::active(departing.clone(), 42));

        assert!(make_departing_manager_binding_dormant(
            &mut instance,
            &departing
        ));
        assert!(instance.connection_policy.auto_connect);
        assert_eq!(
            instance.robot_binding.as_ref().map(|binding| binding.state),
            Some(ManagerRobotBindingState::Dormant)
        );
    }

    #[test]
    fn unrelated_context_profile_is_not_changed() {
        let departing = context("account-a", "organization-a");
        let retained = context("account-b", "organization-b");
        let mut instance = ComputerInstance::new("computer-b", "Computer B");
        instance.connection_policy.target = Some(ComputerConnectionTarget::manager_robot(
            retained.clone(),
            84,
            None,
        ));
        instance.connection_policy.auto_connect = true;
        instance.robot_binding = Some(RobotBindingMetadata::active(retained, 84));
        let before = instance.clone();

        assert!(!make_departing_manager_binding_dormant(
            &mut instance,
            &departing
        ));
        assert_eq!(instance.connection_policy, before.connection_policy);
        assert_eq!(instance.robot_binding, before.robot_binding);
    }

    #[tokio::test]
    async fn lifecycle_cleanup_updates_persisted_profiles_and_runtime_mirrors() {
        let directory = tempfile::tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let departing = context("account-a", "organization-a");

        let mut manager = ComputerInstance::new("computer-manager", "Manager Computer");
        manager.connection_policy.target = Some(ComputerConnectionTarget::manager_robot(
            departing.clone(),
            42,
            Some("robot-account-42".to_string()),
        ));
        manager.connection_policy.auto_connect = true;
        manager.robot_binding = Some(RobotBindingMetadata::active(departing.clone(), 42));

        let mut manual = ComputerInstance::new("computer-manual", "Manual Computer");
        manual.connection_policy.target = Some(ComputerConnectionTarget::manual_smcp("manual-a"));
        manual.connection_policy.auto_connect = true;
        manual.robot_binding = Some(RobotBindingMetadata::active(departing.clone(), 84));

        config.add_computer_instance(manager).unwrap();
        config.add_computer_instance(manual).unwrap();
        let registry = Arc::new(ComputerRegistry::from_config_with_skill_home_base(
            config.load_computer_instances().unwrap(),
            directory.path().join("skills"),
        ));
        let sink = TauriManagerContextLifecycleSink::new(
            config.clone(),
            registry.clone(),
            Arc::new(ChatSessionService::new(std::sync::Weak::new())),
        );

        assert!(sink
            .cleanup_manager_context(Some(&departing))
            .await
            .is_empty());

        for instance_id in ["computer-manager", "computer-manual"] {
            let persisted = config.get_computer_instance(instance_id).unwrap();
            let runtime = registry.runtime(instance_id).await.unwrap();
            assert_eq!(
                runtime.instance.connection_policy,
                persisted.connection_policy
            );
            assert_eq!(runtime.instance.robot_binding, persisted.robot_binding);
            assert_eq!(
                persisted
                    .robot_binding
                    .as_ref()
                    .map(|binding| binding.state),
                Some(ManagerRobotBindingState::Dormant)
            );
        }

        let manager = config.get_computer_instance("computer-manager").unwrap();
        assert!(!manager.connection_policy.auto_connect);
        let manual = config.get_computer_instance("computer-manual").unwrap();
        assert!(matches!(
            manual.connection_policy.target,
            Some(ComputerConnectionTarget::ManualSmcp { .. })
        ));
        assert!(manual.connection_policy.auto_connect);
    }

    #[tokio::test]
    async fn lifecycle_cleanup_does_not_convoy_behind_one_computer() {
        let directory = tempfile::tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let departing = context("account-a", "organization-a");
        for (id, employee_id) in [("computer-a", 42), ("computer-b", 84)] {
            let mut instance = ComputerInstance::new(id, id);
            instance.connection_policy.target = Some(ComputerConnectionTarget::manager_robot(
                departing.clone(),
                employee_id,
                None,
            ));
            instance.connection_policy.auto_connect = true;
            instance.robot_binding =
                Some(RobotBindingMetadata::active(departing.clone(), employee_id));
            config.add_computer_instance(instance).unwrap();
        }
        let registry = Arc::new(ComputerRegistry::from_config_with_skill_home_base(
            config.load_computer_instances().unwrap(),
            directory.path().join("skills"),
        ));
        let sink = Arc::new(TauriManagerContextLifecycleSink::new(
            config.clone(),
            registry.clone(),
            Arc::new(ChatSessionService::new(std::sync::Weak::new())),
        ));

        // Model Computer A waiting indefinitely on Runtime Input. Cleanup for B must still finish,
        // because cleanup coordinates each Computer independently.
        let blocked_a = registry.operation_lease("computer-a").await;
        let cleanup_sink = sink.clone();
        let cleanup_context = departing.clone();
        let cleanup = tokio::spawn(async move {
            cleanup_sink
                .cleanup_manager_context(Some(&cleanup_context))
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let computer_b = config.get_computer_instance("computer-b").unwrap();
                if computer_b
                    .robot_binding
                    .as_ref()
                    .is_some_and(|binding| binding.state == ManagerRobotBindingState::Dormant)
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("Computer B cleanup must not wait for Computer A's operation gate");
        drop(blocked_a);
        assert!(cleanup.await.unwrap().is_empty());
        let computer_a = config.get_computer_instance("computer-a").unwrap();
        assert!(computer_a
            .robot_binding
            .as_ref()
            .is_some_and(|binding| binding.state == ManagerRobotBindingState::Dormant));
    }

    #[tokio::test]
    async fn cancelled_cleanup_request_still_finishes_and_clears_its_tombstone() {
        let directory = tempfile::tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let departing = context("account-a", "organization-a");
        let mut instance = ComputerInstance::new("computer-a", "Computer A");
        instance.connection_policy.target = Some(ComputerConnectionTarget::manager_robot(
            departing.clone(),
            42,
            None,
        ));
        instance.connection_policy.auto_connect = true;
        instance.robot_binding = Some(RobotBindingMetadata::active(departing.clone(), 42));
        config.add_computer_instance(instance).unwrap();
        let registry = Arc::new(ComputerRegistry::from_config_with_skill_home_base(
            config.load_computer_instances().unwrap(),
            directory.path().join("skills"),
        ));
        let sink = Arc::new(TauriManagerContextLifecycleSink::new(
            config.clone(),
            registry.clone(),
            Arc::new(ChatSessionService::new(std::sync::Weak::new())),
        ));
        let blocked = registry.operation_lease("computer-a").await;
        let cleanup_sink = sink.clone();
        let cleanup_context = departing.clone();
        let cleanup = tokio::spawn(async move {
            cleanup_sink
                .cleanup_manager_context(Some(&cleanup_context))
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !registry.is_manager_context_departing(&departing) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("cleanup did not publish its tombstone");

        cleanup.abort();
        assert!(cleanup.await.unwrap_err().is_cancelled());
        drop(blocked);
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while registry.is_manager_context_departing(&departing) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("detached cleanup did not clear its tombstone");
        let persisted = config.get_computer_instance("computer-a").unwrap();
        assert!(persisted
            .robot_binding
            .as_ref()
            .is_some_and(|binding| binding.state == ManagerRobotBindingState::Dormant));
    }

    #[tokio::test]
    async fn lifecycle_cleanup_cannot_miss_a_duplicate_inheriting_the_departing_binding() {
        let directory = tempfile::tempdir().unwrap();
        let config = Arc::new(ConfigService::new(directory.path().to_path_buf()).unwrap());
        let departing = context("account-a", "organization-a");
        let mut source = ComputerInstance::new("computer-source", "Computer Source");
        source.connection_policy.target = Some(ComputerConnectionTarget::manager_robot(
            departing.clone(),
            42,
            Some("robot-account-42".to_string()),
        ));
        source.connection_policy.auto_connect = true;
        source.robot_binding = Some(RobotBindingMetadata::active(departing.clone(), 42));
        config.add_computer_instance(source.clone()).unwrap();
        let registry = Arc::new(ComputerRegistry::from_config_with_skill_home_base(
            config.load_computer_instances().unwrap(),
            directory.path().join("skills"),
        ));
        let sink = Arc::new(TauriManagerContextLifecycleSink::new(
            config.clone(),
            registry.clone(),
            Arc::new(ChatSessionService::new(std::sync::Weak::new())),
        ));

        // Keep source cleanup pending after the Context snapshot. An unrelated duplicate must
        // still acquire membership and publish without waiting for this Computer, while the
        // departing-Context tombstone removes the authority missed by the earlier snapshot.
        let blocked_source = registry.operation_lease("computer-source").await;
        let cleanup_sink = sink.clone();
        let cleanup_context = departing.clone();
        let cleanup = tokio::spawn(async move {
            cleanup_sink
                .cleanup_manager_context(Some(&cleanup_context))
                .await
        });
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while !registry.is_manager_context_departing(&departing) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("Context cleanup did not publish its tombstone");
        assert!(
            !cleanup.is_finished(),
            "source cleanup must still be waiting for its per-Computer operation gate"
        );

        let duplicate_membership = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            registry.membership_lease(),
        )
        .await
        .expect("duplicate publication must not wait for unrelated Computer cleanup");
        let mut duplicate = source;
        duplicate.id = "computer-duplicate".to_string();
        duplicate.name = "Computer Duplicate".to_string();
        assert!(make_departing_manager_binding_dormant(
            &mut duplicate,
            &departing
        ));
        config.add_computer_instance(duplicate.clone()).unwrap();
        registry.upsert_runtime(duplicate).await.unwrap();
        drop(duplicate_membership);
        drop(blocked_source);

        assert!(cleanup.await.unwrap().is_empty());
        let persisted = config.get_computer_instance("computer-duplicate").unwrap();
        assert!(persisted
            .robot_binding
            .as_ref()
            .is_some_and(|binding| binding.state == ManagerRobotBindingState::Dormant));
        assert!(!persisted.connection_policy.auto_connect);
        let runtime = registry.runtime("computer-duplicate").await.unwrap();
        assert_eq!(runtime.instance.robot_binding, persisted.robot_binding);
        assert_eq!(
            runtime.instance.connection_policy,
            persisted.connection_policy
        );
    }
}
