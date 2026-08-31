//! Integration tests for MCP server management through AppState.
//! These tests exercise the full flow: config persistence + SDK Computer MCP runtime.

mod common;

use a2c_smcp::smcp_computer::mcp_clients::bundle_id::resolve_bundle_id;
use a2c_smcp::smcp_computer::mcp_clients::model::{
    BundleId, MCPServerActivationState, MCPServerConnectionState,
};
use a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig;
use a2c_smcp::smcp_computer::settings::config::{
    load_project_config_doc, ProjectConfigDoc, ProvenanceScope,
};
use common::{
    create_test_app_state, echo_server_config, echo_server_path, env_server_path, mcp,
    multi_tool_server_config, slow_echo_server_config, stderr_flood_server_config,
};
use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use socketioxide::extract::{AckSender, Data, SocketRef};
use socketioxide::SocketIo;
use std::collections::HashMap;
use std::convert::Infallible;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tfrobot_client_lib::commands::connection::ConnectionState;
use tfrobot_client_lib::commands::runtime_error::RuntimeActionError;
use tfrobot_client_lib::commands::{
    client_control::{self, UpdateRemoteControlPolicyRequest},
    computer::{self, start_computer_instance_core},
    config_io,
    dashboard::get_dashboard_data_core,
    debug, inputs, sdk_config,
};
use tfrobot_client_lib::services::client_control::{RemoteControlPolicy, CLIENT_CONTROL_BUNDLE_ID};
use tfrobot_client_lib::services::computer::{ComputerInstance, McpServerManagedBy};
use tfrobot_client_lib::services::computer_runtime_events::{
    ComputerRuntimeAffectedCapability, ComputerRuntimeEventCause, ComputerRuntimeEventSink,
    ComputerRuntimeProblemMessage, ComputerRuntimeProblemSeverity, ComputerRuntimeProblemSource,
    ComputerRuntimeStatusEvent,
};
use tfrobot_client_lib::services::config::ConfigService;
use tfrobot_client_lib::services::input_value_index::{self, InputValueStorageKind};
use tfrobot_client_lib::services::input_value_store::InputValueStore;
use tfrobot_client_lib::services::keychain::{KeychainError, SecretStore};
use tfrobot_client_lib::services::observability::{
    ActivityLevel, ActivityOutcome, ActivityQuery, ActivityScopeFilter, ObservabilityService,
};
use tfrobot_client_lib::services::runtime_input_bridge::{
    RuntimeInputCompletion, RuntimeInputRequest, RuntimeInputRequestReason, RuntimeInputRequestSink,
};
use tfrobot_client_lib::services::settings::SettingsService;
use tfrobot_client_lib::AppState;
use tokio::net::TcpListener;
use tokio::time::{sleep, timeout, Duration, Instant};
use tower::service_fn;
use tower::Layer;

const TEST_INSTANCE_ID: &str = "computer-a";
const TEST_COMPUTER_NAME: &str = "Computer A";
const TEST_OFFICE_ID: &str = "office-mcp-sync";
const SERVER_JOIN_OFFICE: &str = "server:join_office";
const SERVER_UPDATE_CONFIG: &str = "server:update_config";
const SERVER_UPDATE_TOOL_LIST: &str = "server:update_tool_list";

fn bundle_id(value: &str) -> BundleId {
    BundleId::try_from(value).unwrap()
}

struct RecordingRuntimeInputSink {
    sender: tokio::sync::mpsc::UnboundedSender<RuntimeInputRequest>,
}

impl RuntimeInputRequestSink for RecordingRuntimeInputSink {
    fn emit(&self, request: &RuntimeInputRequest) -> Result<(), String> {
        self.sender
            .send(request.clone())
            .map_err(|error| error.to_string())
    }
}

fn echo_server_config_with_disabled(name: &str, disabled: bool) -> MCPServerConfig {
    let mut value = serde_json::to_value(echo_server_config(name)).unwrap();
    value["disabled"] = serde_json::Value::Bool(disabled);
    serde_json::from_value(value).unwrap()
}

fn echo_server_config_with_forbidden_tools(
    name: &str,
    forbidden_tools: &[&str],
) -> MCPServerConfig {
    let mut value = serde_json::to_value(echo_server_config(name)).unwrap();
    value["forbidden_tools"] = serde_json::json!(forbidden_tools);
    serde_json::from_value(value).unwrap()
}

fn echo_server_config_with_bundle_id(name: &str, bundle_id: &str) -> MCPServerConfig {
    let mut value = serde_json::to_value(echo_server_config(name)).unwrap();
    value["bundle_id"] = serde_json::Value::String(bundle_id.to_string());
    serde_json::from_value(value).unwrap()
}

fn unavailable_server_config(name: &str) -> MCPServerConfig {
    let mut value = serde_json::to_value(echo_server_config(name)).unwrap();
    value["server_parameters"]["command"] =
        serde_json::Value::String("tfrobot-command-that-does-not-exist".to_string());
    serde_json::from_value(value).unwrap()
}

fn delayed_start_server_config(name: &str, marker_file: &Path, delay_ms: u64) -> MCPServerConfig {
    serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": name,
        "bundle_id": name,
        "server_parameters": {
            "command": "node",
            "args": [common::slow_echo_server_path().to_str().unwrap()],
            "env": {
                "START_DELAY_MS": delay_ms.to_string(),
                "START_MARKER_FILE": marker_file.to_str().unwrap()
            }
        }
    }))
    .unwrap()
}

fn oauth_http_server_config(name: &str, endpoint: Option<&str>) -> MCPServerConfig {
    serde_json::from_value(serde_json::json!({
        "type": "streamable",
        "name": name,
        "bundle_id": name,
        "server_parameters": {
            "url": endpoint.unwrap_or("https://mcp.example.invalid/mcp")
        }
    }))
    .unwrap()
}

fn delayed_oauth_server_config(url: &str, disabled: bool) -> MCPServerConfig {
    serde_json::from_value(serde_json::json!({
        "type": "streamable",
        "name": "oauth-delayed-discovery",
        "bundle_id": "oauth-delayed-discovery",
        "disabled": disabled,
        "server_parameters": {
            "url": format!("{url}/mcp"),
            "headers": {}
        }
    }))
    .unwrap()
}

struct FailingOAuthDeleteStore;

impl SecretStore for FailingOAuthDeleteStore {
    fn set_secret(&self, _key: &str, _secret: &str) -> Result<(), KeychainError> {
        Ok(())
    }

    fn get_secret(&self, _key: &str) -> Result<Option<String>, KeychainError> {
        Ok(None)
    }

    fn delete_secret(&self, _key: &str) -> Result<(), KeychainError> {
        Err(KeychainError::Store("delete unavailable".to_string()))
    }
}

struct FailingInputWriteStore;

impl SecretStore for FailingInputWriteStore {
    fn set_secret(&self, _key: &str, _secret: &str) -> Result<(), KeychainError> {
        Err(KeychainError::Store(
            "input secret write unavailable".to_string(),
        ))
    }

    fn get_secret(&self, _key: &str) -> Result<Option<String>, KeychainError> {
        Ok(None)
    }

    fn delete_secret(&self, _key: &str) -> Result<(), KeychainError> {
        Ok(())
    }
}

fn create_config_only_state_with_store(
    path: &Path,
    secret_store: Arc<dyn SecretStore>,
) -> AppState {
    let config = ConfigService::new(path.to_path_buf()).unwrap();
    let observability = ObservabilityService::new(path).unwrap();
    let settings = SettingsService::new(path.to_path_buf());
    AppState::new_with_secret_store(config, observability, settings, secret_store)
}

async fn create_mcp_test_app_state(path: &std::path::Path) -> AppState {
    let state = create_test_app_state(path);
    if state
        .config
        .get_computer_instance(TEST_INSTANCE_ID)
        .is_err()
    {
        state
            .config
            .add_computer_instance(ComputerInstance::new(TEST_INSTANCE_ID, "Computer A"))
            .unwrap();
    }
    state
        .computer_registry
        .upsert_runtime(
            state
                .config
                .get_computer_instance(TEST_INSTANCE_ID)
                .unwrap(),
        )
        .await
        .unwrap();
    state
}

fn write_legacy_plugin_owned_mcp_profile(path: &std::path::Path) {
    let profile = serde_json::json!({
        "schema_version": 1,
        "instances": [{
            "id": TEST_INSTANCE_ID,
            "name": "Computer A",
            "mcp_servers": [{
                "config": echo_server_config("plugin-owned"),
                "managedBy": {
                    "type": "plugin",
                    "marketplace": "tf-market",
                    "plugin": "desktop-tools",
                    "pluginId": "plugin-1"
                }
            }],
            "inputs": [],
            "input_values": {},
            "connection_policy": {
                "target": null,
                "auto_connect": false
            }
        }]
    });
    std::fs::write(
        path.join("computer_instances.json"),
        serde_json::to_string_pretty(&profile).unwrap(),
    )
    .unwrap();
}

/// Panics if Node.js is not available — CI must have Node.js installed.
fn require_node() {
    let available = std::process::Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    assert!(available, "Node.js is required for MCP integration tests. CI has it configured (test.yml:87-88). If running locally without Node.js, use `cargo test --lib` to skip integration tests.");
}

#[derive(Default)]
struct SmcpSyncStats {
    join_events: AtomicUsize,
    update_config_events: AtomicUsize,
    update_tool_list_events: AtomicUsize,
    update_tool_list_computers: Mutex<Vec<String>>,
}

impl SmcpSyncStats {
    fn join_events(&self) -> usize {
        self.join_events.load(Ordering::SeqCst)
    }

    fn update_config_events(&self) -> usize {
        self.update_config_events.load(Ordering::SeqCst)
    }

    fn update_tool_list_events(&self) -> usize {
        self.update_tool_list_events.load(Ordering::SeqCst)
    }

    fn update_tool_list_computers(&self) -> Vec<String> {
        self.update_tool_list_computers.lock().unwrap().clone()
    }
}

async fn start_sync_capture_smcp_server() -> (String, Arc<SmcpSyncStats>) {
    let stats = Arc::new(SmcpSyncStats::default());
    let (socket_layer, io) = SocketIo::new_layer();

    let connect_stats = stats.clone();
    io.ns("/smcp", move |socket: SocketRef| {
        let join_stats = connect_stats.clone();
        socket.on(SERVER_JOIN_OFFICE, move |ack: AckSender| async move {
            join_stats.join_events.fetch_add(1, Ordering::SeqCst);
            let _ = ack.send(&(true, Option::<String>::None));
        });

        let config_stats = connect_stats.clone();
        socket.on(SERVER_UPDATE_CONFIG, move || {
            config_stats
                .update_config_events
                .fetch_add(1, Ordering::SeqCst);
        });

        let tool_stats = connect_stats.clone();
        socket.on(
            SERVER_UPDATE_TOOL_LIST,
            move |Data(data): Data<serde_json::Value>| {
                let computer = data["computer"]
                    .as_str()
                    .expect("tool-list update should identify its Computer")
                    .to_string();
                tool_stats
                    .update_tool_list_computers
                    .lock()
                    .unwrap()
                    .push(computer);
                tool_stats
                    .update_tool_list_events
                    .fetch_add(1, Ordering::SeqCst);
            },
        );
    });

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("local_addr"));

    tokio::spawn(async move {
        let fallback = service_fn(|_req: hyper::Request<hyper::body::Incoming>| async {
            Ok::<_, Infallible>(
                hyper::Response::builder()
                    .status(hyper::StatusCode::NOT_FOUND)
                    .body(Full::<Bytes>::from("not found"))
                    .unwrap(),
            )
        });
        let service = socket_layer.layer(fallback);

        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let service = service.clone();
            tokio::spawn(async move {
                let stream = hyper_util::rt::TokioIo::new(stream);
                let service = hyper_util::service::TowerToHyperService::new(service);
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(stream, service)
                    .with_upgrades()
                    .await;
            });
        }
    });

    (url, stats)
}

async fn wait_for_sync_event(timeout_message: &str, predicate: impl Fn() -> bool) {
    for _ in 0..120 {
        if predicate() {
            return;
        }
        sleep(Duration::from_millis(100)).await;
    }
    panic!("{timeout_message}");
}

async fn start_oauth_rejecting_mcp_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("local_addr"));

    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            tokio::spawn(async move {
                let service = service_fn(|request: hyper::Request<hyper::body::Incoming>| async {
                    let body = request
                        .into_body()
                        .collect()
                        .await
                        .expect("read request body")
                        .to_bytes();
                    let request: serde_json::Value =
                        serde_json::from_slice(&body).unwrap_or_default();
                    let method = request
                        .get("method")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default();
                    let id = request
                        .get("id")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null);

                    if method.starts_with("notifications/") {
                        return Ok::<_, Infallible>(
                            hyper::Response::builder()
                                .status(hyper::StatusCode::ACCEPTED)
                                .body(Full::<Bytes>::from(Bytes::new()))
                                .unwrap(),
                        );
                    }
                    if method == "tools/call" {
                        return Ok::<_, Infallible>(
                            hyper::Response::builder()
                                .status(hyper::StatusCode::UNAUTHORIZED)
                                .header("content-type", "text/plain")
                                .header("www-authenticate", "Bearer realm=\"mcp\"")
                                .body(Full::<Bytes>::from("Unauthorized"))
                                .unwrap(),
                        );
                    }

                    let result = match method {
                        "initialize" => serde_json::json!({
                            "protocolVersion": "2024-11-05",
                            "serverInfo": { "name": "oauth-mock", "version": "0.1.0" },
                            "capabilities": { "tools": {} }
                        }),
                        "tools/list" => serde_json::json!({
                            "tools": [{
                                "name": "protected",
                                "description": "Requires authorization",
                                "inputSchema": { "type": "object" }
                            }]
                        }),
                        _ => serde_json::json!({}),
                    };
                    let payload = serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": result
                    });
                    let mut response = hyper::Response::builder()
                        .status(hyper::StatusCode::OK)
                        .header("content-type", "application/json");
                    if method == "initialize" {
                        response = response.header("mcp-session-id", "oauth-contract-session");
                    }
                    Ok::<_, Infallible>(
                        response
                            .body(Full::<Bytes>::from(
                                serde_json::to_vec(&payload).expect("serialize response"),
                            ))
                            .unwrap(),
                    )
                });
                let stream = hyper_util::rt::TokioIo::new(stream);
                let service = hyper_util::service::TowerToHyperService::new(service);
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(stream, service)
                    .await;
            });
        }
    });

    url
}

struct AutoOAuthMockStats {
    discovery_requests: AtomicUsize,
    registration_requests: AtomicUsize,
    token_requests: AtomicUsize,
    mcp_requests: AtomicUsize,
    authorized_mcp_requests: AtomicUsize,
    last_token_form: tokio::sync::Mutex<Option<HashMap<String, String>>>,
}

impl AutoOAuthMockStats {
    fn new() -> Self {
        Self {
            discovery_requests: AtomicUsize::new(0),
            registration_requests: AtomicUsize::new(0),
            token_requests: AtomicUsize::new(0),
            mcp_requests: AtomicUsize::new(0),
            authorized_mcp_requests: AtomicUsize::new(0),
            last_token_form: tokio::sync::Mutex::new(None),
        }
    }
}

struct ChannelRuntimeEventSink {
    sender: tokio::sync::mpsc::UnboundedSender<ComputerRuntimeStatusEvent>,
}

impl ComputerRuntimeEventSink for ChannelRuntimeEventSink {
    fn emit(&self, event: &ComputerRuntimeStatusEvent) -> Result<(), String> {
        self.sender
            .send(event.clone())
            .map_err(|_| "runtime event receiver closed".to_string())
    }
}

async fn start_auto_oauth_challenge_server() -> (String, Arc<AutoOAuthMockStats>) {
    start_auto_oauth_challenge_server_with_registration_delay(Duration::ZERO).await
}

async fn start_auto_oauth_challenge_server_with_registration_delay(
    registration_delay: Duration,
) -> (String, Arc<AutoOAuthMockStats>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("local_addr"));
    let stats = Arc::new(AutoOAuthMockStats::new());
    let server_url = url.clone();
    let server_stats = Arc::clone(&stats);

    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let request_url = server_url.clone();
            let stats = Arc::clone(&server_stats);
            tokio::spawn(async move {
                let service = service_fn(move |request: hyper::Request<hyper::body::Incoming>| {
                    let request_url = request_url.clone();
                    let stats = Arc::clone(&stats);
                    async move {
                        let method = request.method().clone();
                        let path = request.uri().path().to_string();
                        let authorized = request
                            .headers()
                            .get(hyper::header::AUTHORIZATION)
                            .is_some_and(|value| value == "Bearer tfrobot-auto-test-token");
                        let body = request
                            .into_body()
                            .collect()
                            .await
                            .expect("read OAuth mock request")
                            .to_bytes();
                        if method == hyper::Method::GET
                            && path.starts_with("/.well-known/oauth-protected-resource")
                        {
                            stats.discovery_requests.fetch_add(1, Ordering::SeqCst);
                            let payload = serde_json::json!({
                                "resource": format!("{request_url}/mcp"),
                                "authorization_servers": [&request_url],
                                "scopes_supported": ["tools.read"]
                            });
                            return Ok::<_, Infallible>(
                                hyper::Response::builder()
                                    .status(hyper::StatusCode::OK)
                                    .header("content-type", "application/json")
                                    .header("connection", "close")
                                    .body(Full::<Bytes>::from(
                                        serde_json::to_vec(&payload)
                                            .expect("serialize protected resource metadata"),
                                    ))
                                    .unwrap(),
                            );
                        }
                        if method == hyper::Method::GET
                            && matches!(
                                path.as_str(),
                                "/.well-known/oauth-authorization-server"
                                    | "/.well-known/oauth-authorization-server/mcp"
                            )
                        {
                            stats.discovery_requests.fetch_add(1, Ordering::SeqCst);
                            let payload = serde_json::json!({
                                "issuer": request_url,
                                "authorization_endpoint": format!("{request_url}/authorize"),
                                "token_endpoint": format!("{request_url}/token"),
                                "registration_endpoint": format!("{request_url}/register"),
                                "response_types_supported": ["code"],
                                "grant_types_supported": ["authorization_code", "client_credentials"],
                                "token_endpoint_auth_methods_supported": ["none", "client_secret_post"],
                                "code_challenge_methods_supported": ["S256"],
                                "client_id_metadata_document_supported": true,
                                "authorization_response_iss_parameter_supported": true,
                            });
                            return Ok::<_, Infallible>(
                                hyper::Response::builder()
                                    .status(hyper::StatusCode::OK)
                                    .header("content-type", "application/json")
                                    .header("connection", "close")
                                    .body(Full::<Bytes>::from(
                                        serde_json::to_vec(&payload)
                                            .expect("serialize authorization server metadata"),
                                    ))
                                    .unwrap(),
                            );
                        }
                        if method == hyper::Method::POST && path == "/register" {
                            stats.registration_requests.fetch_add(1, Ordering::SeqCst);
                            if !registration_delay.is_zero() {
                                sleep(registration_delay).await;
                            }
                            let registration: serde_json::Value =
                                serde_json::from_slice(&body).expect("parse registration request");
                            let payload = serde_json::json!({
                                "client_id": "tfrobot-auto-test-client",
                                "client_name": "A2C Computer",
                                "redirect_uris": registration["redirect_uris"]
                            });
                            return Ok::<_, Infallible>(
                                hyper::Response::builder()
                                    .status(hyper::StatusCode::OK)
                                    .header("content-type", "application/json")
                                    .header("connection", "close")
                                    .body(Full::<Bytes>::from(
                                        serde_json::to_vec(&payload)
                                            .expect("serialize registration response"),
                                    ))
                                    .unwrap(),
                            );
                        }
                        if method == hyper::Method::POST && path == "/token" {
                            let form: HashMap<String, String> =
                                url::form_urlencoded::parse(&body).into_owned().collect();
                            stats.token_requests.fetch_add(1, Ordering::SeqCst);
                            *stats.last_token_form.lock().await = Some(form.clone());
                            let valid = form.get("grant_type").map(String::as_str)
                                == Some("authorization_code")
                                && form.get("code").map(String::as_str)
                                    == Some("authorization-code")
                                && form.get("client_id").map(String::as_str)
                                    == Some("tfrobot-auto-test-client")
                                && form
                                    .get("code_verifier")
                                    .is_some_and(|value| !value.is_empty())
                                && form.get("resource").map(String::as_str)
                                    == Some(format!("{request_url}/mcp").as_str());
                            if !valid {
                                return Ok::<_, Infallible>(
                                    hyper::Response::builder()
                                        .status(hyper::StatusCode::UNAUTHORIZED)
                                        .body(Full::<Bytes>::from(Bytes::new()))
                                        .unwrap(),
                                );
                            }
                            let payload = serde_json::json!({
                                "access_token": "tfrobot-auto-test-token",
                                "token_type": "Bearer",
                                "expires_in": 3600,
                                "scope": "tools.read"
                            });
                            return Ok::<_, Infallible>(
                                hyper::Response::builder()
                                    .status(hyper::StatusCode::OK)
                                    .header("content-type", "application/json")
                                    .body(Full::<Bytes>::from(
                                        serde_json::to_vec(&payload)
                                            .expect("serialize token response"),
                                    ))
                                    .unwrap(),
                            );
                        }
                        if method == hyper::Method::POST && path == "/mcp" {
                            stats.mcp_requests.fetch_add(1, Ordering::SeqCst);
                            if authorized {
                                stats.authorized_mcp_requests.fetch_add(1, Ordering::SeqCst);
                            }
                            if !authorized {
                                return Ok::<_, Infallible>(
                                    hyper::Response::builder()
                                        .status(hyper::StatusCode::UNAUTHORIZED)
                                        .header(
                                            "www-authenticate",
                                            format!(
                                                "Bearer resource_metadata=\"{request_url}/.well-known/oauth-protected-resource/mcp\""
                                            ),
                                        )
                                        .body(Full::<Bytes>::from(Bytes::new()))
                                        .unwrap(),
                                );
                            }
                            let request: serde_json::Value =
                                serde_json::from_slice(&body).unwrap_or_default();
                            let rpc_method = request
                                .get("method")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or_default();
                            if rpc_method.starts_with("notifications/") {
                                return Ok::<_, Infallible>(
                                    hyper::Response::builder()
                                        .status(hyper::StatusCode::ACCEPTED)
                                        .body(Full::<Bytes>::from(Bytes::new()))
                                        .unwrap(),
                                );
                            }
                            let id = request
                                .get("id")
                                .cloned()
                                .unwrap_or(serde_json::Value::Null);
                            let result = match rpc_method {
                                "initialize" => serde_json::json!({
                                    "protocolVersion": "2024-11-05",
                                    "serverInfo": { "name": "oauth-mock", "version": "0.1.0" },
                                    "capabilities": { "tools": {} }
                                }),
                                "tools/list" => serde_json::json!({
                                    "tools": [{
                                        "name": "protected",
                                        "description": "Requires authorization",
                                        "inputSchema": { "type": "object" }
                                    }]
                                }),
                                "tools/call" => serde_json::json!({
                                    "content": [{ "type": "text", "text": "authorized" }]
                                }),
                                _ => serde_json::json!({}),
                            };
                            let payload = serde_json::json!({
                                "jsonrpc": "2.0",
                                "id": id,
                                "result": result
                            });
                            return Ok::<_, Infallible>(
                                hyper::Response::builder()
                                    .status(hyper::StatusCode::OK)
                                    .header("content-type", "application/json")
                                    .header("mcp-session-id", "oauth-test-session")
                                    .body(Full::<Bytes>::from(
                                        serde_json::to_vec(&payload)
                                            .expect("serialize MCP response"),
                                    ))
                                    .unwrap(),
                            );
                        }
                        Ok::<_, Infallible>(
                            hyper::Response::builder()
                                .status(hyper::StatusCode::NOT_FOUND)
                                .body(Full::<Bytes>::from(Bytes::new()))
                                .unwrap(),
                        )
                    }
                });
                let stream = hyper_util::rt::TokioIo::new(stream);
                let service = hyper_util::service::TowerToHyperService::new(service);
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(stream, service)
                    .await;
            });
        }
    });

    (url, stats)
}

async fn connect_runtime_to_mock_robot(state: &AppState, server_url: &str) {
    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .expect("runtime should exist");
    if !runtime.is_running().await {
        runtime.start().await.expect("start runtime");
    }
    runtime
        .connect_smcp_socketio(
            server_url,
            None,
            HashMap::new(),
            Some("/smcp".to_string()),
            TEST_OFFICE_ID,
            TEST_COMPUTER_NAME,
        )
        .await
        .expect("connect mock robot");

    *runtime.connection_handle_for_test().write_owned().await = Some(ConnectionState {
        profile_name: "mock-robot".to_string(),
        url: server_url.to_string(),
        office_id: TEST_OFFICE_ID.to_string(),
        computer_name: TEST_COMPUTER_NAME.to_string(),
        connected_at: chrono::Utc::now(),
        source_type: "manual_smcp".to_string(),
        target_id: Some("mock-robot".to_string()),
        target_name: Some("Mock Robot".to_string()),
        employee_id: None,
        generation: 0,
    });
}

#[tokio::test]
async fn test_legacy_profile_mcp_does_not_enter_sdk_config_projection() {
    let tmp = tempfile::tempdir().unwrap();
    write_legacy_plugin_owned_mcp_profile(tmp.path());
    let state = create_mcp_test_app_state(tmp.path()).await;

    let statuses = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(statuses.is_empty());
    assert!(state
        .sdk_config
        .load(TEST_INSTANCE_ID)
        .mcp
        .servers
        .is_empty());
}

