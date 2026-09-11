//! Generation-bound bridge between Rust Manager lifecycles and the TypeScript
//! `@turingfocus/tfrs-auth`
//! token source.
//!
//! Rust remains authoritative for the Manager session and every consumer lifecycle. The bridge
//! only delegates RFC 8693 construction, parsing, caching, and retry policy to the webview. It
//! never logs request payloads because they contain the User JWT.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StateMutex};
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::{oneshot, RwLock};
use uuid::Uuid;

use crate::services::manager_client::{ExchangedToken, ManagerError};

pub const MANAGER_TOKEN_REQUEST_EVENT: &str = "manager:token-request";
const TOKEN_BRIDGE_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_TRANSPORT_ATTEMPTS: u8 = 3;
const TOKEN_EXCHANGE_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:token-exchange";
const JWT_TOKEN_TYPE: &str = "urn:ietf:params:oauth:token-type:jwt";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagerTokenProfile {
    Session,
}

impl ManagerTokenProfile {
    fn as_str(self) -> &'static str {
        match self {
            Self::Session => "session",
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagerTokenBridgeRequest {
    pub request_id: String,
    pub generation: u64,
    pub token_url: String,
    pub user_jwt: String,
    pub audience: String,
    pub scope: Option<String>,
    pub token_profile: ManagerTokenProfile,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagerTokenHttpResponse {
    pub status: u16,
    pub body: String,
    pub content_type: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ManagerTokenBridgeCompletion {
    Success {
        access_token: String,
        token_type: String,
        expires_in: i64,
        scope: Option<String>,
    },
    Error {
        error: ManagerTokenBridgeFailure,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagerTokenBridgeFailure {
    pub kind: String,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub http_status: Option<u16>,
    #[serde(default)]
    pub redirect_url: Option<String>,
}

#[async_trait]
pub trait ManagerTokenBridgeSink: Send + Sync {
    async fn emit_token_request(&self, request: &ManagerTokenBridgeRequest) -> Result<(), String>;
}

struct PendingRequest {
    generation: u64,
    user_jwt: String,
    audience: String,
    scope: Option<String>,
    token_profile: ManagerTokenProfile,
    attempt_count: u8,
    active_attempt: Option<String>,
    completion: oneshot::Sender<ManagerTokenBridgeCompletion>,
}

#[derive(Default)]
struct BridgeState {
    ready_lease: Option<String>,
    pending: HashMap<String, PendingRequest>,
}

// State mutations never await. A synchronous guard lets cancellation revoke pending
// transport authority immediately, including while event delivery is still awaiting.
struct PendingExchange<'a> {
    state: &'a StateMutex<BridgeState>,
    request_id: &'a str,
}

impl Drop for PendingExchange<'_> {
    fn drop(&mut self) {
        self.state.lock().unwrap().pending.remove(self.request_id);
    }
}

pub struct ManagerTokenBridge {
    sink: RwLock<Option<Arc<dyn ManagerTokenBridgeSink>>>,
    state: StateMutex<BridgeState>,
    timeout: Duration,
}

impl ManagerTokenBridge {
    pub fn new() -> Self {
        Self {
            sink: RwLock::new(None),
            state: StateMutex::new(BridgeState::default()),
            timeout: TOKEN_BRIDGE_TIMEOUT,
        }
    }

    #[cfg(test)]
    fn with_timeout(timeout: Duration) -> Self {
        Self {
            timeout,
            ..Self::new()
        }
    }

    pub async fn set_sink(&self, sink: Arc<dyn ManagerTokenBridgeSink>) {
        *self.sink.write().await = Some(sink);
    }

    pub async fn set_ready(&self, lease_id: &str, ready: bool) {
        let mut state = self.state.lock().unwrap();
        if ready {
            if state.ready_lease.as_deref() != Some(lease_id) {
                state.pending.clear();
                state.ready_lease = Some(lease_id.to_string());
            }
        } else if state.ready_lease.as_deref() == Some(lease_id) {
            state.ready_lease = None;
            state.pending.clear();
        }
    }

    pub async fn force_not_ready(&self) {
        let mut state = self.state.lock().unwrap();
        state.ready_lease = None;
        state.pending.clear();
    }

    pub async fn exchange(
        &self,
        generation: u64,
        token_url: String,
        user_jwt: String,
        audience: String,
        scope: Option<String>,
        token_profile: ManagerTokenProfile,
    ) -> Result<ExchangedToken, ManagerError> {
        let sink = self.sink.read().await.clone().ok_or_else(|| {
            ManagerError::InvalidResponse("Manager token bridge is unavailable".to_string())
        })?;
        let request_id = Uuid::new_v4().to_string();
        let request = ManagerTokenBridgeRequest {
            request_id: request_id.clone(),
            generation,
            token_url: token_url.clone(),
            user_jwt,
            audience,
            scope,
            token_profile,
        };
        let (sender, receiver) = oneshot::channel();
        {
            let mut state = self.state.lock().unwrap();
            if state.ready_lease.is_none() {
                return Err(ManagerError::InvalidResponse(
                    "Manager token bridge is not ready".to_string(),
                ));
            }
            state.pending.insert(
                request_id.clone(),
                PendingRequest {
                    generation,
                    user_jwt: request.user_jwt.clone(),
                    audience: request.audience.clone(),
                    scope: request.scope.clone(),
                    token_profile: request.token_profile,
                    attempt_count: 0,
                    active_attempt: None,
                    completion: sender,
                },
            );
        }

        let _pending = PendingExchange {
            state: &self.state,
            request_id: &request_id,
        };
        if let Err(error) = sink.emit_token_request(&request).await {
            return Err(ManagerError::InvalidResponse(format!(
                "Manager token bridge request could not be delivered: {error}"
            )));
        }

        match tokio::time::timeout(self.timeout, receiver).await {
            Ok(Ok(completion)) => completion.into_result(),
            Ok(Err(_)) => Err(ManagerError::ContextChanged),
            Err(_) => Err(ManagerError::NetworkError(
                "Manager token bridge timed out".to_string(),
            )),
        }
    }

    pub async fn begin_transport(
        &self,
        request_id: &str,
        generation: u64,
        body: &str,
    ) -> Result<String, ManagerError> {
        let mut state = self.state.lock().unwrap();
        if state.ready_lease.is_none() {
            return Err(ManagerError::ContextChanged);
        }
        let request = state
            .pending
            .get_mut(request_id)
            .ok_or(ManagerError::ContextChanged)?;
        if request.generation != generation
            || request.active_attempt.is_some()
            || request.attempt_count >= MAX_TRANSPORT_ATTEMPTS
        {
            return Err(ManagerError::ContextChanged);
        }
        validate_token_exchange_form(request, body)?;
        let attempt_id = Uuid::new_v4().to_string();
        request.attempt_count += 1;
        request.active_attempt = Some(attempt_id.clone());
        Ok(attempt_id)
    }

    pub async fn finish_transport(
        &self,
        request_id: &str,
        generation: u64,
        attempt_id: &str,
    ) -> Result<(), ManagerError> {
        let mut state = self.state.lock().unwrap();
        let request = state
            .pending
            .get_mut(request_id)
            .ok_or(ManagerError::ContextChanged)?;
        if request.generation != generation || request.active_attempt.as_deref() != Some(attempt_id)
        {
            return Err(ManagerError::ContextChanged);
        }
        request.active_attempt = None;
        Ok(())
    }

    pub async fn complete(
        &self,
        request_id: &str,
        generation: u64,
        mut completion: ManagerTokenBridgeCompletion,
    ) -> Result<(), ManagerError> {
        let pending = {
            let mut state = self.state.lock().unwrap();
            let request = state
                .pending
                .get(request_id)
                .ok_or(ManagerError::ContextChanged)?;
            if request.generation != generation || request.active_attempt.is_some() {
                return Err(ManagerError::ContextChanged);
            }
            completion.redact_secret(&request.user_jwt);
            state
                .pending
                .remove(request_id)
                .expect("pending request disappeared")
        };
        pending
            .completion
            .send(completion)
            .map_err(|_| ManagerError::ContextChanged)
    }
}

fn validate_token_exchange_form(request: &PendingRequest, body: &str) -> Result<(), ManagerError> {
    let mut fields = HashMap::new();
    for (key, value) in url::form_urlencoded::parse(body.as_bytes()) {
        if fields
            .insert(key.into_owned(), value.into_owned())
            .is_some()
        {
            return Err(ManagerError::InvalidResponse(
                "Manager token bridge rejected duplicate form fields".to_string(),
            ));
        }
    }
    let expected_field_count = if request.scope.is_some() { 6 } else { 5 };
    let valid = fields.len() == expected_field_count
        && fields.get("grant_type").map(String::as_str) == Some(TOKEN_EXCHANGE_GRANT_TYPE)
        && fields.get("subject_token").map(String::as_str) == Some(request.user_jwt.as_str())
        && fields.get("subject_token_type").map(String::as_str) == Some(JWT_TOKEN_TYPE)
        && fields.get("audience").map(String::as_str) == Some(request.audience.as_str())
        && fields.get("scope").map(String::as_str) == request.scope.as_deref()
        && fields.get("token_profile").map(String::as_str) == Some(request.token_profile.as_str());
    if !valid {
        return Err(ManagerError::InvalidResponse(
            "Manager token bridge rejected unexpected token exchange fields".to_string(),
        ));
    }
    Ok(())
}

impl Default for ManagerTokenBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl ManagerTokenBridgeCompletion {
    fn redact_secret(&mut self, secret: &str) {
        if let Self::Error { error } = self {
            error.redact_secret(secret);
        }
    }

    fn into_result(self) -> Result<ExchangedToken, ManagerError> {
        match self {
            Self::Success {
                access_token,
                token_type,
                expires_in,
                scope,
            } => {
                if access_token.trim().is_empty() || token_type.trim().is_empty() || expires_in < 0
                {
                    return Err(ManagerError::InvalidResponse(
                        "TypeScript token bridge returned invalid token metadata".to_string(),
                    ));
                }
                Ok(ExchangedToken {
                    access_token,
                    token_type,
                    expires_in,
                    scope: scope.filter(|value| !value.is_empty()),
                })
            }
            Self::Error { error } => Err(error.into_manager_error()),
        }
    }
}

impl ManagerTokenBridgeFailure {
    fn redact_secret(&mut self, secret: &str) {
        if secret.is_empty() {
            return;
        }
        for value in [
            &mut self.code,
            &mut self.description,
            &mut self.redirect_url,
        ]
        .into_iter()
        .flatten()
        {
            *value = value.replace(secret, "[REDACTED]");
        }
    }

    fn into_manager_error(self) -> ManagerError {
        match self.kind.as_str() {
            "unauthorized" => ManagerError::Unauthorized,
            "forbidden" => ManagerError::Forbidden,
            "payment_required" => ManagerError::PaymentRequired {
                message: self
                    .description
                    .unwrap_or_else(|| "Payment required".to_string()),
                redirect_url: self.redirect_url,
            },
            "not_found" => ManagerError::NotFound,
            "not_found_or_no_permission" => ManagerError::NotFoundOrNoPermission,
            "signing_unavailable" => ManagerError::SigningUnavailable {
                message: self.description,
            },
            "network_error" => ManagerError::NetworkError(
                self.description
                    .unwrap_or_else(|| "Token endpoint request failed".to_string()),
            ),
            "context_changed" => ManagerError::ContextChanged,
            "no_session" => ManagerError::NoSession,
            "invalid_response" => ManagerError::InvalidResponse(
                self.description
                    .unwrap_or_else(|| "Invalid token bridge response".to_string()),
            ),
            "token_exchange" => ManagerError::TokenExchange {
                error: self
                    .code
                    .unwrap_or_else(|| "token_exchange_error".to_string()),
                description: self.description,
            },
            "rate_limited" | "other" => ManagerError::Other {
                status: self.http_status.unwrap_or(500),
                body: "Unexpected Manager response".to_string(),
            },
            _ => ManagerError::InvalidResponse("Unknown token bridge error kind".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::Mutex;

    fn valid_form(user_jwt: &str, audience: &str, scope: Option<&str>) -> String {
        let mut form = vec![
            ("grant_type", TOKEN_EXCHANGE_GRANT_TYPE),
            ("subject_token", user_jwt),
            ("subject_token_type", JWT_TOKEN_TYPE),
            ("audience", audience),
            ("token_profile", ManagerTokenProfile::Session.as_str()),
        ];
        if let Some(scope) = scope {
            form.push(("scope", scope));
        }
        url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(form)
            .finish()
    }

    struct RecordingSink {
        sender: Mutex<Option<oneshot::Sender<ManagerTokenBridgeRequest>>>,
    }

    #[async_trait]
    impl ManagerTokenBridgeSink for RecordingSink {
        async fn emit_token_request(
            &self,
            request: &ManagerTokenBridgeRequest,
        ) -> Result<(), String> {
            self.sender
                .lock()
                .await
                .take()
                .expect("test request sender")
                .send(request.clone())
                .map_err(|_| "request receiver dropped".to_string())
        }
    }

    #[tokio::test]
    async fn completion_is_bound_to_request_and_generation() {
        let bridge = Arc::new(ManagerTokenBridge::new());
        let (sender, receiver) = oneshot::channel();
        bridge
            .set_sink(Arc::new(RecordingSink {
                sender: Mutex::new(Some(sender)),
            }))
            .await;
        bridge.set_ready("test-lease", true).await;
        let task_bridge = bridge.clone();
        let exchange = tokio::spawn(async move {
            task_bridge
                .exchange(
                    7,
                    "https://manager.example/api/v1/oauth/token".to_string(),
                    "user-jwt".to_string(),
                    "robot:r1".to_string(),
                    None,
                    ManagerTokenProfile::Session,
                )
                .await
        });
        let request = receiver.await.unwrap();

        assert!(matches!(
            bridge
                .complete(
                    &request.request_id,
                    8,
                    ManagerTokenBridgeCompletion::Success {
                        access_token: "short".to_string(),
                        token_type: "Bearer".to_string(),
                        expires_in: 300,
                        scope: None,
                    },
                )
                .await,
            Err(ManagerError::ContextChanged)
        ));
        bridge
            .complete(
                &request.request_id,
                7,
                ManagerTokenBridgeCompletion::Success {
                    access_token: "short".to_string(),
                    token_type: "Bearer".to_string(),
                    expires_in: 300,
                    scope: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(exchange.await.unwrap().unwrap().access_token, "short");
    }

    #[tokio::test]
    async fn cancelled_exchange_revokes_pending_transport_authority() {
        let bridge = Arc::new(ManagerTokenBridge::new());
        let (sender, receiver) = oneshot::channel();
        bridge
            .set_sink(Arc::new(RecordingSink {
                sender: Mutex::new(Some(sender)),
            }))
            .await;
        bridge.set_ready("test-lease", true).await;
        let task_bridge = bridge.clone();
        let exchange = tokio::spawn(async move {
            task_bridge
                .exchange(
                    3,
                    "https://manager.example/api/v1/oauth/token".to_string(),
                    "user-jwt".to_string(),
                    "robot:r1".to_string(),
                    Some("config:write".to_string()),
                    ManagerTokenProfile::Session,
                )
                .await
        });
        let request = receiver.await.unwrap();
        exchange.abort();
        assert!(exchange.await.unwrap_err().is_cancelled());
        assert!(bridge.state.lock().unwrap().pending.is_empty());
        assert!(matches!(
            bridge
                .begin_transport(
                    &request.request_id,
                    3,
                    &valid_form("user-jwt", "robot:r1", Some("config:write")),
                )
                .await,
            Err(ManagerError::ContextChanged)
        ));
    }

    #[tokio::test]
    async fn timeout_removes_transport_authority() {
        let bridge = Arc::new(ManagerTokenBridge::with_timeout(Duration::from_millis(5)));
        let (sender, receiver) = oneshot::channel();
        bridge
            .set_sink(Arc::new(RecordingSink {
                sender: Mutex::new(Some(sender)),
            }))
            .await;
        bridge.set_ready("test-lease", true).await;
        let task_bridge = bridge.clone();
        let exchange = tokio::spawn(async move {
            task_bridge
                .exchange(
                    3,
                    "https://manager.example/api/v1/oauth/token".to_string(),
                    "user-jwt".to_string(),
                    "robot:r1".to_string(),
                    None,
                    ManagerTokenProfile::Session,
                )
                .await
        });
        let request = receiver.await.unwrap();
        assert!(matches!(
            exchange.await.unwrap(),
            Err(ManagerError::NetworkError(_))
        ));
        assert!(matches!(
            bridge
                .begin_transport(
                    &request.request_id,
                    3,
                    &valid_form("user-jwt", "robot:r1", None),
                )
                .await,
            Err(ManagerError::ContextChanged)
        ));
    }

    #[tokio::test]
    async fn transport_is_field_bound_serial_and_limited_to_three_attempts() {
        let bridge = Arc::new(ManagerTokenBridge::new());
        let (sender, receiver) = oneshot::channel();
        bridge
            .set_sink(Arc::new(RecordingSink {
                sender: Mutex::new(Some(sender)),
            }))
            .await;
        bridge.set_ready("test-lease", true).await;
        let task_bridge = bridge.clone();
        let exchange = tokio::spawn(async move {
            task_bridge
                .exchange(
                    9,
                    "https://manager.example/api/v1/oauth/token".to_string(),
                    "secret-user-jwt".to_string(),
                    "robot:r9".to_string(),
                    Some("smcp:connect".to_string()),
                    ManagerTokenProfile::Session,
                )
                .await
        });
        let request = receiver.await.unwrap();

        assert!(matches!(
            bridge
                .begin_transport(
                    &request.request_id,
                    9,
                    &valid_form("different-jwt", "robot:r9", Some("smcp:connect")),
                )
                .await,
            Err(ManagerError::InvalidResponse(_))
        ));

        let legacy_form = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs([
                ("grant_type", TOKEN_EXCHANGE_GRANT_TYPE),
                ("subject_token", "secret-user-jwt"),
                ("subject_token_type", JWT_TOKEN_TYPE),
                ("audience", "robot:r9"),
                ("scope", "smcp:connect"),
            ])
            .finish();
        assert!(matches!(
            bridge
                .begin_transport(&request.request_id, 9, &legacy_form)
                .await,
            Err(ManagerError::InvalidResponse(_))
        ));

        let body = valid_form("secret-user-jwt", "robot:r9", Some("smcp:connect"));
        for attempt in 0..MAX_TRANSPORT_ATTEMPTS {
            let attempt_id = bridge
                .begin_transport(&request.request_id, 9, &body)
                .await
                .unwrap();
            assert!(matches!(
                bridge.begin_transport(&request.request_id, 9, &body).await,
                Err(ManagerError::ContextChanged)
            ));
            assert!(matches!(
                bridge
                    .complete(
                        &request.request_id,
                        9,
                        ManagerTokenBridgeCompletion::Error {
                            error: ManagerTokenBridgeFailure {
                                kind: "context_changed".to_string(),
                                code: None,
                                description: None,
                                http_status: None,
                                redirect_url: None,
                            },
                        },
                    )
                    .await,
                Err(ManagerError::ContextChanged)
            ));
            bridge
                .finish_transport(&request.request_id, 9, &attempt_id)
                .await
                .unwrap();
            assert_eq!(
                attempt + 1,
                bridge.state.lock().unwrap().pending[&request.request_id].attempt_count
            );
        }
        assert!(matches!(
            bridge.begin_transport(&request.request_id, 9, &body).await,
            Err(ManagerError::ContextChanged)
        ));
        bridge
            .complete(
                &request.request_id,
                9,
                ManagerTokenBridgeCompletion::Success {
                    access_token: "short".to_string(),
                    token_type: "Bearer".to_string(),
                    expires_in: 300,
                    scope: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(exchange.await.unwrap().unwrap().access_token, "short");
    }

    #[tokio::test]
    async fn stale_ready_lease_cannot_disable_the_current_bridge() {
        let bridge = ManagerTokenBridge::new();
        bridge.set_ready("old", true).await;
        bridge.set_ready("current", true).await;
        bridge.set_ready("old", false).await;
        assert_eq!(
            bridge.state.lock().unwrap().ready_lease.as_deref(),
            Some("current")
        );
    }

    #[test]
    fn generic_upstream_failures_keep_response_descriptions_out_of_ipc_errors() {
        let result = ManagerTokenBridgeCompletion::Error {
            error: ManagerTokenBridgeFailure {
                kind: "other".to_string(),
                code: Some("server_error".to_string()),
                description: Some("subject_token=secret-user-jwt".to_string()),
                http_status: Some(500),
                redirect_url: None,
            },
        }
        .into_result();

        assert!(matches!(
            result,
            Err(ManagerError::Other { status: 500, body })
                if body == "Unexpected Manager response"
                    && !body.contains("secret-user-jwt")
        ));
    }

    #[test]
    fn every_error_field_is_redacted_before_settlement() {
        let mut completion = ManagerTokenBridgeCompletion::Error {
            error: ManagerTokenBridgeFailure {
                kind: "token_exchange".to_string(),
                code: Some("secret-user-jwt".to_string()),
                description: Some("echo: secret-user-jwt".to_string()),
                http_status: Some(400),
                redirect_url: Some("https://example.test/secret-user-jwt".to_string()),
            },
        };
        completion.redact_secret("secret-user-jwt");
        let serialized = format!("{completion:?}");
        assert!(!serialized.contains("secret-user-jwt"));
        assert!(serialized.contains("[REDACTED]"));
    }
}
