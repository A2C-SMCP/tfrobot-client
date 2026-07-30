//! Integration tests for MCP server management through AppState.
//! These tests exercise the full flow: config persistence + SDK Computer MCP runtime.

mod common;

use a2c_smcp::smcp_computer::mcp_clients::bundle_id::resolve_bundle_id;
use a2c_smcp::smcp_computer::mcp_clients::model::BundleId;
use a2c_smcp::smcp_computer::mcp_clients::MCPServerConfig;
use a2c_smcp::smcp_computer::settings::config::ProjectConfigDoc;
use common::{
    create_test_app_state, echo_server_config, everything_server_config,
    everything_server_config_with_forbidden_tools, mcp, multi_tool_server_config,
    slow_echo_server_config, stderr_flood_server_config,
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
    computer::start_computer_instance_core,
    config_io,
    dashboard::{get_computer_overview_data_core, get_dashboard_data_core},
    debug, inputs, sdk_config,
};
use tfrobot_client_lib::services::computer::ComputerInstance;
use tfrobot_client_lib::AppState;
use tokio::net::TcpListener;
use tokio::time::{sleep, Duration};
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

fn echo_server_config_with_disabled(name: &str, disabled: bool) -> MCPServerConfig {
    let mut value = serde_json::to_value(echo_server_config(name)).unwrap();
    value["disabled"] = serde_json::Value::Bool(disabled);
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
async fn test_start_all_and_stop_all_skip_plugin_owned_mcp_servers() {
    let tmp = tempfile::tempdir().unwrap();
    write_legacy_plugin_owned_mcp_profile(tmp.path());
    let state = create_mcp_test_app_state(tmp.path()).await;
    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();

    mcp::start_all_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    mcp::stop_all_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
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
    assert!(statuses[0].running);
    assert_eq!(statuses[0].status_message, "connected");
}

#[tokio::test]
async fn disabled_user_mcp_is_hidden_and_toggle_applies_to_running_computer() {
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
        .all(|server| server.name != "toggle-server"));

    sdk_config::upsert_computer_mcp_config_core(
        &state,
        TEST_INSTANCE_ID,
        echo_server_config_with_disabled("toggle-server", false),
    )
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
async fn changing_bundle_id_removes_the_previous_runtime_identity() {
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
    assert!(!broken.running);
    assert!(broken.status_message.starts_with("Start failed:"));
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
        stop_err,
        start_all_err.to_string(),
        stop_all_err,
    ] {
        assert_eq!(
            err,
            "runtime action 'manage_mcp' is unavailable while lifecycle is 'created'"
        );
    }
}

#[tokio::test]
async fn test_config_add_applies_without_reloading_or_stopping_existing_servers() {
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
    let added = statuses
        .iter()
        .find(|status| status.name == "newly-added")
        .expect("enabled config addition should enter the live runtime");
    assert!(added.running);
}

#[tokio::test]
async fn test_connected_config_update_syncs_without_rejoining_computer() {
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
        everything_server_config("everything-sync"),
    )
    .await
    .unwrap();
    sdk_config::upsert_computer_mcp_config_core(
        &state,
        TEST_INSTANCE_ID,
        everything_server_config_with_forbidden_tools("everything-sync", &["echo"]),
    )
    .await
    .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(stats.update_config_events() > config_events_before);
    assert_eq!(
        stats.join_events(),
        1,
        "hot config apply must not reconnect"
    );
    let config_events_after_upsert = stats.update_config_events();
    sdk_config::remove_computer_mcp_config_core(&state, TEST_INSTANCE_ID, "everything-sync")
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(stats.update_config_events() > config_events_after_upsert);
    assert_eq!(
        stats.join_events(),
        1,
        "hot config removal must not reconnect"
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
    assert_eq!(
        final_snapshot.generation, initial_snapshot.generation,
        "hot MCP config apply must not rebuild the Computer runtime"
    );
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
    let overview = get_computer_overview_data_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(overview.connected);
    assert_eq!(overview.tools_count, final_snapshot.tools);
    assert_eq!(
        overview.runtime.config_revision,
        final_snapshot.config_revision
    );

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
        .reload()
        .await
        .unwrap();
    mcp::start_all_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    let statuses = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(statuses[0].name, "sdk-all");
    assert!(statuses[0].running);

    mcp::stop_all_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
}

