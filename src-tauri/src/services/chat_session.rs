//! In-memory chat session leases bound to the authoritative Manager Context.
//!
//! Manager credentials never cross this boundary. A lease exposes only resolved, non-secret
//! endpoints; short `chat:read chat:send` tokens are returned on demand for the browser-owned
//! Socket.IO transport and are retained only in memory.

use std::collections::HashMap;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use reqwest::header::{HeaderName, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde::Serialize;
use tokio::sync::{watch, Mutex, RwLock};
use url::Url;
use uuid::Uuid;

use crate::services::manager_client::{
    ConnectionInfoResponse, DigitalEmployeeBrief, ExchangedToken, ManagerError,
};
use crate::services::manager_context::{ManagerContextCoordinator, ManagerContextKey};

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
    routing_headers: HashMap<String, String>,
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
    token: Option<CachedToken>,
    cancelled: watch::Sender<bool>,
}

#[derive(Debug)]
pub struct ChatSessionService {
    manager_context: Weak<ManagerContextCoordinator>,
    http: reqwest::Client,
    leases: RwLock<HashMap<String, Arc<Mutex<ChatLease>>>>,
}

impl ChatSessionService {
    pub fn new(manager_context: Weak<ManagerContextCoordinator>) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(format!(
                "tfrobot-client/{} (chat-bff; {})",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS
            ))
            .pool_max_idle_per_host(0)
            .tcp_keepalive(Duration::from_secs(10))
            .timeout(CHAT_HTTP_TIMEOUT)
            .build()
            .expect("chat BFF reqwest client should build with rustls");
        Self {
            manager_context,
            http,
            leases: RwLock::new(HashMap::new()),
        }
    }

    pub async fn open(&self, employee_id: u64) -> Result<ChatSessionDescriptor, ManagerError> {
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
        let (http_base_url, socket_namespace_url, socket_path) =
            resolve_chat_endpoints(&connection_info)?;
        let exchanged = manager
            .exchange_token_for_generation(
                manager_generation,
                &robot_account_id,
                Some(CHAT_SCOPE.to_string()),
            )
            .await?;
        let lease_id = Uuid::new_v4().to_string();
        let target = ResolvedChatTarget {
            context_key,
            manager_generation,
            employee_id,
            robot_account_id,
            robot_name: employee.name,
            http_base_url,
            socket_namespace_url,
            socket_path,
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
            token: Some(CachedToken::new(exchanged)),
            cancelled: watch::channel(false).0,
        }));
        let committed_id = lease_id;
        manager
            .commit_for_authenticated_generation(manager_generation, || async move {
                self.leases
                    .write()
                    .await
                    .insert(committed_id, committed_lease);
                Ok(())
            })
            .await?;
        Ok(descriptor)
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
            let _ = lease.lock().await.cancelled.send(true);
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
        let mut request = self
            .http
            .request(request_method, url)
            .header(ACCEPT, "application/json")
            .header(AUTHORIZATION, format!("Bearer {}", credential.token));
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
        if let Some(body) = body {
            request = request.header(CONTENT_TYPE, "application/json").body(body);
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
) -> Result<(Url, String, String), ManagerError> {
    let mut http_base = Url::parse(connection_info.socket_base_url.trim()).map_err(|_| {
        ManagerError::InvalidResponse("connection-info returned an invalid socketBaseURL".into())
    })?;
    if !http_base.username().is_empty() || http_base.password().is_some() {
        return Err(ManagerError::InvalidResponse(
            "connection-info endpoint must not contain URL credentials".into(),
        ));
    }
    match http_base.scheme() {
        "http" | "https" => {}
        "ws" => http_base.set_scheme("http").map_err(|_| {
            ManagerError::InvalidResponse("connection-info returned an invalid URL scheme".into())
        })?,
        "wss" => http_base.set_scheme("https").map_err(|_| {
            ManagerError::InvalidResponse("connection-info returned an invalid URL scheme".into())
        })?,
        _ => {
            return Err(ManagerError::InvalidResponse(
                "connection-info endpoint must use HTTP(S) or WS(S)".into(),
            ))
        }
    }
    http_base.set_query(None);
    http_base.set_fragment(None);
    if !http_base.path().ends_with('/') {
        let path = format!("{}/", http_base.path().trim_end_matches('/'));
        http_base.set_path(&path);
    }

    let mut socket_namespace = http_base.clone();
    socket_namespace.set_path("/chat");
    socket_namespace.set_query(None);
    socket_namespace.set_fragment(None);
    let socket_path = connection_info
        .sio_path
        .as_deref()
        .filter(|path| !path.trim().is_empty())
        .unwrap_or("/socket.io")
        .trim()
        .to_string();
    if !socket_path.starts_with('/') || socket_path.contains("..") {
        return Err(ManagerError::InvalidResponse(
            "connection-info returned an invalid Socket.IO path".into(),
        ));
    }
    Ok((http_base, socket_namespace.to_string(), socket_path))
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

    fn test_target(http_base_url: Url, context_key: ManagerContextKey) -> ResolvedChatTarget {
        ResolvedChatTarget {
            context_key,
            manager_generation: 7,
            employee_id: 42,
            robot_account_id: "robot-account-42".into(),
            robot_name: "Robot".into(),
            http_base_url,
            socket_namespace_url: "http://127.0.0.1/chat".into(),
            socket_path: "/socket.io".into(),
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

    async fn insert_test_lease(service: &ChatSessionService, id: &str, target: ResolvedChatTarget) {
        service.leases.write().await.insert(
            id.into(),
            Arc::new(Mutex::new(ChatLease {
                target,
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
    fn endpoints_are_derived_without_credentials_or_manager_secrets() {
        let info = ConnectionInfoResponse {
            socket_base_url: "wss://robot.example.com/proxy".into(),
            sio_path: Some("/robot/socket.io".into()),
            namespace: None,
            rid: Some("rid-42".into()),
            robot_type: None,
            smcp_namespace: None,
            computer_name: None,
            routing_headers: HashMap::new(),
        };
        let (http, socket, path) = resolve_chat_endpoints(&info).unwrap();
        assert_eq!(http.as_str(), "https://robot.example.com/proxy/");
        assert_eq!(socket, "https://robot.example.com/chat");
        assert_eq!(path, "/robot/socket.io");
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
        )
        .await;
        insert_test_lease(&service, "lease-b", test_target(base, context("account-b"))).await;

        service.close_for_context(Some(&context("account-a"))).await;

        assert!(service.lease("lease-a").await.is_err());
        assert!(service.lease("lease-b").await.is_ok());
    }

    #[tokio::test]
    async fn bff_injects_only_lease_owned_auth_and_route_headers() {
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
            headers.get(AUTHORIZATION).unwrap(),
            "Bearer short-chat-token"
        );
        assert_eq!(headers.get("X-TF-Route").unwrap(), "robot-42");
        server.await.unwrap();
    }
}
