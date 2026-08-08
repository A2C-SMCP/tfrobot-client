use super::*;
use a2c_smcp::smcp_computer::oauth::{
    OAuthBeginRequest, OAuthCallback, OAuthCancellation, OAuthCancellationReason, OAuthError,
    OAuthFlow, OAuthFlowOutcome, OAuthStatus,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use tokio::time::{timeout_at, Instant};
use url::Url;

const CALLBACK_PATH: &str = "/oauth/callback";
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const CALLBACK_READ_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_CALLBACK_REQUEST_BYTES: usize = 8 * 1024;

static NEXT_FLOW_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Default)]
pub(super) struct OAuthRequiredScopeCache {
    scopes: HashMap<BundleId, String>,
}

impl OAuthRequiredScopeCache {
    pub(super) fn required_scope(&self, bundle_id: &BundleId) -> Option<String> {
        self.scopes.get(bundle_id).cloned()
    }

    fn apply_status(&mut self, bundle_id: &BundleId, status: Option<&OAuthStatus>) {
        match status {
            Some(OAuthStatus::ReauthorizationRequired { required_scope }) => {
                self.scopes
                    .insert(bundle_id.clone(), required_scope.clone());
            }
            _ => {
                self.scopes.remove(bundle_id);
            }
        }
    }

    pub(super) fn apply_public_status(&mut self, bundle_id: &BundleId, status: &PublicOAuthStatus) {
        match status {
            PublicOAuthStatus::ReauthorizationRequired { required_scope } => {
                self.scopes
                    .insert(bundle_id.clone(), required_scope.clone());
            }
            _ => {
                self.scopes.remove(bundle_id);
            }
        }
    }

    pub(super) fn remove(&mut self, bundle_id: &BundleId) {
        self.scopes.remove(bundle_id);
    }

    pub(super) fn clear(&mut self) {
        self.scopes.clear();
    }

    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.scopes.is_empty()
    }
}

pub(super) struct ActiveOAuthFlow {
    id: u64,
    sdk_flow: Option<OAuthFlow>,
    listener_cancel: Option<oneshot::Sender<()>>,
    phase: OAuthFlowPhase,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum OAuthFlowPhase {
    Preparing,
    WaitingForCallback,
    Completing,
}

enum CallbackResult {
    Complete(OAuthCallback),
    ProviderCancel {
        issuer: Option<String>,
        reason: OAuthCancellationReason,
    },
    Timeout,
}

enum ParsedCallback {
    Ignore,
    Terminal(CallbackResult),
}

struct OAuthCallbackDriver {
    flow_id: u64,
    expected_state: String,
    sdk_flow: OAuthFlow,
    listener: TcpListener,
    cancelled: oneshot::Receiver<()>,
}

fn remove_current_flow(
    flows: &mut HashMap<BundleId, ActiveOAuthFlow>,
    bundle_id: &BundleId,
    flow_id: u64,
) -> Option<ActiveOAuthFlow> {
    if flows.get(bundle_id).is_some_and(|flow| flow.id == flow_id) {
        flows.remove(bundle_id)
    } else {
        None
    }
}

impl ComputerInstanceRuntime {
    pub async fn mcp_server_has_oauth(&self, bundle_id: &BundleId) -> bool {
        self.configured_oauth_server(bundle_id).await.is_some()
    }

    pub async fn mcp_server_oauth_is_interactive(&self, bundle_id: &BundleId) -> Option<bool> {
        let MCPServerConfig::Http(config) = self.configured_oauth_server(bundle_id).await? else {
            return None;
        };
        let effective = effective_http_oauth(&config)?;
        if effective.automatic {
            // Auto is not OAuth merely because discovery overrides are present. Only expose the
            // authorization action after start validated a Bearer challenge and the SDK admitted
            // an OAuth coordinator. Public HTTP and unrelated 401 failures remain non-OAuth.
            return match self.computer.read().await.oauth_status(bundle_id).await {
                Ok(_) => Some(effective.interactive),
                Err(OAuthError::NotConfigured) => None,
                Err(_) => Some(effective.interactive),
            };
        }
        Some(effective.interactive)
    }

    pub async fn oauth_status(&self, bundle_id: &BundleId) -> Result<Option<OAuthStatus>, String> {
        let status_result = match self.computer.read().await.oauth_status(bundle_id).await {
            Ok(status) => Ok(Some(status)),
            Err(OAuthError::NotConfigured) => {
                let Some(config) = self.configured_oauth_server(bundle_id).await else {
                    self.oauth_required_scopes.write().await.remove(bundle_id);
                    return Ok(None);
                };
                match self.transient_oauth_manager(config).await {
                    Ok(manager) => manager
                        .oauth_status(bundle_id)
                        .await
                        .map(Some)
                        .map_err(|error| error.to_string()),
                    Err(error) => Err(error),
                }
            }
            Err(error) => Err(error.to_string()),
        };
        if let Ok(Some(status)) = &status_result {
            self.observe_oauth_status(bundle_id, status).await;
        }
        status_result
    }

    async fn configured_oauth_server(&self, bundle_id: &BundleId) -> Option<MCPServerConfig> {
        self.computer
            .read()
            .await
            .list_mcp_servers()
            .await
            .into_iter()
            .find(|config| {
                resolve_bundle_id(config) == *bundle_id
                    && matches!(config, MCPServerConfig::Http(http) if effective_http_oauth(http).is_some())
            })
    }