#[tokio::test]
async fn test_remove_mcp_server_command_syncs_sdk_runtime() {
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
    assert!(!runtime
        .synced_sdk_servers()
        .await
        .contains_key(&bundle_id("remove-me")));
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
    let statuses = runtime.mcp_server_statuses().await;
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
    assert!(
        !second_sdk_servers.contains_key(&bundle_id("imported-second")),
        "config import must not perform runtime reload or availability checks"
    );
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
async fn test_config_io_input_only_import_does_not_mutate_sdk_config() {
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
    assert_eq!(after_document, before_document);
    assert_eq!(
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
    assert!(state
        .config
        .load_inputs_for_instance(TEST_INSTANCE_ID)
        .unwrap()
        .is_empty());
    assert!(!state
        .config
        .computer_instance_storage_root(TEST_INSTANCE_ID)
        .join("config_import_transaction.json")
        .exists());

    state
        .config
        .save_inputs_for_instance(
            TEST_INSTANCE_ID,
            &[inputs::InputDefinition::Command {
                id: "unsafe-command".to_string(),
                label: "Unsafe command".to_string(),
                command: "helper".to_string(),
                args: Some(vec!["--api-key=plain-export-secret".to_string()]),
            }],
        )
        .unwrap();
    let export_path = tmp.path().join("unsafe-command-export.json");
    std::fs::write(&export_path, "existing-target").unwrap();
    let export_error = config_io::export_config_core(
        &state,
        export_path.to_string_lossy().into_owned(),
        TEST_INSTANCE_ID.to_string(),
        None,
    )
    .await
    .unwrap_err();
    assert!(export_error.contains("inputs.unsafe-command.args[0]"));
    assert_eq!(
        std::fs::read_to_string(export_path).unwrap(),
        "existing-target"
    );
}

#[tokio::test]
async fn test_config_crud_rejects_sensitive_plaintext_before_any_definition_write() {
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
    assert!(server_error.contains("server_parameters.args[1]"));
    assert!(server_error.contains("client_secret"));
    assert!(state
        .sdk_config
        .load(TEST_INSTANCE_ID)
        .mcp
        .servers
        .is_empty());

    let secret_stdio: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "Stdio",
        "name": "redacted-crud-stdio",
        "server_parameters": {
            "command": "helper",
            "args": [],
            "env": { "TOKEN": "literal-crud-env-secret" }
        }
    }))
    .unwrap();
    sdk_config::upsert_computer_mcp_config_core(&state, TEST_INSTANCE_ID, secret_stdio)
        .await
        .unwrap();

    let secret_http: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "Http",
        "name": "redacted-crud-http",
        "server_parameters": {
            "url": "https://user:password@example.com/mcp",
            "headers": { "Authorization": "Bearer literal-crud-header-secret" }
        }
    }))
    .unwrap();
    sdk_config::upsert_computer_mcp_config_core(&state, TEST_INSTANCE_ID, secret_http)
        .await
        .unwrap();

    let persisted = std::fs::read_to_string(
        state
            .sdk_config
            .project_anchor(TEST_INSTANCE_ID)
            .join(".tfrobot/mcp.local.json"),
    )
    .unwrap();
    assert!(!persisted.contains("literal-crud-env-secret"));
    assert!(!persisted.contains("literal-crud-header-secret"));
    assert!(!persisted.contains("user:password"));
    assert!(persisted.contains("${REDACTED}"));

    let view = sdk_config::get_computer_config_state_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    let view_json = serde_json::to_string(&view).unwrap();
    assert!(!view_json.contains("literal-crud-env-secret"));
    assert!(!view_json.contains("literal-crud-header-secret"));
    assert!(!view_json.contains("user:password"));
    assert!(view_json.contains("${REDACTED}"));

    // A legacy/on-disk declaration that predates the guarded CRUD boundary must still never be
    // echoed to the WebView snapshot with its plaintext secret surfaces intact.
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
                                    "url": "https://legacy-user:legacy-password@example.com/mcp",
                                    "headers": {
                                        "Authorization": "Bearer legacy-header-secret"
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
    assert!(legacy_raw.contains("legacy-header-secret"));
    let legacy_view = sdk_config::get_computer_config_state_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    let legacy_view_json = serde_json::to_string(&legacy_view).unwrap();
    assert!(!legacy_view_json.contains("legacy-header-secret"));
    assert!(!legacy_view_json.contains("legacy-user:legacy-password"));
    assert!(legacy_view_json.contains("${REDACTED}"));

    let input_error = inputs::add_or_update_input_core(
        &state,
        TEST_INSTANCE_ID,
        inputs::InputDefinition::Command {
            id: "unsafe-crud-input".to_string(),
            label: "Unsafe CRUD input".to_string(),
            command: "helper".to_string(),
            args: Some(vec!["https://example.com/run?mode=debug".to_string()]),
        },
    )
    .await
    .unwrap_err();
    assert!(input_error.contains("inputs.unsafe-crud-input.args[0]"));
    assert!(input_error.contains("mode"));
    assert!(state
        .config
        .load_inputs_for_instance(TEST_INSTANCE_ID)
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn test_password_input_defaults_are_removed_from_crud_import_and_read_views() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    inputs::add_or_update_input_core(
        &state,
        TEST_INSTANCE_ID,
        inputs::InputDefinition::PromptString {
            id: "crud-password".to_string(),
            label: "CRUD password".to_string(),
            description: None,
            default: Some("crud-plaintext-secret".to_string()),
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
    assert_eq!(
        inputs::import_inputs_core(
            &state,
            TEST_INSTANCE_ID,
            import_path.to_string_lossy().as_ref()
        )
        .await
        .unwrap(),
        1
    );

    let listed = inputs::list_inputs_core(&state, TEST_INSTANCE_ID).unwrap();
    for id in ["crud-password", "imported-password"] {
        assert!(matches!(
            listed.iter().find(|input| input.id() == id),
            Some(inputs::InputDefinition::PromptString {
                default: None,
                password: Some(true),
                ..
            })
        ));
        assert!(matches!(
            inputs::get_input_core(&state, TEST_INSTANCE_ID, id).unwrap(),
            Some(inputs::InputDefinition::PromptString {
                default: None,
                password: Some(true),
                ..
            })
        ));
    }
    let persisted = std::fs::read_to_string(state.config.global_inputs_path()).unwrap();
    assert!(!persisted.contains("crud-plaintext-secret"));
    assert!(!persisted.contains("imported-plaintext-secret"));
}

#[tokio::test]
async fn test_config_import_preflights_sdk_target_before_journal_or_input_write() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    state
        .config
        .save_inputs_for_instance(
            TEST_INSTANCE_ID,
            &[inputs::InputDefinition::PromptString {
                id: "existing-input".to_string(),
                label: "Existing".to_string(),
                description: None,
                default: None,
                password: Some(false),
            }],
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
    let inputs = state
        .config
        .load_inputs_for_instance(TEST_INSTANCE_ID)
        .unwrap();
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
            "servers": [echo_server_config("recovered-server")],
            "inputs": [{
                "type": "Command",
                "id": "recovered-command",
                "label": "Recovered command",
                "command": "helper",
                "args": ["--token", "${env:RECOVERED_TOKEN}"]
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    drop(state);

    let recovered = create_test_app_state(tmp.path());

    assert!(!transaction_path.exists());
    assert!(recovered
        .sdk_config
        .load(TEST_INSTANCE_ID)
        .mcp
        .servers
        .iter()
        .any(|server| server.name == "recovered-server"));
    assert_eq!(
        recovered
            .config
            .load_inputs_for_instance(TEST_INSTANCE_ID)
            .unwrap()[0]
            .id(),
        "recovered-command"
    );
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
        .config
        .save_inputs_for_instance(
            TEST_INSTANCE_ID,
            &[inputs::InputDefinition::PromptString {
                id: "existing-input".to_string(),
                label: "Existing".to_string(),
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
        .config
        .load_inputs_for_instance(TEST_INSTANCE_ID)
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
    let log_service = tfrobot_client_lib::services::logger::LogService::new(tmp.path()).unwrap();
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
    let log_service = tfrobot_client_lib::services::logger::LogService::new(tmp.path()).unwrap();
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
    assert!(config
        .load_inputs_for_instance(TEST_INSTANCE_ID)
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn test_config_io_export_is_complete_and_redacts_secret_surfaces() {
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
                                    }
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
                                    "env": { "LOCAL_TOKEN": "local-secret" }
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
        .config
        .save_inputs_for_instance(
            TEST_INSTANCE_ID,
            &[inputs::InputDefinition::PromptString {
                id: "api-token".to_string(),
                label: "API token".to_string(),
                description: None,
                default: None,
                password: Some(true),
            }],
        )
        .unwrap();
    tfrobot_client_lib::services::keychain::set_input_value(
        state.secret_store.as_ref(),
        "api-token",
        &serde_json::json!("legacy-input-secret"),
    )
    .unwrap();
    tfrobot_client_lib::services::keychain::set_input_secret(
        state.secret_store.as_ref(),
        "api-token",
        "namespaced-input-secret",
    )
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

    assert_eq!(stdio["server_parameters"]["env"]["TOKEN"], "${REDACTED}");
    assert_eq!(
        stdio["server_parameters"]["env"]["TOKEN_REF"],
        "${env:TOKEN}"
    );
    assert_eq!(
        http["server_parameters"]["headers"]["Authorization"],
        "${REDACTED}"
    );
    assert_eq!(
        http["server_parameters"]["headers"]["Authorization-Ref"],
        "${input:api-token}"
    );
    assert_eq!(
        http["server_parameters"]["url"],
        "https://${REDACTED}@example.com/mcp"
    );
    assert_eq!(
        local["server_parameters"]["env"]["LOCAL_TOKEN"],
        "${REDACTED}"
    );
    assert!(exported["inputs"][0].get("default").is_none());
    assert!(!content.contains("literal-secret"));
    assert!(!content.contains("local-secret"));
    assert!(!content.contains("legacy-input-secret"));
    assert!(!content.contains("namespaced-input-secret"));
    assert!(content.contains("local-command"));
}

#[tokio::test]
async fn test_config_io_import_redacts_secret_surfaces_before_persisting() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let import_path = tmp.path().join("secret-import.json");
    let import_data = serde_json::json!({
        "servers": [{
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
                }
            }
        }],
        "inputs": [{
            "type": "PromptString",
            "id": "api-token",
            "label": "API token",
            "default": "input-default-secret",
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
        "${REDACTED}"
    );
    assert_eq!(
        persisted["server_parameters"]["env"]["TOKEN_REF"],
        "${input:api-token}"
    );
    let imported_inputs = state
        .config
        .load_inputs_for_instance(TEST_INSTANCE_ID)
        .unwrap();
    assert!(matches!(
        imported_inputs.as_slice(),
        [inputs::InputDefinition::PromptString {
            default: None,
            password: Some(true),
            ..
        }]
    ));
    assert_eq!(result.servers_imported, 1);
    assert_eq!(result.inputs_imported, 1);
    assert!(!serde_json::to_string(&persisted)
        .unwrap()
        .contains("literal-import-secret"));
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
        .config
        .save_inputs_for_instance(
            TEST_INSTANCE_ID,
            &[inputs::InputDefinition::PickString {
                id: "existing-input".to_string(),
                label: "Existing input".to_string(),
                description: None,
                options: vec![inputs::PickOption {
                    label: "Existing".to_string(),
                    value: "existing".to_string(),
                }],
                default: Some("existing".to_string()),
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
        .config
        .load_inputs_for_instance(TEST_INSTANCE_ID)
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
    state
        .config
        .save_inputs_for_instance(
            TEST_INSTANCE_ID,
            &[inputs::InputDefinition::PromptString {
                id: "shared-token".to_string(),
                label: "Existing token".to_string(),
                description: None,
                default: None,
                password: Some(true),
            }],
        )
        .unwrap();
    tfrobot_client_lib::services::keychain::set_input_secret(
        state.secret_store.as_ref(),
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
            .config
            .load_inputs_for_instance(TEST_INSTANCE_ID)
            .unwrap()
            .as_slice(),
        [inputs::InputDefinition::PromptString { label, .. }] if label == "Existing token"
    ));
    assert_eq!(
        tfrobot_client_lib::services::keychain::get_input_secret(
            state.secret_store.as_ref(),
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
            "command": "${input:missing-command}",
            "args": [],
            "env": { "TOKEN": "${input:missing-secret}" }
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
async fn test_cli_native_import_persists_inputs_and_server_without_runtime_resolution() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
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
    let imported_inputs = state
        .config
        .load_inputs_for_instance(TEST_INSTANCE_ID)
        .unwrap();

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
        !runtime
            .synced_sdk_servers()
            .await
            .contains_key(&bundle_id("input-backed-server")),
        "config import must defer runtime rendering to reload/start"
    );
    assert!(
        !runtime.inputs.read().await.contains_key("node-command"),
        "config import must persist input definitions without synchronizing runtime sessions"
    );
}

// ── SDK Computer MCP lifecycle (requires Node.js) ──
// Echo server uses newline-delimited JSON framing (MCP spec 2025-03-26).

const MCP_RUNTIME_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

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

    let result = tokio::time::timeout(
        MCP_RUNTIME_TIMEOUT,
        mcp::start_all_servers_core(&state, TEST_INSTANCE_ID),
    )
    .await;
    match result {
        Ok(Ok(())) => {}
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
    .await;
    match result {
        Ok(r) => assert!(
            r.is_err(),
            "Starting server with invalid command should fail"
        ),
        Err(_) => { /* Timeout is acceptable — the server doesn't exist / command is invalid */ }
    }
}

// ── Config IO integration ──

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

// ── Logs integration ──

#[tokio::test]
async fn test_log_write_query_export_clear() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    // Write
    state
        .log_service
        .write("info", "system", "App started", None)
        .unwrap();
    state
        .log_service
        .write("error", "mcp", "Server crashed", Some("stack trace"))
        .unwrap();

    // Query
    let logs = state.log_service.query(&Default::default()).unwrap();
    assert_eq!(logs.len(), 2);

    // Export
    let json = state.log_service.export(&Default::default()).unwrap();
    let parsed: Vec<serde_json::Value> = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.len(), 2);

    // Export to file
    let export_path = tmp.path().join("logs.json");
    std::fs::write(&export_path, &json).unwrap();
    assert!(export_path.exists());

    // Clear
    state.log_service.clear_all().unwrap();
    let after = state.log_service.query(&Default::default()).unwrap();
    assert!(after.is_empty());
}

// ── Settings integration ──

#[tokio::test]
async fn test_settings_persist_and_reload() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    let mut settings = state.settings_service.load();
    settings.language = "zh".to_string();
    settings.log_retention_days = 7;
    state.settings_service.save(&settings).unwrap();

    // Create new state pointing to same dir (simulates app restart)
    let state2 = create_mcp_test_app_state(tmp.path()).await;
    let reloaded = state2.settings_service.load();
    assert_eq!(reloaded.language, "zh");
    assert_eq!(reloaded.log_retention_days, 7);
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

    let mut inputs = state
        .config
        .load_inputs_for_instance(TEST_INSTANCE_ID)
        .unwrap();
    inputs.push(input);
    state
        .config
        .save_inputs_for_instance(TEST_INSTANCE_ID, &inputs)
        .unwrap();

    // Read back
    let loaded = state
        .config
        .load_inputs_for_instance(TEST_INSTANCE_ID)
        .unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].id(), "test-input");

    // Remove
    let filtered: Vec<_> = loaded
        .into_iter()
        .filter(|i| i.id() != "test-input")
        .collect();
    state
        .config
        .save_inputs_for_instance(TEST_INSTANCE_ID, &filtered)
        .unwrap();

    let after = state
        .config
        .load_inputs_for_instance(TEST_INSTANCE_ID)
        .unwrap();
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
async fn test_mcp_runtime_reloads_config_after_missing_input_is_supplied() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    inputs::add_or_update_input_core(
        &state,
        TEST_INSTANCE_ID,
        inputs::InputDefinition::PromptString {
            id: "runtime-token".to_string(),
            label: "Runtime token".to_string(),
            description: None,
            default: None,
            password: Some(false),
        },
    )
    .await
    .unwrap();
    let server: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": "runtime-input-reload",
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
    // Recreate the application/runtime so boot reads the persisted SDK configuration,
    // matching the production cold-start path rather than an already-loaded runtime.
    let state = create_mcp_test_app_state(tmp.path()).await;
    let error = start_computer_instance_core(None, &state, TEST_INSTANCE_ID.to_string())
        .await
        .unwrap_err();
    assert!(
        matches!(
            &error,
            RuntimeActionError::MissingInput { input_id, env_hint, .. }
                if input_id == "runtime-token" && env_hint == "A2C_SMCP_runtime_token"
        ),
        "unexpected boot error: {error:?}"
    );

    inputs::set_input_value_core(
        &state,
        TEST_INSTANCE_ID,
        "runtime-token".to_string(),
        serde_json::json!("resolved-at-retry"),
    )
    .await
    .unwrap();
    state
        .computer_registry
        .start_runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, &bundle_id("runtime-input-reload"))
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
        .contains(&bundle_id("runtime-input-reload")));
}

#[tokio::test]
async fn test_input_commands_sync_runtime_definitions() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    inputs::add_or_update_input_core(
        &state,
        TEST_INSTANCE_ID,
        inputs::InputDefinition::PromptString {
            id: "api-key".to_string(),
            label: "API Key".to_string(),
            description: Some("Secret API key".to_string()),
            default: Some("default-key".to_string()),
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
    {
        let runtime_inputs = runtime.inputs.read().await;
        let input = runtime_inputs
            .get("api-key")
            .expect("runtime input definition should be synced");
        assert!(matches!(
            input,
            a2c_smcp::smcp_computer::mcp_clients::model::MCPServerInput::PromptString(prompt)
                if prompt.description == "Secret API key"
                    && prompt.default.as_deref() == Some("default-key")
                    && prompt.password == Some(false)
        ));
    }

    inputs::set_input_value_core(
        &state,
        TEST_INSTANCE_ID,
        "api-key".to_string(),
        serde_json::json!("runtime-key"),
    )
    .await
    .unwrap();

    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    let resolver_probe: MCPServerConfig = serde_json::from_value(serde_json::json!({
        "type": "stdio",
        "name": "input-resolver-probe",
        "disabled": true,
        "server_parameters": {
            "command": "echo",
            "args": ["${input:api-key}"],
            "env": {}
        }
    }))
    .unwrap();
    mcp::add_mcp_server_core(&state, TEST_INSTANCE_ID, resolver_probe)
        .await
        .unwrap();
    assert_eq!(
        tfrobot_client_lib::services::keychain::get_input_value(
            state.secret_store.as_ref(),
            "api-key"
        )
        .unwrap(),
        Some(serde_json::json!("runtime-key"))
    );
    assert!(runtime
        .sdk_mcp_server_ids()
        .await
        .contains(&bundle_id("input-resolver-probe")));

    inputs::remove_input_value_core(&state, TEST_INSTANCE_ID, "api-key")
        .await
        .unwrap();
    assert_eq!(
        tfrobot_client_lib::services::keychain::get_input_value(
            state.secret_store.as_ref(),
            "api-key"
        )
        .unwrap(),
        None
    );

    inputs::set_input_value_core(
        &state,
        TEST_INSTANCE_ID,
        "api-key".to_string(),
        serde_json::json!("runtime-key-2"),
    )
    .await
    .unwrap();
    inputs::clear_input_values_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(
        tfrobot_client_lib::services::keychain::get_input_value(
            state.secret_store.as_ref(),
            "api-key"
        )
        .unwrap(),
        None
    );

    inputs::set_input_value_core(
        &state,
        TEST_INSTANCE_ID,
        "api-key".to_string(),
        serde_json::json!("stale-runtime-key"),
    )
    .await
    .unwrap();
    inputs::remove_input_core(&state, TEST_INSTANCE_ID, "api-key")
        .await
        .unwrap();

    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(!runtime.inputs.read().await.contains_key("api-key"));

    inputs::add_or_update_input_core(
        &state,
        TEST_INSTANCE_ID,
        inputs::InputDefinition::PromptString {
            id: "api-key".to_string(),
            label: "API Key".to_string(),
            description: None,
            default: Some("default-key".to_string()),
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
    let runtime_inputs = runtime.inputs.read().await;
    assert!(matches!(
        runtime_inputs.get("api-key"),
        Some(a2c_smcp::smcp_computer::mcp_clients::model::MCPServerInput::PromptString(prompt))
            if prompt.default.as_deref() == Some("default-key")
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
                label: id.to_string(),
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
        serde_json::json!(42),
    )
    .await
    .unwrap();

    assert_eq!(
        tfrobot_client_lib::services::keychain::get_input_value(
            state.secret_store.as_ref(),
            "key1"
        )
        .unwrap(),
        Some(serde_json::json!("value1"))
    );
    assert_eq!(
        tfrobot_client_lib::services::keychain::get_input_value(
            state.secret_store.as_ref(),
            "key2"
        )
        .unwrap(),
        Some(serde_json::json!(42))
    );

    inputs::clear_input_values_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert!(tfrobot_client_lib::services::keychain::get_input_value(
        state.secret_store.as_ref(),
        "key1"
    )
    .unwrap()
    .is_none());
    assert!(tfrobot_client_lib::services::keychain::get_input_value(
        state.secret_store.as_ref(),
        "key2"
    )
    .unwrap()
    .is_none());
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