#[tokio::test]
async fn test_legacy_profile_plugin_owner_does_not_block_sdk_owned_user_config() {
    let tmp = tempfile::tempdir().unwrap();
    write_legacy_plugin_owned_mcp_profile(tmp.path());
    let state = create_mcp_test_app_state(tmp.path()).await;

    mcp::add_mcp_server_core(&state, TEST_INSTANCE_ID, echo_server_config("plugin-owned"))
        .await
        .unwrap();

    let cli_path = tmp.path().join("cli-native.json");
    let cli_config = serde_json::json!({
        "servers": [echo_server_config("plugin-owned")],
        "inputs": []
    });
    std::fs::write(&cli_path, serde_json::to_string(&cli_config).unwrap()).unwrap();
    let cli_result = config_io::import_config_core(
        &state,
        cli_path.to_string_lossy().to_string(),
        TEST_INSTANCE_ID.to_string(),
        Some(config_io::ConfigFormat::CliNative),
    )
    .await
    .unwrap();
    assert_eq!(cli_result.servers_imported, 1);
    assert!(cli_result.servers_skipped.is_empty());

    let claude_path = tmp.path().join("claude-desktop.json");
    let claude_config = serde_json::json!({
        "mcpServers": {
            "plugin-owned": {
                "command": "node",
                "args": ["replacement.js"],
                "env": {}
            }
        }
    });
    std::fs::write(&claude_path, serde_json::to_string(&claude_config).unwrap()).unwrap();
    let claude_result = config_io::import_config_core(
        &state,
        claude_path.to_string_lossy().to_string(),
        TEST_INSTANCE_ID.to_string(),
        Some(config_io::ConfigFormat::ClaudeDesktop),
    )
    .await
    .unwrap();
    assert_eq!(claude_result.servers_imported, 1);
    assert!(claude_result.servers_skipped.is_empty());

    assert!(mcp::get_mcp_server_config_core(&state, TEST_INSTANCE_ID, "plugin-owned").is_ok());
    assert!(!state.config.legacy_computer_instances_path().exists());
}

#[tokio::test]
async fn legacy_profile_plugin_metadata_does_not_enter_runtime_batch_inventory() {
    let tmp = tempfile::tempdir().unwrap();
    write_legacy_plugin_owned_mcp_profile(tmp.path());
    let state = create_mcp_test_app_state(tmp.path()).await;
    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();

    let start_result = mcp::start_all_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    let stop_result = mcp::stop_all_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    for result in [start_result, stop_result] {
        assert_eq!(result.candidate_count, 0);
        assert_eq!(result.actual_operation_count, 0);
        assert_eq!(result.unchanged_count, 0);
        assert_eq!(result.excluded_plugin_owned_count, 0);
        assert!(result.failures.is_empty());
    }
}

#[tokio::test]
async fn test_mcp_commands_require_instance_id() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    let result = mcp::get_mcp_servers_core(&state, "").await;

    assert!(result.is_err());
    assert!(result.unwrap_err().contains("instance_id is required"));
}

#[tokio::test]
async fn test_mcp_commands_are_instance_scoped() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    state
        .config
        .add_computer_instance(ComputerInstance::new("second", "Second"))
        .unwrap();
    state
        .computer_registry
        .upsert_runtime(state.config.get_computer_instance("second").unwrap())
        .await
        .unwrap();

    sdk_config::upsert_computer_mcp_config_core(
        &state,
        "second",
        echo_server_config("second-only"),
    )
    .await
    .unwrap();

    let default_configs = state.sdk_config.load(TEST_INSTANCE_ID).mcp.servers;
    let second_configs = state.sdk_config.load("second").mcp.servers;
    let second_statuses = mcp::get_mcp_servers_core(&state, "second").await.unwrap();

    assert!(default_configs.is_empty());
    assert_eq!(second_configs.len(), 1);
    assert_eq!(second_configs[0].name, "second-only");
    assert_eq!(second_statuses.len(), 1);
    assert_eq!(second_statuses[0].name, "second-only");
    assert!(!second_statuses[0].running);
}

#[tokio::test]
async fn test_sdk_config_remains_authoritative_across_runtime_sync_and_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let before = state.sdk_config.load(TEST_INSTANCE_ID).revision;

    mcp::add_mcp_server_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config("sdk-authoritative"),
    )
    .await
    .unwrap();

    let after = state.sdk_config.load(TEST_INSTANCE_ID);
    assert_ne!(after.revision, before);
    assert!(after
        .mcp
        .servers
        .iter()
        .any(|server| server.name == "sdk-authoritative"));
    let profile_json = std::fs::read_to_string(
        state
            .config
            .computer_profile_path(TEST_INSTANCE_ID)
            .unwrap(),
    )
    .unwrap();
    assert!(!profile_json.contains("sdk-authoritative"));

    let profile = state
        .config
        .get_computer_instance(TEST_INSTANCE_ID)
        .unwrap();
    state
        .computer_registry
        .update_runtime_instance(profile)
        .await
        .unwrap();
    assert!(mcp::get_mcp_server_config_core(&state, TEST_INSTANCE_ID, "sdk-authoritative").is_ok());

    let restarted = create_test_app_state(tmp.path());
    assert!(
        mcp::get_mcp_server_config_core(&restarted, TEST_INSTANCE_ID, "sdk-authoritative").is_ok()
    );
}

#[tokio::test]
async fn test_get_mcp_servers_uses_sdk_computer_status() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    mcp::add_mcp_server_core(&state, TEST_INSTANCE_ID, echo_server_config("sdk-status"))
        .await
        .unwrap();
    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    let statuses = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();

    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].name, "sdk-status");
    assert_eq!(
        statuses[0].activation_state,
        MCPServerActivationState::Started
    );
    assert_eq!(
        statuses[0].connection_state,
        MCPServerConnectionState::Connected
    );
    assert!(statuses[0].running);
    assert_eq!(statuses[0].status_message, "connected");
}

#[tokio::test]
async fn disabled_user_mcp_toggle_applies_only_after_restart() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    sdk_config::upsert_computer_mcp_config_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config_with_disabled("toggle-server", false),
    )
    .await
    .unwrap();
    start_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    let enabled = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(enabled
        .iter()
        .any(|server| server.name == "toggle-server" && server.running));

    sdk_config::upsert_computer_mcp_config_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config_with_disabled("toggle-server", true),
    )
    .await
    .unwrap();
    assert!(mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap()
        .iter()
        .any(|server| server.name == "toggle-server" && server.running));
    computer::restart_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    assert!(mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap()
        .iter()
        .all(|server| server.name != "toggle-server"));

    sdk_config::upsert_computer_mcp_config_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config_with_disabled("toggle-server", false),
    )
    .await
    .unwrap();
    assert!(mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap()
        .iter()
        .all(|server| server.name != "toggle-server"));
    computer::restart_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    let reenabled = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(reenabled
        .iter()
        .any(|server| server.name == "toggle-server" && server.running));
}

#[tokio::test]
async fn changing_bundle_id_replaces_the_runtime_identity_after_restart() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    start_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();

    sdk_config::upsert_computer_mcp_config_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config_with_bundle_id("identity-server", "identity-old"),
    )
    .await
    .unwrap();
    sdk_config::upsert_computer_mcp_config_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config_with_bundle_id("identity-server", "identity-new"),
    )
    .await
    .unwrap();

    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(runtime.sdk_mcp_server_ids().await.is_empty());
    computer::restart_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();

    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    let runtime_ids = runtime.sdk_mcp_server_ids().await;
    assert!(!runtime_ids.contains(&bundle_id("identity-old")));
    assert!(runtime_ids.contains(&bundle_id("identity-new")));
    let rows = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].bundle_id.as_str(), "identity-new");
    assert!(rows[0].running);
}

#[tokio::test]
async fn computer_start_isolates_mcp_failures_and_surfaces_each_error() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    sdk_config::upsert_computer_mcp_config_core(
        &state,
        TEST_INSTANCE_ID,
        unavailable_server_config("broken-server"),
    )
    .await
    .unwrap();
    sdk_config::upsert_computer_mcp_config_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config("healthy-server"),
    )
    .await
    .unwrap();

    start_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    let servers = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    let healthy = servers
        .iter()
        .find(|server| server.name == "healthy-server")
        .unwrap();
    let broken = servers
        .iter()
        .find(|server| server.name == "broken-server")
        .unwrap();
    assert!(
        healthy.running,
        "healthy MCP must not be blocked by another failure"
    );
    assert_eq!(
        broken.activation_state,
        MCPServerActivationState::Started,
        "a failed connection must not be projected as an explicit stop"
    );
    assert_eq!(broken.connection_state, MCPServerConnectionState::Error);
    assert!(
        broken.running,
        "running remains the started-state projection"
    );
    assert_eq!(broken.status_message, "error");
    let snapshot = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap()
        .runtime_snapshot()
        .await;
    let problem = snapshot
        .problems
        .iter()
        .find(|problem| problem.source == ComputerRuntimeProblemSource::Mcp)
        .expect("failed MCP startup should surface a current structured problem");
    assert_eq!(problem.severity, ComputerRuntimeProblemSeverity::Degraded);
    assert_eq!(
        problem.message,
        ComputerRuntimeProblemMessage::McpStartFailed
    );
    assert!(problem.current);
    assert!(problem.occurred_at.contains('T'));
    assert!(problem.affected_capabilities.iter().any(|capability| {
        matches!(
            capability,
            ComputerRuntimeAffectedCapability::McpServer {
                bundle_id,
                name: Some(name),
            } if bundle_id == "broken-server" && name == "broken-server"
        )
    }));
    assert!(problem
        .technical_detail
        .as_deref()
        .is_some_and(|detail| detail.starts_with("Start failed:")));
    assert!(problem
        .presentation_detail
        .as_deref()
        .is_some_and(|detail| detail.starts_with("Start failed:")));
    let startup_activity = state
        .observability
        .query_activity(&ActivityQuery {
            scope: ActivityScopeFilter::Computer {
                computer_id: TEST_INSTANCE_ID.to_string(),
            },
            ..ActivityQuery::default()
        })
        .unwrap();
    let failed_start = startup_activity
        .items
        .iter()
        .find(|event| event.category == "mcp" && event.operation == "start")
        .expect("Computer startup should persist each failed MCP start as activity");
    assert_eq!(failed_start.level, ActivityLevel::Error);
    assert_eq!(failed_start.outcome, ActivityOutcome::Failed);
    assert!(failed_start.message.contains("broken-server"));

    mcp::stop_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("healthy-server"))
        .await
        .unwrap();
    let batch = mcp::start_all_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(batch.candidate_count, 2);
    assert_eq!(batch.actual_operation_count, 1);
    assert_eq!(batch.unchanged_count, 0);
    assert_eq!(batch.excluded_plugin_owned_count, 0);
    assert!(matches!(
        batch.failures.as_slice(),
        [mcp::McpBatchFailure {
            bundle_id,
            name,
            error: RuntimeActionError::RuntimeError { .. },
        }] if bundle_id.as_str() == "broken-server" && name == "broken-server"
    ));
    let batch_activity = state
        .observability
        .query_activity(&ActivityQuery::default())
        .unwrap();
    let start_all = batch_activity
        .items
        .iter()
        .find(|event| event.operation == "start_all")
        .expect("start-all should persist a terminal activity event");
    assert_eq!(start_all.level, ActivityLevel::Error);
    assert_eq!(start_all.outcome, ActivityOutcome::Failed);
}

#[tokio::test]
async fn computer_start_does_not_fail_when_one_mcp_input_definition_is_missing() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let missing_input_server: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": "picture",
        "server_parameters": {
            "command": "node",
            "args": [common::echo_server_path().to_str().unwrap()],
            "env": {
                "OPENROUTER_API_KEY": "${input:openrouterkey}",
                "ZHIPUAI_API_KEY": "${input:zhipukey}"
            }
        }
    }))
    .unwrap();

    let config_error =
        sdk_config::upsert_computer_mcp_config_core(&state, TEST_INSTANCE_ID, missing_input_server)
            .await
            .unwrap_err();
    assert_eq!(
        serde_json::to_value(&config_error).unwrap()["requesting_mcp"],
        serde_json::json!({
            "bundle_id": "picture",
            "name": "picture"
        })
    );
    assert!(matches!(
        config_error,
        RuntimeActionError::MissingInputDefinition { input_id, .. }
            if input_id == "openrouterkey"
    ));
    sdk_config::upsert_computer_mcp_config_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config("healthy-server"),
    )
    .await
    .unwrap();

    let started = start_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .expect("an MCP input failure must not fail Computer startup");
    assert!(started.running);

    let servers = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(servers
        .iter()
        .find(|server| server.name == "healthy-server")
        .is_some_and(|server| server.running));
    let picture = servers
        .iter()
        .find(|server| server.name == "picture")
        .expect("the failed MCP must remain visible for a targeted retry");
    assert_eq!(picture.activation_state, MCPServerActivationState::Stopped);
    assert_eq!(
        picture.connection_state,
        MCPServerConnectionState::Disconnected
    );

    let start_error = mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("picture"))
        .await
        .unwrap_err();
    assert_eq!(
        serde_json::to_value(&start_error).unwrap()["requesting_mcp"],
        serde_json::json!({
            "bundle_id": "picture",
            "name": "picture"
        })
    );
    assert!(matches!(
        start_error,
        RuntimeActionError::MissingInputDefinition { input_id, .. }
            if input_id == "openrouterkey"
    ));

    inputs::add_or_update_input_core(
        &state,
        TEST_INSTANCE_ID,
        inputs::InputDefinition::PromptString {
            id: "openrouterkey".to_string(),
            label: Some("openrouterkey".to_string()),
            description: Some("OpenRouter API key".to_string()),
            default: None,
            password: Some(true),
        },
    )
    .await
    .unwrap();
    let second_definition_error =
        mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("picture"))
            .await
            .unwrap_err();
    assert!(matches!(
        second_definition_error,
        RuntimeActionError::MissingInputDefinition { input_id, .. }
            if input_id == "zhipukey"
    ));
    inputs::add_or_update_input_core(
        &state,
        TEST_INSTANCE_ID,
        inputs::InputDefinition::PromptString {
            id: "zhipukey".to_string(),
            label: Some("zhipukey".to_string()),
            description: Some("Zhipu API key".to_string()),
            default: None,
            password: Some(true),
        },
    )
    .await
    .unwrap();
    let missing_openrouter_entry =
        mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("picture"))
            .await
            .unwrap_err();
    assert!(
        matches!(
            &missing_openrouter_entry,
            RuntimeActionError::ResolverFailed { input_id, message }
                if input_id == "openrouterkey"
                    && message.contains("user confirmation is required")
        ),
        "unexpected error after defining both inputs: {missing_openrouter_entry:?}"
    );
    inputs::upsert_input_entry_core(
        &state,
        TEST_INSTANCE_ID,
        "openrouterkey",
        Some("test-secret".to_string()),
        true,
    )
    .await
    .unwrap();
    let missing_zhipu_entry =
        mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("picture"))
            .await
            .unwrap_err();
    assert!(
        matches!(
            &missing_zhipu_entry,
            RuntimeActionError::ResolverFailed { input_id, message }
                if input_id == "zhipukey"
                    && message.contains("user confirmation is required")
        ),
        "unexpected error after defining zhipukey: {missing_zhipu_entry:?}"
    );
    inputs::upsert_input_entry_core(
        &state,
        TEST_INSTANCE_ID,
        "zhipukey",
        Some("second-test-secret".to_string()),
        true,
    )
    .await
    .unwrap();

    mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("picture"))
        .await
        .expect("saving missing definitions sequentially must allow retrying the affected MCP");
    let servers = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(servers
        .iter()
        .find(|server| server.name == "picture")
        .is_some_and(|server| server.running));
}

#[tokio::test]
async fn client_mcp_draft_atomically_projects_inputs_and_collects_only_unused_definitions() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let definition = inputs::InputDefinition::PromptString {
        id: "api-key".to_string(),
        label: Some("API key".to_string()),
        description: None,
        default: None,
        password: Some(true),
    };
    let first: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": "first-client-draft",
        "server_parameters": {
            "command": "echo",
            "args": [],
            "env": {
                "LOG_LEVEL": "debug",
                "API_KEY": "${input:api-key}"
            }
        }
    }))
    .unwrap();

    sdk_config::upsert_computer_mcp_config_with_inputs_core(
        &state,
        TEST_INSTANCE_ID,
        first,
        vec![definition.clone()],
        Vec::new(),
    )
    .await
    .unwrap();

    assert_eq!(
        state.sdk_config.load_input_definitions(TEST_INSTANCE_ID),
        vec![definition]
    );
    let first_saved = state
        .sdk_config
        .load(TEST_INSTANCE_ID)
        .mcp
        .servers
        .into_iter()
        .find(|server| server.name == "first-client-draft")
        .unwrap()
        .config;
    let first_saved = serde_json::to_value(first_saved).unwrap();
    assert_eq!(
        first_saved["server_parameters"]["env"]["LOG_LEVEL"],
        "debug"
    );
    assert_eq!(
        first_saved["server_parameters"]["env"]["API_KEY"],
        "${input:api-key}"
    );

    let shared: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": "second-client-draft",
        "server_parameters": {
            "command": "echo",
            "args": [],
            "env": {"API_KEY": "${input:api-key}"}
        }
    }))
    .unwrap();
    sdk_config::upsert_computer_mcp_config_core(&state, TEST_INSTANCE_ID, shared)
        .await
        .unwrap();

    let first_literal: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": "first-client-draft",
        "server_parameters": {
            "command": "echo",
            "args": [],
            "env": {"API_KEY": "literal-first"}
        }
    }))
    .unwrap();
    sdk_config::upsert_computer_mcp_config_with_inputs_core(
        &state,
        TEST_INSTANCE_ID,
        first_literal,
        Vec::new(),
        vec!["api-key".to_string()],
    )
    .await
    .unwrap();
    assert_eq!(
        state
            .sdk_config
            .load_input_definitions(TEST_INSTANCE_ID)
            .len(),
        1
    );

    let second_literal: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": "second-client-draft",
        "server_parameters": {
            "command": "echo",
            "args": [],
            "env": {"API_KEY": "literal-second"}
        }
    }))
    .unwrap();
    sdk_config::upsert_computer_mcp_config_with_inputs_core(
        &state,
        TEST_INSTANCE_ID,
        second_literal,
        Vec::new(),
        vec!["api-key".to_string()],
    )
    .await
    .unwrap();
    assert!(state
        .sdk_config
        .load_input_definitions(TEST_INSTANCE_ID)
        .is_empty());

    let composite_literal: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": "composite-literal",
        "server_parameters": {
            "command": "echo",
            "args": [],
            "env": {"REGION": "prefix-${input:not-a-reference}"}
        }
    }))
    .unwrap();
    sdk_config::upsert_computer_mcp_config_with_inputs_core(
        &state,
        TEST_INSTANCE_ID,
        composite_literal,
        Vec::new(),
        Vec::new(),
    )
    .await
    .expect("composite editor constants must not require an Input definition");
    let composite_saved = state
        .sdk_config
        .load(TEST_INSTANCE_ID)
        .mcp
        .servers
        .into_iter()
        .find(|server| server.name == "composite-literal")
        .unwrap();
    assert_eq!(
        serde_json::to_value(composite_saved.config).unwrap()["server_parameters"]["env"]["REGION"],
        "prefix-${input:not-a-reference}"
    );

    let rejected: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": "rejected-client-draft",
        "server_parameters": {
            "command": "echo",
            "args": ["--api-key=plaintext"],
            "env": {"NEXT": "${input:next}"}
        }
    }))
    .unwrap();
    let error = sdk_config::upsert_computer_mcp_config_with_inputs_core(
        &state,
        TEST_INSTANCE_ID,
        rejected,
        vec![inputs::InputDefinition::PromptString {
            id: "next".to_string(),
            label: None,
            description: None,
            default: None,
            password: None,
        }],
        Vec::new(),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, RuntimeActionError::RuntimeError { .. }));
    assert!(state
        .sdk_config
        .load_input_definitions(TEST_INSTANCE_ID)
        .is_empty());
    assert!(!state
        .sdk_config
        .load(TEST_INSTANCE_ID)
        .mcp
        .servers
        .iter()
        .any(|server| server.name == "rejected-client-draft"));

    let removable: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": "removable-client-draft",
        "server_parameters": {
            "command": "echo",
            "args": [],
            "env": {"TOKEN": "${input:removable}"}
        }
    }))
    .unwrap();
    sdk_config::upsert_computer_mcp_config_with_inputs_core(
        &state,
        TEST_INSTANCE_ID,
        removable,
        vec![inputs::InputDefinition::PromptString {
            id: "removable".to_string(),
            label: None,
            description: None,
            default: None,
            password: Some(true),
        }],
        Vec::new(),
    )
    .await
    .unwrap();
    sdk_config::remove_computer_mcp_config_core(&state, TEST_INSTANCE_ID, "removable-client-draft")
        .await
        .unwrap();
    assert!(state
        .sdk_config
        .load_input_definitions(TEST_INSTANCE_ID)
        .is_empty());
}

#[tokio::test]
async fn client_mcp_draft_preserves_user_provenance_and_cross_scope_input_references() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let anchor = state.sdk_config.project_anchor(TEST_INSTANCE_ID);
    let user_mcp_path = anchor.join("a2c/mcp.json");
    std::fs::create_dir_all(user_mcp_path.parent().unwrap()).unwrap();
    std::fs::write(
        &user_mcp_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "servers": {
                "user-owned": {
                    "type": "stdio",
                    "server_parameters": {
                        "command": "echo",
                        "args": [],
                        "env": {"TOKEN": "${input:shared-user-input}"}
                    }
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let project_definition = inputs::InputDefinition::PromptString {
        id: "shared-user-input".to_string(),
        label: None,
        description: None,
        default: None,
        password: Some(true),
    };
    let local_owner: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": "local-owner",
        "server_parameters": {
            "command": "echo",
            "args": [],
            "env": {"TOKEN": "${input:shared-user-input}"}
        }
    }))
    .unwrap();
    sdk_config::upsert_computer_mcp_config_with_inputs_core(
        &state,
        TEST_INSTANCE_ID,
        local_owner,
        vec![project_definition.clone()],
        Vec::new(),
    )
    .await
    .unwrap();

    let edited_user: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": "user-owned",
        "server_parameters": {
            "command": "printf",
            "args": ["edited"],
            "env": {"TOKEN": "${input:shared-user-input}"}
        }
    }))
    .unwrap();
    sdk_config::upsert_computer_mcp_config_with_inputs_core(
        &state,
        TEST_INSTANCE_ID,
        edited_user,
        Vec::new(),
        Vec::new(),
    )
    .await
    .unwrap();
    let snapshot = state.sdk_config.load(TEST_INSTANCE_ID);
    let user_server = snapshot
        .mcp
        .servers
        .iter()
        .find(|server| server.name == "user-owned")
        .unwrap();
    assert_eq!(user_server.origin, ProvenanceScope::User);
    assert_eq!(
        serde_json::to_value(&user_server.config).unwrap()["server_parameters"]["command"],
        "printf"
    );
    let project_doc = load_project_config_doc(&anchor).unwrap();
    assert!(!project_doc
        .mcp_local
        .as_ref()
        .and_then(|layer| layer.get("servers"))
        .and_then(serde_json::Value::as_object)
        .is_some_and(|servers| servers.contains_key("user-owned")));

    let local_literal: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": "local-owner",
        "server_parameters": {
            "command": "echo",
            "args": [],
            "env": {"TOKEN": "literal"}
        }
    }))
    .unwrap();
    sdk_config::upsert_computer_mcp_config_with_inputs_core(
        &state,
        TEST_INSTANCE_ID,
        local_literal,
        Vec::new(),
        vec!["shared-user-input".to_string()],
    )
    .await
    .unwrap();
    assert!(
        state
            .sdk_config
            .load_input_definitions(TEST_INSTANCE_ID)
            .iter()
            .any(|definition| definition.id() == project_definition.id()),
        "a raw User declaration still references the Project Input"
    );

    sdk_config::remove_computer_mcp_config_core(&state, TEST_INSTANCE_ID, "user-owned")
        .await
        .unwrap();
    assert!(!state
        .sdk_config
        .load(TEST_INSTANCE_ID)
        .mcp
        .servers
        .iter()
        .any(|server| server.name == "user-owned"));
    assert!(!state
        .sdk_config
        .load_input_definitions(TEST_INSTANCE_ID)
        .iter()
        .any(|definition| definition.id() == "shared-user-input"));
}

#[tokio::test]
async fn client_mcp_draft_missing_definition_reports_the_exact_requester() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let server: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": "profile-editor",
        "bundle_id": "profile-editor-bundle",
        "server_parameters": {
            "command": "echo",
            "args": [],
            "env": {"NAME": "${input:missing-name}"}
        }
    }))
    .unwrap();

    let error = sdk_config::upsert_computer_mcp_config_with_inputs_core(
        &state,
        TEST_INSTANCE_ID,
        server,
        Vec::new(),
        Vec::new(),
    )
    .await
    .unwrap_err();

    assert!(matches!(
        error,
        RuntimeActionError::MissingInputDefinition {
            input_id,
            requesting_mcp: Some(requester),
            ..
        } if input_id == "missing-name"
            && requester.bundle_id == "profile-editor-bundle"
            && requester.name == "profile-editor"
    ));
}

