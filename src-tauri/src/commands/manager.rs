//! TFRSManager Tauri commands.
//!
//! Authentication and authenticated requests are delegated to the backend-owned
//! [`ManagerContextCoordinator`]. Commands never construct identity context in the webview.

use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};
use tokio::sync::Mutex;

use crate::services::chat_session::ChatSessionService;
use crate::services::computer::{
    ComputerConnectionTarget, ComputerInstance, ComputerRegistry, ManagerRobotBindingState,
    RobotBindingMetadata,
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
use crate::AppState;

pub(crate) struct TauriManagerContextEventSink {
    app: AppHandle,
}

impl TauriManagerContextEventSink {
    pub(crate) fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl ManagerContextEventSink for TauriManagerContextEventSink {
    fn emit_context_changed(&self, snapshot: &ManagerContextSnapshot) -> Result<(), String> {
        self.app
            .emit(MANAGER_CONTEXT_CHANGED_EVENT, snapshot)
            .map_err(|error| error.to_string())
    }

    fn emit_auth_expired(&self) -> Result<(), String> {
        self.app
            .emit(MANAGER_AUTH_EXPIRED_EVENT, ())
            .map_err(|error| error.to_string())
    }
}

/// Bridges a Manager identity transaction to all client-owned Computer runtimes without retaining
/// `AppState` (and therefore without creating an `AppState -> coordinator -> sink` Arc cycle).
pub(crate) struct TauriManagerContextLifecycleSink {
    config: Arc<ConfigService>,
    computer_registry: Arc<ComputerRegistry>,
    computer_lifecycle_lock: Arc<Mutex<()>>,
    chat_sessions: Arc<ChatSessionService>,
}

impl TauriManagerContextLifecycleSink {
    pub(crate) fn new(
        config: Arc<ConfigService>,
        computer_registry: Arc<ComputerRegistry>,
        computer_lifecycle_lock: Arc<Mutex<()>>,
        chat_sessions: Arc<ChatSessionService>,
    ) -> Self {
        Self {
            config,
            computer_registry,
            computer_lifecycle_lock,
            chat_sessions,
        }
    }

    async fn persist_dormant_profile(
        &self,
        instance_id: &str,
        departing_context: &ManagerContextKey,
    ) -> Result<(), String> {
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
}

#[async_trait::async_trait]
impl ManagerContextLifecycleSink for TauriManagerContextLifecycleSink {
    async fn cleanup_manager_context(
        &self,
        departing_context: Option<&ManagerContextKey>,
    ) -> Vec<String> {
        // Chat is single-current-Context state. Clear every lease fail-closed so an abnormal
        // historical lease can never survive an account, organization, environment, or auth
        // transition.
        self.chat_sessions.close_for_context(None).await;
        let _lifecycle = self.computer_lifecycle_lock.lock().await;
        let runtimes = self.computer_registry.list_runtimes().await;
        let mut diagnostics = Vec::new();
        for runtime in runtimes {
            if let Err(error) = runtime
                .clear_manager_connection_for_context_transaction()
                .await
            {
                diagnostics.push(format!(
                    "Computer {} Manager connection teardown: {error}",
                    runtime.instance.id
                ));
            }
            if let Some(context) = departing_context {
                if let Err(error) = self
                    .persist_dormant_profile(&runtime.instance.id, context)
                    .await
                {
                    diagnostics.push(format!(
                        "Computer {} Manager binding cleanup: {error}",
                        runtime.instance.id
                    ));
                }
            }
        }
        diagnostics
    }
}

fn make_departing_manager_binding_dormant(
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
pub async fn manager_login(
    state: State<'_, AppState>,
    environment: ManagerEnvironment,
    identifier: String,
    password: String,
) -> Result<LoginResult, ManagerError> {
    log::info!("manager_login: environment={environment:?}");
    state
        .manager_context
        .login(environment, &identifier, &password)
        .await
}

#[tauri::command]
pub async fn manager_select_account(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<UserInfo, ManagerError> {
    log::info!("manager_select_account: account_id={account_id}");
    state.manager_context.select_account(&account_id).await
}

#[tauri::command]
pub async fn manager_restore_session(
    state: State<'_, AppState>,
) -> Result<Option<RestoredManagerSession>, ManagerError> {
    state.manager_context.restore_session().await
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
    log::info!("manager_switch_account: account_id={account_id}");
    state.manager_context.switch_account(&account_id).await
}

#[tauri::command]
pub async fn manager_logout(state: State<'_, AppState>) -> Result<(), ManagerError> {
    log::info!("manager_logout");
    state.manager_context.logout().await
}

#[cfg(test)]
mod tests {
    use super::*;
    fn context(account_id: &str, organization_id: &str) -> ManagerContextKey {
        ManagerContextKey {
            environment: ManagerEnvironment::Staging,
            account_id: account_id.to_string(),
            organization_id: organization_id.to_string(),
        }
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
            Arc::new(Mutex::new(())),
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
}
