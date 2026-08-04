//! Backend-owned TFRSManager identity context.
//!
//! This coordinator is the only production entry point for Manager authentication and
//! authenticated HTTP calls. It keeps credentials inside [`ManagerClient`], publishes only a
//! redacted snapshot, and serializes identity-changing transactions so a late response cannot
//! overwrite a newer account context.

use std::{
    future::Future,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

use crate::services::manager_client::{
    ConnectionInfoResponse, DigitalEmployeeBrief, ExchangedToken, LoginResult,
    ManagerAccountSummary, ManagerClient, ManagerCurrentUser, ManagerError, ManagerRequestOutcome,
    SwitchedManagerAccount, UserInfo,
};
use crate::services::manager_environment::ManagerEnvironment;
use crate::services::settings::{
    ManagerSessionConfig, ManagerSessionConfigError, PersistedManagerSession, SettingsService,
    MANAGER_SESSION_SCHEMA_VERSION,
};

pub const MANAGER_CONTEXT_CHANGED_EVENT: &str = "manager:context-changed";
pub const MANAGER_AUTH_EXPIRED_EVENT: &str = "manager:auth-expired";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagerAuthState {
    SignedOut,
    AccountSelectionRequired,
    OnboardingRequired,
    Authenticated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagerContextKey {
    pub environment: ManagerEnvironment,
    pub account_id: String,
    pub organization_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagerContextUser {
    pub id: String,
    pub nickname: String,
    pub email: String,
    pub phone: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagerContextAccount {
    pub id: String,
    pub name: String,
    pub nickname: String,
    pub avatar: String,
    pub employee_no: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagerContextOrganization {
    pub id: String,
    pub name: String,
    pub organization_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagerContextSnapshot {
    pub revision: u64,
    pub auth_state: ManagerAuthState,
    pub environment: Option<ManagerEnvironment>,
    pub context_key: Option<ManagerContextKey>,
    pub user: Option<ManagerContextUser>,
    pub account: Option<ManagerContextAccount>,
    pub organization: Option<ManagerContextOrganization>,
    pub permissions: Vec<String>,
}

impl Default for ManagerContextSnapshot {
    fn default() -> Self {
        Self {
            revision: 0,
            auth_state: ManagerAuthState::SignedOut,
            environment: None,
            context_key: None,
            user: None,
            account: None,
            organization: None,
            permissions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoredManagerSession {
    pub environment: ManagerEnvironment,
    pub user: UserInfo,
}

pub trait ManagerContextEventSink: Send + Sync {
    fn emit_context_changed(&self, snapshot: &ManagerContextSnapshot) -> Result<(), String>;
    fn emit_auth_expired(&self) -> Result<(), String>;
}

#[async_trait::async_trait]
pub trait ManagerContextLifecycleSink: Send + Sync {
    /// Returns diagnostic cleanup errors. Local authority must already be cleared for every item.
    async fn cleanup_manager_context(
        &self,
        departing_context: Option<&ManagerContextKey>,
    ) -> Vec<String>;
}

struct ManagerContextTransitionGuard<'a> {
    transitioning: &'a AtomicBool,
    owns_transition: bool,
}

impl Drop for ManagerContextTransitionGuard<'_> {
    fn drop(&mut self) {
        if self.owns_transition {
            self.transitioning.store(false, Ordering::Release);
        }
    }
}

pub struct ManagerContextCoordinator {
    client: Arc<ManagerClient>,
    settings: Arc<SettingsService>,
    snapshot: RwLock<ManagerContextSnapshot>,
    transaction_lock: Mutex<()>,
    transitioning: AtomicBool,
    event_sink: RwLock<Option<Arc<dyn ManagerContextEventSink>>>,
    lifecycle_sink: RwLock<Option<Arc<dyn ManagerContextLifecycleSink>>>,
    base_url_override: Option<String>,
}

impl ManagerContextCoordinator {
    pub fn new(client: Arc<ManagerClient>, settings: Arc<SettingsService>) -> Self {
        Self {
            client,
            settings,
            snapshot: RwLock::new(ManagerContextSnapshot::default()),
            transaction_lock: Mutex::new(()),
            transitioning: AtomicBool::new(false),
            event_sink: RwLock::new(None),
            lifecycle_sink: RwLock::new(None),
            base_url_override: None,
        }
    }

    /// Creates a coordinator against a scripted HTTP origin while retaining a real
    /// [`ManagerClient`] transport. Production must use [`Self::new`].
    #[doc(hidden)]
    pub fn new_with_base_url_override(
        client: Arc<ManagerClient>,
        settings: Arc<SettingsService>,
        base_url: String,
    ) -> Self {
        Self {
            client,
            settings,
            snapshot: RwLock::new(ManagerContextSnapshot::default()),
            transaction_lock: Mutex::new(()),
            transitioning: AtomicBool::new(false),
            event_sink: RwLock::new(None),
            lifecycle_sink: RwLock::new(None),
            base_url_override: Some(base_url),
        }
    }

    pub async fn set_event_sink(&self, sink: Arc<dyn ManagerContextEventSink>) {
        *self.event_sink.write().await = Some(sink);
    }

    pub async fn set_lifecycle_sink(&self, sink: Arc<dyn ManagerContextLifecycleSink>) {
        *self.lifecycle_sink.write().await = Some(sink);
    }

    pub async fn snapshot(&self) -> ManagerContextSnapshot {
        self.snapshot.read().await.clone()
    }

    pub async fn login(
        &self,
        environment: ManagerEnvironment,
        identifier: &str,
        password: &str,
    ) -> Result<LoginResult, ManagerError> {
        let _transaction = self.transaction_lock.lock().await;
        let (_transition, cleanup_errors) = self.begin_context_transition().await;
        self.log_cleanup_errors("login", &cleanup_errors);
        // Persist SignedOut before mutating keychain/session state. A crash or rollback failure
        // can then only lose auto-restore, never revive the previous account. Failed login
        // attempts restore the prior metadata because ManagerClient leaves that session intact.
        let previous_metadata = self
            .settings
            .load_global_manager_session()
            .map_err(manager_settings_error)?;
        self.clear_persisted_session()?;
        let result = self
            .client
            .login(Some(self.base_url(environment)), identifier, password)
            .await;
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                self.restore_persisted_session(&previous_metadata)?;
                return Err(error);
            }
        };

        match &result {
            LoginResult::Authenticated { user } => {
                let current = match self.get_current_user_in_identity_transaction().await {
                    Ok(current) => current,
                    Err(error) => {
                        self.abort_incomplete_authentication().await;
                        return Err(error);
                    }
                };
                if let Err(error) = validate_authenticated_identity(user, &current) {
                    self.abort_incomplete_authentication().await;
                    return Err(error);
                }
                if let Err(error) = self.persist_authenticated(environment, &current) {
                    self.abort_incomplete_authentication().await;
                    return Err(error);
                }
                self.transition_identity(authenticated_snapshot(environment, current))
                    .await;
            }
            LoginResult::AccountSelectionRequired { .. } => {
                if let Err(error) = self.clear_persisted_session() {
                    self.abort_incomplete_authentication().await;
                    return Err(error);
                }
                self.transition_identity(ManagerContextSnapshot {
                    auth_state: ManagerAuthState::AccountSelectionRequired,
                    environment: Some(environment),
                    ..ManagerContextSnapshot::default()
                })
                .await;
            }
            LoginResult::OnboardingRequired { user_id } => {
                if let Err(error) = self.clear_persisted_session() {
                    self.abort_incomplete_authentication().await;
                    return Err(error);
                }
                self.transition_identity(ManagerContextSnapshot {
                    auth_state: ManagerAuthState::OnboardingRequired,
                    environment: Some(environment),
                    user: Some(ManagerContextUser {
                        id: user_id.clone(),
                        nickname: String::new(),
                        email: String::new(),
                        phone: String::new(),
                    }),
                    ..ManagerContextSnapshot::default()
                })
                .await;
            }
        }
        Ok(result)
    }

    pub async fn select_account(&self, account_id: &str) -> Result<UserInfo, ManagerError> {
        let _transaction = self.transaction_lock.lock().await;
        let (_transition, cleanup_errors) = self.begin_context_transition().await;
        self.log_cleanup_errors("account selection", &cleanup_errors);
        let pending_generation = self.client.current_session_generation();
        let user = match self.client.select_account(account_id).await {
            Ok(user) => user,
            Err(ManagerError::Unauthorized) => {
                self.apply_auth_failure_for_generation(pending_generation)
                    .await;
                return Err(ManagerError::Unauthorized);
            }
            Err(error) => return Err(error),
        };
        let current = match self.get_current_user_in_identity_transaction().await {
            Ok(current) => current,
            Err(error) => {
                self.abort_incomplete_authentication().await;
                return Err(error);
            }
        };
        if let Err(error) = validate_authenticated_identity(&user, &current) {
            self.abort_incomplete_authentication().await;
            return Err(error);
        }
        if current.account_id != account_id {
            self.abort_incomplete_authentication().await;
            return Err(ManagerError::InvalidResponse(
                "selected account does not match /auth/me".to_string(),
            ));
        }
        let environment = self
            .snapshot()
            .await
            .environment
            .ok_or(ManagerError::MissingBaseUrl)?;
        if let Err(error) = self.persist_authenticated(environment, &current) {
            self.abort_incomplete_authentication().await;
            return Err(error);
        }
        self.transition_identity(authenticated_snapshot(environment, current))
            .await;
        Ok(user)
    }

    pub async fn restore_session(&self) -> Result<Option<RestoredManagerSession>, ManagerError> {
        let _transaction = self.transaction_lock.lock().await;
        let (_transition, cleanup_errors) = self.begin_context_transition().await;
        self.log_cleanup_errors("session restore", &cleanup_errors);
        let saved = self
            .settings
            .load_global_manager_session()
            .map_err(|error| ManagerError::Other {
                status: 0,
                body: format!("failed to load Manager session metadata: {error}"),
            })?;
        let Some(saved) = saved.session else {
            return Ok(None);
        };
        if !self
            .client
            .restore_session_from_base_url(saved.environment, self.base_url(saved.environment))
            .await?
        {
            self.clear_persisted_session()?;
            return Ok(None);
        }
        let current = match self.get_current_user_in_identity_transaction().await {
            Ok(current) => current,
            Err(error) => {
                if matches!(error, ManagerError::Unauthorized) {
                    self.abort_incomplete_authentication().await;
                } else {
                    self.client.suspend_restored_session().await;
                    self.transition(ManagerContextSnapshot::default()).await;
                }
                return Err(error);
            }
        };
        if saved.has_complete_identity()
            && (saved.account_id != current.account_id
                || saved.organization_id.as_deref() != Some(current.organization_id.as_str()))
        {
            log::warn!(
                "manager: persisted identity differs from live /auth/me; repairing metadata from server truth"
            );
        }
        if let Err(error) = self.persist_authenticated(saved.environment, &current) {
            self.abort_incomplete_authentication().await;
            return Err(error);
        }
        let restored = RestoredManagerSession {
            environment: saved.environment,
            user: UserInfo {
                user_id: current.id.clone(),
                account_id: current.account_id.clone(),
                account_name: current.account_name.clone(),
            },
        };
        self.transition_identity(authenticated_snapshot(saved.environment, current))
            .await;
        Ok(Some(restored))
    }

    pub async fn logout(&self) -> Result<(), ManagerError> {
        let _transaction = self.transaction_lock.lock().await;
        let (_transition, mut cleanup_errors) = self.begin_context_transition().await;
        // Local authority is cleared even if persistence, Keychain, or remote connection teardown
        // reports an error. A failed cleanup must never leave the old Context usable in memory.
        if let Err(error) = self.clear_persisted_session() {
            cleanup_errors.push(error.to_string());
        }
        if let Err(error) = self.client.logout().await {
            cleanup_errors.push(error.to_string());
        }
        self.transition(ManagerContextSnapshot::default()).await;
        if cleanup_errors.is_empty() {
            Ok(())
        } else {
            self.log_cleanup_errors("logout", &cleanup_errors);
            Err(ManagerError::Other {
                status: 0,
                body: "Manager logout completed locally with cleanup diagnostics".to_string(),
            })
        }
    }

    pub async fn list_accounts(&self) -> Result<Vec<ManagerAccountSummary>, ManagerError> {
        let generation = self.capture_authenticated_generation().await?;
        let outcome = self.client.list_accounts_outcome().await?;
        self.handle_authenticated_outcome_for_generation(outcome, Some(generation))
            .await
    }

    pub async fn switch_account(&self, account_id: &str) -> Result<(), ManagerError> {
        let _transaction = self.transaction_lock.lock().await;
        let departing_generation = self.ensure_authenticated_generation_locked(None).await?;
        let environment = self
            .snapshot
            .read()
            .await
            .environment
            .ok_or(ManagerError::MissingBaseUrl)?;
        let (_transition, cleanup_errors) = self.begin_context_transition().await;
        self.log_cleanup_errors("account switch", &cleanup_errors);

        let switched = match self.client.switch_account(account_id).await {
            Ok(switched) => switched,
            Err(ManagerError::Unauthorized) => {
                self.apply_auth_failure_for_generation(departing_generation)
                    .await;
                return Err(ManagerError::Unauthorized);
            }
            Err(error) => return Err(error),
        };
        let current = match self.get_current_user_in_identity_transaction().await {
            Ok(current) => current,
            Err(ManagerError::Unauthorized) => return Err(ManagerError::Unauthorized),
            Err(error) => {
                self.abort_incomplete_authentication().await;
                return Err(error);
            }
        };
        if let Err(error) = validate_switched_identity(&switched, &current) {
            self.abort_incomplete_authentication().await;
            return Err(error);
        }
        if let Err(error) = self.persist_authenticated(environment, &current) {
            self.abort_incomplete_authentication().await;
            return Err(error);
        }
        self.transition_identity(authenticated_snapshot(environment, current))
            .await;
        Ok(())
    }

    pub async fn list_digital_employees(&self) -> Result<Vec<DigitalEmployeeBrief>, ManagerError> {
        let generation = self.capture_authenticated_generation().await?;
        let outcome = self.client.list_digital_employees_outcome().await?;
        self.handle_authenticated_outcome_for_generation(outcome, Some(generation))
            .await
    }

    pub async fn capture_authenticated_generation(&self) -> Result<u64, ManagerError> {
        let _transaction = self.transaction_lock.lock().await;
        self.ensure_authenticated_generation_locked(None).await
    }

    pub async fn ensure_authenticated_generation(
        &self,
        expected_generation: u64,
    ) -> Result<(), ManagerError> {
        let _transaction = self.transaction_lock.lock().await;
        self.ensure_authenticated_generation_locked(Some(expected_generation))
            .await
            .map(|_| ())
    }

    pub async fn context_key_for_generation(
        &self,
        expected_generation: u64,
    ) -> Result<ManagerContextKey, ManagerError> {
        let _transaction = self.transaction_lock.lock().await;
        self.ensure_authenticated_generation_locked(Some(expected_generation))
            .await?;
        self.snapshot
            .read()
            .await
            .context_key
            .clone()
            .ok_or(ManagerError::NoSession)
    }

    /// Runs an irreversible local side effect while identity transitions are excluded.
    ///
    /// Network discovery and token exchange should happen before this boundary. Operations such
    /// as installing an SMCP runtime, persisting a Robot binding, or swapping a refreshed socket
    /// must use it so an account switch/logout cannot occur between generation validation and the
    /// actual commit.
    pub async fn commit_for_authenticated_generation<T, F, Fut>(
        &self,
        expected_generation: u64,
        operation: F,
    ) -> Result<T, ManagerError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, ManagerError>>,
    {
        let _transaction = self.transaction_lock.lock().await;
        self.ensure_authenticated_generation_locked(Some(expected_generation))
            .await?;
        let value = operation().await?;
        self.ensure_authenticated_generation_locked(Some(expected_generation))
            .await?;
        Ok(value)
    }

    pub async fn list_digital_employees_for_generation(
        &self,
        expected_generation: u64,
    ) -> Result<Vec<DigitalEmployeeBrief>, ManagerError> {
        self.ensure_authenticated_generation(expected_generation)
            .await?;
        let outcome = self.client.list_digital_employees_outcome().await?;
        self.handle_authenticated_outcome_for_generation(outcome, Some(expected_generation))
            .await
    }

    pub async fn get_connection_info(
        &self,
        employee_id: u64,
    ) -> Result<ConnectionInfoResponse, ManagerError> {
        let generation = self.capture_authenticated_generation().await?;
        let outcome = self.client.get_connection_info_outcome(employee_id).await?;
        self.handle_authenticated_outcome_for_generation(outcome, Some(generation))
            .await
    }

    pub async fn get_connection_info_for_generation(
        &self,
        expected_generation: u64,
        employee_id: u64,
    ) -> Result<ConnectionInfoResponse, ManagerError> {
        self.ensure_authenticated_generation(expected_generation)
            .await?;
        let outcome = self.client.get_connection_info_outcome(employee_id).await?;
        self.handle_authenticated_outcome_for_generation(outcome, Some(expected_generation))
            .await
    }

    pub async fn exchange_token(
        &self,
        robot_account_id: &str,
        scope: Option<String>,
    ) -> Result<ExchangedToken, ManagerError> {
        let generation = self.capture_authenticated_generation().await?;
        let outcome = self
            .client
            .exchange_token_outcome(robot_account_id, scope)
            .await?;
        self.handle_authenticated_outcome_for_generation(outcome, Some(generation))
            .await
    }

    pub async fn exchange_token_for_generation(
        &self,
        expected_generation: u64,
        robot_account_id: &str,
        scope: Option<String>,
    ) -> Result<ExchangedToken, ManagerError> {
        self.ensure_authenticated_generation(expected_generation)
            .await?;
        let outcome = self
            .client
            .exchange_token_outcome(robot_account_id, scope)
            .await?;
        self.handle_authenticated_outcome_for_generation(outcome, Some(expected_generation))
            .await
    }

    async fn get_current_user_in_identity_transaction(
        &self,
    ) -> Result<ManagerCurrentUser, ManagerError> {
        let outcome = self.client.get_current_user_outcome().await?;
        self.settle_authenticated_outcome(outcome).await
    }

    async fn handle_authenticated_outcome_for_generation<T>(
        &self,
        outcome: ManagerRequestOutcome<T>,
        expected_generation: Option<u64>,
    ) -> Result<T, ManagerError> {
        // Request start is linearized by capture/ensure under this same lock. The network work may
        // overlap a Context transition, but its result can never settle after that generation is
        // replaced. New request starts see `transitioning` and fail closed.
        let _transaction = self.transaction_lock.lock().await;
        if expected_generation.is_some_and(|expected| outcome.generation != expected) {
            return Err(ManagerError::ContextChanged);
        }
        self.settle_authenticated_outcome(outcome).await
    }

    async fn ensure_authenticated_generation_locked(
        &self,
        expected_generation: Option<u64>,
    ) -> Result<u64, ManagerError> {
        if self.transitioning.load(Ordering::Acquire) {
            return Err(ManagerError::ContextChanged);
        }
        let generation = self.client.current_session_generation();
        let snapshot = self.snapshot.read().await;
        if snapshot.auth_state != ManagerAuthState::Authenticated
            || !self.client.has_session().await
        {
            return Err(ManagerError::NoSession);
        }
        if expected_generation.is_some_and(|expected| expected != generation) {
            return Err(ManagerError::ContextChanged);
        }
        Ok(generation)
    }

    async fn settle_authenticated_outcome<T>(
        &self,
        outcome: ManagerRequestOutcome<T>,
    ) -> Result<T, ManagerError> {
        let current_generation = self.client.current_session_generation();
        if outcome.generation != current_generation {
            return Err(ManagerError::ContextChanged);
        }

        if matches!(&outcome.result, Err(ManagerError::Unauthorized)) {
            self.apply_auth_failure_for_generation(outcome.generation)
                .await;
            return outcome.result;
        }

        if !self.client.has_session().await {
            return Err(ManagerError::ContextChanged);
        }

        outcome.result
    }

    async fn apply_auth_failure_for_generation(&self, failed_generation: u64) {
        if failed_generation == 0 || failed_generation != self.client.current_session_generation() {
            return;
        }
        let (_transition, cleanup_errors) = self.begin_context_transition().await;
        self.log_cleanup_errors("authentication expiry", &cleanup_errors);
        // The coordinator transaction lock is held by every caller. Persist the SignedOut
        // tombstone before clearing the session/keychain, then publish the matching snapshot and
        // events in the same serialized transition. If the response future is cancelled before
        // settlement, no partial mutation has happened and a later request can retry the 401.
        if let Err(error) = self.clear_persisted_session() {
            log::warn!("manager: failed to clear expired session metadata: {error}");
        }
        if self
            .client
            .clear_session_for_generation(failed_generation)
            .await
        {
            self.transition(ManagerContextSnapshot::default()).await;
            self.publish_auth_expired().await;
        }
    }

    async fn begin_context_transition(&self) -> (ManagerContextTransitionGuard<'_>, Vec<String>) {
        let owns_transition = self
            .transitioning
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok();
        let guard = ManagerContextTransitionGuard {
            transitioning: &self.transitioning,
            owns_transition,
        };
        if !owns_transition {
            return (guard, Vec::new());
        }
        let departing_context = self.snapshot.read().await.context_key.clone();
        let sink = self.lifecycle_sink.read().await.clone();
        let errors = match sink {
            Some(sink) => {
                sink.cleanup_manager_context(departing_context.as_ref())
                    .await
            }
            None => Vec::new(),
        };
        (guard, errors)
    }

    fn log_cleanup_errors(&self, operation: &str, errors: &[String]) {
        for error in errors {
            log::warn!("manager: {operation} cleanup diagnostic: {error}");
        }
    }

    fn base_url(&self, environment: ManagerEnvironment) -> String {
        self.base_url_override
            .clone()
            .unwrap_or_else(|| environment.base_url().to_string())
    }

    async fn abort_incomplete_authentication(&self) {
        if let Err(error) = self.client.logout().await {
            log::warn!("manager: failed to roll back incomplete authentication: {error}");
        }
        if let Err(error) = self.clear_persisted_session() {
            log::warn!("manager: failed to clear incomplete session metadata: {error}");
        }
        self.transition(ManagerContextSnapshot::default()).await;
    }

    fn persist_authenticated(
        &self,
        environment: ManagerEnvironment,
        current: &ManagerCurrentUser,
    ) -> Result<(), ManagerError> {
        let config = ManagerSessionConfig {
            schema_version: MANAGER_SESSION_SCHEMA_VERSION,
            session: Some(PersistedManagerSession {
                environment,
                user_id: current.id.clone(),
                user_nickname: Some(current.nickname.clone()),
                user_email: Some(current.email.clone()),
                user_phone: Some(current.phone.clone()),
                account_id: current.account_id.clone(),
                account_name: current.account_name.clone(),
                account_nickname: Some(current.nickname.clone()),
                account_avatar: Some(current.account_avatar.clone()),
                employee_no: Some(current.employee_no.clone()),
                organization_id: Some(current.organization_id.clone()),
                organization_name: Some(current.organization_name.clone()),
                organization_type: Some(current.organization_type.clone()),
                permissions: Some(current.permissions.clone()),
            }),
        };
        self.settings
            .save_global_manager_session(&config)
            .map_err(manager_settings_error)
    }

    fn clear_persisted_session(&self) -> Result<(), ManagerError> {
        self.settings
            .save_global_manager_session(&ManagerSessionConfig::default())
            .map_err(manager_settings_error)
    }

    fn restore_persisted_session(&self, config: &ManagerSessionConfig) -> Result<(), ManagerError> {
        self.settings
            .save_global_manager_session(config)
            .map_err(manager_settings_error)
    }

    async fn transition(&self, mut next: ManagerContextSnapshot) {
        self.transition_with_revision_policy(&mut next, false).await;
    }

    /// Commits a successful identity transaction as a public revision boundary even when the
    /// redacted identity payload is unchanged. This keeps frontend resource scopes aligned with
    /// the Manager client's new authenticated session generation after an explicit re-login,
    /// account selection, switch, or restore.
    async fn transition_identity(&self, mut next: ManagerContextSnapshot) {
        self.transition_with_revision_policy(&mut next, true).await;
    }

    async fn transition_with_revision_policy(
        &self,
        next: &mut ManagerContextSnapshot,
        force_revision: bool,
    ) {
        let snapshot = {
            let mut current = self.snapshot.write().await;
            if !force_revision && same_payload(&current, next) {
                return;
            }
            next.revision = current
                .revision
                .checked_add(1)
                .expect("Manager Context revision overflow");
            *current = next.clone();
            next.clone()
        };
        let sink = self.event_sink.read().await.clone();
        if let Some(sink) = sink {
            if let Err(error) = sink.emit_context_changed(&snapshot) {
                log::warn!("manager: failed to emit context change: {error}");
            }
        }
    }

    async fn publish_auth_expired(&self) {
        let sink = self.event_sink.read().await.clone();
        if let Some(sink) = sink {
            if let Err(error) = sink.emit_auth_expired() {
                log::warn!("manager: failed to emit auth expiry: {error}");
            }
        }
    }
}

fn manager_settings_error(error: ManagerSessionConfigError) -> ManagerError {
    ManagerError::Other {
        status: 0,
        body: format!("Manager session metadata error: {error}"),
    }
}

fn authenticated_snapshot(
    environment: ManagerEnvironment,
    current: ManagerCurrentUser,
) -> ManagerContextSnapshot {
    let context_key = ManagerContextKey {
        environment,
        account_id: current.account_id.clone(),
        organization_id: current.organization_id.clone(),
    };
    ManagerContextSnapshot {
        revision: 0,
        auth_state: ManagerAuthState::Authenticated,
        environment: Some(environment),
        context_key: Some(context_key),
        user: Some(ManagerContextUser {
            id: current.id,
            nickname: current.nickname.clone(),
            email: current.email,
            phone: current.phone,
        }),
        account: Some(ManagerContextAccount {
            id: current.account_id,
            name: current.account_name,
            nickname: current.nickname,
            avatar: current.account_avatar,
            employee_no: current.employee_no,
        }),
        organization: Some(ManagerContextOrganization {
            id: current.organization_id,
            name: current.organization_name,
            organization_type: current.organization_type,
        }),
        permissions: current.permissions,
    }
}

fn validate_authenticated_identity(
    login_user: &UserInfo,
    current: &ManagerCurrentUser,
) -> Result<(), ManagerError> {
    if login_user.user_id != current.id || login_user.account_id != current.account_id {
        return Err(ManagerError::InvalidResponse(
            "login identity does not match /auth/me".to_string(),
        ));
    }
    if current.id.trim().is_empty()
        || current.account_id.trim().is_empty()
        || current.account_name.trim().is_empty()
        || current.organization_id.trim().is_empty()
        || current.organization_name.trim().is_empty()
        || current.organization_type.trim().is_empty()
    {
        return Err(ManagerError::InvalidResponse(
            "/auth/me missing complete user, account, or organization scope".to_string(),
        ));
    }
    Ok(())
}

fn validate_switched_identity(
    switched: &SwitchedManagerAccount,
    current: &ManagerCurrentUser,
) -> Result<(), ManagerError> {
    let switched_user = UserInfo {
        user_id: switched.user_id.clone(),
        account_id: switched.account_id.clone(),
        account_name: switched.account_name.clone(),
    };
    validate_authenticated_identity(&switched_user, current)?;
    if switched.account_name != current.account_name
        || switched.organization_id != current.organization_id
        || switched.organization_name != current.organization_name
        || (!switched.organization_type.is_empty()
            && switched.organization_type != current.organization_type)
    {
        return Err(ManagerError::InvalidResponse(
            "switch-account identity does not match /auth/me".to_string(),
        ));
    }
    Ok(())
}

fn same_payload(current: &ManagerContextSnapshot, next: &ManagerContextSnapshot) -> bool {
    current.auth_state == next.auth_state
        && current.environment == next.environment
        && current.context_key == next.context_key
        && current.user == next.user
        && current.account == next.account
        && current.organization == next.organization
        && current.permissions == next.permissions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::keychain::InMemorySecretStore;
    use tempfile::tempdir;

    fn coordinator() -> (ManagerContextCoordinator, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let settings = Arc::new(SettingsService::new(dir.path().to_path_buf()));
        let client = Arc::new(ManagerClient::new_with_secret_store(
            InMemorySecretStore::shared(),
        ));
        (ManagerContextCoordinator::new(client, settings), dir)
    }

    #[tokio::test]
    async fn transitions_are_monotonic_and_noops_do_not_advance_revision() {
        let (coordinator, _dir) = coordinator();
        assert_eq!(coordinator.snapshot().await.revision, 0);

        let selecting = ManagerContextSnapshot {
            auth_state: ManagerAuthState::AccountSelectionRequired,
            environment: Some(ManagerEnvironment::Staging),
            ..ManagerContextSnapshot::default()
        };
        coordinator.transition(selecting.clone()).await;
        assert_eq!(coordinator.snapshot().await.revision, 1);
        coordinator.transition(selecting).await;
        assert_eq!(coordinator.snapshot().await.revision, 1);
        coordinator
            .transition_identity(ManagerContextSnapshot {
                auth_state: ManagerAuthState::AccountSelectionRequired,
                environment: Some(ManagerEnvironment::Staging),
                ..ManagerContextSnapshot::default()
            })
            .await;
        assert_eq!(coordinator.snapshot().await.revision, 2);
        coordinator
            .transition(ManagerContextSnapshot::default())
            .await;
        assert_eq!(coordinator.snapshot().await.revision, 3);
    }

    #[test]
    fn context_snapshot_serialization_contains_no_credential_fields() {
        let snapshot = authenticated_snapshot(
            ManagerEnvironment::Staging,
            ManagerCurrentUser {
                id: "7".to_string(),
                nickname: "user".to_string(),
                email: "user@example.com".to_string(),
                phone: String::new(),
                account_avatar: String::new(),
                account_id: "org-a:account-7".to_string(),
                account_name: "account".to_string(),
                employee_no: "000007".to_string(),
                organization_id: "org-a".to_string(),
                organization_name: "Organization A".to_string(),
                organization_type: "enterprise".to_string(),
                permissions: vec!["robot:read".to_string()],
            },
        );
        let value = serde_json::to_value(snapshot).unwrap();
        let serialized = value.to_string().to_ascii_lowercase();
        for forbidden in ["jwt", "password", "temptoken", "access_token"] {
            assert!(!serialized.contains(forbidden), "found {forbidden}");
        }
    }

    #[test]
    fn authenticated_identity_rejects_empty_scoped_ids() {
        let login_user = UserInfo {
            user_id: "7".to_string(),
            account_id: String::new(),
            account_name: "account".to_string(),
        };
        let current = ManagerCurrentUser {
            id: "7".to_string(),
            nickname: String::new(),
            email: String::new(),
            phone: String::new(),
            account_avatar: String::new(),
            account_id: String::new(),
            account_name: "account".to_string(),
            employee_no: String::new(),
            organization_id: "org-a".to_string(),
            organization_name: "Organization A".to_string(),
            organization_type: "enterprise".to_string(),
            permissions: Vec::new(),
        };

        assert!(matches!(
            validate_authenticated_identity(&login_user, &current),
            Err(ManagerError::InvalidResponse(_))
        ));
    }
}