#[tokio::test]
async fn client_mcp_draft_rejects_project_definition_shadowed_by_local_scope() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let anchor = state.sdk_config.project_anchor(TEST_INSTANCE_ID);
    let mut document = load_project_config_doc(&anchor).unwrap();
    document.mcp_local = Some(
        serde_json::json!({
            "inputs": [{
                "id": "shadowed",
                "type": "PromptString",
                "description": "Local definition",
                "password": false
            }]
        })
        .as_object()
        .unwrap()
        .clone(),
    );
    state.sdk_config.save(TEST_INSTANCE_ID, &document).unwrap();

    let server: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": "shadowed-input-server",
        "server_parameters": {
            "command": "echo",
            "args": [],
            "env": {"TOKEN": "${input:shadowed}"}
        }
    }))
    .unwrap();
    let error = sdk_config::upsert_computer_mcp_config_with_inputs_core(
        &state,
        TEST_INSTANCE_ID,
        server,
        vec![inputs::InputDefinition::PromptString {
            id: "shadowed".to_string(),
            label: Some("Edited Project definition".to_string()),
            description: None,
            default: None,
            password: Some(true),
        }],
        Vec::new(),
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("shadowed by a higher-priority"));
    let after = load_project_config_doc(&anchor).unwrap();
    assert!(!after
        .mcp
        .as_ref()
        .and_then(|layer| layer.get("inputs"))
        .and_then(serde_json::Value::as_array)
        .is_some_and(|inputs| inputs.iter().any(|input| input["id"] == "shadowed")));
    assert!(!state
        .sdk_config
        .load(TEST_INSTANCE_ID)
        .mcp
        .servers
        .iter()
        .any(|item| item.name == "shadowed-input-server"));
}

#[tokio::test]
async fn start_all_materializes_a_new_input_definition_before_batch_retry() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let missing_input_server: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": "batch-picture",
        "server_parameters": {
            "command": "node",
            "args": [common::echo_server_path().to_str().unwrap()],
            "env": { "OPENROUTER_API_KEY": "${input:batch-openrouterkey}" }
        }
    }))
    .unwrap();
    sdk_config::upsert_computer_mcp_config_core(&state, TEST_INSTANCE_ID, missing_input_server)
        .await
        .unwrap_err();
    sdk_config::upsert_computer_mcp_config_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config("batch-healthy"),
    )
    .await
    .unwrap();
    start_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();

    let definition_error = mcp::start_all_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap_err();
    assert!(matches!(
        &definition_error,
        RuntimeActionError::MissingInputDefinition { input_id, .. }
            if input_id == "batch-openrouterkey"
    ));
    assert_eq!(
        serde_json::to_value(&definition_error).unwrap()["requesting_mcp"],
        serde_json::json!({
            "bundle_id": "batch-picture",
            "name": "batch-picture"
        })
    );

    inputs::add_or_update_input_core(
        &state,
        TEST_INSTANCE_ID,
        inputs::InputDefinition::PromptString {
            id: "batch-openrouterkey".to_string(),
            label: Some("batch-openrouterkey".to_string()),
            description: Some("Batch OpenRouter API key".to_string()),
            default: None,
            password: Some(true),
        },
    )
    .await
    .unwrap();
    let missing_entry_batch = mcp::start_all_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(missing_entry_batch.candidate_count, 2);
    assert_eq!(missing_entry_batch.actual_operation_count, 0);
    assert_eq!(missing_entry_batch.unchanged_count, 1);
    assert!(matches!(
        missing_entry_batch.failures.as_slice(),
        [mcp::McpBatchFailure {
            error: RuntimeActionError::ResolverFailed { input_id, message },
            ..
        }] if input_id == "batch-openrouterkey"
            && message.contains("user confirmation is required")
    ));
    inputs::upsert_input_entry_core(
        &state,
        TEST_INSTANCE_ID,
        "batch-openrouterkey",
        Some("batch-test-secret".to_string()),
        true,
    )
    .await
    .unwrap();

    let retry_batch = mcp::start_all_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(retry_batch.candidate_count, 2);
    assert_eq!(retry_batch.actual_operation_count, 1);
    assert_eq!(retry_batch.unchanged_count, 1);
    assert!(retry_batch.failures.is_empty());
    let servers = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(servers.iter().all(|server| server.running));
}

#[tokio::test]
async fn computer_shutdown_drains_one_in_flight_start_and_rejects_queued_batch_starts() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = Arc::new(create_mcp_test_app_state(tmp.path()).await);
    computer::rename_computer_instance_core(
        &state,
        computer::RenameComputerInstanceRequest {
            id: TEST_INSTANCE_ID.to_string(),
            name: TEST_COMPUTER_NAME.to_string(),
            description: None,
            mcp_start_concurrency: Some(1),
        },
    )
    .await
    .unwrap();
    start_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();

    let marker_dir = tmp.path().join("start-markers");
    std::fs::create_dir_all(&marker_dir).unwrap();
    for index in 0..3 {
        let name = format!("shutdown-gate-{index}");
        sdk_config::upsert_computer_mcp_config_core(
            &state,
            TEST_INSTANCE_ID,
            delayed_start_server_config(&name, &marker_dir.join(format!("{name}.marker")), 6_000),
        )
        .await
        .unwrap();
    }

    let start_state = Arc::clone(&state);
    let start_task =
        tokio::spawn(
            async move { mcp::start_all_servers_core(&start_state, TEST_INSTANCE_ID).await },
        );
    timeout(Duration::from_secs(5), async {
        loop {
            if std::fs::read_dir(&marker_dir).unwrap().count() == 1 {
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the first MCP start never entered initialization");

    let stopped = timeout(
        Duration::from_secs(12),
        computer::stop_computer_instance_core(&state, TEST_INSTANCE_ID.to_string()),
    )
    .await
    .expect("Computer Stop waited for the entire queued start batch")
    .unwrap();
    assert!(!stopped.running);

    let batch = timeout(Duration::from_secs(5), start_task)
        .await
        .expect("start-all did not settle after SDK shutdown")
        .unwrap()
        .unwrap();
    assert_eq!(std::fs::read_dir(&marker_dir).unwrap().count(), 1);
    assert_eq!(batch.actual_operation_count, 1);
    assert_eq!(batch.failures.len(), 2);
    assert_eq!(
        batch
            .failures
            .iter()
            .map(|failure| failure.bundle_id.as_str())
            .collect::<Vec<_>>(),
        vec!["shutdown-gate-1", "shutdown-gate-2"]
    );
}

#[tokio::test]
async fn test_stop_all_servers_uses_sdk_computer_runtime() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    mcp::stop_all_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_start_stop_mcp_server_use_sdk_computer_runtime() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    mcp::add_mcp_server_core(&state, TEST_INSTANCE_ID, echo_server_config("sdk-single"))
        .await
        .unwrap();
    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("sdk-single"))
        .await
        .unwrap();
    let started = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(started[0].name, "sdk-single");
    assert!(started[0].running);

    mcp::stop_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("sdk-single"))
        .await
        .unwrap();
    let stopped = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(stopped[0].name, "sdk-single");
    assert!(!stopped[0].running);
}

#[tokio::test]
async fn single_start_mounts_a_user_mcp_added_after_computer_start() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    start_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();

    sdk_config::upsert_computer_mcp_config_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config("added-after-start"),
    )
    .await
    .unwrap();

    mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("added-after-start"))
        .await
        .unwrap();

    let statuses = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(statuses.iter().any(|status| {
        status.bundle_id == bundle_id("added-after-start")
            && status.activation_state == MCPServerActivationState::Started
            && status.connection_state == MCPServerConnectionState::Connected
    }));
}

#[tokio::test]
async fn single_start_uses_latest_user_config_without_falling_back_to_runtime_config() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    sdk_config::upsert_computer_mcp_config_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config("latest-single"),
    )
    .await
    .unwrap();
    start_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    mcp::stop_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("latest-single"))
        .await
        .unwrap();

    sdk_config::upsert_computer_mcp_config_core(
        &state,
        TEST_INSTANCE_ID,
        unavailable_server_config("latest-single"),
    )
    .await
    .unwrap();
    let error = mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("latest-single"))
        .await
        .expect_err("the latest unavailable command must replace the old runnable declaration");
    assert!(matches!(error, RuntimeActionError::RuntimeError { .. }));

    let _ = mcp::stop_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("latest-single")).await;
    sdk_config::upsert_computer_mcp_config_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config("latest-single"),
    )
    .await
    .unwrap();
    mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("latest-single"))
        .await
        .expect("retry must mount and start the corrected latest declaration");
}

#[tokio::test]
async fn start_all_uses_latest_user_snapshot_and_excludes_disabled_or_removed_servers() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    start_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();

    sdk_config::upsert_computer_mcp_config_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config("latest-batch"),
    )
    .await
    .unwrap();
    let started = mcp::start_all_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(started.candidate_count, 1);
    assert_eq!(started.actual_operation_count, 1);
    assert!(started.failures.is_empty());

    mcp::stop_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("latest-batch"))
        .await
        .unwrap();
    sdk_config::upsert_computer_mcp_config_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config_with_disabled("latest-batch", true),
    )
    .await
    .unwrap();
    let disabled = mcp::start_all_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(disabled.candidate_count, 0);
    assert!(
        mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("latest-batch"))
            .await
            .unwrap_err()
            .to_string()
            .contains("disabled")
    );

    sdk_config::remove_computer_mcp_config_core(&state, TEST_INSTANCE_ID, "latest-batch")
        .await
        .unwrap();
    let removed = mcp::start_all_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(removed.candidate_count, 0);
    assert!(
        mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("latest-batch"))
            .await
            .unwrap_err()
            .to_string()
            .contains("Server not found")
    );
}

#[tokio::test]
async fn start_all_restarts_an_error_with_the_latest_user_config() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    sdk_config::upsert_computer_mcp_config_core(
        &state,
        TEST_INSTANCE_ID,
        unavailable_server_config("latest-batch-retry"),
    )
    .await
    .unwrap();
    start_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();

    sdk_config::upsert_computer_mcp_config_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config("latest-batch-retry"),
    )
    .await
    .unwrap();
    let result = mcp::start_all_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(result.candidate_count, 1);
    assert_eq!(result.actual_operation_count, 1);
    assert!(result.failures.is_empty());
    let statuses = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(statuses.iter().any(|status| {
        status.bundle_id == bundle_id("latest-batch-retry")
            && status.activation_state == MCPServerActivationState::Started
            && status.connection_state == MCPServerConnectionState::Connected
    }));
}

#[tokio::test]
async fn actual_start_syncs_the_latest_referenced_input_definition() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    inputs::add_or_update_input_core(
        &state,
        TEST_INSTANCE_ID,
        inputs::InputDefinition::PromptString {
            id: "fresh-definition".to_string(),
            label: Some("Fresh definition".to_string()),
            description: Some("before start".to_string()),
            default: None,
            password: Some(false),
        },
    )
    .await
    .unwrap();
    inputs::set_input_value_core(
        &state,
        TEST_INSTANCE_ID,
        "fresh-definition".to_string(),
        serde_json::json!("available-value"),
    )
    .await
    .unwrap();
    let server: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": "fresh-input-server",
        "bundle_id": "fresh-input-server",
        "disabled": false,
        "server_parameters": {
            "command": "node",
            "args": [common::echo_server_path().to_str().unwrap()],
            "env": { "TOKEN": "${input:fresh-definition}" }
        }
    }))
    .unwrap();
    sdk_config::upsert_computer_mcp_config_core(&state, TEST_INSTANCE_ID, server)
        .await
        .unwrap();
    start_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    mcp::stop_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("fresh-input-server"))
        .await
        .unwrap();

    inputs::add_or_update_input_core(
        &state,
        TEST_INSTANCE_ID,
        inputs::InputDefinition::PromptString {
            id: "fresh-definition".to_string(),
            label: Some("Latest definition".to_string()),
            description: Some("latest before actual start".to_string()),
            default: None,
            password: Some(false),
        },
    )
    .await
    .unwrap();
    mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("fresh-input-server"))
        .await
        .unwrap();

    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    let definition = runtime
        .runtime_input_definition("fresh-definition")
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(definition).unwrap()["description"],
        "Latest definition"
    );
}

#[tokio::test]
async fn test_mcp_lifecycle_requires_started_computer() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    mcp::add_mcp_server_core(&state, TEST_INSTANCE_ID, echo_server_config("not-started"))
        .await
        .unwrap();

    let start_err = mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("not-started"))
        .await
        .unwrap_err();
    let stop_err = mcp::stop_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("not-started"))
        .await
        .unwrap_err();
    let start_all_err = mcp::start_all_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap_err();
    let stop_all_err = mcp::stop_all_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap_err();

    for err in [
        start_err.to_string(),
        stop_err.to_string(),
        start_all_err.to_string(),
        stop_all_err.to_string(),
    ] {
        assert_eq!(
            err,
            "runtime action 'manage_mcp' is unavailable while lifecycle is 'created' (not_running)"
        );
    }
}

#[tokio::test]
async fn test_config_add_keeps_existing_process_and_applies_after_restart() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    mcp::add_mcp_server_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config("already-running"),
    )
    .await
    .unwrap();
    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("already-running"))
        .await
        .unwrap();

    sdk_config::upsert_computer_mcp_config_core(
        &state,
        TEST_INSTANCE_ID,
        slow_echo_server_config("newly-added"),
    )
    .await
    .unwrap();

    let statuses = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    let active = statuses
        .iter()
        .find(|status| status.name == "already-running")
        .expect("active server status should exist");
    assert!(active.running, "active server should remain running");
    assert!(statuses
        .iter()
        .any(|status| status.name == "newly-added" && !status.running));
    computer::restart_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    let statuses = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(statuses
        .iter()
        .any(|status| status.name == "newly-added" && status.running));
}

#[tokio::test]
async fn test_connected_config_update_does_not_hot_sync_or_rejoin_computer() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let (server_url, stats) = start_sync_capture_smcp_server().await;

    connect_runtime_to_mock_robot(&state, &server_url).await;
    wait_for_sync_event("mock robot never observed the SMCP join event", || {
        stats.join_events() == 1
    })
    .await;

    let config_events_before = stats.update_config_events();
    sdk_config::upsert_computer_mcp_config_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config("echo-sync"),
    )
    .await
    .unwrap();
    sdk_config::upsert_computer_mcp_config_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config_with_forbidden_tools("echo-sync", &["echo"]),
    )
    .await
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(stats.update_config_events(), config_events_before);
    assert_eq!(stats.join_events(), 1, "saved config must not reconnect");
    let config_events_after_upsert = stats.update_config_events();
    sdk_config::remove_computer_mcp_config_core(&state, TEST_INSTANCE_ID, "echo-sync")
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(stats.update_config_events(), config_events_after_upsert);
    assert_eq!(
        stats.join_events(),
        1,
        "saved config removal must not reconnect"
    );
}

#[tokio::test]
async fn test_config_runtime_tool_and_robot_capability_sync_full_chain() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    runtime.start().await.unwrap();
    let initial_snapshot = runtime.runtime_snapshot().await;

    sdk_config::upsert_computer_mcp_config_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config("full-chain-echo"),
    )
    .await
    .unwrap();
    assert!(state
        .sdk_config
        .load(TEST_INSTANCE_ID)
        .mcp
        .servers
        .iter()
        .any(|server| server.name == "full-chain-echo"));
    assert!(!runtime
        .sdk_mcp_server_ids()
        .await
        .contains(&bundle_id("full-chain-echo")));
    runtime.restart().await.unwrap();
    assert!(runtime
        .sdk_mcp_server_ids()
        .await
        .contains(&bundle_id("full-chain-echo")));
    let tools = debug::get_available_tools_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(tools
        .iter()
        .any(|tool| tool.name == "full-chain-echo__echo"));
    let call = debug::execute_tool_core(
        &state,
        TEST_INSTANCE_ID,
        "full-chain-echo__echo",
        serde_json::json!({ "message": "TFRC-68 full chain" }),
        Some(5.0),
    )
    .await
    .unwrap();
    assert!(call.success);

    let (server_url, stats) = start_sync_capture_smcp_server().await;
    connect_runtime_to_mock_robot(&state, &server_url).await;
    wait_for_sync_event("mock robot never observed the SMCP join event", || {
        stats.join_events() == 1
    })
    .await;
    let tool_events_before_restart = stats.update_tool_list_events();

    mcp::stop_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("full-chain-echo"))
        .await
        .unwrap();
    wait_for_sync_event(
        "MCP stop was not synchronized to the connected robot",
        || stats.update_tool_list_events() == tool_events_before_restart + 1,
    )
    .await;
    let tool_events_after_stop = stats.update_tool_list_events();
    assert_eq!(tool_events_after_stop, tool_events_before_restart + 1);
    assert!(
        stats.update_tool_list_computers()[tool_events_before_restart..tool_events_after_stop]
            .iter()
            .all(|computer| computer == TEST_COMPUTER_NAME)
    );

    mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("full-chain-echo"))
        .await
        .unwrap();
    wait_for_sync_event(
        "MCP start was not synchronized to the connected robot",
        || stats.update_tool_list_events() == tool_events_after_stop + 1,
    )
    .await;
    let tool_events_after_start = stats.update_tool_list_events();
    assert_eq!(tool_events_after_start, tool_events_after_stop + 1);
    assert!(
        stats.update_tool_list_computers()[tool_events_after_stop..tool_events_after_start]
            .iter()
            .all(|computer| computer == TEST_COMPUTER_NAME)
    );

    let final_snapshot = runtime.runtime_snapshot().await;
    assert!(final_snapshot.generation > initial_snapshot.generation);
    assert!(final_snapshot.capability_revision > initial_snapshot.capability_revision);
    assert_eq!(final_snapshot.active_mcp_servers, 1);
    assert_eq!(final_snapshot.tools, 1);

    let dashboard = get_dashboard_data_core(&state).await.unwrap();
    let dashboard_computer = dashboard
        .computers
        .iter()
        .find(|computer| computer.id == TEST_INSTANCE_ID)
        .unwrap();
    assert_eq!(dashboard_computer.runtime.tools, final_snapshot.tools);
    assert_eq!(
        dashboard_computer.runtime.capability_revision,
        final_snapshot.capability_revision
    );
    assert!(dashboard_computer.connected);
    assert_eq!(
        dashboard_computer.runtime.config_revision,
        final_snapshot.config_revision
    );

    runtime.disconnect_smcp_socketio().await.unwrap();
}

#[tokio::test]
async fn disabling_robot_control_preserves_smcp_and_unrelated_mcp_runtime() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    mcp::add_mcp_server_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config_with_bundle_id("unrelated-echo", "unrelated-echo"),
    )
    .await
    .unwrap();
    client_control::update_remote_control_policy_core(
        &state,
        UpdateRemoteControlPolicyRequest {
            computer_id: TEST_INSTANCE_ID.to_string(),
            policy: RemoteControlPolicy {
                enabled: true,
                ..RemoteControlPolicy::default()
            },
        },
    )
    .await
    .unwrap();
    start_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();

    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    let before = runtime.runtime_snapshot().await;
    assert!(runtime
        .mcp_server_runtime_statuses()
        .await
        .iter()
        .any(|status| {
            status.bundle_id.as_str() == CLIENT_CONTROL_BUNDLE_ID && status.is_connected()
        }));
    assert!(runtime
        .mcp_server_runtime_statuses()
        .await
        .iter()
        .any(|status| status.bundle_id.as_str() == "unrelated-echo" && status.is_connected()));

    let (server_url, stats) = start_sync_capture_smcp_server().await;
    connect_runtime_to_mock_robot(&state, &server_url).await;
    wait_for_sync_event("mock robot never observed the SMCP join event", || {
        stats.join_events() == 1
    })
    .await;
    let tool_events_before_disable = stats.update_tool_list_events();

    client_control::update_remote_control_policy_core(
        &state,
        UpdateRemoteControlPolicyRequest {
            computer_id: TEST_INSTANCE_ID.to_string(),
            policy: RemoteControlPolicy::default(),
        },
    )
    .await
    .unwrap();
    wait_for_sync_event(
        "Robot control removal was not synchronized to the connected robot",
        || stats.update_tool_list_events() > tool_events_before_disable,
    )
    .await;

    let after = runtime.runtime_snapshot().await;
    assert_eq!(after.generation, before.generation);
    assert_eq!(
        stats.join_events(),
        1,
        "policy save must not reconnect SMCP"
    );
    assert!(runtime
        .connection_handle_for_test()
        .read_owned()
        .await
        .is_some());
    assert!(runtime
        .mcp_server_runtime_statuses()
        .await
        .iter()
        .all(|status| status.bundle_id.as_str() != CLIENT_CONTROL_BUNDLE_ID));
    assert!(runtime
        .mcp_server_runtime_statuses()
        .await
        .iter()
        .any(|status| status.bundle_id.as_str() == "unrelated-echo" && status.is_connected()));
    assert!(
        stats.update_tool_list_computers()[tool_events_before_disable..]
            .iter()
            .all(|computer| computer == TEST_COMPUTER_NAME)
    );

    let call = debug::execute_tool_core(
        &state,
        TEST_INSTANCE_ID,
        "unrelated-echo__echo",
        serde_json::json!({ "message": "still running" }),
        Some(5.0),
    )
    .await
    .unwrap();
    assert!(call.success);

    runtime.disconnect_smcp_socketio().await.unwrap();
}

#[tokio::test]
async fn test_start_all_servers_uses_sdk_computer_runtime() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    mcp::add_mcp_server_core(&state, TEST_INSTANCE_ID, echo_server_config("sdk-all"))
        .await
        .unwrap();
    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap()
        .restart()
        .await
        .unwrap();
    let stopped = mcp::stop_all_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(stopped.candidate_count, 1);
    assert_eq!(stopped.actual_operation_count, 1);
    assert_eq!(stopped.unchanged_count, 0);
    assert!(stopped.failures.is_empty());

    let started = mcp::start_all_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(started.candidate_count, 1);
    assert_eq!(started.actual_operation_count, 1);
    assert_eq!(started.unchanged_count, 0);
    assert!(started.failures.is_empty());
    let statuses = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(statuses[0].name, "sdk-all");
    assert!(statuses[0].running);

    let stopped = mcp::stop_all_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(stopped.actual_operation_count, 1);
    assert!(stopped.failures.is_empty());
}

#[tokio::test]
async fn test_remove_mcp_server_command_applies_to_runtime_after_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    mcp::add_mcp_server_core(&state, TEST_INSTANCE_ID, echo_server_config("remove-me"))
        .await
        .unwrap();
    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    runtime.start().await.unwrap();
    assert!(runtime
        .synced_sdk_servers()
        .await
        .contains_key(&bundle_id("remove-me")));

    mcp::remove_mcp_server_core(&state, TEST_INSTANCE_ID, "remove-me")
        .await
        .unwrap();

    let configs = state.sdk_config.load(TEST_INSTANCE_ID).mcp.servers;
    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();

    assert!(configs.is_empty());
    assert!(runtime
        .synced_sdk_servers()
        .await
        .contains_key(&bundle_id("remove-me")));
    runtime.restart().await.unwrap();
    assert!(!runtime
        .sdk_mcp_server_ids()
        .await
        .contains(&bundle_id("remove-me")));
}

#[tokio::test]
async fn test_debug_get_available_tools_uses_sdk_computer() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    mcp::add_mcp_server_core(&state, TEST_INSTANCE_ID, echo_server_config("debug-tools"))
        .await
        .unwrap();
    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("debug-tools"))
        .await
        .unwrap();
    let tools = debug::get_available_tools_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();

    let echo = tools
        .iter()
        .find(|tool| tool.name == "debug-tools__echo")
        .unwrap();
    assert_eq!(echo.display_name, "echo");
    assert_eq!(echo.server, "debug-tools");
}

#[tokio::test]
async fn test_default_tool_meta_alias_does_not_override_individual_tool_names() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    let mut config = multi_tool_server_config("test");
    match &mut config {
        MCPServerConfig::Stdio(stdio) => {
            stdio.default_tool_meta = Some(a2c_smcp::smcp_computer::mcp_clients::ToolMeta {
                auto_apply: Some(true),
                alias: Some("123".to_string()),
                tags: None,
                ret_object_mapper: None,
            });
        }
        _ => panic!("expected stdio config"),
    }
    mcp::add_mcp_server_core(&state, TEST_INSTANCE_ID, config)
        .await
        .unwrap();

    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("test"))
        .await
        .expect("default alias metadata must not collapse all tool names");
    let tools = debug::get_available_tools_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    let names = tools
        .into_iter()
        .map(|tool| tool.name)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        names,
        std::collections::BTreeSet::from([
            "test__first-tool".to_string(),
            "test__second-tool".to_string(),
        ])
    );
}

#[tokio::test]
async fn test_blank_default_tool_meta_alias_does_not_collide_on_first_start() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    let mut config = multi_tool_server_config("test");
    match &mut config {
        MCPServerConfig::Stdio(stdio) => {
            stdio.default_tool_meta = Some(a2c_smcp::smcp_computer::mcp_clients::ToolMeta {
                auto_apply: None,
                alias: Some("".to_string()),
                tags: None,
                ret_object_mapper: None,
            });
        }
        _ => panic!("expected stdio config"),
    }
    mcp::add_mcp_server_core(&state, TEST_INSTANCE_ID, config)
        .await
        .unwrap();

    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("test"))
        .await
        .expect("blank aliases should be ignored before the SDK validates tool names");
}

#[tokio::test]
async fn test_debug_get_available_tools_keeps_unknown_server_for_multiple_running_servers() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    mcp::add_mcp_server_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config("debug-tools-one"),
    )
    .await
    .unwrap();
    mcp::add_mcp_server_core(
        &state,
        TEST_INSTANCE_ID,
        slow_echo_server_config("debug-tools-two"),
    )
    .await
    .unwrap();
    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    mcp::start_all_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    let tools = debug::get_available_tools_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();

    assert!(tools
        .iter()
        .any(|tool| tool.name == "debug-tools-one__echo"));
    assert!(tools
        .iter()
        .any(|tool| tool.name == "debug-tools-two__slow_echo"));
    assert!(tools.iter().all(|tool| tool.server == "unknown"));
}