    async fn transient_oauth_manager(
        &self,
        config: MCPServerConfig,
    ) -> Result<MCPServerManager, String> {
        let manager =
            MCPServerManager::with_oauth_credential_store(self.oauth_credential_store.clone());
        manager
            .initialize(vec![config])
            .await
            .map_err(|error| error.to_string())?;
        Ok(manager)
    }

    /// Starts an OAuth flow after binding the loopback callback listener. The returned URL is
    /// backend-only and must be passed directly to the system opener, never to frontend state.
    pub async fn begin_oauth_authorization(&self, bundle_id: &BundleId) -> Result<String, String> {
        self.ensure_active()?;
        let flow_id = NEXT_FLOW_ID.fetch_add(1, Ordering::Relaxed);
        let listener = match TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await {
            Ok(listener) => listener,
            Err(_) => return Err("Unable to bind the OAuth callback listener".to_string()),
        };
        let address = match listener.local_addr() {
            Ok(address) => address,
            Err(_) => return Err("Unable to resolve the OAuth callback listener".to_string()),
        };
        let redirect_uri = format!("http://127.0.0.1:{}{CALLBACK_PATH}", address.port());
        // Server-local mutation holds this same fence from pre-cancellation through SDK
        // mount/unmount. Admission and SDK handle registration therefore happen entirely before
        // or after the mutation, never in the gap after its one-time cancellation sweep.
        let oauth_server_guard = self.oauth_server_lifecycle_lock.lock().await;
        self.ensure_active()?;
        let begin_request = self.oauth_begin_request(bundle_id, redirect_uri).await;
        let (listener_cancel, mut cancelled) = oneshot::channel();
        {
            let mut flows = self.oauth_flows.lock().await;
            if !self.oauth_admission_open.load(Ordering::Acquire)
                || self
                    .oauth_server_change_admission_blocks
                    .load(Ordering::Acquire)
                    != 0
            {
                return Err(
                    "Authorization is unavailable while the runtime is restarting".to_string(),
                );
            }
            if flows.contains_key(bundle_id) {
                return Err("Authorization is already pending for this MCP server".to_string());
            }
            flows.insert(
                bundle_id.clone(),
                ActiveOAuthFlow {
                    id: flow_id,
                    sdk_flow: None,
                    listener_cancel: Some(listener_cancel),
                    phase: OAuthFlowPhase::Preparing,
                },
            );
        }

        // `create_oauth_flow` registers a host-owned cancellation handle before provider I/O. The
        // handle, rather than an eventual OAuth state string, is the lifecycle ownership boundary.
        let sdk_flow = match self
            .computer
            .read()
            .await
            .create_oauth_flow(bundle_id, begin_request)
            .await
        {
            Ok(flow) => flow,
            Err(error) => {
                self.remove_oauth_flow_if_current(bundle_id, flow_id).await;
                return Err(error.to_string());
            }
        };

        let registered = {
            let mut flows = self.oauth_flows.lock().await;
            flows.get_mut(bundle_id).is_some_and(|flow| {
                if flow.id != flow_id || flow.phase != OAuthFlowPhase::Preparing {
                    return false;
                }
                flow.sdk_flow = Some(sdk_flow.clone());
                true
            })
        };
        if !registered {
            drop(oauth_server_guard);
            let _ = sdk_flow.cancel(OAuthCancellationReason::Cancelled).await;
            return Err("Authorization was cancelled".to_string());
        }
        drop(oauth_server_guard);

        let launch = tokio::select! {
            _ = &mut cancelled => {
                let _ = sdk_flow.cancel(OAuthCancellationReason::Cancelled).await;
                self.remove_oauth_flow_if_current(bundle_id, flow_id).await;
                return Err("Authorization was cancelled".to_string());
            }
            result = sdk_flow.launch() => match result {
                Ok(launch) => launch,
                Err(error) => {
                    self.remove_oauth_flow_if_current(bundle_id, flow_id).await;
                    return Err(error.to_string());
                }
            }
        };

        let transitioned = {
            let mut flows = self.oauth_flows.lock().await;
            flows.get_mut(bundle_id).is_some_and(|flow| {
                if flow.id != flow_id || flow.phase != OAuthFlowPhase::Preparing {
                    return false;
                }
                flow.phase = OAuthFlowPhase::WaitingForCallback;
                true
            })
        };
        if !transitioned {
            let _ = sdk_flow.cancel(OAuthCancellationReason::Cancelled).await;
            return Err("Authorization was cancelled".to_string());
        }

        let runtime = self.clone();
        let flow_bundle_id = bundle_id.clone();
        let expected_state = launch.state;
        tokio::spawn(async move {
            runtime
                .drive_oauth_callback(
                    flow_bundle_id,
                    OAuthCallbackDriver {
                        flow_id,
                        expected_state,
                        sdk_flow,
                        listener,
                        cancelled,
                    },
                )
                .await;
        });

        Ok(launch.authorization_url)
    }

