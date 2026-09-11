//! In-memory chat session leases bound to the authoritative Manager Context.
//!
//! Manager credentials never cross this boundary. A lease exposes only resolved, non-secret
//! endpoints; short `chat:read chat:send` tokens are returned on demand for the browser-owned
//! Socket.IO transport and are retained only in memory.

mod resources;
mod transfers;
pub use resources::{ChatResourceDiagnostic, ChatResourceError, ChatResourceHandle};
pub use transfers::ChatUploadFile;

use std::collections::HashMap;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use reqwest::header::{HeaderName, HeaderValue, ACCEPT, CONTENT_TYPE, COOKIE};
use serde::Serialize;
use tokio::sync::{watch, Mutex, RwLock};
use url::Url;
use uuid::Uuid;

use crate::services::manager_client::{
    ConnectionInfoResponse, DigitalEmployeeBrief, ExchangedToken, ManagerError,
};
use crate::services::manager_context::{ManagerContextCoordinator, ManagerContextKey};
use crate::services::settings::SettingsService;

const CHAT_SCOPE: &str = "chat:read chat:send";
const TOKEN_REFRESH_SKEW: Duration = Duration::from_secs(60);
const CHAT_HTTP_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_CHAT_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const MAX_CHAT_REQUEST_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSessionDescriptor {
    pub lease_id: String,
    pub employee_id: u64,
    pub robot_name: String,
    pub http_base_url: String,
    pub socket_namespace_url: String,
    pub socket_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatSessionCredential {
    pub token: String,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatHttpResponse {
    pub status: u16,
    pub body: String,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone)]
struct ResolvedChatTarget {
    context_key: ManagerContextKey,
    manager_generation: u64,
    employee_id: u64,
    robot_account_id: String,
    robot_name: String,
    http_base_url: Url,
    socket_namespace_url: String,
    socket_path: String,
    frontend_namespace: String,
    frontend_robot_id: String,
    routing_headers: HashMap<String, String>,
}

#[derive(Debug)]
struct ResolvedChatEndpoints {
    http_base_url: Url,
    socket_namespace_url: String,
    socket_path: String,
    frontend_namespace: String,
    frontend_robot_id: String,
}

#[derive(Debug)]
struct CachedToken {
    value: ExchangedToken,
    expires_at: Instant,
    expires_at_unix_ms: u64,
}

impl CachedToken {
    fn new(value: ExchangedToken) -> Self {
        let lifetime = Duration::from_secs(value.expires_in.max(0) as u64);
        let now_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        Self {
            value,
            expires_at: Instant::now() + lifetime,
            expires_at_unix_ms: now_unix_ms.saturating_add(lifetime.as_millis() as u64),
        }
    }

    fn is_fresh(&self) -> bool {
        self.expires_at.saturating_duration_since(Instant::now()) > TOKEN_REFRESH_SKEW
    }

    fn credential(&self) -> ChatSessionCredential {
        ChatSessionCredential {
            token: self.value.access_token.clone(),
            expires_at: self.expires_at_unix_ms,
        }
    }
}

#[derive(Debug)]
struct ChatLease {
    target: ResolvedChatTarget,
    selection_revision: u64,
    token: Option<CachedToken>,
    cancelled: watch::Sender<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveChatSelection {
    revision: u64,
    lease_id: Option<String>,
}

#[derive(Debug)]
pub struct ChatSessionService {
    manager_context: Weak<ManagerContextCoordinator>,
    settings: Option<Arc<SettingsService>>,
    http: reqwest::Client,
    leases: RwLock<HashMap<String, Arc<Mutex<ChatLease>>>>,
    active_selections: Mutex<HashMap<ManagerContextKey, ActiveChatSelection>>,
    preference_write: Mutex<()>,
    transfers: std::sync::Mutex<HashMap<String, transfers::PendingTransfer>>,
    resources: resources::ResourceState,
}

impl ChatSessionService {
    pub fn new(manager_context: Weak<ManagerContextCoordinator>) -> Self {
        Self::build(manager_context, None)
    }

    pub fn new_with_settings(
        manager_context: Weak<ManagerContextCoordinator>,
        settings: Arc<SettingsService>,
    ) -> Self {
        Self::build(manager_context, Some(settings))
    }

    fn build(
        manager_context: Weak<ManagerContextCoordinator>,
        settings: Option<Arc<SettingsService>>,
    ) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(format!(
                "tfrobot-client/{} (chat-bff; {})",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS
            ))
            .pool_max_idle_per_host(0)
            .tcp_keepalive(Duration::from_secs(10))
            .timeout(CHAT_HTTP_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("chat BFF reqwest client should build with rustls");
        Self {
            manager_context,
            settings,
            http,
            leases: RwLock::new(HashMap::new()),
            active_selections: Mutex::new(HashMap::new()),
            preference_write: Mutex::new(()),
            transfers: std::sync::Mutex::new(HashMap::new()),
            resources: resources::ResourceState::new(),
        }
    }

    pub async fn recent_employee(&self) -> Result<Option<u64>, ManagerError> {
        let manager = self.manager()?;
        let generation = manager.capture_authenticated_generation().await?;
        let context = manager.context_key_for_generation(generation).await?;
        let Some(settings) = &self.settings else {
            return Ok(None);
        };
        match settings.load_recent_chat_employee(&context) {
            Ok(employee_id) => Ok(employee_id),
            Err(error) => {
                log::warn!("failed to load recent chat Robot preference: {error}");
                Ok(None)
            }
        }
    }

    pub async fn open(
        &self,
        employee_id: u64,
        selection_revision: u64,
    ) -> Result<ChatSessionDescriptor, ManagerError> {
        let manager = self.manager()?;
        let manager_generation = manager.capture_authenticated_generation().await?;
        let context_key = manager
            .context_key_for_generation(manager_generation)
            .await?;
        let employees = manager
            .list_digital_employees_for_generation(manager_generation)
            .await?;
        let employee = resolve_chat_employee(employees, employee_id)?;
        let robot_account_id = employee.robot_account_id.clone().ok_or_else(|| {
            ManagerError::InvalidResponse(format!(
                "robotAccountId missing for chat employee {employee_id}"
            ))
        })?;
        let connection_info = manager
            .get_connection_info_for_generation(manager_generation, employee_id)
            .await?;
        let endpoints = resolve_chat_endpoints(&connection_info)?;
        let exchanged = manager
            .exchange_token_for_generation(
                manager_generation,
                &robot_account_id,
                Some(CHAT_SCOPE.to_string()),
            )
            .await?;
        let lease_id = Uuid::new_v4().to_string();
        let target = ResolvedChatTarget {
            context_key: context_key.clone(),
            manager_generation,
            employee_id,
            robot_account_id,
            robot_name: employee.name,
            http_base_url: endpoints.http_base_url,
            socket_namespace_url: endpoints.socket_namespace_url,
            socket_path: endpoints.socket_path,
            frontend_namespace: endpoints.frontend_namespace,
            frontend_robot_id: endpoints.frontend_robot_id,
            routing_headers: connection_info.routing_headers,
        };
        let descriptor = ChatSessionDescriptor {
            lease_id: lease_id.clone(),
            employee_id,
            robot_name: target.robot_name.clone(),
            http_base_url: target.http_base_url.to_string(),
            socket_namespace_url: target.socket_namespace_url.clone(),
            socket_path: target.socket_path.clone(),
        };
        let committed_lease = Arc::new(Mutex::new(ChatLease {
            target,
            selection_revision,
            token: Some(CachedToken::new(exchanged)),
            cancelled: watch::channel(false).0,
        }));
        let committed_id = lease_id.clone();
        let committed_context = context_key;
        manager
            .commit_for_authenticated_generation(manager_generation, || async move {
                self.leases
                    .write()
                    .await
                    .insert(committed_id.clone(), committed_lease);
                self.activate_selection(committed_context, selection_revision, committed_id)
                    .await;
                Ok(())
            })
            .await?;
        Ok(descriptor)
    }

    pub async fn remember(&self, lease_id: &str) -> Result<(), ManagerError> {
        let lease = self.lease(lease_id).await?;
        let (target, selection_revision) = {
            let current = lease.lock().await;
            (current.target.clone(), current.selection_revision)
        };
        let target_context = target.context_key.clone();
        let target_generation = target.manager_generation;
        let _preference_write = self.preference_write.lock().await;
        let manager = self.manager()?;
        manager
            .commit_for_authenticated_generation(target_generation, || async move {
                self.validate_active_lease(lease_id, &lease, &target_context, selection_revision)
                    .await
            })
            .await?;
        self.save_recent_preference(target).await;
        Ok(())
    }

    async fn validate_active_lease(
        &self,
        lease_id: &str,
        lease: &Arc<Mutex<ChatLease>>,
        context: &ManagerContextKey,
        selection_revision: u64,
    ) -> Result<(), ManagerError> {
        let leases = self.leases.read().await;
        if !leases
            .get(lease_id)
            .is_some_and(|current| Arc::ptr_eq(current, lease))
        {
            return Err(ManagerError::ContextChanged);
        }
        if *lease.lock().await.cancelled.borrow() {
            return Err(ManagerError::ContextChanged);
        }
        if !self
            .is_active_selection(context, selection_revision, lease_id)
            .await
        {
            return Err(ManagerError::ContextChanged);
        }
        Ok(())
    }

    async fn save_recent_preference(&self, target: ResolvedChatTarget) {
        if let Some(settings) = self.settings.clone() {
            let context = target.context_key;
            let employee_id = target.employee_id;
            match tokio::task::spawn_blocking(move || {
                settings.save_recent_chat_employee(&context, employee_id)
            })
            .await
            {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    log::warn!("failed to save recent chat Robot preference: {error}");
                }
                Err(error) => {
                    log::warn!("failed to join recent chat Robot preference save: {error}");
                }
            }
        }
    }

    #[cfg(test)]
    async fn remember_active_for_test(&self, lease_id: &str) -> Result<(), ManagerError> {
        let lease = self.lease(lease_id).await?;
        let (target, selection_revision) = {
            let current = lease.lock().await;
            (current.target.clone(), current.selection_revision)
        };
        let _preference_write = self.preference_write.lock().await;
        self.validate_active_lease(lease_id, &lease, &target.context_key, selection_revision)
            .await?;
        self.save_recent_preference(target).await;
        Ok(())
    }

    async fn activate_selection(
        &self,
        context: ManagerContextKey,
        revision: u64,
        lease_id: String,
    ) -> bool {
        let mut active = self.active_selections.lock().await;
        if let Some(current) = active.get(&context) {
            if current.revision > revision
                || (current.revision == revision
                    && current.lease_id.as_deref() != Some(lease_id.as_str()))
            {
                return false;
            }
        }
        active.insert(
            context,
            ActiveChatSelection {
                revision,
                lease_id: Some(lease_id),
            },
        );
        true
    }

    async fn is_active_selection(
        &self,
        context: &ManagerContextKey,
        revision: u64,
        lease_id: &str,
    ) -> bool {
        self.active_selections.lock().await.get(context)
            == Some(&ActiveChatSelection {
                revision,
                lease_id: Some(lease_id.to_string()),
            })
    }

    pub async fn credential(&self, lease_id: &str) -> Result<ChatSessionCredential, ManagerError> {
        let lease = self.lease(lease_id).await?;
        let target = lease.lock().await.target.clone();
        let manager = self.manager()?;
        if let Some(credential) = manager
            .commit_for_authenticated_generation(target.manager_generation, || async {
                self.cached_credential_for_active_lease(lease_id, &lease)
                    .await
            })
            .await?
        {
            return Ok(credential);
        }
        let employees = manager
            .list_digital_employees_for_generation(target.manager_generation)
            .await?;
        let employee = resolve_chat_employee(employees, target.employee_id)?;
        if employee.robot_account_id.as_deref() != Some(target.robot_account_id.as_str()) {
            return Err(ManagerError::ContextChanged);
        }
        let exchanged = manager
            .exchange_token_for_generation(
                target.manager_generation,
                &target.robot_account_id,
                Some(CHAT_SCOPE.to_string()),
            )
            .await?;
        manager
            .commit_for_authenticated_generation(target.manager_generation, || async move {
                let leases = self.leases.read().await;
                if !leases
                    .get(lease_id)
                    .is_some_and(|current| Arc::ptr_eq(current, &lease))
                {
                    return Err(ManagerError::ContextChanged);
                }
                let mut current = lease.lock().await;
                if *current.cancelled.borrow() {
                    return Err(ManagerError::ContextChanged);
                }
                let cached = CachedToken::new(exchanged);
                let credential = cached.credential();
                current.token = Some(cached);
                Ok(credential)
            })
            .await
    }

    pub async fn invalidate(&self, lease_id: &str) -> Result<(), ManagerError> {
        let lease = self.lease(lease_id).await?;
        lease.lock().await.token = None;
        Ok(())
    }

    pub async fn close(&self, lease_id: &str) {
        if let Some(lease) = self.leases.write().await.remove(lease_id) {
            let current = lease.lock().await;
            let context = current.target.context_key.clone();
            let revision = current.selection_revision;
            current.cancelled.send_replace(true);
            self.cancel_transfers(lease_id);
            self.resources.revoke_lease(lease_id);
            drop(current);
            let mut active = self.active_selections.lock().await;
            if active.get(&context)
                == Some(&ActiveChatSelection {
                    revision,
                    lease_id: Some(lease_id.to_string()),
                })
            {
                active.insert(
                    context,
                    ActiveChatSelection {
                        revision,
                        lease_id: None,
                    },
                );
            }
        }
    }

    pub async fn close_for_context(&self, context: Option<&ManagerContextKey>) {
        let entries = {
            let leases = self.leases.read().await;
            leases
                .iter()
                .map(|(id, lease)| (id.clone(), lease.clone()))
                .collect::<Vec<_>>()
        };
        let mut remove = Vec::new();
        for (id, lease) in entries {
            let lease_context = lease.lock().await.target.context_key.clone();
            if context.is_none_or(|context| context == &lease_context) {
                remove.push(id);
            }
        }
        for id in remove {
            self.close(&id).await;
        }
    }

    pub async fn proxy(
        &self,
        lease_id: &str,
        request_url: &str,
        method: &str,
        body: Option<String>,
    ) -> Result<ChatHttpResponse, ManagerError> {
        if body
            .as_ref()
            .is_some_and(|body| body.len() > MAX_CHAT_REQUEST_BYTES)
        {
            return Err(ManagerError::InvalidResponse(
                "chat request body exceeds the BFF limit".to_string(),
            ));
        }
        let lease = self.lease(lease_id).await?;
        let (target, cancelled) = {
            let current = lease.lock().await;
            (current.target.clone(), current.cancelled.subscribe())
        };
        if *cancelled.borrow() {
            return Err(ManagerError::ContextChanged);
        }
        let url = validate_chat_request_url(&target.http_base_url, request_url, method)?;
        let credential = self.credential(lease_id).await?;

        self.proxy_authorized(target, cancelled, url, method, body, credential)
            .await
    }

    async fn proxy_authorized(
        &self,
        target: ResolvedChatTarget,
        mut cancelled: watch::Receiver<bool>,
        url: Url,
        method: &str,
        body: Option<String>,
        credential: ChatSessionCredential,
    ) -> Result<ChatHttpResponse, ManagerError> {
        let request_method = match method {
            "GET" => reqwest::Method::GET,
            "POST" => reqwest::Method::POST,
            _ => {
                return Err(ManagerError::InvalidResponse(
                    "unsupported chat BFF method".to_string(),
                ))
            }
        };
        let mut request = self.authorized_request(&target, url, request_method, &credential)?;
        if let Some(body) = body {
            request = request.header(CONTENT_TYPE, "application/json").body(body);
        }

        self.read_response(request, &mut cancelled).await
    }

    fn authorized_request(
        &self,
        target: &ResolvedChatTarget,
        url: Url,
        request_method: reqwest::Method,
        credential: &ChatSessionCredential,
    ) -> Result<reqwest::RequestBuilder, ManagerError> {
        let mut request = self
            .http
            .request(request_method, url)
            .header(ACCEPT, "application/json")
            .header(COOKIE, frontend_session_cookie(target, &credential.token)?);
        for (name, value) in &target.routing_headers {
            let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
                ManagerError::InvalidResponse(
                    "connection-info returned an invalid route header".into(),
                )
            })?;
            let header_value = HeaderValue::from_str(value).map_err(|_| {
                ManagerError::InvalidResponse(
                    "connection-info returned an invalid route header".into(),
                )
            })?;
            request = request.header(header_name, header_value);
        }
        Ok(request)
    }

    async fn read_response(
        &self,
        request: reqwest::RequestBuilder,
        cancelled: &mut watch::Receiver<bool>,
    ) -> Result<ChatHttpResponse, ManagerError> {
        if *cancelled.borrow() {
            return Err(ManagerError::ContextChanged);
        }
        let mut response = tokio::select! {
            result = request.send() => result.map_err(|error| ManagerError::NetworkError(error.to_string()))?,
            _ = cancelled.changed() => return Err(ManagerError::ContextChanged),
        };
        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        if response
            .content_length()
            .is_some_and(|length| length > MAX_CHAT_RESPONSE_BYTES as u64)
        {
            return Err(ManagerError::InvalidResponse(
                "chat response exceeds the BFF limit".to_string(),
            ));
        }
        let mut bytes = Vec::new();
        loop {
            let chunk = tokio::select! {
                result = response.chunk() => result.map_err(|error| ManagerError::NetworkError(error.to_string()))?,
                _ = cancelled.changed() => return Err(ManagerError::ContextChanged),
            };
            let Some(chunk) = chunk else {
                break;
            };
            if bytes.len().saturating_add(chunk.len()) > MAX_CHAT_RESPONSE_BYTES {
                return Err(ManagerError::InvalidResponse(
                    "chat response exceeds the BFF limit".to_string(),
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(ChatHttpResponse {
            status,
            body: String::from_utf8_lossy(&bytes).into_owned(),
            content_type,
        })
    }

    fn manager(&self) -> Result<Arc<ManagerContextCoordinator>, ManagerError> {
        self.manager_context
            .upgrade()
            .ok_or_else(|| ManagerError::Other {
                status: 0,
                body: "Manager Context is unavailable".to_string(),
            })
    }

    async fn lease(&self, lease_id: &str) -> Result<Arc<Mutex<ChatLease>>, ManagerError> {
        self.leases
            .read()
            .await
            .get(lease_id)
            .cloned()
            .ok_or(ManagerError::ContextChanged)
    }

    async fn cached_credential_for_active_lease(
        &self,
        lease_id: &str,
        lease: &Arc<Mutex<ChatLease>>,
    ) -> Result<Option<ChatSessionCredential>, ManagerError> {
        let leases = self.leases.read().await;
        if !leases
            .get(lease_id)
            .is_some_and(|current| Arc::ptr_eq(current, lease))
        {
            return Err(ManagerError::ContextChanged);
        }
        let current = lease.lock().await;
        if *current.cancelled.borrow() {
            return Err(ManagerError::ContextChanged);
        }
        Ok(current
            .token
            .as_ref()
            .filter(|token| token.is_fresh())
            .map(CachedToken::credential))
    }
}

fn resolve_chat_employee(
    employees: Vec<DigitalEmployeeBrief>,
    employee_id: u64,
) -> Result<DigitalEmployeeBrief, ManagerError> {
    let employee = employees
        .into_iter()
        .find(|employee| employee.id == employee_id)
        .ok_or(ManagerError::NotFoundOrNoPermission)?;
    if employee.status.as_deref().unwrap_or("running") != "running" {
        return Err(ManagerError::InvalidResponse(format!(
            "chat employee {employee_id} is not running"
        )));
    }
    if employee
        .template_type
        .as_deref()
        .is_some_and(|template_type| template_type != "tfrserver")
    {
        return Err(ManagerError::InvalidResponse(format!(
            "chat employee {employee_id} does not expose the TFRobotServer chat API"
        )));
    }
    Ok(employee)
}

fn resolve_chat_endpoints(
    connection_info: &ConnectionInfoResponse,
) -> Result<ResolvedChatEndpoints, ManagerError> {
    let mut transport_base = Url::parse(connection_info.socket_base_url.trim()).map_err(|_| {
        ManagerError::InvalidResponse("connection-info returned an invalid socketBaseURL".into())
    })?;
    if !transport_base.username().is_empty() || transport_base.password().is_some() {
        return Err(ManagerError::InvalidResponse(
            "connection-info endpoint must not contain URL credentials".into(),
        ));
    }
    match transport_base.scheme() {
        "http" | "https" => {}
        "ws" => transport_base.set_scheme("http").map_err(|_| {
            ManagerError::InvalidResponse("connection-info returned an invalid URL scheme".into())
        })?,
        "wss" => transport_base.set_scheme("https").map_err(|_| {
            ManagerError::InvalidResponse("connection-info returned an invalid URL scheme".into())
        })?,
        _ => {
            return Err(ManagerError::InvalidResponse(
                "connection-info endpoint must use HTTP(S) or WS(S)".into(),
            ))
        }
    }
    transport_base.set_query(None);
    transport_base.set_fragment(None);

    let robot_type =
        required_frontend_route_id(connection_info.robot_type.as_deref(), "robotType")?;
    let namespace = required_frontend_route_id(connection_info.namespace.as_deref(), "namespace")?;
    let robot_id = required_frontend_route_id(connection_info.rid.as_deref(), "rid")?;
    let instance_prefix = format!("/c/{robot_type}/{namespace}/{robot_id}");

    let mut http_base = transport_base.clone();
    http_base.set_path(&format!("{instance_prefix}/api/"));

    let mut socket_namespace = transport_base;
    socket_namespace.set_path("/chat");
    socket_namespace.set_query(None);
    socket_namespace.set_fragment(None);
    let socket_transport_path = connection_info
        .sio_path
        .as_deref()
        .filter(|path| !path.trim().is_empty())
        .unwrap_or("/socket.io")
        .trim()
        .to_string();
    if !socket_transport_path.starts_with('/') || socket_transport_path.contains("..") {
        return Err(ManagerError::InvalidResponse(
            "connection-info returned an invalid Socket.IO path".into(),
        ));
    }
    let socket_path = format!("{instance_prefix}{socket_transport_path}");
    Ok(ResolvedChatEndpoints {
        http_base_url: http_base,
        socket_namespace_url: socket_namespace.to_string(),
        socket_path,
        frontend_namespace: namespace.to_string(),
        frontend_robot_id: robot_id.to_string(),
    })
}

fn required_frontend_route_id<'a>(
    value: Option<&'a str>,
    field: &str,
) -> Result<&'a str, ManagerError> {
    let value = value.filter(|value| {
        !value.is_empty()
            && value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    });
    value.ok_or_else(|| {
        ManagerError::InvalidResponse(format!(
            "connection-info returned a missing or invalid {field} for chat"
        ))
    })
}

fn frontend_session_cookie(
    target: &ResolvedChatTarget,
    short_token: &str,
) -> Result<HeaderValue, ManagerError> {
    if short_token.is_empty()
        || short_token
            .bytes()
            .any(|byte| byte <= 0x20 || byte >= 0x7f || byte == b';' || byte == b',')
    {
        return Err(ManagerError::InvalidResponse(
            "token exchange returned an invalid chat credential".into(),
        ));
    }
    let mut cookie = HeaderValue::from_str(&format!(
        "tfNamespace={}; tfRobotId={}; tfUserToken={short_token}",
        target.frontend_namespace, target.frontend_robot_id
    ))
    .map_err(|_| {
        ManagerError::InvalidResponse("failed to construct the chat BFF session".into())
    })?;
    cookie.set_sensitive(true);
    Ok(cookie)
}

fn validate_chat_request_url(
    base_url: &Url,
    request_url: &str,
    method: &str,
) -> Result<Url, ManagerError> {
    let url = Url::parse(request_url)
        .map_err(|_| ManagerError::InvalidResponse("chat BFF received an invalid URL".into()))?;
    if url.origin() != base_url.origin()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(ManagerError::InvalidResponse(
            "chat BFF rejected an endpoint outside the active lease".into(),
        ));
    }
    let base_path = base_url.path();
    let relative_path = url
        .path()
        .strip_prefix(base_path)
        .ok_or_else(|| {
            ManagerError::InvalidResponse(
                "chat BFF rejected a route outside the active base path".into(),
            )
        })?
        .trim_start_matches('/');
    let segments = relative_path.split('/').collect::<Vec<_>>();
    let allowed_path = match segments.as_slice() {
        ["v1", "chat", "conversations"] => method == "GET" || method == "POST",
        ["v1", "chat", "conversations", id, "messages"] if !id.is_empty() => {
            method == "GET" || method == "POST"
        }
        ["v1", "chat", "conversations", id, "status"] if !id.is_empty() => method == "GET",
        ["v1", "chat", "conversations", id, "interrupt"] if !id.is_empty() => method == "POST",
        _ => false,
    };
    if !allowed_path {
        return Err(ManagerError::InvalidResponse(
            "chat BFF rejected an unsupported route".into(),
        ));
    }
    const ALLOWED_QUERY: &[&str] = &["count", "cursor", "platformId", "title"];
    if url
        .query_pairs()
        .any(|(name, _)| !ALLOWED_QUERY.contains(&name.as_ref()))
    {
        return Err(ManagerError::InvalidResponse(
            "chat BFF rejected an unsupported query parameter".into(),
        ));
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::Full;
    use hyper::body::Bytes;
    use hyper::server::conn::http1;
    use hyper::service::service_fn;
    use hyper::{Request, Response};
    use hyper_util::rt::TokioIo;

    fn employee(status: Option<&str>, template_type: Option<&str>) -> DigitalEmployeeBrief {
        DigitalEmployeeBrief {
            id: 42,
            name: "Robot".into(),
            description: String::new(),
            robot_id: Some("rid-42".into()),
            robot_account_id: Some("robot-account-42".into()),
            status: status.map(str::to_string),
            template_display_name: None,
            template_type: template_type.map(str::to_string),
            namespace: None,
            cluster_name: None,
            departments: vec![],
        }
    }

    pub(super) fn test_target(
        http_base_url: Url,
        context_key: ManagerContextKey,
    ) -> ResolvedChatTarget {
        ResolvedChatTarget {
            context_key,
            manager_generation: 7,
            employee_id: 42,
            robot_account_id: "robot-account-42".into(),
            robot_name: "Robot".into(),
            http_base_url,
            socket_namespace_url: "http://127.0.0.1/chat".into(),
            socket_path: "/socket.io".into(),
            frontend_namespace: "tenant-acme".into(),
            frontend_robot_id: "rid-42".into(),
            routing_headers: HashMap::from([("X-TF-Route".into(), "robot-42".into())]),
        }
    }

    fn context(account_id: &str) -> ManagerContextKey {
        ManagerContextKey {
            environment: crate::services::manager_environment::ManagerEnvironment::Staging,
            account_id: account_id.into(),
            organization_id: "organization-1".into(),
        }
    }

    pub(super) async fn insert_test_lease(
        service: &ChatSessionService,
        id: &str,
        target: ResolvedChatTarget,
        selection_revision: u64,
    ) {
        service.leases.write().await.insert(
            id.into(),
            Arc::new(Mutex::new(ChatLease {
                target,
                selection_revision,
                token: Some(CachedToken::new(ExchangedToken {
                    access_token: "short-chat-token".into(),
                    token_type: "Bearer".into(),
                    expires_in: 300,
                    scope: Some(CHAT_SCOPE.into()),
                })),
                cancelled: watch::channel(false).0,
            })),
        );
    }

    #[test]
    fn only_explicitly_incompatible_or_stopped_employees_are_rejected() {
        assert!(
            resolve_chat_employee(vec![employee(Some("running"), Some("tfrserver"))], 42).is_ok()
        );
        assert!(resolve_chat_employee(vec![employee(None, None)], 42).is_ok());
        assert!(
            resolve_chat_employee(vec![employee(Some("stopped"), Some("tfrserver"))], 42).is_err()
        );
        assert!(
            resolve_chat_employee(vec![employee(Some("running"), Some("tfropenclaw"))], 42)
                .is_err()
        );
    }

    #[test]
    fn http_endpoint_uses_instance_bff_without_exposing_credentials_or_manager_secrets() {
        let info = ConnectionInfoResponse {
            socket_base_url: "wss://robot.example.com/proxy".into(),
            sio_path: Some("/robot/socket.io".into()),
            namespace: Some("tenant-acme".into()),
            rid: Some("rid-42".into()),
            robot_type: Some("tfrobot".into()),
            smcp_namespace: None,
            computer_name: None,
            routing_headers: HashMap::new(),
        };
        let endpoints = resolve_chat_endpoints(&info).unwrap();
        assert_eq!(
            endpoints.http_base_url.as_str(),
            "https://robot.example.com/c/tfrobot/tenant-acme/rid-42/api/"
        );
        assert_eq!(
            endpoints.socket_namespace_url,
            "https://robot.example.com/chat"
        );
        assert_eq!(
            endpoints.socket_path,
            "/c/tfrobot/tenant-acme/rid-42/robot/socket.io"
        );
        assert_eq!(endpoints.frontend_namespace, "tenant-acme");
        assert_eq!(endpoints.frontend_robot_id, "rid-42");
    }

    #[test]
    fn chat_endpoints_require_valid_frontend_route_identity() {
        let mut info = ConnectionInfoResponse {
            socket_base_url: "https://robot.example.com".into(),
            sio_path: None,
            namespace: None,
            rid: Some("rid-42".into()),
            robot_type: Some("tfrobot".into()),
            smcp_namespace: None,
            computer_name: None,
            routing_headers: HashMap::new(),
        };
        assert!(resolve_chat_endpoints(&info).is_err());

        info.namespace = Some("Tenant_Acme".into());
        assert!(resolve_chat_endpoints(&info).is_err());

        info.namespace = Some("tenant-acme".into());
        info.rid = None;
        assert!(resolve_chat_endpoints(&info).is_err());
    }

    #[test]
    fn frontend_session_cookie_rejects_credential_header_injection() {
        let target = test_target(
            Url::parse("https://robot.example.com/c/tfrobot/tenant-acme/rid-42/api/").unwrap(),
            context("account-a"),
        );

        assert!(frontend_session_cookie(&target, "short-token; adminToken=forged").is_err());
        assert!(frontend_session_cookie(&target, "short-token\r\nX-Forged: value").is_err());
    }

    #[test]
    fn bff_allowlist_rejects_arbitrary_hosts_routes_and_queries() {
        let base = Url::parse("https://robot.example.com/proxy/").unwrap();
        assert!(validate_chat_request_url(
            &base,
            "https://robot.example.com/proxy/v1/chat/conversations?count=20",
            "GET"
        )
        .is_ok());
        assert!(validate_chat_request_url(
            &base,
            "https://attacker.example/v1/chat/conversations",
            "GET"
        )
        .is_err());
        assert!(
            validate_chat_request_url(&base, "https://robot.example.com/proxy/admin", "GET")
                .is_err()
        );
        assert!(validate_chat_request_url(
            &base,
            "https://robot.example.com/proxy/v1/chat/conversations?token=secret",
            "GET"
        )
        .is_err());
    }

    #[tokio::test]
    async fn context_cleanup_revokes_only_departing_context_leases() {
        let service = ChatSessionService::new(Weak::new());
        let base = Url::parse("https://robot.example/proxy/").unwrap();
        insert_test_lease(
            &service,
            "lease-a",
            test_target(base.clone(), context("account-a")),
            1,
        )
        .await;
        insert_test_lease(
            &service,
            "lease-b",
            test_target(base, context("account-b")),
            1,
        )
        .await;

        service.close_for_context(Some(&context("account-a"))).await;

        assert!(service.lease("lease-a").await.is_err());
        assert!(service.lease("lease-b").await.is_ok());
    }

    #[tokio::test]
    async fn older_selection_revision_cannot_reclaim_active_preference() {
        let service = ChatSessionService::new(Weak::new());
        let context = context("account-a");

        assert!(
            service
                .activate_selection(context.clone(), 10, "lease-old".into())
                .await
        );
        assert!(
            service
                .activate_selection(context.clone(), 20, "lease-new".into())
                .await
        );
        assert!(
            !service
                .activate_selection(context.clone(), 10, "lease-old".into())
                .await
        );

        assert!(!service.is_active_selection(&context, 10, "lease-old").await);
        assert!(service.is_active_selection(&context, 20, "lease-new").await);
    }

    #[tokio::test]
    async fn same_revision_cannot_replace_a_different_active_lease() {
        let service = ChatSessionService::new(Weak::new());
        let context = context("account-a");
        let base = Url::parse("https://robot.example/proxy/").unwrap();
        insert_test_lease(
            &service,
            "lease-live",
            test_target(base.clone(), context.clone()),
            20,
        )
        .await;
        insert_test_lease(
            &service,
            "lease-stale",
            test_target(base, context.clone()),
            20,
        )
        .await;

        assert!(
            service
                .activate_selection(context.clone(), 20, "lease-live".into())
                .await
        );
        assert!(
            !service
                .activate_selection(context.clone(), 20, "lease-stale".into())
                .await
        );
        assert!(
            service
                .is_active_selection(&context, 20, "lease-live")
                .await
        );
        assert!(
            !service
                .is_active_selection(&context, 20, "lease-stale")
                .await
        );

        service.close("lease-stale").await;
        assert!(
            service
                .is_active_selection(&context, 20, "lease-live")
                .await
        );
    }

    #[tokio::test]
    async fn closing_old_lease_does_not_clear_newer_active_selection() {
        let service = ChatSessionService::new(Weak::new());
        let base = Url::parse("https://robot.example/proxy/").unwrap();
        let context = context("account-a");
        insert_test_lease(
            &service,
            "lease-old",
            test_target(base.clone(), context.clone()),
            1,
        )
        .await;
        insert_test_lease(&service, "lease-new", test_target(base, context.clone()), 2).await;
        service
            .activate_selection(context.clone(), 1, "lease-old".into())
            .await;
        service
            .activate_selection(context.clone(), 2, "lease-new".into())
            .await;

        service.close("lease-old").await;

        assert!(service.is_active_selection(&context, 2, "lease-new").await);
        assert!(!service.is_active_selection(&context, 1, "lease-old").await);

        service.close("lease-new").await;
        assert!(
            !service
                .activate_selection(context.clone(), 1, "lease-old".into())
                .await
        );
        assert!(!service.is_active_selection(&context, 1, "lease-old").await);
    }

    #[tokio::test]
    async fn late_old_remember_cannot_overwrite_newer_persisted_preference() {
        let directory = tempfile::tempdir().unwrap();
        let settings = Arc::new(SettingsService::new(directory.path().to_path_buf()));
        let service = ChatSessionService::new_with_settings(Weak::new(), settings.clone());
        let base = Url::parse("https://robot.example/proxy/").unwrap();
        let context = context("account-a");
        let old_target = test_target(base.clone(), context.clone());
        let mut new_target = test_target(base, context.clone());
        new_target.employee_id = 43;
        new_target.robot_name = "Robot B".into();
        insert_test_lease(&service, "lease-old", old_target, 10).await;
        insert_test_lease(&service, "lease-new", new_target, 20).await;
        service
            .activate_selection(context.clone(), 10, "lease-old".into())
            .await;
        service
            .activate_selection(context.clone(), 20, "lease-new".into())
            .await;

        service.remember_active_for_test("lease-new").await.unwrap();
        assert!(matches!(
            service.remember_active_for_test("lease-old").await,
            Err(ManagerError::ContextChanged)
        ));

        assert_eq!(
            settings.load_recent_chat_employee(&context).unwrap(),
            Some(43)
        );
        service.close("lease-new").await;
        assert!(service.remember_active_for_test("lease-new").await.is_err());
        assert_eq!(
            settings.load_recent_chat_employee(&context).unwrap(),
            Some(43)
        );
    }

    #[tokio::test]
    async fn bff_injects_only_lease_owned_session_and_route_headers() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (request_tx, mut request_rx) = tokio::sync::mpsc::unbounded_channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let io = TokioIo::new(stream);
            http1::Builder::new()
                .serve_connection(
                    io,
                    service_fn(move |request: Request<hyper::body::Incoming>| {
                        let request_tx = request_tx.clone();
                        async move {
                            request_tx
                                .send((request.uri().to_string(), request.headers().clone()))
                                .unwrap();
                            Ok::<_, std::convert::Infallible>(Response::new(Full::new(
                                Bytes::from_static(br#"{"code":200,"data":{}}"#),
                            )))
                        }
                    }),
                )
                .await
                .unwrap();
        });

        let service = ChatSessionService::new(Weak::new());
        let base = Url::parse(&format!("http://{address}/proxy/")).unwrap();
        let target = test_target(base.clone(), context("account-a"));
        let (_cancel_tx, cancelled) = watch::channel(false);
        let url = base.join("v1/chat/conversations?count=20").unwrap();
        let response = service
            .proxy_authorized(
                target,
                cancelled,
                url,
                "GET",
                None,
                ChatSessionCredential {
                    token: "short-chat-token".into(),
                    expires_at: 0,
                },
            )
            .await
            .unwrap();
        let (uri, headers) = request_rx.recv().await.unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(uri, "/proxy/v1/chat/conversations?count=20");
        assert_eq!(
            headers.get(COOKIE).unwrap(),
            "tfNamespace=tenant-acme; tfRobotId=rid-42; tfUserToken=short-chat-token"
        );
        assert!(headers.get("authorization").is_none());
        assert_eq!(headers.get("X-TF-Route").unwrap(), "robot-42");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn bff_does_not_follow_login_redirects() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let io = TokioIo::new(stream);
            http1::Builder::new()
                .serve_connection(
                    io,
                    service_fn(|_request: Request<hyper::body::Incoming>| async move {
                        Ok::<_, std::convert::Infallible>(
                            Response::builder()
                                .status(hyper::StatusCode::TEMPORARY_REDIRECT)
                                .header(hyper::header::LOCATION, "/login")
                                .body(Full::new(Bytes::from_static(b"redirecting to login")))
                                .unwrap(),
                        )
                    }),
                )
                .await
                .unwrap();
        });

        let service = ChatSessionService::new(Weak::new());
        let base = Url::parse(&format!("http://{address}/proxy/")).unwrap();
        let target = test_target(base.clone(), context("account-a"));
        let (_cancel_tx, cancelled) = watch::channel(false);
        let response = service
            .proxy_authorized(
                target,
                cancelled,
                base.join("v1/chat/conversations?count=20").unwrap(),
                "GET",
                None,
                ChatSessionCredential {
                    token: "short-chat-token".into(),
                    expires_at: 0,
                },
            )
            .await
            .unwrap();

        assert_eq!(response.status, 307);
        assert_eq!(response.body, "redirecting to login");
        server.await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires an explicit staging chat credential and route context"]
    async fn staging_instance_bff_returns_a_valid_conversation_envelope() {
        let socket_base_url = std::env::var("TFRC_CHAT_SOCKET_BASE_URL").unwrap();
        let namespace = std::env::var("TFRC_CHAT_NAMESPACE").unwrap();
        let robot_id = std::env::var("TFRC_CHAT_ROBOT_ID").unwrap();
        let robot_type = std::env::var("TFRC_CHAT_ROBOT_TYPE").unwrap();
        let short_token = std::env::var("TFRC_CHAT_SHORT_TOKEN").unwrap();
        let info = ConnectionInfoResponse {
            socket_base_url,
            sio_path: Some("/socket.io/".into()),
            namespace: Some(namespace.clone()),
            rid: Some(robot_id.clone()),
            robot_type: Some(robot_type.clone()),
            smcp_namespace: None,
            computer_name: None,
            routing_headers: HashMap::from([
                ("X-TF-Namespace".into(), namespace),
                ("X-TF-RobotId".into(), robot_id),
                ("X-TF-RobotType".into(), robot_type),
            ]),
        };
        let endpoints = resolve_chat_endpoints(&info).unwrap();
        let target = ResolvedChatTarget {
            context_key: context("staging-account"),
            manager_generation: 1,
            employee_id: 1,
            robot_account_id: "staging-robot-account".into(),
            robot_name: "staging-robot".into(),
            http_base_url: endpoints.http_base_url.clone(),
            socket_namespace_url: endpoints.socket_namespace_url,
            socket_path: endpoints.socket_path,
            frontend_namespace: endpoints.frontend_namespace,
            frontend_robot_id: endpoints.frontend_robot_id,
            routing_headers: info.routing_headers,
        };
        let service = ChatSessionService::new(Weak::new());
        let (_cancel_tx, cancelled) = watch::channel(false);
        let response = service
            .proxy_authorized(
                target,
                cancelled,
                endpoints
                    .http_base_url
                    .join("v1/chat/conversations?count=1")
                    .unwrap(),
                "GET",
                None,
                ChatSessionCredential {
                    token: short_token,
                    expires_at: 0,
                },
            )
            .await
            .unwrap();
        let body: serde_json::Value = serde_json::from_str(&response.body).unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(body["code"], 200);
        assert!(body["message"].is_string());
        assert!(body["data"]["conversations"].is_array());
    }
}