#[tokio::test]
async fn test_sdk_available_tools_raw_metadata_with_multiple_servers() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    mcp::add_mcp_server_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config("raw-meta-echo"),
    )
    .await
    .unwrap();
    mcp::add_mcp_server_core(
        &state,
        TEST_INSTANCE_ID,
        slow_echo_server_config("raw-meta-slow"),
    )
    .await
    .unwrap();
    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    mcp::start_all_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();

    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    let statuses = runtime.mcp_server_runtime_statuses().await;
    eprintln!("server_statuses={statuses:#?}");

    let tools = runtime.available_tools().await.unwrap();
    for tool in &tools {
        eprintln!(
            "raw_tool name={} description={:?} meta={:#?}",
            tool.name, tool.description, tool.meta
        );
    }

    assert!(tools
        .iter()
        .any(|tool| tool.name.as_ref() == "raw-meta-echo__echo"));
    assert!(tools
        .iter()
        .any(|tool| tool.name.as_ref() == "raw-meta-slow__slow_echo"));
    assert!(tools.iter().all(|tool| {
        tool.meta
            .as_ref()
            .and_then(|meta| meta.get("server_name"))
            .is_none()
    }));
}

#[tokio::test]
async fn test_debug_execute_tool_uses_sdk_computer_and_logs_redacted_history() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    mcp::add_mcp_server_core(&state, TEST_INSTANCE_ID, echo_server_config("debug-exec"))
        .await
        .unwrap();
    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("debug-exec"))
        .await
        .unwrap();
    let first = debug::execute_tool_core(
        &state,
        TEST_INSTANCE_ID,
        "debug-exec__echo",
        serde_json::json!({
            "message": "hello from sdk computer",
            "api_key": "secret-key"
        }),
        Some(5.0),
    )
    .await
    .unwrap();
    let second = debug::execute_tool_core(
        &state,
        TEST_INSTANCE_ID,
        "debug-exec__echo",
        serde_json::json!({ "message": "second call" }),
        Some(5.0),
    )
    .await
    .unwrap();

    assert!(first.success);
    assert!(second.success);
    let history = debug::get_tool_history_core(&state, TEST_INSTANCE_ID).unwrap();
    assert_eq!(history.len(), 2);
    assert_ne!(history[0].req_id, history[1].req_id);
    assert!(history
        .iter()
        .any(|record| record.parameters["api_key"] == "[REDACTED]"));
    assert!(history.iter().all(|record| record.server == "debug-exec"));

    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(runtime.sdk_tool_history().await.unwrap().len(), 2);
}

#[tokio::test]
async fn test_debug_execute_tool_uses_sdk_timeout_result() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    mcp::add_mcp_server_core(
        &state,
        TEST_INSTANCE_ID,
        slow_echo_server_config("debug-timeout"),
    )
    .await
    .unwrap();
    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("debug-timeout"))
        .await
        .unwrap();
    let response = debug::execute_tool_core(
        &state,
        TEST_INSTANCE_ID,
        "debug-timeout__slow_echo",
        serde_json::json!({ "message": "too slow", "delayMs": 1000 }),
        Some(0.05),
    )
    .await
    .unwrap();

    assert!(!response.success);
    let result = response
        .result
        .expect("timeout should return an SDK error result");
    let value = serde_json::to_value(result).unwrap();
    assert_eq!(value["_meta"]["a2c_timeout"], true);
    let history = debug::get_tool_history_core(&state, TEST_INSTANCE_ID).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(
        history[0].error.as_deref(),
        Some("工具调用超时 / Tool call timed out")
    );
}

#[tokio::test]
async fn test_config_io_requires_instance_id() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let export_path = tmp.path().join("export.json");

    let result = config_io::export_config_core(
        &state,
        export_path.to_string_lossy().to_string(),
        "".into(),
        None,
    )
    .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().contains("instance_id is required"));

    let import_path = tmp.path().join("orphan-import.json");
    std::fs::write(
        &import_path,
        serde_json::to_vec(&serde_json::json!({
            "servers": [echo_server_config("orphan")],
            "inputs": []
        }))
        .unwrap(),
    )
    .unwrap();
    let missing = config_io::import_config_core(
        &state,
        import_path.to_string_lossy().into_owned(),
        "missing-computer".to_string(),
        None,
    )
    .await;
    assert!(missing.is_err());
    assert!(!state.sdk_config.project_anchor("missing-computer").exists());
}

#[tokio::test]
async fn test_config_io_import_export_are_instance_scoped() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    state
        .config
        .add_computer_instance(ComputerInstance::new("second", "Second"))
        .unwrap();
    state
        .computer_registry
        .upsert_runtime(state.config.get_computer_instance("second").unwrap())
        .await
        .unwrap();

    let import_path = tmp.path().join("import.json");
    let export_path = tmp.path().join("export.json");
    let import_data = serde_json::json!({
        "servers": [echo_server_config("imported-second")],
        "inputs": []
    });
    std::fs::write(
        &import_path,
        serde_json::to_string_pretty(&import_data).unwrap(),
    )
    .unwrap();

    let import_result = config_io::import_config_core(
        &state,
        import_path.to_string_lossy().to_string(),
        "second".into(),
        None,
    )
    .await
    .unwrap();
    state
        .sdk_config
        .save(
            "second",
            &ProjectConfigDoc {
                mcp: Some(
                    serde_json::json!({
                        "servers": {
                            "exported-second": {
                                "type": "stdio",
                                "server_parameters": {
                                    "command": "node",
                                    "args": [],
                                    "env": {}
                                }
                            }
                        }
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
                ..Default::default()
            },
        )
        .unwrap();
    config_io::export_config_core(
        &state,
        export_path.to_string_lossy().to_string(),
        "second".into(),
        None,
    )
    .await
    .unwrap();

    let default_configs = state.sdk_config.load(TEST_INSTANCE_ID).mcp.servers;
    let second_configs = state.sdk_config.load("second").mcp.servers;
    let second_runtime = state.computer_registry.runtime("second").await.unwrap();
    let second_sdk_servers = second_runtime.synced_sdk_servers().await;
    let default_runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    let default_sdk_servers = default_runtime.synced_sdk_servers().await;
    let exported: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&export_path).unwrap()).unwrap();

    assert_eq!(import_result.servers_imported, 1);
    assert!(default_configs.is_empty());
    assert_eq!(second_configs.len(), 2);
    assert!(second_configs
        .iter()
        .any(|server| server.name == "imported-second"));
    assert!(second_configs
        .iter()
        .any(|server| server.name == "exported-second"));
    assert!(!second_sdk_servers.contains_key(&bundle_id("imported-second")));
    assert!(!default_sdk_servers.contains_key(&bundle_id("imported-second")));
    let exported_names: std::collections::HashSet<_> = exported["servers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|server| server["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        exported_names,
        std::collections::HashSet::from(["imported-second", "exported-second"])
    );
    second_runtime.start().await.unwrap();
    assert!(second_runtime
        .synced_sdk_servers()
        .await
        .contains_key(&bundle_id("imported-second")));

    let round_trip = config_io::import_config_core(
        &state,
        export_path.to_string_lossy().to_string(),
        TEST_INSTANCE_ID.to_string(),
        None,
    )
    .await
    .unwrap();
    assert_eq!(round_trip.servers_imported, 2);
    assert_eq!(state.sdk_config.load(TEST_INSTANCE_ID).mcp.servers.len(), 2);
}

#[tokio::test]
async fn test_config_io_import_atomically_merges_all_servers_into_local_sdk_config() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    state
        .sdk_config
        .save(
            TEST_INSTANCE_ID,
            &ProjectConfigDoc {
                mcp_local: Some(
                    serde_json::json!({
                        "servers": {
                            "existing-local": {
                                "type": "stdio",
                                "server_parameters": {
                                    "command": "existing-command",
                                    "args": [],
                                    "env": {}
                                }
                            }
                        },
                        "clientMetadata": {"preserved": true}
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
                ..Default::default()
            },
        )
        .unwrap();

    let import_path = tmp.path().join("atomic-import.json");
    std::fs::write(
        &import_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "servers": [
                echo_server_config("atomic-a"),
                echo_server_config("atomic-b")
            ],
            "inputs": []
        }))
        .unwrap(),
    )
    .unwrap();

    let result = config_io::import_config_core(
        &state,
        import_path.to_string_lossy().into_owned(),
        TEST_INSTANCE_ID.to_string(),
        None,
    )
    .await
    .unwrap();

    assert_eq!(result.servers_imported, 2);
    let document = a2c_smcp::smcp_computer::settings::config::load_project_config_doc(
        &state.sdk_config.project_anchor(TEST_INSTANCE_ID),
    )
    .unwrap();
    let local_mcp = document.mcp_local.unwrap();
    let local_servers = local_mcp["servers"].as_object().unwrap();
    assert!(local_servers.contains_key("existing-local"));
    assert!(local_servers.contains_key("atomic-a"));
    assert!(local_servers.contains_key("atomic-b"));
    assert_eq!(local_mcp["clientMetadata"]["preserved"], true);
}