    pub async fn cancel_oauth_authorization(&self, bundle_id: &BundleId) -> Result<(), String> {
        let (flow_id, sdk_flow, listener_cancel) = {
            let mut flows = self.oauth_flows.lock().await;
            let Some(flow) = flows.get_mut(bundle_id) else {
                return Ok(());
            };
            flow.phase = OAuthFlowPhase::Completing;
            (flow.id, flow.sdk_flow.clone(), flow.listener_cancel.take())
        };

        let Some(sdk_flow) = sdk_flow else {
            if let Some(listener_cancel) = listener_cancel {
                let _ = listener_cancel.send(());
            }
            self.remove_oauth_flow_if_current(bundle_id, flow_id).await;
            return Ok(());
        };

        // The detached SDK cancellation task owns terminalization and local map cleanup even if a
        // frontend invoke is dropped. All clones converge on the SDK's single terminal outcome.
        let runtime = self.clone();
        let flow_bundle_id = bundle_id.clone();
        let cancellation = tokio::spawn(async move {
            let result = sdk_flow
                .cancel(OAuthCancellationReason::Cancelled)
                .await
                .map_err(|error| error.to_string());
            if let Ok(outcome) = result.as_ref() {
                runtime
                    .observe_oauth_outcome(&flow_bundle_id, outcome)
                    .await;
            }
            runtime
                .remove_oauth_flow_if_current(&flow_bundle_id, flow_id)
                .await;
            result.map(|_| ())
        });
        if let Some(listener_cancel) = listener_cancel {
            let _ = listener_cancel.send(());
        }
        cancellation
            .await
            .map_err(|error| format!("OAuth cancellation task failed: {error}"))?
    }

    /// Retires client-owned callback state before the SDK replaces or removes one MCP server.
    /// Credentials are intentionally retained; identity-changing configuration paths clear them
    /// separately before persistence.
    pub(super) async fn cancel_oauth_before_server_lifecycle_change(
        &self,
        bundle_id: &BundleId,
    ) -> ComputerResult<()> {
        self.cancel_oauth_authorization(bundle_id)
            .await
            .map_err(ComputerError::RuntimeError)?;
        self.oauth_required_scopes.write().await.remove(bundle_id);
        Ok(())
    }

    pub async fn clear_oauth_authorization(&self, bundle_id: &BundleId) -> Result<(), String> {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;
        let _oauth_server_guard = self.oauth_server_lifecycle_lock.lock().await;
        self.ensure_active()?;
        self.clear_oauth_authorization_inner(bundle_id).await
    }

    pub(super) async fn clear_oauth_authorization_inner(
        &self,
        bundle_id: &BundleId,
    ) -> Result<(), String> {
        if !self.mcp_server_has_oauth(bundle_id).await {
            return Ok(());
        }
        let _admission_guard = self.block_oauth_admission_for_server_change().await;
        self.cancel_oauth_authorization(bundle_id).await?;
        self.stop_mcp_server_if_running(bundle_id).await?;
        let result = match self.computer.read().await.clear_oauth(bundle_id).await {
            Ok(()) => Ok(()),
            Err(OAuthError::NotConfigured) => {
                let Some(config) = self.configured_oauth_server(bundle_id).await else {
                    return Ok(());
                };
                let Some(config) =
                    crate::services::oauth_credential_store::oauth_cleanup_config(config)
                else {
                    return Ok(());
                };
                self.transient_oauth_manager(config)
                    .await?
                    .clear_oauth(bundle_id)
                    .await
                    .map_err(|error| error.to_string())
            }
            Err(error) => Err(error.to_string()),
        };
        if result.is_ok() {
            self.oauth_required_scopes.write().await.remove(bundle_id);
        }
        result
    }

    pub(crate) async fn clear_oauth_for_server_config(
        &self,
        config: MCPServerConfig,
    ) -> Result<(), String> {
        let bundle_id = resolve_bundle_id(&config);
        let Some(config) = crate::services::oauth_credential_store::oauth_cleanup_config(config)
        else {
            return Ok(());
        };
        let _admission_guard = self.block_oauth_admission_for_server_change().await;
        self.cancel_oauth_authorization(&bundle_id).await?;
        self.stop_mcp_server_if_running(&bundle_id).await?;
        let result = self
            .transient_oauth_manager(config)
            .await?
            .clear_oauth(&bundle_id)
            .await
            .map_err(|error| error.to_string());
        if result.is_ok() {
            self.oauth_required_scopes.write().await.remove(&bundle_id);
        }
        result
    }