#[tokio::test]
async fn test_config_io_input_only_import_updates_sdk_owned_top_level_inputs() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let before_document = a2c_smcp::smcp_computer::settings::config::load_project_config_doc(
        &state.sdk_config.project_anchor(TEST_INSTANCE_ID),
    )
    .unwrap();
    let before_revision = state.sdk_config.load(TEST_INSTANCE_ID).revision;
    let import_path = tmp.path().join("input-only-import.json");
    std::fs::write(
        &import_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "servers": [],
            "inputs": [{
                "type": "PromptString",
                "id": "input-only",
                "label": "Input only",
                "password": false
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    let result = config_io::import_config_core(
        &state,
        import_path.to_string_lossy().into_owned(),
        TEST_INSTANCE_ID.to_string(),
        None,
    )
    .await
    .unwrap();

    let after_document = a2c_smcp::smcp_computer::settings::config::load_project_config_doc(
        &state.sdk_config.project_anchor(TEST_INSTANCE_ID),
    )
    .unwrap();
    assert_eq!(result.servers_imported, 0);
    assert_eq!(result.inputs_imported, 1);
    assert_ne!(after_document, before_document);
    assert_eq!(after_document.mcp.unwrap()["inputs"][0]["id"], "input-only");
    assert_ne!(
        state.sdk_config.load(TEST_INSTANCE_ID).revision,
        before_revision
    );
}

#[tokio::test]
async fn test_config_io_rejects_sensitive_command_input_args_before_import_or_export_write() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let import_path = tmp.path().join("unsafe-command-input.json");
    std::fs::write(
        &import_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "servers": [],
            "inputs": [{
                "type": "Command",
                "id": "unsafe-command",
                "label": "Unsafe command",
                "command": "helper",
                "args": ["https://example.com/run?access_token=plain-command-secret"]
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    let import_error = config_io::import_config_core(
        &state,
        import_path.to_string_lossy().into_owned(),
        TEST_INSTANCE_ID.to_string(),
        None,
    )
    .await
    .unwrap_err();
    assert!(import_error.contains("inputs.unsafe-command.args[0]"));
    assert!(import_error.contains("access_token"));
    assert!(inputs::list_inputs_core(&state, TEST_INSTANCE_ID)
        .unwrap()
        .is_empty());
    assert!(!state
        .config
        .computer_instance_storage_root(TEST_INSTANCE_ID)
        .join("config_import_transaction.json")
        .exists());

    let save_error = inputs::add_or_update_input_core(
        &state,
        TEST_INSTANCE_ID,
        inputs::InputDefinition::Command {
            id: "unsafe-command".to_string(),
            label: Some("Unsafe command".to_string()),
            command: "helper".to_string(),
            args: Some(vec!["--api-key=plain-export-secret".to_string()]),
        },
    )
    .await
    .unwrap_err();
    assert!(save_error
        .to_string()
        .contains("inputs.unsafe-command.args[0]"));
    assert!(inputs::list_inputs_core(&state, TEST_INSTANCE_ID)
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn test_local_config_crud_preserves_field_literals_but_rejects_sensitive_cli_plaintext() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let unsafe_server: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "Stdio",
        "name": "unsafe-crud-server",
        "server_parameters": {
            "command": "helper",
            "args": ["--endpoint", "https://example.com/run?client_secret=plain-crud-secret"],
            "env": {}
        }
    }))
    .unwrap();

    let server_error =
        sdk_config::upsert_computer_mcp_config_core(&state, TEST_INSTANCE_ID, unsafe_server)
            .await
            .unwrap_err();
    let server_error = server_error.to_string();
    assert!(server_error.contains("server_parameters.args[1]"));
    assert!(server_error.contains("client_secret"));
    assert!(state
        .sdk_config
        .load(TEST_INSTANCE_ID)
        .mcp
        .servers
        .is_empty());

    inputs::add_or_update_input_core(
        &state,
        TEST_INSTANCE_ID,
        inputs::InputDefinition::PromptString {
            id: "REGION".to_string(),
            label: Some("Region".to_string()),
            description: None,
            default: Some("cn".to_string()),
            password: Some(false),
        },
    )
    .await
    .unwrap();

    let literal_stdio: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "Stdio",
        "name": "literal-crud-stdio",
        "server_parameters": {
            "command": "helper",
            "args": [],
            "env": {
                "LOG_LEVEL": "debug",
                "MUSTACHE_LITERAL": "{{REGION}}",
                "PREFIXED_MUSTACHE_LITERAL": "prefix-{{REGION}}",
                "REGION": "${input:REGION}",
                "PREFIXED_INPUT": "prefix-${input:REGION}"
            }
        }
    }))
    .unwrap();
    sdk_config::upsert_computer_mcp_config_core(&state, TEST_INSTANCE_ID, literal_stdio)
        .await
        .unwrap();

    let literal_http: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "Http",
        "name": "literal-crud-http",
        "server_parameters": {
            "url": "https://example.com/mcp",
            "headers": { "X-Deployment": "staging" }
        }
    }))
    .unwrap();
    sdk_config::upsert_computer_mcp_config_core(&state, TEST_INSTANCE_ID, literal_http)
        .await
        .unwrap();

    let persisted = std::fs::read_to_string(
        state
            .sdk_config
            .project_anchor(TEST_INSTANCE_ID)
            .join(".tfrobot/mcp.local.json"),
    )
    .unwrap();
    assert!(persisted.contains("debug"));
    assert!(persisted.contains("{{REGION}}"));
    assert!(persisted.contains("prefix-{{REGION}}"));
    assert!(persisted.contains("${input:REGION}"));
    assert!(persisted.contains("prefix-${input:REGION}"));
    assert!(persisted.contains("staging"));
    assert!(!persisted.contains("${REDACTED}"));

    let view = sdk_config::get_computer_config_state_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    let stdio_view = view
        .snapshot
        .mcp
        .servers
        .iter()
        .find(|server| server.name == "literal-crud-stdio")
        .expect("local view should include the literal stdio declaration");
    let stdio_value = serde_json::to_value(&stdio_view.config).unwrap();
    let env = &stdio_value["server_parameters"]["env"];
    assert_eq!(env["MUSTACHE_LITERAL"], "{{REGION}}");
    assert_eq!(env["PREFIXED_MUSTACHE_LITERAL"], "prefix-{{REGION}}");
    assert_eq!(env["REGION"], "${input:REGION}");
    assert_eq!(env["PREFIXED_INPUT"], "prefix-${input:REGION}");
    let view_json = serde_json::to_string(&view).unwrap();
    assert!(view_json.contains("debug"));
    assert!(view_json.contains("{{REGION}}"));
    assert!(view_json.contains("prefix-{{REGION}}"));
    assert!(view_json.contains("${input:REGION}"));
    assert!(view_json.contains("prefix-${input:REGION}"));
    assert!(view_json.contains("staging"));
    assert!(!view_json.contains("${REDACTED}"));

    // Same-machine configuration is an editable source of truth, so existing literals must also
    // be visible to the trusted local WebView instead of being replaced by export sentinels.
    state
        .sdk_config
        .save(
            TEST_INSTANCE_ID,
            &ProjectConfigDoc {
                mcp: Some(
                    serde_json::json!({
                        "servers": {
                            "legacy-plaintext-http": {
                                "type": "http",
                                "server_parameters": {
                                    "url": "https://example.com/mcp",
                                    "headers": {
                                        "X-Legacy-Mode": "compatibility"
                                    }
                                }
                            }
                        }
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
                ..Default::default()
            },
        )
        .unwrap();
    let legacy_raw = std::fs::read_to_string(
        state
            .sdk_config
            .project_anchor(TEST_INSTANCE_ID)
            .join(".tfrobot/mcp.json"),
    )
    .unwrap();
    assert!(legacy_raw.contains("compatibility"));
    let legacy_view = sdk_config::get_computer_config_state_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    let legacy_view_json = serde_json::to_string(&legacy_view).unwrap();
    assert!(legacy_view_json.contains("compatibility"));
    assert!(!legacy_view_json.contains("${REDACTED}"));

    let input_error = inputs::add_or_update_input_core(
        &state,
        TEST_INSTANCE_ID,
        inputs::InputDefinition::Command {
            id: "unsafe-crud-input".to_string(),
            label: Some("Unsafe CRUD input".to_string()),
            command: "helper".to_string(),
            args: Some(vec!["https://example.com/run?mode=debug".to_string()]),
        },
    )
    .await
    .unwrap_err();
    assert!(input_error.contains("inputs.unsafe-crud-input.args[0]"));
    assert!(input_error.contains("mode"));
    assert!(!inputs::list_inputs_core(&state, TEST_INSTANCE_ID)
        .unwrap()
        .iter()
        .any(|input| input.id() == "unsafe-crud-input"));
}

#[tokio::test]
async fn test_password_input_import_rejects_plaintext_defaults() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    inputs::add_or_update_input_core(
        &state,
        TEST_INSTANCE_ID,
        inputs::InputDefinition::PromptString {
            id: "crud-password".to_string(),
            label: Some("CRUD password".to_string()),
            description: None,
            default: None,
            password: Some(true),
        },
    )
    .await
    .unwrap();

    let import_path = tmp.path().join("password-inputs.json");
    std::fs::write(
        &import_path,
        serde_json::to_vec_pretty(&serde_json::json!([{
            "type": "PromptString",
            "id": "imported-password",
            "label": "Imported password",
            "default": "imported-plaintext-secret",
            "password": true
        }]))
        .unwrap(),
    )
    .unwrap();
    let error = inputs::import_inputs_core(
        &state,
        TEST_INSTANCE_ID,
        import_path.to_string_lossy().as_ref(),
    )
    .await
    .unwrap_err();
    assert!(error.contains("cannot contain a plaintext default"));

    let listed = inputs::list_inputs_core(&state, TEST_INSTANCE_ID).unwrap();
    assert!(matches!(
        listed.as_slice(),
        [inputs::InputDefinition::PromptString {
            id,
            password: Some(true),
            ..
        }] if id == "crud-password"
    ));
    let persisted = serde_json::to_string(
        &a2c_smcp::smcp_computer::settings::config::load_project_config_doc(
            &state.sdk_config.project_anchor(TEST_INSTANCE_ID),
        )
        .unwrap()
        .mcp,
    )
    .unwrap();
    assert!(!persisted.contains("default"));
    assert!(!persisted.contains("imported-plaintext-secret"));
}

#[tokio::test]
async fn test_config_import_preflights_sdk_target_before_journal_or_input_write() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    state
        .sdk_config
        .save(
            TEST_INSTANCE_ID,
            &ProjectConfigDoc {
                mcp: Some(
                    serde_json::json!({
                        "inputs": [{
                            "type": "PromptString",
                            "id": "existing-input",
                            "description": "Existing",
                            "password": false
                        }],
                        "servers": {
                            "existing-invalid": { "type": "carrier-pigeon" }
                        }
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
                ..Default::default()
            },
        )
        .unwrap();

    let import_path = tmp.path().join("preflight-import.json");
    std::fs::write(
        &import_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "servers": [{
                "type": "Stdio",
                "name": "must-not-persist",
                "server_parameters": { "command": "helper", "args": [], "env": {} }
            }],
            "inputs": [{
                "type": "PromptString",
                "id": "must-not-persist-input",
                "label": "Must not persist",
                "password": false
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    let error = config_io::import_config_core(
        &state,
        import_path.to_string_lossy().into_owned(),
        TEST_INSTANCE_ID.to_string(),
        None,
    )
    .await
    .unwrap_err();
    assert!(error.contains("servers.existing-invalid"), "{error}");
    assert!(!state
        .config
        .computer_instance_storage_root(TEST_INSTANCE_ID)
        .join("config_import_transaction.json")
        .exists());
    let inputs = state.sdk_config.load_input_definitions(TEST_INSTANCE_ID);
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].id(), "existing-input");
}

#[tokio::test]
async fn test_startup_recovers_a_crash_interrupted_config_import_transaction() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let transaction_path = state
        .config
        .computer_instance_storage_root(TEST_INSTANCE_ID)
        .join("config_import_transaction.json");
    std::fs::create_dir_all(transaction_path.parent().unwrap()).unwrap();
    std::fs::write(
        &transaction_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "version": 1,
            "instance_id": TEST_INSTANCE_ID,
            "servers": [{
                "type": "Stdio",
                "name": "recovered-server",
                "disabled": false,
                "forbidden_tools": [],
                "tool_meta": {},
                "server_parameters": {
                    "command": "echo",
                    "args": [],
                    "env": {
                        "TOKEN": "recovered-literal",
                        "TOKEN_REF": "${input:recovered-command}"
                    },
                    "cwd": "/workspace/recovered"
                }
            }],
            "inputs": [{
                "type": "Command",
                "id": "recovered-command",
                "label": "Recovered command",
                "command": "helper",
                "args": ["--mode", "recovery"]
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    drop(state);

    let recovered = create_test_app_state(tmp.path());

    assert!(!transaction_path.exists());
    let recovered_server = recovered
        .sdk_config
        .load(TEST_INSTANCE_ID)
        .mcp
        .servers
        .iter()
        .find(|server| server.name == "recovered-server")
        .map(|server| serde_json::to_value(&server.config).unwrap())
        .unwrap();
    assert_eq!(
        recovered_server["server_parameters"]["env"]["TOKEN"],
        "recovered-literal"
    );
    assert_eq!(
        recovered_server["server_parameters"]["env"]["TOKEN_REF"],
        "${input:recovered-command}"
    );
    assert_eq!(
        recovered_server["server_parameters"]["cwd"],
        "/workspace/recovered"
    );
    assert!(matches!(
        recovered
            .sdk_config
            .load_input_definitions(TEST_INSTANCE_ID)
            .as_slice(),
        [inputs::InputDefinition::Command {
            id,
            command,
            args: Some(args),
            ..
        }] if id == "recovered-command"
            && command == "helper"
            && args == &["--mode".to_string(), "recovery".to_string()]
    ));
    let runtime = recovered
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .expect("startup should build the recovered Computer runtime");
    assert!(runtime
        .inputs
        .read()
        .await
        .contains_key("recovered-command"));
}

#[test]
fn test_startup_discards_an_aborted_config_import_transaction_without_replay() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_test_app_state(tmp.path());
    state
        .config
        .add_computer_instance(ComputerInstance::new(TEST_INSTANCE_ID, "Computer A"))
        .unwrap();
    state
        .sdk_config
        .replace_input_definitions(
            TEST_INSTANCE_ID,
            &[inputs::InputDefinition::PromptString {
                id: "existing-input".to_string(),
                label: Some("Existing".to_string()),
                description: None,
                default: None,
                password: Some(false),
            }],
        )
        .unwrap();
    let transaction_path = state
        .config
        .computer_instance_storage_root(TEST_INSTANCE_ID)
        .join("config_import_transaction.json");
    std::fs::create_dir_all(transaction_path.parent().unwrap()).unwrap();
    std::fs::write(
        &transaction_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "version": 1,
            "phase": "aborted",
            "instance_id": TEST_INSTANCE_ID,
            "servers": [echo_server_config("must-not-replay")],
            "inputs": [{
                "type": "PromptString",
                "id": "must-not-replay-input",
                "label": "Must not replay",
                "password": false
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    drop(state);

    let recovered = create_test_app_state(tmp.path());

    assert!(!transaction_path.exists());
    assert!(!recovered
        .sdk_config
        .load(TEST_INSTANCE_ID)
        .mcp
        .servers
        .iter()
        .any(|server| server.name == "must-not-replay"));
    let persisted_inputs = recovered
        .sdk_config
        .load_project_input_definitions(TEST_INSTANCE_ID)
        .unwrap();
    assert_eq!(persisted_inputs.len(), 1);
    assert_eq!(persisted_inputs[0].id(), "existing-input");
}

#[test]
fn test_startup_rejects_an_unrecoverable_config_import_transaction() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_test_app_state(tmp.path());
    state
        .config
        .add_computer_instance(ComputerInstance::new(TEST_INSTANCE_ID, "Computer A"))
        .unwrap();
    let transaction_path = state
        .config
        .computer_instance_storage_root(TEST_INSTANCE_ID)
        .join("config_import_transaction.json");
    std::fs::create_dir_all(transaction_path.parent().unwrap()).unwrap();
    std::fs::write(&transaction_path, b"{not-valid-json").unwrap();
    drop(state);

    let config =
        tfrobot_client_lib::services::config::ConfigService::new(tmp.path().to_path_buf()).unwrap();
    let log_service =
        tfrobot_client_lib::services::observability::ObservabilityService::new(tmp.path()).unwrap();
    let settings_service =
        tfrobot_client_lib::services::settings::SettingsService::new(tmp.path().to_path_buf());
    let result = AppState::try_new_with_secret_store(
        config,
        log_service,
        settings_service,
        tfrobot_client_lib::services::keychain::InMemorySecretStore::shared(),
    );

    assert!(matches!(
        result,
        Err(tfrobot_client_lib::AppStateInitError::ConfigImportRecovery(
            _
        ))
    ));
    assert!(transaction_path.exists());
}

#[test]
fn test_startup_recovery_preflights_sdk_target_before_replaying_inputs() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_test_app_state(tmp.path());
    state
        .config
        .add_computer_instance(ComputerInstance::new(TEST_INSTANCE_ID, "Computer A"))
        .unwrap();
    state
        .sdk_config
        .save(
            TEST_INSTANCE_ID,
            &ProjectConfigDoc {
                mcp_local: Some(
                    serde_json::json!({
                        "servers": {
                            "existing-invalid": { "type": "carrier-pigeon" }
                        }
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
                ..Default::default()
            },
        )
        .unwrap();
    let transaction_path = state
        .config
        .computer_instance_storage_root(TEST_INSTANCE_ID)
        .join("config_import_transaction.json");
    std::fs::create_dir_all(transaction_path.parent().unwrap()).unwrap();
    std::fs::write(
        &transaction_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "version": 1,
            "instance_id": TEST_INSTANCE_ID,
            "servers": [echo_server_config("must-not-recover")],
            "inputs": [{
                "type": "PromptString",
                "id": "must-not-replay-input",
                "label": "Must not replay",
                "password": false
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    drop(state);

    let config =
        tfrobot_client_lib::services::config::ConfigService::new(tmp.path().to_path_buf()).unwrap();
    let log_service =
        tfrobot_client_lib::services::observability::ObservabilityService::new(tmp.path()).unwrap();
    let settings_service =
        tfrobot_client_lib::services::settings::SettingsService::new(tmp.path().to_path_buf());
    let result = AppState::try_new_with_secret_store(
        config,
        log_service,
        settings_service,
        tfrobot_client_lib::services::keychain::InMemorySecretStore::shared(),
    );

    assert!(matches!(
        result,
        Err(tfrobot_client_lib::AppStateInitError::ConfigImportRecovery(
            _
        ))
    ));
    assert!(transaction_path.exists());
    let config =
        tfrobot_client_lib::services::config::ConfigService::new(tmp.path().to_path_buf()).unwrap();
    let sdk_config =
        tfrobot_client_lib::services::sdk_config::SdkConfigService::new(Arc::new(config));
    assert!(sdk_config
        .load_project_input_definitions(TEST_INSTANCE_ID)
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn test_config_io_export_preserves_literals_without_exporting_input_values() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let export_path = tmp.path().join("shareable-export.json");

    state
        .sdk_config
        .save(
            TEST_INSTANCE_ID,
            &ProjectConfigDoc {
                mcp: Some(
                    serde_json::json!({
                        "servers": {
                            "secret-stdio": {
                                "type": "stdio",
                                "server_parameters": {
                                    "command": "node",
                                    "args": [],
                                    "env": {
                                        "TOKEN": "literal-secret",
                                        "TOKEN_REF": "${env:TOKEN}"
                                    },
                                    "cwd": "/workspace/source"
                                }
                            },
                            "secret-http": {
                                "type": "http",
                                "server_parameters": {
                                    "url": "https://user:password@example.com/mcp",
                                    "headers": {
                                        "Authorization": "Bearer literal-secret",
                                        "Authorization-Ref": "${input:api-token}"
                                    }
                                }
                            }
                        }
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
                mcp_local: Some(
                    serde_json::json!({
                        "servers": {
                            "local-only": {
                                "type": "stdio",
                                "server_parameters": {
                                    "command": "local-command",
                                    "env": {
                                        "LOCAL_TOKEN": "local-secret",
                                        "LOCAL_REF": "${input:local-region}"
                                    }
                                }
                            }
                        },
                        "inputs": [{
                            "type": "PromptString",
                            "id": "local-region",
                            "description": "Local region",
                            "password": false
                        }]
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
                ..Default::default()
            },
        )
        .unwrap();
    let sdk_env = state.sdk_config.env(TEST_INSTANCE_ID);
    let user_path = a2c_smcp::smcp_computer::settings::user_mcp_config_path(Some(&sdk_env));
    std::fs::create_dir_all(user_path.parent().unwrap()).unwrap();
    std::fs::write(
        &user_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "servers": {
                "user-only": {
                    "type": "stdio",
                    "server_parameters": {
                        "command": "user-command",
                        "env": { "USER_REF": "${input:user-token}" }
                    }
                }
            },
            "inputs": [{
                "type": "PromptString",
                "id": "user-token",
                "description": "User token",
                "password": true
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    state
        .sdk_config
        .replace_input_definitions(
            TEST_INSTANCE_ID,
            &[inputs::InputDefinition::PromptString {
                id: "api-token".to_string(),
                label: Some("API token".to_string()),
                description: None,
                default: None,
                password: Some(true),
            }],
        )
        .unwrap();
    inputs::set_input_value_core(
        &state,
        TEST_INSTANCE_ID,
        "api-token".to_string(),
        serde_json::json!("actual-input-secret"),
    )
    .await
    .unwrap();

    config_io::export_config_core(
        &state,
        export_path.to_string_lossy().to_string(),
        TEST_INSTANCE_ID.to_string(),
        None,
    )
    .await
    .unwrap();

    let content = std::fs::read_to_string(export_path).unwrap();
    let exported: serde_json::Value = serde_json::from_str(&content).unwrap();
    let servers = exported["servers"].as_array().unwrap();
    let stdio = servers
        .iter()
        .find(|server| server["name"] == "secret-stdio")
        .unwrap();
    let http = servers
        .iter()
        .find(|server| server["name"] == "secret-http")
        .unwrap();
    let local = servers
        .iter()
        .find(|server| server["name"] == "local-only")
        .unwrap();
    let user = servers
        .iter()
        .find(|server| server["name"] == "user-only")
        .unwrap();

    assert_eq!(stdio["server_parameters"]["env"]["TOKEN"], "literal-secret");
    assert_eq!(
        stdio["server_parameters"]["env"]["TOKEN_REF"],
        "${env:TOKEN}"
    );
    assert_eq!(
        http["server_parameters"]["headers"]["Authorization"],
        "Bearer literal-secret"
    );
    assert_eq!(
        http["server_parameters"]["headers"]["Authorization-Ref"],
        "${input:api-token}"
    );
    assert_eq!(
        http["server_parameters"]["url"],
        "https://user:password@example.com/mcp"
    );
    assert_eq!(
        local["server_parameters"]["env"]["LOCAL_TOKEN"],
        "local-secret"
    );
    assert_eq!(stdio["server_parameters"]["cwd"], "/workspace/source");
    assert_eq!(
        local["server_parameters"]["env"]["LOCAL_REF"],
        "${input:local-region}"
    );
    assert_eq!(
        user["server_parameters"]["env"]["USER_REF"],
        "${input:user-token}"
    );
    let mut exported_input_ids = exported["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|input| input["id"].as_str())
        .collect::<Vec<_>>();
    exported_input_ids.sort_unstable();
    assert_eq!(
        exported_input_ids,
        ["api-token", "local-region", "user-token"]
    );
    assert!(exported["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .all(|input| input.get("default").is_none()));
    assert!(content.contains("literal-secret"));
    assert!(content.contains("local-secret"));
    assert!(!content.contains("actual-input-secret"));
    assert!(content.contains("local-command"));
}

#[tokio::test]
async fn test_config_io_import_and_reexport_preserve_literals_and_input_definitions() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let import_path = tmp.path().join("secret-import.json");
    let import_data = serde_json::json!({
        "servers": [
            {
                "type": "Stdio",
                "name": "secret-import",
                "disabled": true,
                "forbidden_tools": [],
                "tool_meta": {},
                "server_parameters": {
                    "command": "node",
                    "args": [],
                    "env": {
                        "TOKEN": "literal-import-secret",
                        "TOKEN_REF": "${input:api-token}"
                    },
                    "cwd": "/workspace/imported"
                }
            },
            {
                "type": "Http",
                "name": "http-import",
                "disabled": false,
                "forbidden_tools": [],
                "tool_meta": {},
                "server_parameters": {
                    "url": "https://example.com/mcp",
                    "headers": {
                        "Authorization": "Bearer literal-http-secret",
                        "X-Token": "${input:api-token}"
                    }
                }
            },
            {
                "type": "Sse",
                "name": "sse-import",
                "disabled": false,
                "forbidden_tools": [],
                "tool_meta": {},
                "server_parameters": {
                    "url": "https://example.com/sse",
                    "headers": {
                        "Authorization": "Bearer literal-sse-secret",
                        "X-Token": "${input:api-token}"
                    }
                }
            }
        ],
        "inputs": [{
            "type": "PromptString",
            "id": "api-token",
            "label": "API token",
            "password": true
        }]
    });
    std::fs::write(
        &import_path,
        serde_json::to_string_pretty(&import_data).unwrap(),
    )
    .unwrap();

    let result = config_io::import_config_core(
        &state,
        import_path.to_string_lossy().to_string(),
        TEST_INSTANCE_ID.to_string(),
        None,
    )
    .await
    .unwrap();

    let snapshot = state.sdk_config.load(TEST_INSTANCE_ID);
    let server = snapshot
        .mcp
        .servers
        .iter()
        .find(|server| server.name == "secret-import")
        .unwrap();
    let persisted = serde_json::to_value(&server.config).unwrap();
    assert_eq!(
        persisted["server_parameters"]["env"]["TOKEN"],
        "literal-import-secret"
    );
    assert_eq!(
        persisted["server_parameters"]["env"]["TOKEN_REF"],
        "${input:api-token}"
    );
    assert_eq!(persisted["server_parameters"]["cwd"], "/workspace/imported");
    let imported_http = snapshot
        .mcp
        .servers
        .iter()
        .find(|server| server.name == "http-import")
        .map(|server| serde_json::to_value(&server.config).unwrap())
        .unwrap();
    assert_eq!(
        imported_http["server_parameters"]["headers"]["Authorization"],
        "Bearer literal-http-secret"
    );
    assert_eq!(
        imported_http["server_parameters"]["headers"]["X-Token"],
        "${input:api-token}"
    );
    let imported_sse = snapshot
        .mcp
        .servers
        .iter()
        .find(|server| server.name == "sse-import")
        .map(|server| serde_json::to_value(&server.config).unwrap())
        .unwrap();
    assert_eq!(
        imported_sse["server_parameters"]["headers"]["Authorization"],
        "Bearer literal-sse-secret"
    );
    assert_eq!(
        imported_sse["server_parameters"]["headers"]["X-Token"],
        "${input:api-token}"
    );
    let imported_inputs = state.sdk_config.load_input_definitions(TEST_INSTANCE_ID);
    assert!(matches!(
        imported_inputs.as_slice(),
        [inputs::InputDefinition::PromptString {
            password: Some(true),
            ..
        }]
    ));
    assert_eq!(result.servers_imported, 3);
    assert_eq!(result.inputs_imported, 1);
    assert_eq!(
        inputs::get_input_value_core(&state, TEST_INSTANCE_ID, "api-token").unwrap(),
        Some(inputs::InputValueView {
            configured: false,
            status: inputs::InputValueStatus::Missing,
            value: None,
        })
    );

    let reexport_path = tmp.path().join("reexported.json");
    config_io::export_config_core(
        &state,
        reexport_path.to_string_lossy().to_string(),
        TEST_INSTANCE_ID.to_string(),
        None,
    )
    .await
    .unwrap();
    let reexported: serde_json::Value =
        serde_json::from_slice(&std::fs::read(reexport_path).unwrap()).unwrap();
    let reexported_servers = reexported["servers"].as_array().unwrap();
    let reexported_server = reexported_servers
        .iter()
        .find(|server| server["name"] == "secret-import")
        .unwrap();
    assert_eq!(
        reexported_server["server_parameters"]["env"]["TOKEN"],
        "literal-import-secret"
    );
    assert_eq!(
        reexported_server["server_parameters"]["env"]["TOKEN_REF"],
        "${input:api-token}"
    );
    assert_eq!(
        reexported_server["server_parameters"]["cwd"],
        "/workspace/imported"
    );
    let reexported_http = reexported_servers
        .iter()
        .find(|server| server["name"] == "http-import")
        .unwrap();
    assert_eq!(
        reexported_http["server_parameters"]["headers"]["Authorization"],
        "Bearer literal-http-secret"
    );
    assert_eq!(
        reexported_http["server_parameters"]["headers"]["X-Token"],
        "${input:api-token}"
    );
    let reexported_sse = reexported_servers
        .iter()
        .find(|server| server["name"] == "sse-import")
        .unwrap();
    assert_eq!(
        reexported_sse["server_parameters"]["headers"]["Authorization"],
        "Bearer literal-sse-secret"
    );
    assert_eq!(
        reexported_sse["server_parameters"]["headers"]["X-Token"],
        "${input:api-token}"
    );
    assert_eq!(reexported["inputs"][0]["id"], "api-token");
    assert!(reexported["inputs"][0].get("default").is_none());
}

#[tokio::test]
async fn test_config_io_export_rejects_sensitive_argument_plaintext_without_overwriting_target() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    state
        .sdk_config
        .save(
            TEST_INSTANCE_ID,
            &ProjectConfigDoc {
                mcp: Some(
                    serde_json::json!({
                        "servers": {
                            "unsafe-export": {
                                "type": "stdio",
                                "server_parameters": {
                                    "command": "node",
                                    "args": ["server.js", "--token", "plain-export-secret"]
                                }
                            },
                            "unsafe-relative-export": {
                                "type": "streamable",
                                "server_parameters": {
                                    "url": "mcp?client_secret=plain-relative-export-secret"
                                }
                            }
                        }
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
                ..Default::default()
            },
        )
        .unwrap();
    let export_path = tmp.path().join("unsafe-export.json");
    std::fs::write(&export_path, "existing-target").unwrap();

    let error = config_io::export_config_core(
        &state,
        export_path.to_string_lossy().into_owned(),
        TEST_INSTANCE_ID.to_string(),
        None,
    )
    .await
    .unwrap_err();

    assert!(error.contains("plaintext in a sensitive field"));
    assert!(error.contains("args[2]"));
    assert!(error.contains("unsafe-relative-export"));
    assert!(error.contains("client_secret"));
    assert_eq!(
        std::fs::read_to_string(export_path).unwrap(),
        "existing-target"
    );
}

#[tokio::test]
async fn test_config_io_import_rejects_sensitive_plaintext_before_any_persistence() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    state
        .sdk_config
        .replace_input_definitions(
            TEST_INSTANCE_ID,
            &[inputs::InputDefinition::PickString {
                id: "existing-input".to_string(),
                label: Some("Existing input".to_string()),
                description: None,
                default: None,
                options: vec![inputs::PickOption {
                    label: "Existing".to_string(),
                    value: "existing".to_string(),
                }],
            }],
        )
        .unwrap();
    let import_path = tmp.path().join("unsafe-import.json");
    std::fs::write(
        &import_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "servers": [
                {
                    "type": "Http",
                    "name": "unsafe-import",
                    "disabled": false,
                    "forbidden_tools": [],
                    "tool_meta": {},
                    "server_parameters": {
                        "url": "https://example.com/mcp?client_secret=plain-import-secret",
                        "headers": {}
                    }
                },
                {
                    "type": "Http",
                    "name": "unsafe-relative-import",
                    "disabled": false,
                    "forbidden_tools": [],
                    "tool_meta": {},
                    "server_parameters": {
                        "url": "mcp?access_token=plain-relative-import-secret",
                        "headers": {}
                    }
                }
            ],
            "inputs": [{
                "type": "PromptString",
                "id": "new-input",
                "label": "Must not persist",
                "password": false
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    let error = config_io::import_config_core(
        &state,
        import_path.to_string_lossy().into_owned(),
        TEST_INSTANCE_ID.to_string(),
        None,
    )
    .await
    .unwrap_err();

    assert!(error.contains("plaintext in a sensitive field"));
    assert!(error.contains("client_secret"));
    assert!(error.contains("unsafe-relative-import"));
    assert!(error.contains("access_token"));
    let inputs = state
        .sdk_config
        .load_project_input_definitions(TEST_INSTANCE_ID)
        .unwrap();
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].id(), "existing-input");
    assert!(!state
        .sdk_config
        .load(TEST_INSTANCE_ID)
        .mcp
        .servers
        .iter()
        .any(|server| server.name == "unsafe-import"));
}

#[tokio::test]
async fn test_config_io_import_preflights_unwritable_sdk_target_before_inputs() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    tfrobot_client_lib::services::keychain::set_input_secret(
        state.secret_store.as_ref(),
        TEST_INSTANCE_ID,
        "shared-token",
        "existing-secret",
    )
    .unwrap();
    state
        .sdk_config
        .save(
            TEST_INSTANCE_ID,
            &ProjectConfigDoc {
                mcp: Some(
                    serde_json::json!({
                        "servers": {
                            "existing-server": {
                                "type": "stdio",
                                "server_parameters": {
                                    "command": "existing-command",
                                    "args": [],
                                    "env": {}
                                }
                            }
                        }
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
                ..Default::default()
            },
        )
        .unwrap();
    state
        .sdk_config
        .replace_input_definitions(
            TEST_INSTANCE_ID,
            &[inputs::InputDefinition::PromptString {
                id: "shared-token".to_string(),
                label: Some("Existing token".to_string()),
                description: None,
                default: None,
                password: Some(true),
            }],
        )
        .unwrap();
    let blocked_local_mcp = state
        .sdk_config
        .project_anchor(TEST_INSTANCE_ID)
        .join(".tfrobot/mcp.local.json");
    std::fs::create_dir_all(&blocked_local_mcp).unwrap();
    let import_path = tmp.path().join("rollback-import.json");
    std::fs::write(
        &import_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "servers": [
                echo_server_config("rollback-server-a"),
                echo_server_config("rollback-server-b")
            ],
            "inputs": [{
                "type": "PickString",
                "id": "shared-token",
                "label": "Replacement token",
                "options": [{ "label": "One", "value": "one" }],
                "default": "one"
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    let error = config_io::import_config_core(
        &state,
        import_path.to_string_lossy().into_owned(),
        TEST_INSTANCE_ID.to_string(),
        None,
    )
    .await
    .unwrap_err();

    assert!(error.contains("mcp.local.json"), "{error}");
    assert!(!state
        .config
        .computer_instance_storage_root(TEST_INSTANCE_ID)
        .join("config_import_transaction.json")
        .exists());
    assert!(matches!(
        state
            .sdk_config
            .load_project_input_definitions(TEST_INSTANCE_ID)
            .unwrap()
            .as_slice(),
        [inputs::InputDefinition::PromptString { label, .. }]
            if label.as_deref() == Some("Existing token")
    ));
    assert_eq!(
        tfrobot_client_lib::services::keychain::get_input_secret(
            state.secret_store.as_ref(),
            TEST_INSTANCE_ID,
            "shared-token",
        )
        .unwrap()
        .as_deref(),
        Some("existing-secret")
    );
    let snapshot = state.sdk_config.load(TEST_INSTANCE_ID);
    assert!(snapshot
        .mcp
        .servers
        .iter()
        .any(|server| server.name == "existing-server"));
    assert!(!snapshot.mcp.servers.iter().any(|server| matches!(
        server.name.as_str(),
        "rollback-server-a" | "rollback-server-b"
    )));
}

#[tokio::test]
async fn test_validate_computer_config_is_schema_only() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    state
        .sdk_config
        .save(
            TEST_INSTANCE_ID,
            &ProjectConfigDoc {
                mcp: Some(
                    serde_json::json!({
                        "servers": {
                            "unavailable-command": {
                                "type": "stdio",
                                "server_parameters": {
                                    "command": "this-command-does-not-exist-anywhere-xyzzy"
                                }
                            }
                        }
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
                ..Default::default()
            },
        )
        .unwrap();

    let valid = sdk_config::get_computer_config_state_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap()
        .validation;
    assert!(
        valid.valid,
        "schema validation must not probe command availability"
    );

    state
        .sdk_config
        .save(
            TEST_INSTANCE_ID,
            &ProjectConfigDoc {
                mcp: Some(
                    serde_json::json!({
                        "servers": { "invalid": { "type": "carrier-pigeon" } }
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
                ..Default::default()
            },
        )
        .unwrap();

    let invalid = sdk_config::get_computer_config_state_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap()
        .validation;
    assert!(!invalid.valid);
    assert!(invalid
        .errors
        .iter()
        .any(|error| error.field == "servers.invalid"));

    let combined = sdk_config::get_computer_config_state_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(
        combined.snapshot.revision,
        state.sdk_config.load(TEST_INSTANCE_ID).revision.0
    );
    assert!(!combined.validation.valid);
}

#[tokio::test]
async fn test_validate_computer_config_includes_sdk_user_scope() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let user_mcp_path = state
        .sdk_config
        .project_anchor(TEST_INSTANCE_ID)
        .join("a2c/mcp.json");
    std::fs::create_dir_all(user_mcp_path.parent().unwrap()).unwrap();
    std::fs::write(
        &user_mcp_path,
        serde_json::to_vec_pretty(&serde_json::json!({
            "servers": {
                "invalid-user": { "type": "carrier-pigeon" }
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let validation = sdk_config::get_computer_config_state_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap()
        .validation;

    assert!(!validation.valid);
    assert!(validation.errors.iter().any(|error| {
        error.field == "servers.invalid-user"
            && error
                .source_path
                .as_deref()
                .is_some_and(|path| Path::new(path).ends_with(Path::new("a2c").join("mcp.json")))
    }));
}

#[tokio::test]
async fn test_sdk_config_crud_does_not_require_or_reload_runtime() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_test_app_state(tmp.path());
    state
        .config
        .add_computer_instance(ComputerInstance::new(TEST_INSTANCE_ID, "Computer A"))
        .unwrap();
    assert!(state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .is_none());
    let config: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "Stdio",
        "name": "config-only",
        "disabled": false,
        "forbidden_tools": [],
        "tool_meta": {},
        "server_parameters": {
            "command": "node",
            "args": [],
            "env": { "MODE": "config-only" }
        }
    }))
    .unwrap();

    sdk_config::upsert_computer_mcp_config_core(&state, TEST_INSTANCE_ID, config)
        .await
        .unwrap();
    assert!(state
        .sdk_config
        .load(TEST_INSTANCE_ID)
        .mcp
        .servers
        .iter()
        .any(|server| server.name == "config-only"));
    assert!(state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .is_none());

    sdk_config::remove_computer_mcp_config_core(&state, TEST_INSTANCE_ID, "config-only")
        .await
        .unwrap();
    assert!(!state
        .sdk_config
        .load(TEST_INSTANCE_ID)
        .mcp
        .servers
        .iter()
        .any(|server| server.name == "config-only"));
}

#[tokio::test]
async fn oauth_delete_failure_keeps_the_durable_user_config_retryable() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_config_only_state_with_store(tmp.path(), Arc::new(FailingOAuthDeleteStore));
    state
        .config
        .add_computer_instance(ComputerInstance::new(TEST_INSTANCE_ID, TEST_COMPUTER_NAME))
        .unwrap();
    state
        .sdk_config
        .upsert_mcp_configs(
            TEST_INSTANCE_ID,
            &[oauth_http_server_config("protected", None)],
        )
        .unwrap();

    assert!(
        sdk_config::remove_computer_mcp_config_core(&state, TEST_INSTANCE_ID, "protected",)
            .await
            .is_err()
    );

    assert!(state
        .sdk_config
        .load(TEST_INSTANCE_ID)
        .mcp
        .servers
        .iter()
        .any(|server| server.name == "protected"));
}

#[tokio::test]
async fn static_authorization_config_removal_does_not_touch_oauth_credentials() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_config_only_state_with_store(tmp.path(), Arc::new(FailingOAuthDeleteStore));
    state
        .config
        .add_computer_instance(ComputerInstance::new(TEST_INSTANCE_ID, TEST_COMPUTER_NAME))
        .unwrap();
    let static_auth: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "streamable",
        "name": "static-auth",
        "bundle_id": "static-auth",
        "server_parameters": {
            "url": "https://mcp.example.invalid/mcp",
            "headers": { "Authorization": "Bearer static-token" }
        }
    }))
    .unwrap();
    state
        .sdk_config
        .upsert_mcp_configs(TEST_INSTANCE_ID, &[static_auth])
        .unwrap();

    sdk_config::remove_computer_mcp_config_core(&state, TEST_INSTANCE_ID, "static-auth")
        .await
        .expect("static Authorization removal must not depend on OAuth keychain deletion");

    assert!(state
        .sdk_config
        .load(TEST_INSTANCE_ID)
        .mcp
        .servers
        .iter()
        .all(|server| server.name != "static-auth"));
}

#[tokio::test]
async fn oauth_identity_cleanup_failure_does_not_overwrite_the_old_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_config_only_state_with_store(tmp.path(), Arc::new(FailingOAuthDeleteStore));
    state
        .config
        .add_computer_instance(ComputerInstance::new(TEST_INSTANCE_ID, TEST_COMPUTER_NAME))
        .unwrap();
    let original = oauth_http_server_config("protected", None);
    state
        .sdk_config
        .upsert_mcp_configs(TEST_INSTANCE_ID, std::slice::from_ref(&original))
        .unwrap();
    let replacement =
        oauth_http_server_config("protected", Some("https://different-resource.example/mcp"));

    assert!(
        sdk_config::upsert_computer_mcp_config_core(&state, TEST_INSTANCE_ID, replacement,)
            .await
            .is_err()
    );

    let persisted = state
        .sdk_config
        .load(TEST_INSTANCE_ID)
        .mcp
        .servers
        .into_iter()
        .find(|server| server.name == "protected")
        .unwrap()
        .config;
    assert_eq!(persisted, original);
}

#[tokio::test]
async fn test_config_io_export_rejects_unknown_selected_servers() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let export_path = tmp.path().join("selected-export.json");

    mcp::add_mcp_server_core(&state, TEST_INSTANCE_ID, echo_server_config("known-server"))
        .await
        .unwrap();

    let error = config_io::export_config_core(
        &state,
        export_path.to_string_lossy().to_string(),
        TEST_INSTANCE_ID.to_string(),
        Some(vec![
            "known-server".to_string(),
            "missing-server".to_string(),
        ]),
    )
    .await
    .unwrap_err();

    assert!(error.contains("missing-server"));
    assert!(!export_path.exists());
}

#[tokio::test]
async fn test_config_io_export_does_not_overwrite_target_when_source_is_corrupt() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let export_path = tmp.path().join("existing-export.json");
    let original_export = b"existing backup";
    std::fs::write(&export_path, original_export).unwrap();

    let source_mcp = state
        .sdk_config
        .project_anchor(TEST_INSTANCE_ID)
        .join(".tfrobot/mcp.json");
    std::fs::create_dir_all(source_mcp.parent().unwrap()).unwrap();
    std::fs::write(&source_mcp, "{not-json").unwrap();

    let error = config_io::export_config_core(
        &state,
        export_path.to_string_lossy().to_string(),
        TEST_INSTANCE_ID.to_string(),
        None,
    )
    .await
    .unwrap_err();

    assert!(error.contains("Cannot export invalid SDK MCP configuration"));
    assert!(error.replace('\\', "/").contains(".tfrobot/mcp.json"));
    assert_eq!(std::fs::read(export_path).unwrap(), original_export);
}

#[tokio::test]
async fn test_cli_native_import_persists_sdk_inputs_without_hot_updating_runtimes() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let other_instance = ComputerInstance::new("other-computer", "Other Computer");
    state
        .config
        .add_computer_instance(other_instance.clone())
        .unwrap();
    let other_runtime = state
        .computer_registry
        .upsert_runtime(other_instance)
        .await
        .unwrap();
    let import_path = tmp.path().join("import-with-input.json");
    let server_path = common::echo_server_path();
    let server: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "Stdio",
        "name": "input-backed-server",
        "server_parameters": {
            "command": "${input:node-command}",
            "args": [server_path.to_str().unwrap()],
            "env": {}
        }
    }))
    .unwrap();
    let import_data = serde_json::json!({
        "servers": [server],
        "inputs": [{
            "type": "PromptString",
            "id": "node-command",
            "label": "Node command",
            "default": "node"
        }]
    });
    std::fs::write(
        &import_path,
        serde_json::to_string_pretty(&import_data).unwrap(),
    )
    .unwrap();

    let import_result = config_io::import_config_core(
        &state,
        import_path.to_string_lossy().to_string(),
        TEST_INSTANCE_ID.into(),
        None,
    )
    .await
    .unwrap();

    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    let imported_inputs = state.sdk_config.load_input_definitions(TEST_INSTANCE_ID);

    assert_eq!(import_result.servers_imported, 1);
    assert_eq!(import_result.inputs_imported, 1);
    assert_eq!(imported_inputs[0].id(), "node-command");
    assert!(state
        .sdk_config
        .load(TEST_INSTANCE_ID)
        .mcp
        .servers
        .iter()
        .any(|server| server.name == "input-backed-server"));
    assert!(
        !runtime.inputs.read().await.contains_key("node-command"),
        "saved Input definitions must not hot-update the target runtime"
    );
    assert!(
        !other_runtime
            .inputs
            .read()
            .await
            .contains_key("node-command"),
        "the import must not synchronize Input definitions to another Computer runtime"
    );

    start_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    assert!(state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap()
        .inputs
        .read()
        .await
        .contains_key("node-command"));
}

// ── SDK Computer MCP lifecycle (requires Node.js) ──
// Echo server uses newline-delimited JSON framing (MCP spec 2025-03-26).

const MCP_RUNTIME_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[tokio::test]
async fn test_robot_control_provider_is_visible_but_not_user_manageable() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    client_control::update_remote_control_policy_core(
        &state,
        UpdateRemoteControlPolicyRequest {
            computer_id: TEST_INSTANCE_ID.to_string(),
            policy: RemoteControlPolicy {
                enabled: true,
                ..RemoteControlPolicy::default()
            },
        },
    )
    .await
    .unwrap();
    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();

    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    let snapshot = runtime.runtime_snapshot().await;
    let servers = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    let provider = servers
        .iter()
        .find(|server| server.bundle_id.as_str() == CLIENT_CONTROL_BUNDLE_ID)
        .expect("Robot control provider should be projected into the MCP runtime list");

    assert_eq!(snapshot.mcp_servers, 1);
    assert_eq!(snapshot.active_mcp_servers, 1);
    assert_eq!(provider.name, "Client Control");
    assert_eq!(
        provider.managed_by,
        McpServerManagedBy::BuiltIn {
            provider: "robot_control".to_string(),
        }
    );
    assert_eq!(provider.activation_state, MCPServerActivationState::Started);
    assert_eq!(
        provider.connection_state,
        MCPServerConnectionState::Connected
    );

    let start_all = mcp::start_all_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    let stop_all = mcp::stop_all_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(start_all.candidate_count, 0);
    assert_eq!(stop_all.candidate_count, 0);
    assert!(runtime
        .mcp_server_runtime_statuses()
        .await
        .into_iter()
        .any(|status| {
            status.bundle_id.as_str() == CLIENT_CONTROL_BUNDLE_ID && status.is_connected()
        }));
}

#[tokio::test]
async fn test_sdk_computer_add_and_start_server() {
    require_node();

    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    mcp::add_mcp_server_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config("lifecycle-test"),
    )
    .await
    .unwrap();
    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();

    let result = tokio::time::timeout(
        MCP_RUNTIME_TIMEOUT,
        mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("lifecycle-test")),
    )
    .await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => panic!("start_mcp_server failed: {e}"),
        Err(_) => panic!("start_mcp_server timed out after {MCP_RUNTIME_TIMEOUT:?}"),
    }

    let statuses = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    let found = statuses
        .iter()
        .find(|status| status.name == "lifecycle-test")
        .expect("server status should exist");
    assert!(found.running, "Server should be running");

    mcp::stop_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("lifecycle-test"))
        .await
        .unwrap();
}

#[tokio::test]
async fn test_sdk_computer_list_tools_after_start() {
    require_node();

    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    mcp::add_mcp_server_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config("tool-list-test"),
    )
    .await
    .unwrap();
    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();

    let result = tokio::time::timeout(
        MCP_RUNTIME_TIMEOUT,
        mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("tool-list-test")),
    )
    .await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => panic!("start_mcp_server failed: {e}"),
        Err(_) => panic!("start_mcp_server timed out after {MCP_RUNTIME_TIMEOUT:?}"),
    }

    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let tools = debug::get_available_tools_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(
        !tools.is_empty(),
        "Expected at least one tool from echo server"
    );
    assert!(tools.iter().any(|tool| tool.name == "tool-list-test__echo"));

    mcp::stop_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("tool-list-test"))
        .await
        .unwrap();
}

#[tokio::test]
async fn test_sdk_computer_execute_echo_tool() {
    require_node();

    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    mcp::add_mcp_server_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config("echo-call-test"),
    )
    .await
    .unwrap();
    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("echo-call-test"))
        .await
        .unwrap();

    let response = debug::execute_tool_core(
        &state,
        TEST_INSTANCE_ID,
        "echo-call-test__echo",
        serde_json::json!({"message": "hello from test"}),
        Some(5.0),
    )
    .await
    .unwrap();

    assert!(response.success);
    assert!(response.result.is_some());
}

#[tokio::test]
async fn test_real_http_oauth_401_returns_structured_tool_result() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let (runtime_event_sender, mut runtime_events) = tokio::sync::mpsc::unbounded_channel();
    state
        .computer_registry
        .set_runtime_event_sink(Arc::new(ChannelRuntimeEventSink {
            sender: runtime_event_sender,
        }))
        .await;
    let url = start_oauth_rejecting_mcp_server().await;
    let config: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "Http",
        "name": "oauth-contract",
        "disabled": false,
        "server_parameters": {
            "url": url,
            "headers": {}
        }
    }))
    .unwrap();
    let oauth_bundle_id = resolve_bundle_id(&config);
    mcp::add_mcp_server_core(&state, TEST_INSTANCE_ID, config)
        .await
        .unwrap();
    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &oauth_bundle_id)
        .await
        .unwrap();
    timeout(Duration::from_secs(5), async {
        while let Some(event) = runtime_events.recv().await {
            if event.instance_id == TEST_INSTANCE_ID && event.snapshot.tools > 0 {
                return;
            }
        }
        panic!("runtime event stream closed before the HTTP mock tool was projected");
    })
    .await
    .expect("HTTP mock tool projection timed out");
    let protected_tool = debug::get_available_tools_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap()
        .into_iter()
        .find(|tool| tool.display_name == "protected")
        .expect("HTTP mock tool must be active");
    let response = debug::execute_tool_core(
        &state,
        TEST_INSTANCE_ID,
        &protected_tool.name,
        serde_json::json!({}),
        Some(5.0),
    )
    .await
    .unwrap();

    assert!(!response.success);
    assert!(
        response.error.is_none(),
        "authorization is a structured tool result, got {:?}",
        response.error
    );
    let result = serde_json::to_value(response.result.expect("structured result")).unwrap();
    assert_eq!(result["isError"], true);
    assert_eq!(result["_meta"]["error_code"], 4006);
}

#[tokio::test]
async fn test_auto_oauth_challenge_is_authorization_state_not_start_diagnostic() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    let (url, stats) = start_auto_oauth_challenge_server().await;
    let config: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "streamable",
        "name": "oauth-auto-challenge",
        "bundle_id": "oauth-auto-challenge",
        "server_parameters": {
            "url": format!("{url}/mcp"),
            "headers": {}
        }
    }))
    .unwrap();
    let bundle_id = resolve_bundle_id(&config);

    runtime
        .apply_user_mcp_server_config(config)
        .await
        .expect("validated OAuth challenge must not fail configuration application");

    assert!(stats.discovery_requests.load(Ordering::SeqCst) > 0);
    assert!(!runtime
        .mcp_start_diagnostics()
        .await
        .contains_key(&bundle_id));
    assert_eq!(
        runtime.mcp_server_oauth_is_interactive(&bundle_id).await,
        Some(true)
    );
    assert!(matches!(
        runtime.oauth_status(&bundle_id).await,
        Ok(Some(
            a2c_smcp::smcp_computer::oauth::OAuthStatus::Unauthorized
        ))
    ));
    let runtime_status = runtime
        .mcp_server_runtime_statuses()
        .await
        .into_iter()
        .find(|status| status.bundle_id == bundle_id)
        .expect("OAuth MCP runtime status");
    assert_eq!(runtime_status.activation, MCPServerActivationState::Started);
    assert_eq!(
        runtime_status.connection,
        MCPServerConnectionState::AuthorizationRequired
    );

    let projected = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap()
        .into_iter()
        .find(|status| status.bundle_id == bundle_id)
        .expect("projected OAuth MCP status");
    assert_eq!(
        projected.activation_state,
        MCPServerActivationState::Started
    );
    assert_eq!(
        projected.connection_state,
        MCPServerConnectionState::AuthorizationRequired
    );
    assert!(projected.running, "legacy running must project activation");
    assert_eq!(projected.status_message, "authorization_required");
}

#[tokio::test]
async fn test_legacy_omitted_http_auth_defaults_to_auto_oauth_after_challenge() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    let (url, stats) = start_auto_oauth_challenge_server().await;
    let config: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "streamable",
        "name": "oauth-legacy-auto",
        "bundle_id": "oauth-legacy-auto",
        "server_parameters": {
            "url": format!("{url}/mcp"),
            "headers": {}
        }
    }))
    .unwrap();
    let bundle_id = resolve_bundle_id(&config);

    runtime
        .apply_user_mcp_server_config(config)
        .await
        .expect("legacy HTTP defaults must perform anonymous-first admission");

    assert!(stats.discovery_requests.load(Ordering::SeqCst) > 0);
    assert_eq!(
        runtime.mcp_server_oauth_is_interactive(&bundle_id).await,
        Some(true)
    );
    assert!(matches!(
        runtime.oauth_status(&bundle_id).await,
        Ok(Some(
            a2c_smcp::smcp_computer::oauth::OAuthStatus::Unauthorized
        ))
    ));
    let authorization_url = runtime
        .begin_oauth_authorization(&bundle_id)
        .await
        .expect("admitted legacy Auto config must create an authorization flow");
    let parsed = url::Url::parse(&authorization_url).expect("authorization URL");
    assert_eq!(parsed.path(), "/authorize");
    drop(authorization_url);
    runtime
        .cancel_oauth_authorization(&bundle_id)
        .await
        .unwrap();
    runtime.clear_oauth_authorization(&bundle_id).await.unwrap();
    runtime
        .remove_user_mcp_server_config(&bundle_id)
        .await
        .unwrap();
    assert!(!runtime
        .sdk_mcp_server_configs()
        .await
        .contains_key(&bundle_id));
}