    async fn stop_mcp_server_if_running(&self, bundle_id: &BundleId) -> Result<(), String> {
        let running = self
            .computer
            .read()
            .await
            .get_server_status()
            .await
            .into_iter()
            .any(|(id, _, running, _)| id == *bundle_id && running);
        if running {
            self.computer
                .read()
                .await
                .stop_mcp_client(bundle_id)
                .await
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    pub(super) async fn cancel_all_oauth_authorizations(&self) {
        let bundle_ids = self
            .oauth_flows
            .lock()
            .await
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for bundle_id in bundle_ids {
            if let Err(error) = self.cancel_oauth_authorization(&bundle_id).await {
                log::warn!(
                    "Failed to cancel pending MCP OAuth authorization during runtime teardown: {}",
                    error
                );
            }
        }
        self.oauth_required_scopes.write().await.clear();
        // A handle replacement is the final ownership boundary for these SDK flow states. Any
        // failed terminal cleanup belongs to the retiring Computer and must not gate authorization
        // on its replacement; all listeners/tasks have already been signalled or joined above.
        self.oauth_flows.lock().await.clear();
    }

    pub(super) async fn close_oauth_admission(&self) {
        let _flows = self.oauth_flows.lock().await;
        self.oauth_admission_open.store(false, Ordering::Release);
    }

    /// Blocks new OAuth reservations while a durable server identity or credential transaction is
    /// in progress. The counter makes nested runtime helpers safe: an inner cleanup cannot reopen
    /// admission before its outer config/plugin transaction commits.
    pub(crate) async fn block_oauth_admission_for_server_change(
        &self,
    ) -> OAuthServerChangeAdmissionGuard {
        let _flows = self.oauth_flows.lock().await;
        self.oauth_server_change_admission_blocks
            .fetch_add(1, Ordering::AcqRel);
        OAuthServerChangeAdmissionGuard(self.oauth_server_change_admission_blocks.clone())
    }

    async fn drive_oauth_callback(&self, bundle_id: BundleId, driver: OAuthCallbackDriver) {
        let OAuthCallbackDriver {
            flow_id,
            expected_state,
            sdk_flow,
            listener,
            mut cancelled,
        } = driver;
        let Some(terminal) =
            await_callback(&listener, &expected_state, CALLBACK_TIMEOUT, &mut cancelled).await
        else {
            return;
        };
        drop(listener);

        if !self
            .claim_oauth_callback_if_current(&bundle_id, flow_id)
            .await
        {
            return;
        }

        let result = match terminal {
            CallbackResult::Complete(callback) => sdk_flow.complete(callback).await,
            CallbackResult::ProviderCancel { issuer, reason } => {
                sdk_flow
                    .cancel_callback(OAuthCancellation {
                        state: expected_state,
                        issuer,
                        reason,
                    })
                    .await
            }
            CallbackResult::Timeout => sdk_flow.cancel(OAuthCancellationReason::Timeout).await,
        };

        if let Ok(outcome) = result.as_ref() {
            self.observe_oauth_outcome(&bundle_id, outcome).await;
        }
        self.remove_oauth_flow_if_current(&bundle_id, flow_id).await;
        match result {
            Ok(OAuthFlowOutcome::Authorized { .. }) => {
                // Token persistence has committed. Start failures remain on the existing MCP
                // diagnostic path and never roll credentials back.
                let _ = self.start_mcp_server(&bundle_id).await;
            }
            Ok(OAuthFlowOutcome::Terminated { .. }) => {}
            Err(error) => {
                // State/issuer validation errors do not consume an SDK flow. The listener has
                // already accepted a terminal callback, so explicitly retire the unusable flow.
                if let Err(cleanup_error) =
                    sdk_flow.cancel(OAuthCancellationReason::Cancelled).await
                {
                    log::warn!(
                        "MCP OAuth flow could not be retired after callback failure: {}",
                        cleanup_error
                    );
                }
                log::warn!("MCP OAuth callback could not be completed: {}", error);
            }
        }
    }

    async fn remove_oauth_flow_if_current(
        &self,
        bundle_id: &BundleId,
        flow_id: u64,
    ) -> Option<ActiveOAuthFlow> {
        let mut flows = self.oauth_flows.lock().await;
        remove_current_flow(&mut flows, bundle_id, flow_id)
    }

    async fn claim_oauth_callback_if_current(&self, bundle_id: &BundleId, flow_id: u64) -> bool {
        let mut flows = self.oauth_flows.lock().await;
        flows.get_mut(bundle_id).is_some_and(|flow| {
            if flow.id != flow_id || flow.phase != OAuthFlowPhase::WaitingForCallback {
                return false;
            }
            flow.phase = OAuthFlowPhase::Completing;
            flow.listener_cancel = None;
            true
        })
    }

    pub(super) async fn observe_oauth_status(&self, bundle_id: &BundleId, status: &OAuthStatus) {
        self.oauth_required_scopes
            .write()
            .await
            .apply_status(bundle_id, Some(status));
    }

    async fn observe_oauth_outcome(&self, bundle_id: &BundleId, outcome: &OAuthFlowOutcome) {
        match outcome {
            OAuthFlowOutcome::Authorized { scopes } => {
                self.observe_oauth_status(
                    bundle_id,
                    &OAuthStatus::Authorized {
                        scopes: scopes.clone(),
                    },
                )
                .await;
            }
            OAuthFlowOutcome::Terminated { status, .. } => {
                self.observe_oauth_status(bundle_id, status).await;
            }
        }
    }

    async fn oauth_begin_request(
        &self,
        bundle_id: &BundleId,
        redirect_uri: String,
    ) -> OAuthBeginRequest {
        OAuthBeginRequest {
            redirect_uri,
            required_scope: self
                .oauth_required_scopes
                .read()
                .await
                .required_scope(bundle_id),
        }
    }
}

async fn await_callback(
    listener: &TcpListener,
    expected_state: &str,
    callback_timeout: Duration,
    cancelled: &mut oneshot::Receiver<()>,
) -> Option<CallbackResult> {
    let deadline = Instant::now() + callback_timeout;
    loop {
        let accepted = tokio::select! {
            _ = &mut *cancelled => return None,
            result = timeout_at(deadline, listener.accept()) => result,
        };
        let Ok(Ok((mut stream, _peer))) = accepted else {
            return Some(CallbackResult::Timeout);
        };
        match read_callback(&mut stream, expected_state).await {
            ParsedCallback::Ignore => continue,
            ParsedCallback::Terminal(result) => return Some(result),
        }
    }
}

async fn read_callback(stream: &mut TcpStream, expected_state: &str) -> ParsedCallback {
    let deadline = Instant::now() + CALLBACK_READ_TIMEOUT;
    let mut bytes = Vec::with_capacity(1024);
    loop {
        let mut chunk = [0_u8; 1024];
        let Ok(Ok(read)) = timeout_at(deadline, stream.read(&mut chunk)).await else {
            let _ = respond(stream, 408, "Authorization callback timed out").await;
            return ParsedCallback::Ignore;
        };
        if read == 0 {
            let _ = respond(stream, 400, "Invalid authorization callback").await;
            return ParsedCallback::Ignore;
        }
        bytes.extend_from_slice(&chunk[..read]);
        if bytes.len() > MAX_CALLBACK_REQUEST_BYTES {
            let _ = respond(stream, 400, "Authorization callback was too large").await;
            return ParsedCallback::Ignore;
        }
        if bytes.windows(2).any(|window| window == b"\r\n") || bytes.contains(&b'\n') {
            break;
        }
    }
    let Ok(request) = std::str::from_utf8(&bytes) else {
        let _ = respond(stream, 400, "Invalid authorization callback").await;
        return ParsedCallback::Ignore;
    };
    let Some(request_line) = request.lines().next() else {
        let _ = respond(stream, 400, "Invalid authorization callback").await;
        return ParsedCallback::Ignore;
    };
    let mut request_parts = request_line.split_whitespace();
    let (Some(method), Some(target), Some(_version), None) = (
        request_parts.next(),
        request_parts.next(),
        request_parts.next(),
        request_parts.next(),
    ) else {
        let _ = respond(stream, 400, "Invalid authorization callback").await;
        return ParsedCallback::Ignore;
    };
    if method != "GET" {
        let _ = respond(stream, 405, "Only GET callbacks are accepted").await;
        return ParsedCallback::Ignore;
    }
    let Ok(url) = Url::parse(&format!("http://127.0.0.1{target}")) else {
        let _ = respond(stream, 400, "Invalid authorization callback").await;
        return ParsedCallback::Ignore;
    };
    if url.path() != CALLBACK_PATH {
        let _ = respond(stream, 404, "Not found").await;
        return ParsedCallback::Ignore;
    }

    let mut code = None;
    let mut state = None;
    let mut issuer = None;
    let mut provider_error = None;
    for (key, value) in url.query_pairs() {
        let slot = match key.as_ref() {
            "code" => &mut code,
            "state" => &mut state,
            "iss" => &mut issuer,
            "error" => &mut provider_error,
            _ => continue,
        };
        if slot.replace(value.into_owned()).is_some() {
            let _ = respond(stream, 400, "Duplicate authorization callback parameter").await;
            return ParsedCallback::Ignore;
        }
    }
    if state.as_deref() != Some(expected_state) {
        let _ = respond(stream, 400, "Authorization callback state did not match").await;
        return ParsedCallback::Ignore;
    }
    if let Some(error) = provider_error {
        let reason = if error == "access_denied" {
            OAuthCancellationReason::AccessDenied
        } else {
            OAuthCancellationReason::AuthorizationError
        };
        let _ = respond(
            stream,
            200,
            "Authorization was not completed. You may close this window.",
        )
        .await;
        return ParsedCallback::Terminal(CallbackResult::ProviderCancel { issuer, reason });
    }
    let Some(code) = code else {
        let _ = respond(stream, 400, "Authorization code was missing").await;
        return ParsedCallback::Ignore;
    };
    let _ = respond(
        stream,
        200,
        "Authorization callback received. Authorization is still completing; you may close this window.",
    )
    .await;
    ParsedCallback::Terminal(CallbackResult::Complete(OAuthCallback {
        code,
        state: state.expect("state was validated"),
        issuer,
    }))
}

async fn respond(stream: &mut TcpStream, status: u16, message: &str) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        _ => "Error",
    };
    let body =
        format!("<!doctype html><meta charset=\"utf-8\"><title>OAuth</title><p>{message}</p>");
    let response = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[cfg(target_os = "macos")]
    #[ignore = "requires interactive Atlassian OAuth in a system browser"]
    async fn atlassian_automatic_oauth_lifecycle_e2e() {
        use a2c_smcp::smcp_computer::mcp_clients::model::{
            HttpAuthPolicy, HttpServerConfig, HttpServerParameters,
        };
        use a2c_smcp::smcp_computer::oauth::{
            OAuthClientMode, OAuthClientRegistration, OAuthOptions,
        };
        use std::process::{Command, Stdio};

        const BUNDLE: &str = "atlassian-automatic-oauth-e2e";
        const RESOURCE_TOOL: &str = "getAccessibleAtlassianResources";
        const JQL_TOOL: &str = "searchJiraIssuesUsingJql";
        const SEARCH_TOOLS: &[&str] = &["search", "searchAtlassian"];

        println!("ATLASSIAN_E2E: START automatic-discovery-empty-scopes");
        let directory = tempfile::tempdir().unwrap();
        let runtime = ComputerInstanceRuntime::new(
            ComputerInstance::new("oauth-host-e2e", "OAuth Host E2E"),
            directory.path().join("skills"),
        );
        runtime.start().await.unwrap();
        let bundle_id = BundleId::try_from(BUNDLE).unwrap();
        let mut http = HttpServerConfig::new(
            "Atlassian automatic OAuth E2E",
            HttpServerParameters {
                url: "https://mcp.atlassian.com/v1/mcp/authv2".to_string(),
                headers: HashMap::new(),
            },
        );
        http.bundle_id = Some(bundle_id.clone());
        http.auth_policy = Some(HttpAuthPolicy::Auto);
        http.oauth = Some(OAuthOptions {
            resource: None,
            scopes: Vec::new(),
            client_name: Some("TFRobot".to_string()),
            mode: OAuthClientMode::AuthorizationCode {
                registration: OAuthClientRegistration::Dynamic,
            },
        });
        runtime
            .apply_user_mcp_server_config(MCPServerConfig::Http(http))
            .await
            .unwrap();

        assert_eq!(
            runtime.mcp_server_oauth_is_interactive(&bundle_id).await,
            Some(true)
        );
        assert!(matches!(
            runtime.oauth_status(&bundle_id).await.unwrap(),
            Some(OAuthStatus::Unauthorized)
        ));
        assert!(!runtime
            .mcp_start_diagnostics()
            .await
            .contains_key(&bundle_id));
        assert!(runtime
            .available_tools()
            .await
            .unwrap_or_default()
            .iter()
            .all(|tool| !tool.name.as_ref().starts_with(BUNDLE)));
        println!("ATLASSIAN_E2E: PASS phase=before-authorization tools=unavailable");

        let authorization_url = runtime.begin_oauth_authorization(&bundle_id).await.unwrap();
        let browser = Command::new("open")
            .arg(&authorization_url)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap();
        drop(authorization_url);
        assert!(browser.success(), "system browser did not open");
        println!("ATLASSIAN_E2E: WAIT browser-authorization");

        let authorization_deadline = Instant::now() + Duration::from_secs(5 * 60);
        loop {
            let status = runtime.oauth_status(&bundle_id).await.unwrap();
            if matches!(status, Some(OAuthStatus::Authorized { .. })) {
                break;
            }
            assert!(
                !matches!(status, Some(OAuthStatus::Error { .. })),
                "Atlassian authorization entered an error state: {status:?}"
            );
            assert!(
                Instant::now() < authorization_deadline,
                "Atlassian authorization callback did not complete: {status:?}"
            );
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        println!("ATLASSIAN_E2E: PASS phase=authorization-callback");

        let exposed_resource_tool = format!("{BUNDLE}__{RESOURCE_TOOL}");
        let tools_deadline = Instant::now() + Duration::from_secs(30);
        let tools = loop {
            if let Ok(tools) = runtime.available_tools().await {
                if tools
                    .iter()
                    .any(|tool| tool.name.as_ref() == exposed_resource_tool)
                {
                    break tools;
                }
            }
            assert!(
                Instant::now() < tools_deadline,
                "host did not expose Atlassian tools after authorization; diagnostics={:?}",
                runtime.mcp_start_diagnostics().await
            );
            tokio::time::sleep(Duration::from_millis(250)).await;
        };

        assert!(matches!(
            runtime.oauth_status(&bundle_id).await.unwrap(),
            Some(OAuthStatus::Authorized { .. })
        ));
        println!("ATLASSIAN_E2E: PASS phase=authorized tools={}", tools.len());

        let resource_result = runtime
            .computer
            .read()
            .await
            .execute_tool_cancellable(
                "atlassian-oauth-e2e-resources",
                &exposed_resource_tool,
                serde_json::json!({}),
                Some(30.0),
            )
            .await
            .unwrap();
        assert_ne!(
            resource_result.is_error,
            Some(true),
            "Atlassian resource discovery failed"
        );
        let cloud_id = extract_atlassian_cloud_id(&resource_result)
            .expect("resource response did not contain a cloud id");
        println!("ATLASSIAN_E2E: PASS tool={RESOURCE_TOOL}");

        let exposed_jql_tool = format!("{BUNDLE}__{JQL_TOOL}");
        assert!(
            tools
                .iter()
                .any(|tool| tool.name.as_ref() == exposed_jql_tool),
            "JQL search tool was not discovered"
        );
        let jql_result = runtime
            .computer
            .read()
            .await
            .execute_tool_cancellable(
                "atlassian-oauth-e2e-jql",
                &exposed_jql_tool,
                serde_json::json!({
                    "cloudId": cloud_id,
                    "jql": "key = TFROBOT-E2E-0"
                }),
                Some(30.0),
            )
            .await
            .unwrap();

        let exposed_search_tool = SEARCH_TOOLS
            .iter()
            .map(|tool| format!("{BUNDLE}__{tool}"))
            .find(|name| tools.iter().any(|tool| tool.name.as_ref() == name))
            .expect("generic Atlassian search tool was not discovered");
        let search_result = runtime
            .computer
            .read()
            .await
            .execute_tool_cancellable(
                "atlassian-oauth-e2e-search",
                &exposed_search_tool,
                serde_json::json!({
                    "cloudId": cloud_id,
                    "query": "tfrobot-e2e-deliberately-nonexistent-query-7a6f0a"
                }),
                Some(30.0),
            )
            .await
            .unwrap();
        assert_ne!(
            jql_result.is_error,
            Some(true),
            "authorized JQL search returned an error"
        );
        assert_ne!(
            search_result.is_error,
            Some(true),
            "authorized generic search returned an error"
        );
        println!("ATLASSIAN_E2E: PASS tool={JQL_TOOL}");
        println!("ATLASSIAN_E2E: PASS tool=generic-search");

        runtime.clear_oauth_authorization(&bundle_id).await.unwrap();
        assert!(matches!(
            runtime.oauth_status(&bundle_id).await.unwrap(),
            Some(OAuthStatus::Unauthorized)
        ));
        let post_clear_call_is_unavailable = match runtime
            .computer
            .read()
            .await
            .execute_tool_cancellable(
                "atlassian-oauth-e2e-after-clear",
                &exposed_resource_tool,
                serde_json::json!({}),
                Some(30.0),
            )
            .await
        {
            Err(_) => true,
            Ok(result) => result.is_error == Some(true),
        };
        assert!(runtime
            .available_tools()
            .await
            .unwrap_or_default()
            .iter()
            .all(|tool| !tool.name.as_ref().starts_with(BUNDLE)));
        runtime.start_mcp_server(&bundle_id).await.unwrap();
        assert!(matches!(
            runtime.oauth_status(&bundle_id).await.unwrap(),
            Some(OAuthStatus::Unauthorized)
        ));
        assert!(!runtime
            .mcp_start_diagnostics()
            .await
            .contains_key(&bundle_id));
        assert!(runtime
            .available_tools()
            .await
            .unwrap_or_default()
            .iter()
            .all(|tool| !tool.name.as_ref().starts_with(BUNDLE)));
        runtime.shutdown().await;

        assert!(
            post_clear_call_is_unavailable,
            "protected tool call still succeeded after authorization was cleared"
        );
        println!("ATLASSIAN_E2E: PASS phase=after-clear tools=unavailable");
        println!("ATLASSIAN_E2E: PASS automatic-discovery-empty-scopes");
    }

    fn extract_atlassian_cloud_id(result: &CallToolResult) -> Option<String> {
        if let Some(value) = result.structured_content.as_ref() {
            if let Some(id) = find_atlassian_cloud_id(value) {
                return Some(id);
            }
        }
        result
            .content
            .iter()
            .filter_map(|content| content.as_text())
            .find_map(|text| {
                serde_json::from_str::<serde_json::Value>(&text.text)
                    .ok()
                    .and_then(|value| find_atlassian_cloud_id(&value))
            })
    }

    fn find_atlassian_cloud_id(value: &serde_json::Value) -> Option<String> {
        match value {
            serde_json::Value::Array(items) => items.iter().find_map(find_atlassian_cloud_id),
            serde_json::Value::Object(fields) => fields
                .get("cloudId")
                .or_else(|| fields.get("id"))
                .and_then(serde_json::Value::as_str)
                .map(ToString::to_string)
                .or_else(|| fields.values().find_map(find_atlassian_cloud_id)),
            _ => None,
        }
    }

    async fn parse(request_target: &str, expected_state: &str) -> ParsedCallback {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let request = format!("GET {request_target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
        let client = tokio::spawn(async move {
            let mut stream = TcpStream::connect(address).await.unwrap();
            stream.write_all(request.as_bytes()).await.unwrap();
        });
        let (mut server, _) = listener.accept().await.unwrap();
        let result = read_callback(&mut server, expected_state).await;
        client.await.unwrap();
        result
    }

    #[tokio::test]
    async fn accepts_unique_valid_callback() {
        let result = parse(
            "/oauth/callback?code=secret&state=expected&iss=https%3A%2F%2Fissuer",
            "expected",
        )
        .await;
        assert!(matches!(
            result,
            ParsedCallback::Terminal(CallbackResult::Complete(_))
        ));
    }

    #[tokio::test]
    async fn rejects_wrong_path_duplicate_or_mismatched_state_without_terminating() {
        assert!(matches!(
            parse("/wrong?code=a&state=expected", "expected").await,
            ParsedCallback::Ignore
        ));
        assert!(matches!(
            parse("/oauth/callback?code=a&code=b&state=expected", "expected").await,
            ParsedCallback::Ignore
        ));
        assert!(matches!(
            parse("/oauth/callback?code=a&state=wrong", "expected").await,
            ParsedCallback::Ignore
        ));
    }

    #[tokio::test]
    async fn maps_provider_denial_to_structured_cancellation() {
        let result = parse(
            "/oauth/callback?error=access_denied&state=expected",
            "expected",
        )
        .await;
        assert!(matches!(
            result,
            ParsedCallback::Terminal(CallbackResult::ProviderCancel {
                reason: OAuthCancellationReason::AccessDenied,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn total_deadline_maps_to_timeout_cancellation() {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let (_cancel, mut cancelled) = oneshot::channel();
        let result = await_callback(
            &listener,
            "expected",
            Duration::from_millis(10),
            &mut cancelled,
        )
        .await;
        assert!(matches!(result, Some(CallbackResult::Timeout)));
    }

    #[tokio::test]
    async fn begin_request_uses_only_latest_backend_observed_required_scope() {
        let directory = tempfile::tempdir().unwrap();
        let runtime = ComputerInstanceRuntime::new(
            ComputerInstance::new("scope-cache", "Scope Cache"),
            directory.path().join("skills"),
        );
        let bundle_id = BundleId::try_from("protected").unwrap();

        runtime
            .observe_oauth_status(
                &bundle_id,
                &OAuthStatus::ReauthorizationRequired {
                    required_scope: "tools.read".to_string(),
                },
            )
            .await;
        runtime
            .observe_oauth_status(
                &bundle_id,
                &OAuthStatus::ReauthorizationRequired {
                    required_scope: "tools.write".to_string(),
                },
            )
            .await;
        assert_eq!(
            runtime
                .oauth_begin_request(
                    &bundle_id,
                    "http://127.0.0.1:1234/oauth/callback".to_string(),
                )
                .await
                .required_scope
                .as_deref(),
            Some("tools.write")
        );

        runtime
            .observe_oauth_status(
                &bundle_id,
                &OAuthStatus::Authorized {
                    scopes: vec!["tools.write".to_string()],
                },
            )
            .await;
        assert!(runtime
            .oauth_begin_request(
                &bundle_id,
                "http://127.0.0.1:1234/oauth/callback".to_string(),
            )
            .await
            .required_scope
            .is_none());

        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn server_mutation_fence_blocks_late_oauth_admission_without_orphaning_flow() {
        let directory = tempfile::tempdir().unwrap();
        let runtime = ComputerInstanceRuntime::new(
            ComputerInstance::new("oauth-fence", "OAuth Fence"),
            directory.path().join("skills"),
        );
        let bundle_id = BundleId::try_from("protected").unwrap();
        let mutation_guard = runtime.oauth_server_lifecycle_lock.lock().await;
        let begin_runtime = runtime.clone();
        let begin_bundle_id = bundle_id.clone();
        let begin = tokio::spawn(async move {
            begin_runtime
                .begin_oauth_authorization(&begin_bundle_id)
                .await
        });

        tokio::time::timeout(Duration::from_millis(100), async {
            loop {
                if !runtime.oauth_flows.lock().await.is_empty() {
                    panic!("OAuth admission crossed the active server-mutation fence");
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect_err("begin must remain fenced until server mutation completes");

        drop(mutation_guard);
        assert!(begin.await.unwrap().is_err());
        assert!(runtime.oauth_flows.lock().await.is_empty());
        assert!(!runtime
            .begin_oauth_authorization(&bundle_id)
            .await
            .unwrap_err()
            .contains("already pending"));
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn nested_server_change_guards_do_not_reopen_oauth_admission_early() {
        let directory = tempfile::tempdir().unwrap();
        let runtime = ComputerInstanceRuntime::new(
            ComputerInstance::new("oauth-admission", "OAuth Admission"),
            directory.path().join("skills"),
        );
        let bundle_id = BundleId::try_from("protected").unwrap();
        let outer = runtime.block_oauth_admission_for_server_change().await;
        let inner = runtime.block_oauth_admission_for_server_change().await;
        assert_eq!(
            runtime
                .oauth_server_change_admission_blocks
                .load(Ordering::Acquire),
            2
        );

        drop(inner);
        assert_eq!(
            runtime
                .oauth_server_change_admission_blocks
                .load(Ordering::Acquire),
            1
        );
        assert!(runtime
            .begin_oauth_authorization(&bundle_id)
            .await
            .unwrap_err()
            .contains("runtime is restarting"));

        drop(outer);
        assert_eq!(
            runtime
                .oauth_server_change_admission_blocks
                .load(Ordering::Acquire),
            0
        );
        assert!(!runtime
            .begin_oauth_authorization(&bundle_id)
            .await
            .unwrap_err()
            .contains("runtime is restarting"));
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn server_lifecycle_change_invalidates_backend_required_scope() {
        let directory = tempfile::tempdir().unwrap();
        let runtime = ComputerInstanceRuntime::new(
            ComputerInstance::new("scope-lifecycle", "Scope Lifecycle"),
            directory.path().join("skills"),
        );
        let bundle_id = BundleId::try_from("protected").unwrap();
        runtime
            .observe_oauth_status(
                &bundle_id,
                &OAuthStatus::ReauthorizationRequired {
                    required_scope: "tools.write".to_string(),
                },
            )
            .await;

        runtime
            .cancel_oauth_before_server_lifecycle_change(&bundle_id)
            .await
            .unwrap();

        assert!(runtime
            .oauth_begin_request(
                &bundle_id,
                "http://127.0.0.1:1234/oauth/callback".to_string(),
            )
            .await
            .required_scope
            .is_none());
        runtime.shutdown().await;
    }

    #[tokio::test]
    async fn terminal_callback_listener_can_be_released_before_sdk_completion() {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let client = tokio::spawn(async move {
            let mut stream = TcpStream::connect(address).await.unwrap();
            stream
                .write_all(
                    b"GET /oauth/callback?code=secret&state=expected HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
                )
                .await
                .unwrap();
        });
        let (_cancel, mut cancelled) = oneshot::channel();
        assert!(matches!(
            await_callback(
                &listener,
                "expected",
                Duration::from_secs(1),
                &mut cancelled,
            )
            .await,
            Some(CallbackResult::Complete(_))
        ));
        drop(listener);
        client.await.unwrap();

        TcpListener::bind(address).await.unwrap();
    }

    #[test]
    fn stale_callback_cannot_remove_a_replacement_flow() {
        let bundle_id = BundleId::try_from("protected-server").unwrap();
        let (cancel, _cancelled) = oneshot::channel();
        let mut flows = HashMap::from([(
            bundle_id.clone(),
            ActiveOAuthFlow {
                id: 2,
                sdk_flow: None,
                listener_cancel: Some(cancel),
                phase: OAuthFlowPhase::WaitingForCallback,
            },
        )]);

        assert!(remove_current_flow(&mut flows, &bundle_id, 1).is_none());
        assert_eq!(flows.get(&bundle_id).map(|flow| flow.id), Some(2));
        assert!(remove_current_flow(&mut flows, &bundle_id, 2).is_some());
        assert!(!flows.contains_key(&bundle_id));
    }
}