#[tokio::test]
async fn test_clear_oauth_preserves_runtime_and_withdraws_authorized_tools() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let (runtime_event_sender, mut runtime_events) = tokio::sync::mpsc::unbounded_channel();
    state
        .computer_registry
        .set_runtime_event_sink(Arc::new(ChannelRuntimeEventSink {
            sender: runtime_event_sender,
        }))
        .await;
    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    let (url, stats) = start_auto_oauth_challenge_server().await;
    let config: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "streamable",
        "name": "oauth-clear-active",
        "bundle_id": "oauth-clear-active",
        "server_parameters": {
            "url": format!("{url}/mcp"),
            "headers": {}
        }
    }))
    .unwrap();
    let bundle_id = resolve_bundle_id(&config);
    runtime.apply_user_mcp_server_config(config).await.unwrap();
    let unauthorized_snapshot = runtime.runtime_snapshot().await;

    let authorization_url = runtime
        .begin_oauth_authorization(&bundle_id)
        .await
        .expect("automatic OAuth server must begin authorization");
    let authorization_url = url::Url::parse(&authorization_url).expect("authorization URL");
    let authorization_query: HashMap<_, _> = authorization_url.query_pairs().into_owned().collect();
    let redirect_uri = authorization_query
        .get("redirect_uri")
        .expect("authorization URL redirect_uri");
    let oauth_state = authorization_query
        .get("state")
        .expect("authorization URL state");
    let mut callback_url = url::Url::parse(redirect_uri).expect("callback URL");
    callback_url
        .query_pairs_mut()
        .append_pair("code", "authorization-code")
        .append_pair("state", oauth_state)
        .append_pair("iss", &url);
    let callback_response = reqwest::get(callback_url)
        .await
        .expect("submit OAuth callback");
    assert!(callback_response.status().is_success());
    let projected = timeout(MCP_RUNTIME_TIMEOUT, async {
        while let Some(event) = runtime_events.recv().await {
            if event.instance_id == TEST_INSTANCE_ID
                && event.snapshot.capability_revision > unauthorized_snapshot.capability_revision
                && event.snapshot.tools > unauthorized_snapshot.tools
            {
                return Some(event);
            }
        }
        None
    })
    .await
    .ok()
    .flatten();
    if projected.is_none() {
        panic!(
            "authorized MCP capability projection was not published; oauth_status={:?}, runtime_snapshot={:?}, token_requests={}, registration_requests={}, token_form={:?}",
            runtime.oauth_status(&bundle_id).await,
            runtime.runtime_snapshot().await,
            stats.token_requests.load(Ordering::SeqCst),
            stats.registration_requests.load(Ordering::SeqCst),
            *stats.last_token_form.lock().await,
        );
    }

    assert_eq!(stats.token_requests.load(Ordering::SeqCst), 1);
    assert!(matches!(
        runtime.oauth_status(&bundle_id).await,
        Ok(Some(
            a2c_smcp::smcp_computer::oauth::OAuthStatus::Authorized { .. }
        ))
    ));
    let protected_tool = debug::get_available_tools_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap()
        .into_iter()
        .find(|tool| tool.name == "oauth-clear-active__protected")
        .expect("authorized MCP tool must be projected");
    let authorized_call = debug::execute_tool_core(
        &state,
        TEST_INSTANCE_ID,
        &protected_tool.name,
        serde_json::json!({}),
        Some(5.0),
    )
    .await
    .unwrap();
    assert!(authorized_call.success);
    assert!(stats.authorized_mcp_requests.load(Ordering::SeqCst) > 0);

    let authorized_snapshot = runtime.runtime_snapshot().await;
    runtime.clear_oauth_authorization(&bundle_id).await.unwrap();

    let revoked = timeout(MCP_RUNTIME_TIMEOUT, async {
        while let Some(event) = runtime_events.recv().await {
            if event.instance_id == TEST_INSTANCE_ID
                && matches!(
                    &event.cause,
                    ComputerRuntimeEventCause::CapabilityRevisionBumped { .. }
                )
                && event.snapshot.capability_revision > authorized_snapshot.capability_revision
                && event.snapshot.tools < authorized_snapshot.tools
            {
                return Some(event);
            }
        }
        None
    })
    .await
    .ok()
    .flatten();
    assert!(
        revoked.is_some(),
        "clear_oauth did not publish a capability invalidation; before={authorized_snapshot:?}, after={:?}",
        runtime.runtime_snapshot().await
    );

    assert!(matches!(
        runtime.oauth_status(&bundle_id).await,
        Ok(Some(
            a2c_smcp::smcp_computer::oauth::OAuthStatus::Unauthorized
        ))
    ));
    let runtime_status = runtime
        .mcp_server_runtime_statuses()
        .await
        .into_iter()
        .find(|status| status.bundle_id == bundle_id)
        .expect("cleared OAuth MCP runtime status");
    assert_eq!(runtime_status.activation, MCPServerActivationState::Started);
    assert_eq!(
        runtime_status.connection,
        MCPServerConnectionState::AuthorizationRequired
    );
    assert!(runtime
        .available_tools()
        .await
        .unwrap()
        .iter()
        .all(|tool| tool.name.as_ref() != "oauth-clear-active__protected"));
    let mcp_requests_after_clear = stats.mcp_requests.load(Ordering::SeqCst);
    let authorized_requests_after_clear = stats.authorized_mcp_requests.load(Ordering::SeqCst);
    let unauthorized_call = debug::execute_tool_core(
        &state,
        TEST_INSTANCE_ID,
        &protected_tool.name,
        serde_json::json!({}),
        Some(5.0),
    )
    .await
    .unwrap();
    assert!(!unauthorized_call.success);
    assert_eq!(
        stats.mcp_requests.load(Ordering::SeqCst),
        mcp_requests_after_clear,
        "cleared credentials must block the tool call before MCP transport"
    );
    assert_eq!(
        stats.authorized_mcp_requests.load(Ordering::SeqCst),
        authorized_requests_after_clear,
        "the cleared bearer token must never be reused"
    );

    runtime.stop_mcp_server(&bundle_id).await.unwrap();
    let stopped = runtime
        .mcp_server_runtime_statuses()
        .await
        .into_iter()
        .find(|status| status.bundle_id == bundle_id)
        .expect("stopped OAuth MCP runtime status");
    assert_eq!(stopped.activation, MCPServerActivationState::Stopped);
    assert_eq!(stopped.connection, MCPServerConnectionState::Disconnected);
}

#[tokio::test]
async fn test_client_oauth_cancel_interrupts_delayed_dynamic_registration() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .expect("test runtime");
    let (url, stats) =
        start_auto_oauth_challenge_server_with_registration_delay(Duration::from_secs(5)).await;
    let config = delayed_oauth_server_config(&url, false);
    let oauth_bundle_id = resolve_bundle_id(&config);
    runtime
        .apply_user_mcp_server_config(config)
        .await
        .expect("mount OAuth server");

    let begin_runtime = runtime.clone();
    let begin_bundle_id = oauth_bundle_id.clone();
    let begin = tokio::spawn(async move {
        begin_runtime
            .begin_oauth_authorization(&begin_bundle_id)
            .await
    });

    timeout(Duration::from_secs(1), async {
        while stats.registration_requests.load(Ordering::SeqCst) == 0 {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("OAuth dynamic registration request must start after automatic admission");

    let started = Instant::now();
    timeout(
        Duration::from_secs(1),
        runtime.cancel_oauth_authorization(&oauth_bundle_id),
    )
    .await
    .expect("client cancellation must not wait for provider registration")
    .expect("client cancellation succeeds");
    assert!(started.elapsed() < Duration::from_secs(1));

    let begin_error = timeout(Duration::from_secs(1), begin)
        .await
        .expect("begin task must observe cancellation")
        .expect("begin task joins")
        .expect_err("cancelled begin must not return an authorization URL");
    assert!(
        begin_error.to_ascii_lowercase().contains("cancel"),
        "unexpected begin error: {begin_error}"
    );
    runtime.shutdown().await;
}

#[tokio::test]
async fn test_oauth_server_update_retires_client_flow_before_sdk_replacement() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .expect("test runtime");
    let (url, stats) =
        start_auto_oauth_challenge_server_with_registration_delay(Duration::from_secs(5)).await;
    let enabled = delayed_oauth_server_config(&url, false);
    let oauth_bundle_id = resolve_bundle_id(&enabled);
    runtime
        .apply_user_mcp_server_config(enabled)
        .await
        .expect("mount OAuth server");

    let first_runtime = runtime.clone();
    let first_bundle_id = oauth_bundle_id.clone();
    let first_begin = tokio::spawn(async move {
        first_runtime
            .begin_oauth_authorization(&first_bundle_id)
            .await
    });
    timeout(Duration::from_secs(1), async {
        while stats.registration_requests.load(Ordering::SeqCst) < 1 {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("first OAuth registration request must start");

    timeout(
        Duration::from_secs(1),
        runtime.apply_user_mcp_server_config(delayed_oauth_server_config(&url, true)),
    )
    .await
    .expect("server replacement must not wait for delayed registration")
    .expect("replace OAuth server");
    let first_error = timeout(Duration::from_secs(1), first_begin)
        .await
        .expect("replaced flow must finish")
        .expect("first begin task joins")
        .expect_err("server replacement must cancel the first flow");
    assert!(first_error.to_ascii_lowercase().contains("cancel"));

    runtime
        .apply_user_mcp_server_config(delayed_oauth_server_config(&url, false))
        .await
        .expect("re-enable server and repeat automatic admission");

    let second_runtime = runtime.clone();
    let second_bundle_id = oauth_bundle_id.clone();
    let second_begin = tokio::spawn(async move {
        second_runtime
            .begin_oauth_authorization(&second_bundle_id)
            .await
    });
    timeout(Duration::from_secs(1), async {
        while stats.registration_requests.load(Ordering::SeqCst) < 2 {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("replacement must admit a new OAuth flow immediately");
    timeout(
        Duration::from_secs(1),
        runtime.cancel_oauth_authorization(&oauth_bundle_id),
    )
    .await
    .expect("replacement flow cancellation must be prompt")
    .expect("cancel replacement flow");
    timeout(Duration::from_secs(1), second_begin)
        .await
        .expect("replacement begin task must finish")
        .expect("replacement begin task joins")
        .expect_err("replacement flow was cancelled");

    runtime.shutdown().await;
}

#[tokio::test]
async fn test_plugin_oauth_unmount_retires_client_flow_before_sdk_removal() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .expect("test runtime");
    let (url, stats) =
        start_auto_oauth_challenge_server_with_registration_delay(Duration::from_secs(5)).await;
    let config = delayed_oauth_server_config(&url, false);
    let oauth_bundle_id = resolve_bundle_id(&config);
    runtime
        .add_or_update_plugin_server(config.clone())
        .await
        .expect("mount plugin OAuth server");
    runtime
        .start_mcp_server(&oauth_bundle_id)
        .await
        .expect("automatic admission must expose plugin OAuth state");

    let begin_runtime = runtime.clone();
    let begin_bundle_id = oauth_bundle_id.clone();
    let begin = tokio::spawn(async move {
        begin_runtime
            .begin_oauth_authorization(&begin_bundle_id)
            .await
    });
    timeout(Duration::from_secs(1), async {
        while stats.registration_requests.load(Ordering::SeqCst) < 1 {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("plugin OAuth registration request must start");

    timeout(
        Duration::from_secs(1),
        runtime.remove_plugin_server(&oauth_bundle_id),
    )
    .await
    .expect("plugin unmount must not wait for delayed registration")
    .expect("unmount plugin OAuth server");
    timeout(Duration::from_secs(1), begin)
        .await
        .expect("unmounted flow must finish")
        .expect("begin task joins")
        .expect_err("plugin unmount must cancel the flow");

    runtime
        .add_or_update_plugin_server(config)
        .await
        .expect("remount plugin OAuth server");
    runtime
        .start_mcp_server(&oauth_bundle_id)
        .await
        .expect("remounted plugin must repeat automatic admission");
    let replacement_runtime = runtime.clone();
    let replacement_bundle_id = oauth_bundle_id.clone();
    let replacement = tokio::spawn(async move {
        replacement_runtime
            .begin_oauth_authorization(&replacement_bundle_id)
            .await
    });
    timeout(Duration::from_secs(1), async {
        while stats.registration_requests.load(Ordering::SeqCst) < 2 {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("remounted server must admit a new OAuth flow immediately");
    runtime
        .cancel_oauth_authorization(&oauth_bundle_id)
        .await
        .expect("cancel remounted flow");
    timeout(Duration::from_secs(1), replacement)
        .await
        .expect("remounted begin task must finish")
        .expect("remounted begin task joins")
        .expect_err("remounted flow was cancelled");

    runtime.shutdown().await;
}

#[tokio::test]
async fn test_sdk_computer_start_all_stop_all() {
    require_node();

    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    // Use a single server to avoid tool name conflicts (all echo servers
    // expose the same "echo" tool, triggering ToolNameDuplicated).
    mcp::add_mcp_server_core(&state, TEST_INSTANCE_ID, echo_server_config("batch-single"))
        .await
        .unwrap();
    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    let initial_stop = mcp::stop_all_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(initial_stop.candidate_count, 1);
    assert_eq!(initial_stop.actual_operation_count, 1);
    assert!(initial_stop.failures.is_empty());

    let result = tokio::time::timeout(
        MCP_RUNTIME_TIMEOUT,
        mcp::start_all_servers_core(&state, TEST_INSTANCE_ID),
    )
    .await;
    match result {
        Ok(Ok(result)) => {
            assert_eq!(result.candidate_count, 1);
            assert_eq!(result.actual_operation_count, 1);
            assert_eq!(result.unchanged_count, 0);
            assert_eq!(result.excluded_plugin_owned_count, 0);
            assert!(result.failures.is_empty());
        }
        Ok(Err(e)) => panic!("start_all_servers failed: {e}"),
        Err(_) => panic!("start_all_servers timed out after {MCP_RUNTIME_TIMEOUT:?}"),
    }

    let statuses = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    let found = statuses
        .iter()
        .find(|status| status.name == "batch-single")
        .expect("server status should exist");
    assert!(found.running, "Server should be running after start_all");

    mcp::stop_all_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    let after = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(
        after.iter().all(|status| !status.running),
        "All servers should be stopped"
    );
}

#[tokio::test]
async fn test_sdk_computer_start_nonexistent_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();

    let result = tokio::time::timeout(
        MCP_RUNTIME_TIMEOUT,
        mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("ghost-server")),
    )
    .await;
    match result {
        Ok(r) => assert!(r.is_err()),
        Err(_) => { /* Timeout is acceptable — the server doesn't exist / command is invalid */ }
    }
}

#[tokio::test]
async fn test_sdk_computer_invalid_command_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    let config: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "Stdio",
        "name": "bad-server",
        "server_parameters": {
            "command": "/nonexistent/binary",
            "args": [],
            "env": {}
        }
    }))
    .unwrap();

    mcp::add_mcp_server_core(&state, TEST_INSTANCE_ID, config)
        .await
        .unwrap();
    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();

    let result = tokio::time::timeout(
        MCP_RUNTIME_TIMEOUT,
        mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("bad-server")),
    )
    .await
    .expect("invalid command should fail before the MCP runtime timeout");
    assert!(
        result.is_err(),
        "Starting server with invalid command should fail"
    );

    let activity = state
        .observability
        .query_activity(&ActivityQuery {
            scope: ActivityScopeFilter::Computer {
                computer_id: TEST_INSTANCE_ID.to_string(),
            },
            ..ActivityQuery::default()
        })
        .unwrap();
    let failed_start = activity
        .items
        .iter()
        .find(|event| event.operation == "start")
        .expect("failed manual start should be queryable as activity");
    assert_eq!(failed_start.level, ActivityLevel::Error);
    assert_eq!(failed_start.outcome, ActivityOutcome::Failed);
    assert!(failed_start.message.contains("bad-server"));
}

// ── Config IO integration ──

#[tokio::test]
async fn test_import_official_remote_url_updates_runtime_after_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    let path = tmp.path().join("atlassian.json");
    std::fs::write(
        &path,
        serde_json::json!({
            "mcpServers": {
                "atlassian": {
                    "url": "https://mcp.atlassian.com/v1/mcp/authv2"
                }
            }
        })
        .to_string(),
    )
    .unwrap();

    let result = config_io::import_config_core(
        &state,
        path.to_string_lossy().to_string(),
        TEST_INSTANCE_ID.to_string(),
        None,
    )
    .await
    .unwrap();

    assert_eq!(result.servers_imported, 1);
    let imported = state
        .sdk_config
        .load(TEST_INSTANCE_ID)
        .mcp
        .servers
        .into_iter()
        .find(|server| server.name == "atlassian")
        .expect("Atlassian server must be persisted");
    let MCPServerConfig::Http(http) = imported.config else {
        panic!("official URL config must import as Streamable HTTP");
    };
    assert_eq!(
        http.server_parameters.url,
        "https://mcp.atlassian.com/v1/mcp/authv2"
    );
    let serialized = serde_json::to_value(&http).unwrap();
    assert!(serialized.get("authPolicy").is_none());
    assert!(serialized.get("oauth").is_none());

    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(!runtime
        .sdk_mcp_server_configs()
        .await
        .contains_key(&bundle_id("atlassian")));
    runtime.restart().await.unwrap();
    assert!(runtime
        .sdk_mcp_server_configs()
        .await
        .contains_key(&bundle_id("atlassian")));
    let status = runtime
        .mcp_server_runtime_statuses()
        .await
        .into_iter()
        .find(|status| status.bundle_id.as_str() == "atlassian")
        .expect("imported server must have runtime status");
    assert_eq!(status.activation, MCPServerActivationState::Started);
    assert_eq!(
        status.connection,
        MCPServerConnectionState::AuthorizationRequired
    );
}

#[tokio::test]
async fn test_import_stdio_server_updates_an_active_runtime_after_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    let path = tmp.path().join("active-runtime-import.json");
    std::fs::write(
        &path,
        serde_json::json!({
            "mcpServers": {
                "runtime-import": {
                    "command": "node",
                    "args": [echo_server_path().to_string_lossy().to_string()],
                    "env": {}
                }
            }
        })
        .to_string(),
    )
    .unwrap();

    config_io::import_config_core(
        &state,
        path.to_string_lossy().to_string(),
        TEST_INSTANCE_ID.to_string(),
        Some(config_io::ConfigFormat::ClaudeDesktop),
    )
    .await
    .unwrap();

    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(!runtime
        .sdk_mcp_server_configs()
        .await
        .contains_key(&bundle_id("runtime-import")));
    runtime.restart().await.unwrap();
    assert!(runtime
        .sdk_mcp_server_configs()
        .await
        .contains_key(&bundle_id("runtime-import")));
    assert!(runtime
        .mcp_server_runtime_statuses()
        .await
        .iter()
        .any(|status| status.bundle_id.as_str() == "runtime-import" && status.is_connected()));
}

#[tokio::test]
async fn test_export_and_reimport_config() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    // Add configs
    mcp::add_mcp_server_core(&state, TEST_INSTANCE_ID, echo_server_config("export-me"))
        .await
        .unwrap();

    // Export
    let configs: Vec<_> = state
        .sdk_config
        .load(TEST_INSTANCE_ID)
        .mcp
        .servers
        .into_iter()
        .map(|server| server.config)
        .collect();
    let export_path = tmp.path().join("exported.json");
    let export_data = serde_json::json!({
        "servers": configs,
        "inputs": []
    });
    std::fs::write(
        &export_path,
        serde_json::to_string_pretty(&export_data).unwrap(),
    )
    .unwrap();

    // Verify exported file is valid JSON
    let content = std::fs::read_to_string(&export_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(parsed["servers"].is_array());
    assert_eq!(parsed["servers"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn test_claude_desktop_format_detection() {
    let tmp = tempfile::tempdir().unwrap();

    // Claude Desktop format
    let claude_config = serde_json::json!({
        "mcpServers": {
            "myserver": {
                "command": "node",
                "args": ["server.js"]
            }
        }
    });
    let path = tmp.path().join("claude_config.json");
    std::fs::write(&path, serde_json::to_string(&claude_config).unwrap()).unwrap();

    let content = std::fs::read_to_string(&path).unwrap();
    let value: serde_json::Value = serde_json::from_str(&content).unwrap();
    assert!(value.get("mcpServers").is_some());

    // CLI native format
    let native_config = serde_json::json!({
        "servers": [],
        "inputs": []
    });
    let path2 = tmp.path().join("native_config.json");
    std::fs::write(&path2, serde_json::to_string(&native_config).unwrap()).unwrap();

    let content2 = std::fs::read_to_string(&path2).unwrap();
    let value2: serde_json::Value = serde_json::from_str(&content2).unwrap();
    assert!(value2.get("servers").is_some());
}

// ── Activity integration ──

#[tokio::test]
async fn test_activity_write_query_export_clear() {
    use tfrobot_client_lib::services::observability::{
        ActivityEventDraft, ActivityLevel, ActivityOutcome, ActivityQuery, ActivityScopeFilter,
    };
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    // Write
    state
        .observability
        .record_activity(&ActivityEventDraft::client(
            ActivityLevel::Info,
            "system",
            "lifecycle",
            "start",
            ActivityOutcome::Succeeded,
            "App started",
        ))
        .unwrap();
    let mut failed = ActivityEventDraft::computer(
        "computer-a",
        ActivityLevel::Error,
        "mcp",
        "server",
        "start",
        ActivityOutcome::Failed,
        "Server crashed",
    );
    failed.fields = Some(serde_json::json!({"summary": "stack trace"}));
    state.observability.record_activity(&failed).unwrap();

    // Query
    let page = state
        .observability
        .query_activity(&ActivityQuery::default())
        .unwrap();
    assert_eq!(page.items.len(), 2);
    assert_eq!(page.total, 2);

    // Export
    let json = state
        .observability
        .export_activity(&ActivityQuery::default())
        .unwrap();
    let parsed: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.len(), 2);

    // Export to file
    let export_path = tmp.path().join("activity.json");
    std::fs::write(&export_path, &json).unwrap();
    assert!(export_path.exists());

    // Clear
    state
        .observability
        .clear_activity(&ActivityScopeFilter::All)
        .unwrap();
    let after = state
        .observability
        .query_activity(&ActivityQuery::default())
        .unwrap();
    assert!(after.items.is_empty());
}

// ── Settings integration ──

#[tokio::test]
async fn test_settings_persist_and_reload() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    let mut settings = state.settings_service.load();
    settings.language = "zh".to_string();
    settings.activity_retention_days = 7;
    state.settings_service.save(&settings).unwrap();

    // Create new state pointing to same dir (simulates app restart)
    let state2 = create_mcp_test_app_state(tmp.path()).await;
    let reloaded = state2.settings_service.load();
    assert_eq!(reloaded.language, "zh");
    assert_eq!(reloaded.activity_retention_days, 7);
}

// ── Input definitions integration ──

#[tokio::test]
async fn test_input_definitions_crud() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    // Add
    let input: tfrobot_client_lib::commands::inputs::InputDefinition =
        serde_json::from_value(serde_json::json!({
            "type": "PromptString",
            "id": "test-input",
            "label": "Test Input"
        }))
        .unwrap();

    inputs::add_or_update_input_core(&state, TEST_INSTANCE_ID, input)
        .await
        .unwrap();

    // Read back
    let loaded = inputs::list_inputs_core(&state, TEST_INSTANCE_ID).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].id(), "test-input");

    // Remove
    inputs::remove_input_core(&state, TEST_INSTANCE_ID, "test-input")
        .await
        .unwrap();

    let after = inputs::list_inputs_core(&state, TEST_INSTANCE_ID).unwrap();
    assert!(after.is_empty());
}

#[tokio::test]
async fn test_project_scope_enable_allowlist_is_rejected_with_actionable_validation() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    state
        .sdk_config
        .save(
            TEST_INSTANCE_ID,
            &ProjectConfigDoc {
                settings: Some(
                    serde_json::json!({
                        "enabledMcpjsonServers": ["untrusted-project-server"]
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                ),
                ..Default::default()
            },
        )
        .unwrap();

    let config_state = sdk_config::get_computer_config_state_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(!config_state.validation.valid);
    let error = config_state
        .validation
        .errors
        .iter()
        .find(|error| error.field == "enabledMcpjsonServers")
        .expect("project-scoped trusted field must be surfaced to the UI");
    assert_eq!(error.scope.as_str(), "project");
    assert!(
        error.reason.contains("local") || error.reason.contains("user"),
        "validation must explain the trusted destination: {}",
        error.reason
    );
}

#[tokio::test]
async fn test_mcp_runtime_applies_config_after_missing_input_is_supplied() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    inputs::add_or_update_input_core(
        &state,
        TEST_INSTANCE_ID,
        inputs::InputDefinition::PromptString {
            id: "runtime-token".to_string(),
            label: Some("Runtime token".to_string()),
            description: None,
            default: None,
            password: Some(false),
        },
    )
    .await
    .unwrap();
    let server: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": "runtime-input-apply",
        "disabled": false,
        "server_parameters": {
            "command": "node",
            "args": [common::echo_server_path().to_str().unwrap()],
            "env": {
                "RUNTIME_TOKEN": "${input:runtime-token}"
            }
        }
    }))
    .unwrap();

    sdk_config::upsert_computer_mcp_config_core(&state, TEST_INSTANCE_ID, server)
        .await
        .unwrap();
    let persisted = state
        .sdk_config
        .load(TEST_INSTANCE_ID)
        .mcp
        .servers
        .into_iter()
        .find(|server| server.name == "runtime-input-apply")
        .unwrap();
    assert_eq!(
        serde_json::to_value(persisted.config).unwrap()["server_parameters"]["env"]
            ["RUNTIME_TOKEN"],
        "${input:runtime-token}"
    );
    // Recreate the application/runtime so boot reads the persisted SDK configuration,
    // matching the production cold-start path rather than an already-loaded runtime.
    let state = create_mcp_test_app_state(tmp.path()).await;
    let started = start_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    assert!(started.running);
    let error =
        mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("runtime-input-apply"))
            .await
            .unwrap_err();
    assert!(
        matches!(
            &error,
            RuntimeActionError::ResolverFailed { input_id, message }
                if input_id == "runtime-token" && message.contains("user confirmation is required")
        ),
        "unexpected MCP start error: {error:?}"
    );

    inputs::set_input_value_core(
        &state,
        TEST_INSTANCE_ID,
        "runtime-token".to_string(),
        serde_json::json!("resolved-at-retry"),
    )
    .await
    .unwrap();
    mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("runtime-input-apply"))
        .await
        .unwrap();

    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(runtime
        .sdk_mcp_server_ids()
        .await
        .contains(&bundle_id("runtime-input-apply")));
}

#[tokio::test]
async fn test_user_mcp_start_prompts_in_place_and_continues_without_retry() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    inputs::add_or_update_input_core(
        &state,
        TEST_INSTANCE_ID,
        inputs::InputDefinition::PromptString {
            id: "runtime-token".to_string(),
            label: Some("Runtime token".to_string()),
            description: Some("Token supplied during start".to_string()),
            default: Some("definition-default-must-not-auto-resolve".to_string()),
            password: Some(false),
        },
    )
    .await
    .unwrap();
    let server: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": "runtime-input-interactive",
        "bundle_id": "runtime-input-interactive",
        "disabled": false,
        "server_parameters": {
            "command": "node",
            "args": [common::echo_server_path().to_str().unwrap()],
            "env": { "RUNTIME_TOKEN": "${input:runtime-token}" }
        }
    }))
    .unwrap();
    sdk_config::upsert_computer_mcp_config_core(&state, TEST_INSTANCE_ID, server)
        .await
        .unwrap();

    let state = Arc::new(create_mcp_test_app_state(tmp.path()).await);
    start_computer_instance_core(None, state.as_ref(), TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    let bridge = state.computer_registry.runtime_input_bridge();
    let (sender, mut requests) = tokio::sync::mpsc::unbounded_channel();
    bridge.set_sink(Arc::new(RecordingRuntimeInputSink { sender }));
    bridge.set_ready("integration-test", true);
    let start_state = state.clone();
    let start = tokio::spawn(async move {
        mcp::start_mcp_server_interactive_core(
            start_state.as_ref(),
            TEST_INSTANCE_ID,
            &bundle_id("runtime-input-interactive"),
        )
        .await
    });

    let request = timeout(Duration::from_secs(5), requests.recv())
        .await
        .expect("interactive start must emit a runtime input request")
        .expect("runtime input bridge must remain connected");
    assert_eq!(request.instance_id, TEST_INSTANCE_ID);
    assert_eq!(request.reason, RuntimeInputRequestReason::Missing);
    assert!(!request.secret);
    assert!(matches!(
        &request.definition,
        a2c_smcp::smcp_computer::mcp_clients::model::MCPServerInput::PromptString(prompt)
            if prompt.id == "runtime-token"
                && prompt.default.as_deref() == Some("definition-default-must-not-auto-resolve")
    ));
    assert!(
        inputs::list_input_entries_core(state.as_ref(), TEST_INSTANCE_ID)
            .unwrap()
            .is_empty()
    );

    let completion = bridge.complete(
        &request.request_id,
        RuntimeInputCompletion::Confirmed {
            value: "confirmed-at-start".to_string(),
        },
    );
    let (completion, start) = tokio::join!(completion, start);
    completion.unwrap();
    start.unwrap().unwrap();

    let entries = inputs::list_input_entries_core(state.as_ref(), TEST_INSTANCE_ID).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].key, "runtime-token");
    assert_eq!(
        entries[0].value,
        Some(serde_json::json!("confirmed-at-start"))
    );
    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(runtime
        .sdk_mcp_server_ids()
        .await
        .contains(&bundle_id("runtime-input-interactive")));
}

#[tokio::test]
async fn test_pending_foreground_prompt_does_not_block_another_computer_background_start() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    inputs::add_or_update_input_core(
        &state,
        TEST_INSTANCE_ID,
        inputs::InputDefinition::PromptString {
            id: "runtime-token".to_string(),
            label: Some("Runtime token".to_string()),
            description: None,
            default: None,
            password: Some(false),
        },
    )
    .await
    .unwrap();
    let waiting_server: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": "waiting-server",
        "bundle_id": "waiting-server",
        "disabled": false,
        "server_parameters": {
            "command": "node",
            "args": [common::echo_server_path().to_str().unwrap()],
            "env": { "RUNTIME_TOKEN": "${input:runtime-token}" }
        }
    }))
    .unwrap();
    sdk_config::upsert_computer_mcp_config_core(&state, TEST_INSTANCE_ID, waiting_server)
        .await
        .unwrap();

    const SECOND_INSTANCE_ID: &str = "computer-b";
    state
        .config
        .add_computer_instance(ComputerInstance::new(SECOND_INSTANCE_ID, "Computer B"))
        .unwrap();
    let independent_server =
        echo_server_config_with_bundle_id("independent-server", "independent-server");
    sdk_config::upsert_computer_mcp_config_core(&state, SECOND_INSTANCE_ID, independent_server)
        .await
        .unwrap();
    drop(state);

    let state = Arc::new(create_mcp_test_app_state(tmp.path()).await);
    start_computer_instance_core(None, state.as_ref(), TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    let bridge = state.computer_registry.runtime_input_bridge();
    let (sender, mut requests) = tokio::sync::mpsc::unbounded_channel();
    bridge.set_sink(Arc::new(RecordingRuntimeInputSink { sender }));
    bridge.set_ready("cross-computer-test", true);

    let waiting_state = state.clone();
    let waiting_start = tokio::spawn(async move {
        mcp::start_mcp_server_interactive_core(
            waiting_state.as_ref(),
            TEST_INSTANCE_ID,
            &bundle_id("waiting-server"),
        )
        .await
    });
    let request = timeout(Duration::from_secs(5), requests.recv())
        .await
        .expect("Computer A must reach its foreground prompt")
        .expect("prompt bridge must remain available");

    let stop_state = state.clone();
    let same_computer_stop = tokio::spawn(async move {
        computer::stop_computer_instance_core(stop_state.as_ref(), TEST_INSTANCE_ID.to_string())
            .await
    });
    tokio::task::yield_now().await;
    assert!(
        !same_computer_stop.is_finished(),
        "Computer A stop must wait behind its foreground Runtime Input operation"
    );

    timeout(
        Duration::from_secs(5),
        start_computer_instance_core(None, state.as_ref(), SECOND_INSTANCE_ID.to_string()),
    )
    .await
    .expect("Computer B background start must not wait behind Computer A's queued stop")
    .unwrap();

    let completion = bridge.complete(&request.request_id, RuntimeInputCompletion::Cancelled);
    let (completion, waiting_start) = tokio::join!(completion, waiting_start);
    completion.unwrap();
    assert!(matches!(
        waiting_start.unwrap().unwrap_err(),
        RuntimeActionError::RuntimeInputCancelled { .. }
    ));
    timeout(Duration::from_secs(5), same_computer_stop)
        .await
        .expect("Computer A stop must resume after its prompt is cancelled")
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn test_interactive_start_all_serializes_distinct_inputs_and_reuses_shared_entry() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    for input_id in ["shared-token", "region"] {
        inputs::add_or_update_input_core(
            &state,
            TEST_INSTANCE_ID,
            inputs::InputDefinition::PromptString {
                id: input_id.to_string(),
                label: None,
                description: None,
                default: None,
                password: Some(false),
            },
        )
        .await
        .unwrap();
    }
    for (name, input_id) in [
        ("shared-one", "shared-token"),
        ("region-server", "region"),
        ("shared-two", "shared-token"),
    ] {
        let server: MCPServerConfig = serde_json::from_value(serde_json::json!({
            "type": "stdio",
            "name": name,
            "bundle_id": name,
            "disabled": false,
            "server_parameters": {
                "command": "node",
                "args": [common::echo_server_path().to_str().unwrap()],
                "env": { "VALUE": format!("${{input:{input_id}}}") }
            }
        }))
        .unwrap();
        sdk_config::upsert_computer_mcp_config_core(&state, TEST_INSTANCE_ID, server)
            .await
            .unwrap();
    }
    drop(state);

    let state = Arc::new(create_mcp_test_app_state(tmp.path()).await);
    start_computer_instance_core(None, state.as_ref(), TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    let bridge = state.computer_registry.runtime_input_bridge();
    let (sender, mut requests) = tokio::sync::mpsc::unbounded_channel();
    bridge.set_sink(Arc::new(RecordingRuntimeInputSink { sender }));
    bridge.set_ready("start-all-test", true);
    let start_state = state.clone();
    let start_all = tokio::spawn(async move {
        mcp::start_all_servers_interactive_core(start_state.as_ref(), TEST_INSTANCE_ID).await
    });

    let first = timeout(Duration::from_secs(5), requests.recv())
        .await
        .expect("start-all must request its first missing input")
        .unwrap();
    let first_id = first.definition.id().to_string();
    let first_value = format!("confirmed-{first_id}");
    let completion_bridge = bridge.clone();
    let first_completion = tokio::spawn(async move {
        completion_bridge
            .complete(
                &first.request_id,
                RuntimeInputCompletion::Confirmed { value: first_value },
            )
            .await
    });
    let second = timeout(Duration::from_secs(5), requests.recv())
        .await
        .expect("start-all must serialize its second distinct input")
        .unwrap();
    first_completion.await.unwrap().unwrap();
    let second_id = second.definition.id().to_string();
    assert_ne!(first_id, second_id);
    assert_eq!(
        [first_id.as_str(), second_id.as_str()]
            .into_iter()
            .collect::<std::collections::HashSet<_>>(),
        ["shared-token", "region"]
            .into_iter()
            .collect::<std::collections::HashSet<_>>()
    );

    let completion = bridge.complete(
        &second.request_id,
        RuntimeInputCompletion::Confirmed {
            value: format!("confirmed-{second_id}"),
        },
    );
    let (completion, result) = tokio::join!(completion, start_all);
    completion.unwrap();
    let result = result.unwrap().unwrap();
    assert!(
        result.failures.is_empty(),
        "unexpected failures: {:?}",
        result.failures
    );
    assert!(
        requests.try_recv().is_err(),
        "shared input must not prompt twice"
    );
    let entries = inputs::list_input_entries_core(state.as_ref(), TEST_INSTANCE_ID).unwrap();
    assert_eq!(entries.len(), 2);
}

#[tokio::test]
async fn test_computer_start_and_restart_share_the_foreground_runtime_input_contract() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    inputs::add_or_update_input_core(
        &state,
        TEST_INSTANCE_ID,
        inputs::InputDefinition::PromptString {
            id: "lifecycle-token".to_string(),
            label: None,
            description: None,
            default: Some("prefill-only".to_string()),
            password: Some(false),
        },
    )
    .await
    .unwrap();
    let server: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": "computer-lifecycle-input",
        "bundle_id": "computer-lifecycle-input",
        "disabled": false,
        "server_parameters": {
            "command": "node",
            "args": [common::echo_server_path().to_str().unwrap()],
            "env": { "TOKEN": "${input:lifecycle-token}" }
        }
    }))
    .unwrap();
    sdk_config::upsert_computer_mcp_config_core(&state, TEST_INSTANCE_ID, server)
        .await
        .unwrap();
    drop(state);

    let state = Arc::new(create_mcp_test_app_state(tmp.path()).await);
    let bridge = state.computer_registry.runtime_input_bridge();
    let (sender, mut requests) = tokio::sync::mpsc::unbounded_channel();
    bridge.set_sink(Arc::new(RecordingRuntimeInputSink { sender }));
    bridge.set_ready("computer-lifecycle-test", true);

    let start_state = state.clone();
    let start = tokio::spawn(async move {
        computer::start_computer_instance_interactive_core(
            None,
            start_state.as_ref(),
            TEST_INSTANCE_ID.to_string(),
        )
        .await
    });
    let start_prompt = timeout(Duration::from_secs(5), requests.recv())
        .await
        .expect("Computer start must prompt")
        .unwrap();
    let completion = bridge.complete(
        &start_prompt.request_id,
        RuntimeInputCompletion::Confirmed {
            value: "start-value".to_string(),
        },
    );
    let (completion, start) = tokio::join!(completion, start);
    completion.unwrap();
    assert!(start.unwrap().unwrap().running);

    inputs::delete_input_entry_core(state.as_ref(), TEST_INSTANCE_ID, "lifecycle-token")
        .await
        .unwrap();
    let restart_state = state.clone();
    let restart = tokio::spawn(async move {
        computer::restart_computer_instance_interactive_core(
            None,
            restart_state.as_ref(),
            TEST_INSTANCE_ID.to_string(),
        )
        .await
    });
    let restart_prompt = timeout(Duration::from_secs(5), requests.recv())
        .await
        .expect("Computer restart must prompt again after the entry is deleted")
        .unwrap();
    let completion = bridge.complete(
        &restart_prompt.request_id,
        RuntimeInputCompletion::Confirmed {
            value: "restart-value".to_string(),
        },
    );
    let (completion, restart) = tokio::join!(completion, restart);
    completion.unwrap();
    assert!(restart.unwrap().unwrap().running);
}

#[tokio::test]
async fn test_foreground_computer_start_propagates_runtime_input_bridge_failure() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    inputs::add_or_update_input_core(
        &state,
        TEST_INSTANCE_ID,
        inputs::InputDefinition::PromptString {
            id: "start-token".to_string(),
            label: None,
            description: None,
            default: None,
            password: Some(false),
        },
    )
    .await
    .unwrap();
    let server: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": "start-bridge-failure",
        "bundle_id": "start-bridge-failure",
        "disabled": false,
        "server_parameters": {
            "command": "node",
            "args": [common::echo_server_path().to_str().unwrap()],
            "env": { "TOKEN": "${input:start-token}" }
        }
    }))
    .unwrap();
    sdk_config::upsert_computer_mcp_config_core(&state, TEST_INSTANCE_ID, server)
        .await
        .unwrap();
    drop(state);

    let state = create_mcp_test_app_state(tmp.path()).await;
    let error = computer::start_computer_instance_interactive_core(
        None,
        &state,
        TEST_INSTANCE_ID.to_string(),
    )
    .await
    .expect_err("foreground start must not swallow an unavailable Runtime Input bridge");
    assert!(matches!(
        error,
        RuntimeActionError::ResolverFailed { input_id, .. } if input_id == "start-token"
    ));
    assert!(
        !state
            .computer_registry
            .runtime(TEST_INSTANCE_ID)
            .await
            .unwrap()
            .is_running()
            .await,
        "failed foreground start must roll back the partially started runtime"
    );
}

#[tokio::test]
async fn test_foreground_computer_start_stops_after_first_input_failure() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    for input_id in ["first-token", "second-token"] {
        inputs::add_or_update_input_core(
            &state,
            TEST_INSTANCE_ID,
            inputs::InputDefinition::PromptString {
                id: input_id.to_string(),
                label: None,
                description: None,
                default: None,
                password: Some(false),
            },
        )
        .await
        .unwrap();
        let server: MCPServerConfig = serde_json::from_value(serde_json::json!({
            "type": "stdio",
            "name": format!("server-{input_id}"),
            "bundle_id": format!("server-{input_id}"),
            "disabled": false,
            "server_parameters": {
                "command": "node",
                "args": [common::echo_server_path().to_str().unwrap()],
                "env": { "TOKEN": format!("${{input:{input_id}}}") }
            }
        }))
        .unwrap();
        sdk_config::upsert_computer_mcp_config_core(&state, TEST_INSTANCE_ID, server)
            .await
            .unwrap();
    }
    drop(state);

    let state = Arc::new(create_mcp_test_app_state(tmp.path()).await);
    let bridge = state.computer_registry.runtime_input_bridge();
    let (sender, mut requests) = tokio::sync::mpsc::unbounded_channel();
    bridge.set_sink(Arc::new(RecordingRuntimeInputSink { sender }));
    bridge.set_ready("computer-fail-fast-test", true);
    let start_state = state.clone();
    let start = tokio::spawn(async move {
        computer::start_computer_instance_interactive_core(
            None,
            start_state.as_ref(),
            TEST_INSTANCE_ID.to_string(),
        )
        .await
    });
    let first = timeout(Duration::from_secs(5), requests.recv())
        .await
        .expect("foreground Computer start must request its first missing input")
        .unwrap();

    let completion = bridge.complete(&first.request_id, RuntimeInputCompletion::Cancelled);
    let (completion, start) = tokio::join!(completion, start);
    completion.unwrap();
    assert!(matches!(
        start.unwrap().unwrap_err(),
        RuntimeActionError::RuntimeInputCancelled { .. }
    ));
    assert!(
        requests.try_recv().is_err(),
        "foreground Computer start must not request another input after cancellation"
    );
    assert!(
        !state
            .computer_registry
            .runtime(TEST_INSTANCE_ID)
            .await
            .unwrap()
            .is_running()
            .await
    );
}

#[tokio::test]
async fn test_foreground_computer_restart_propagates_secret_persistence_failure() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = Arc::new(create_config_only_state_with_store(
        tmp.path(),
        Arc::new(FailingInputWriteStore),
    ));
    state
        .config
        .add_computer_instance(ComputerInstance::new(TEST_INSTANCE_ID, TEST_COMPUTER_NAME))
        .unwrap();
    state
        .computer_registry
        .upsert_runtime(
            state
                .config
                .get_computer_instance(TEST_INSTANCE_ID)
                .unwrap(),
        )
        .await
        .unwrap();
    start_computer_instance_core(None, state.as_ref(), TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    inputs::add_or_update_input_core(
        state.as_ref(),
        TEST_INSTANCE_ID,
        inputs::InputDefinition::PromptString {
            id: "restart-secret".to_string(),
            label: None,
            description: None,
            default: None,
            password: Some(true),
        },
    )
    .await
    .unwrap();
    let server: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": "restart-persistence-failure",
        "bundle_id": "restart-persistence-failure",
        "disabled": false,
        "server_parameters": {
            "command": "node",
            "args": [common::echo_server_path().to_str().unwrap()],
            "env": { "TOKEN": "${input:restart-secret}" }
        }
    }))
    .unwrap();
    sdk_config::upsert_computer_mcp_config_core(state.as_ref(), TEST_INSTANCE_ID, server)
        .await
        .unwrap();

    let bridge = state.computer_registry.runtime_input_bridge();
    let (sender, mut requests) = tokio::sync::mpsc::unbounded_channel();
    bridge.set_sink(Arc::new(RecordingRuntimeInputSink { sender }));
    bridge.set_ready("restart-persistence-test", true);
    let restart_state = state.clone();
    let restart = tokio::spawn(async move {
        computer::restart_computer_instance_interactive_core(
            None,
            restart_state.as_ref(),
            TEST_INSTANCE_ID.to_string(),
        )
        .await
    });
    let request = timeout(Duration::from_secs(5), requests.recv())
        .await
        .expect("foreground restart must request its missing secret")
        .unwrap();
    assert!(request.secret);

    let completion = bridge.complete(
        &request.request_id,
        RuntimeInputCompletion::Confirmed {
            value: "must-not-persist".to_string(),
        },
    );
    let (completion, restart) = tokio::join!(completion, restart);
    assert!(
        completion.is_err(),
        "native persistence failure must reject completion"
    );
    assert!(matches!(
        restart.unwrap().unwrap_err(),
        RuntimeActionError::ResolverFailed { input_id, .. } if input_id == "restart-secret"
    ));
    assert!(
        !state
            .computer_registry
            .runtime(TEST_INSTANCE_ID)
            .await
            .unwrap()
            .is_running()
            .await,
        "failed foreground restart must not leave a partially restarted runtime"
    );
}

#[tokio::test]
async fn test_mcp_runtime_materializes_literal_and_input_sources_on_each_actual_start() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    inputs::add_or_update_input_core(
        &state,
        TEST_INSTANCE_ID,
        inputs::InputDefinition::PromptString {
            id: "region".to_string(),
            label: Some("Region".to_string()),
            description: None,
            default: None,
            password: Some(false),
        },
    )
    .await
    .unwrap();
    inputs::set_input_value_core(
        &state,
        TEST_INSTANCE_ID,
        "region".to_string(),
        serde_json::json!("cn"),
    )
    .await
    .unwrap();
    let server: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": "mixed-env-sources",
        "bundle_id": "mixed-env-sources",
        "disabled": false,
        "server_parameters": {
            "command": "node",
            "args": [env_server_path().to_str().unwrap()],
            "env": {
                "LOG_LEVEL": "debug",
                "REGION": "${input:region}"
            }
        }
    }))
    .unwrap();
    sdk_config::upsert_computer_mcp_config_core(&state, TEST_INSTANCE_ID, server)
        .await
        .unwrap();

    start_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    let bundle = bundle_id("mixed-env-sources");
    mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle)
        .await
        .unwrap();

    async fn read_env(state: &AppState) -> serde_json::Value {
        let response = debug::execute_tool_core(
            state,
            TEST_INSTANCE_ID,
            "mixed-env-sources__read_env",
            serde_json::json!({}),
            Some(10.0),
        )
        .await
        .unwrap();
        assert!(response.success, "tool call failed: {:?}", response.error);
        let result = serde_json::to_value(response.result.unwrap()).unwrap();
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap()
    }

    assert_eq!(
        read_env(&state).await,
        serde_json::json!({ "LOG_LEVEL": "debug", "REGION": "cn" })
    );
    inputs::set_input_value_core(
        &state,
        TEST_INSTANCE_ID,
        "region".to_string(),
        serde_json::json!("eu"),
    )
    .await
    .unwrap();
    assert_eq!(read_env(&state).await["REGION"], "cn");

    mcp::stop_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle)
        .await
        .unwrap();
    mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle)
        .await
        .unwrap();
    assert_eq!(
        read_env(&state).await,
        serde_json::json!({ "LOG_LEVEL": "debug", "REGION": "eu" })
    );
}

#[tokio::test]
async fn test_mcp_runtime_migrates_legacy_secret_before_any_management_read() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    inputs::add_or_update_input_core(
        &state,
        TEST_INSTANCE_ID,
        inputs::InputDefinition::PromptString {
            id: "region".to_string(),
            label: Some("Region".to_string()),
            description: None,
            default: None,
            password: Some(false),
        },
    )
    .await
    .unwrap();
    tfrobot_client_lib::services::keychain::set_input_secret(
        state.secret_store.as_ref(),
        TEST_INSTANCE_ID,
        "region",
        "cn",
    )
    .unwrap();
    input_value_index::record(
        state.config.as_ref(),
        TEST_INSTANCE_ID,
        "region",
        InputValueStorageKind::Secret,
    )
    .unwrap();
    let legacy_plain_values =
        InputValueStore::for_computer(state.config.as_ref(), TEST_INSTANCE_ID);
    legacy_plain_values
        .set("region", &serde_json::json!("stale-plain"))
        .unwrap();
    let entry_metadata = state
        .config
        .computer_instance_storage_root(TEST_INSTANCE_ID)
        .join("input_entries.json");
    assert!(!entry_metadata.exists());
    let server: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": "entry-storage-authority",
        "bundle_id": "entry-storage-authority",
        "disabled": false,
        "server_parameters": {
            "command": "node",
            "args": [env_server_path().to_str().unwrap()],
            "env": { "REGION": "${input:region}" }
        }
    }))
    .unwrap();
    sdk_config::upsert_computer_mcp_config_core(&state, TEST_INSTANCE_ID, server)
        .await
        .unwrap();
    start_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    let bundle = bundle_id("entry-storage-authority");
    mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle)
        .await
        .unwrap();

    let response = debug::execute_tool_core(
        &state,
        TEST_INSTANCE_ID,
        "entry-storage-authority__read_env",
        serde_json::json!({}),
        Some(10.0),
    )
    .await
    .unwrap();
    let result = serde_json::to_value(response.result.unwrap()).unwrap();
    let env: serde_json::Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(env["REGION"], "cn");
    assert!(entry_metadata.exists());
    assert_eq!(legacy_plain_values.get("region").unwrap(), None);
    let entries = inputs::list_input_entries_core(&state, TEST_INSTANCE_ID).unwrap();
    assert!(entries[0].secret);
    assert_eq!(entries[0].value, None);
}

#[tokio::test]
async fn test_mcp_config_reports_a_reference_without_an_input_definition() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let server: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": "missing-definition",
        "disabled": false,
        "server_parameters": {
            "command": "node",
            "args": [common::echo_server_path().to_str().unwrap()],
            "env": {
                "OPENAI_API_KEY": "${input:OPENAI_KEY}"
            }
        }
    }))
    .unwrap();

    let error = sdk_config::upsert_computer_mcp_config_core(&state, TEST_INSTANCE_ID, server)
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        RuntimeActionError::MissingInputDefinition { input_id, .. } if input_id == "OPENAI_KEY"
    ));
    let persisted = state
        .sdk_config
        .load(TEST_INSTANCE_ID)
        .mcp
        .servers
        .into_iter()
        .find(|server| server.name == "missing-definition")
        .expect("MCP declaration should remain durable while the UI prompts for its Input");
    let persisted = serde_json::to_value(persisted.config).unwrap();
    assert_eq!(
        persisted["server_parameters"]["env"]["OPENAI_API_KEY"],
        "${input:OPENAI_KEY}"
    );
}

#[tokio::test]
async fn test_mcp_config_persists_a_canonical_pick_reference() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    inputs::add_or_update_input_core(
        &state,
        TEST_INSTANCE_ID,
        inputs::InputDefinition::PickString {
            id: "region".to_string(),
            label: None,
            description: None,
            default: None,
            options: vec![inputs::PickOption {
                label: "US".to_string(),
                value: "us".to_string(),
            }],
        },
    )
    .await
    .unwrap();
    let server: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": "dynamic-pick",
        "disabled": true,
        "server_parameters": {
            "command": "echo",
            "args": [],
            "env": {"REGION": "${input:region}"}
        }
    }))
    .unwrap();

    sdk_config::upsert_computer_mcp_config_core(&state, TEST_INSTANCE_ID, server)
        .await
        .unwrap();
    let persisted = state
        .sdk_config
        .load(TEST_INSTANCE_ID)
        .mcp
        .servers
        .into_iter()
        .find(|server| server.name == "dynamic-pick")
        .unwrap();
    assert_eq!(
        serde_json::to_value(persisted.config).unwrap()["server_parameters"]["env"]["REGION"],
        "${input:region}"
    );
}

async fn create_running_input_backed_state(path: &std::path::Path, server_name: &str) -> AppState {
    let state = create_mcp_test_app_state(path).await;
    inputs::add_or_update_input_core(
        &state,
        TEST_INSTANCE_ID,
        inputs::InputDefinition::PromptString {
            id: "runtime-token".to_string(),
            label: Some("Runtime token".to_string()),
            description: None,
            default: None,
            password: Some(false),
        },
    )
    .await
    .unwrap();
    inputs::set_input_value_core(
        &state,
        TEST_INSTANCE_ID,
        "runtime-token".to_string(),
        serde_json::json!("configured"),
    )
    .await
    .unwrap();
    let server: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": server_name,
        "disabled": false,
        "server_parameters": {
            "command": "node",
            "args": [common::echo_server_path().to_str().unwrap()],
            "env": { "RUNTIME_TOKEN": "${input:runtime-token}" }
        }
    }))
    .unwrap();
    sdk_config::upsert_computer_mcp_config_core(&state, TEST_INSTANCE_ID, server)
        .await
        .unwrap();
    start_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    state
}

#[tokio::test]
async fn test_restart_is_not_blocked_and_mcp_start_preserves_missing_input_error() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_running_input_backed_state(tmp.path(), "restart-runtime-input").await;
    inputs::remove_input_value_core(&state, TEST_INSTANCE_ID, "runtime-token")
        .await
        .unwrap();

    let restarted =
        computer::restart_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
            .await
            .unwrap();
    assert!(restarted.running);
    let error = mcp::start_mcp_server_core(
        &state,
        TEST_INSTANCE_ID,
        &bundle_id("restart-runtime-input"),
    )
    .await
    .unwrap_err();

    assert!(
        matches!(
            error,
            RuntimeActionError::ResolverFailed { ref input_id, ref message }
                if input_id == "runtime-token"
                    && message.contains("user confirmation is required")
        ),
        "MCP start returned an unexpected error after restart: {error:?}"
    );
}

#[tokio::test]
async fn test_mcp_start_preserves_structured_invalid_pick_selection_and_stored_value() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let pick = |value: &str| inputs::InputDefinition::PickString {
        id: "region".to_string(),
        label: None,
        description: Some("Region".to_string()),
        default: Some(value.to_string()),
        options: vec![inputs::PickOption {
            label: value.to_uppercase(),
            value: value.to_string(),
        }],
    };
    inputs::add_or_update_input_core(&state, TEST_INSTANCE_ID, pick("eu"))
        .await
        .unwrap();
    inputs::set_input_value_core(
        &state,
        TEST_INSTANCE_ID,
        "region".to_string(),
        serde_json::json!("eu"),
    )
    .await
    .unwrap();
    inputs::add_or_update_input_core(&state, TEST_INSTANCE_ID, pick("cn"))
        .await
        .unwrap();
    let server: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": "invalid-pick",
        "disabled": false,
        "server_parameters": {
            "command": "node",
            "args": [common::echo_server_path().to_str().unwrap()],
            "env": { "REGION": "${input:region}" }
        }
    }))
    .unwrap();
    sdk_config::upsert_computer_mcp_config_core(&state, TEST_INSTANCE_ID, server)
        .await
        .unwrap();

    let started = start_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();
    assert!(started.running);
    let error = mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("invalid-pick"))
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        RuntimeActionError::InvalidSelection {
            ref input_id,
            ref value,
            ..
        } if input_id == "region" && value.as_deref() == Some("eu")
    ));
    let view = inputs::get_input_value_core(&state, TEST_INSTANCE_ID, "region")
        .unwrap()
        .unwrap();
    assert_eq!(view.status, inputs::InputValueStatus::InvalidSelection);
    assert_eq!(view.value, Some(serde_json::json!("eu")));
}

#[tokio::test]
async fn test_secret_pick_invalid_selection_never_leaves_keychain_plaintext() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    inputs::add_or_update_input_core(
        &state,
        TEST_INSTANCE_ID,
        inputs::InputDefinition::PickString {
            id: "region".to_string(),
            label: None,
            description: Some("Region".to_string()),
            default: None,
            options: vec![inputs::PickOption {
                label: "China".to_string(),
                value: "cn".to_string(),
            }],
        },
    )
    .await
    .unwrap();
    inputs::upsert_input_entry_core(
        &state,
        TEST_INSTANCE_ID,
        "region",
        Some("private-retired-region".to_string()),
        true,
    )
    .await
    .unwrap();
    let server: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": "invalid-secret-pick",
        "disabled": false,
        "server_parameters": {
            "command": "node",
            "args": [common::echo_server_path().to_str().unwrap()],
            "env": { "REGION": "${input:region}" }
        }
    }))
    .unwrap();
    sdk_config::upsert_computer_mcp_config_core(&state, TEST_INSTANCE_ID, server)
        .await
        .unwrap();
    start_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap();

    let error =
        mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("invalid-secret-pick"))
            .await
            .unwrap_err();
    let error_json = serde_json::to_string(&error).unwrap();
    assert!(matches!(
        error,
        RuntimeActionError::InvalidSelection {
            ref input_id,
            value: None,
            ..
        } if input_id == "region"
    ));
    assert!(!error_json.contains("private-retired-region"));
    assert!(!error_json.contains("redacted secret selection"));
    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    let diagnostics = serde_json::to_string(&runtime.runtime_snapshot().await).unwrap();
    assert!(!diagnostics.contains("private-retired-region"));
    assert!(!serde_json::to_string(
        &inputs::list_input_entries_core(&state, TEST_INSTANCE_ID).unwrap()
    )
    .unwrap()
    .contains("private-retired-region"));
}

#[tokio::test]
async fn test_input_definition_changes_require_runtime_recreation() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    inputs::add_or_update_input_core(
        &state,
        TEST_INSTANCE_ID,
        inputs::InputDefinition::PromptString {
            id: "api-key".to_string(),
            label: Some("API Key".to_string()),
            description: Some("Secret API key".to_string()),
            default: None,
            password: Some(false),
        },
    )
    .await
    .unwrap();

    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(!runtime.inputs.read().await.contains_key("api-key"));

    inputs::set_input_value_core(
        &state,
        TEST_INSTANCE_ID,
        "api-key".to_string(),
        serde_json::json!("runtime-key"),
    )
    .await
    .unwrap();

    assert_eq!(
        inputs::get_input_value_core(&state, TEST_INSTANCE_ID, "api-key").unwrap(),
        Some(inputs::InputValueView {
            configured: true,
            status: inputs::InputValueStatus::Configured,
            value: Some(serde_json::json!("runtime-key")),
        })
    );
    assert!(!runtime.inputs.read().await.contains_key("api-key"));

    drop(state);
    let restarted_state = create_mcp_test_app_state(tmp.path()).await;
    let restarted_runtime = restarted_state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    let runtime_inputs = restarted_runtime.inputs.read().await;
    assert!(matches!(
        runtime_inputs.get("api-key"),
        Some(a2c_smcp::smcp_computer::mcp_clients::model::MCPServerInput::PromptString(prompt))
            if prompt.description == "API Key"
                && prompt.default.is_none()
                && prompt.password == Some(false)
    ));
}

#[tokio::test]
async fn test_input_values_crud() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    for id in ["key1", "key2"] {
        inputs::add_or_update_input_core(
            &state,
            TEST_INSTANCE_ID,
            inputs::InputDefinition::PromptString {
                id: id.to_string(),
                label: Some(id.to_string()),
                description: None,
                default: None,
                password: None,
            },
        )
        .await
        .unwrap();
    }
    inputs::set_input_value_core(
        &state,
        TEST_INSTANCE_ID,
        "key1".to_string(),
        serde_json::json!("value1"),
    )
    .await
    .unwrap();
    inputs::set_input_value_core(
        &state,
        TEST_INSTANCE_ID,
        "key2".to_string(),
        serde_json::json!("42"),
    )
    .await
    .unwrap();

    assert_eq!(
        inputs::get_input_value_core(&state, TEST_INSTANCE_ID, "key1").unwrap(),
        Some(inputs::InputValueView {
            configured: true,
            status: inputs::InputValueStatus::Configured,
            value: Some(serde_json::json!("value1")),
        })
    );
    assert_eq!(
        inputs::get_input_value_core(&state, TEST_INSTANCE_ID, "key2").unwrap(),
        Some(inputs::InputValueView {
            configured: true,
            status: inputs::InputValueStatus::Configured,
            value: Some(serde_json::json!("42")),
        })
    );

    inputs::clear_input_values_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(
        inputs::get_input_value_core(&state, TEST_INSTANCE_ID, "key1")
            .unwrap()
            .is_some_and(|view| !view.configured && view.value.is_none())
    );
    assert!(
        inputs::get_input_value_core(&state, TEST_INSTANCE_ID, "key2")
            .unwrap()
            .is_some_and(|view| !view.configured && view.value.is_none())
    );
}

// ── Issue #19 regression: stderr pipe deadlock ──
// The stderr-flood server writes >64 KB to stderr on startup and per tool call.
// If smcp-computer does not consume the stderr pipe, the child process blocks
// and tool calls time out.  This test will FAIL until smcp-computer is fixed.

/// Longer timeout for this test — the server may be slow to initialize while
/// flushing stderr, but 30 s is more than enough if the pipe is being consumed.
const STDERR_FLOOD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

#[tokio::test]
async fn test_sdk_computer_stderr_flood_does_not_block() {
    require_node();

    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let config = stderr_flood_server_config("stderr-flood-test");

    mcp::add_mcp_server_core(&state, TEST_INSTANCE_ID, config)
        .await
        .unwrap();
    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();

    // Start the server — this itself may hang if stderr blocks during init.
    match tokio::time::timeout(
        STDERR_FLOOD_TIMEOUT,
        mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("stderr-flood-test")),
    )
    .await
    {
        Ok(Ok(())) => {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;

            // Execute a tool call.  The server writes another burst of stderr
            // during this call, so if the pipe is not being drained this will
            // time out.
            match tokio::time::timeout(
                STDERR_FLOOD_TIMEOUT,
                debug::execute_tool_core(
                    &state,
                    TEST_INSTANCE_ID,
                    "stderr-flood-test__echo",
                    serde_json::json!({"message": "hello through stderr storm"}),
                    Some(25.0),
                ),
            )
            .await
            {
                Ok(Ok(response)) => {
                    assert!(
                        response.success,
                        "Tool call should succeed despite heavy stderr output"
                    );
                    assert!(response.result.is_some(), "Should have content");
                }
                Ok(Err(e)) => panic!("Tool call failed: {e}"),
                Err(_) => panic!(
                    "Tool call timed out after {STDERR_FLOOD_TIMEOUT:?} — \
                     stderr pipe is likely blocked (Issue #19)"
                ),
            }

            let _ = mcp::stop_mcp_server_core(
                &state,
                TEST_INSTANCE_ID,
                &bundle_id("stderr-flood-test"),
            )
            .await;
        }
        Ok(Err(e)) => panic!("start_mcp_server failed: {e}"),
        Err(_) => panic!(
            "start_mcp_server timed out after {STDERR_FLOOD_TIMEOUT:?} — \
             stderr pipe is likely blocked during initialization (Issue #19)"
        ),
    }
}
