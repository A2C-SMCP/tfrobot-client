//! Integration tests for MCP server management through AppState.
//! These tests exercise the full flow: config persistence + SDK Computer MCP runtime.

mod common;

use common::{
    create_test_app_state, echo_server_config, everything_server_config,
    everything_server_config_with_forbidden_tools, slow_echo_server_config,
    stderr_flood_server_config,
};
use http_body_util::Full;
use hyper::body::Bytes;
use smcp_computer::mcp_clients::MCPServerConfig;
use socketioxide::extract::{AckSender, SocketRef};
use socketioxide::SocketIo;
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tfrobot_client_lib::commands::connection::ConnectionState;
use tfrobot_client_lib::commands::{config_io, debug, inputs, mcp};
use tfrobot_client_lib::services::computer::{
    ComputerInstance, ManagedMcpServer, McpServerManagedBy,
};
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
        .await;
    state
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
        socket.on(SERVER_UPDATE_TOOL_LIST, move || {
            tool_stats
                .update_tool_list_events
                .fetch_add(1, Ordering::SeqCst);
        });
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

async fn connect_runtime_to_mock_robot(state: &AppState, server_url: &str) {
    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .expect("runtime should exist");
    runtime.start().await.expect("start runtime");
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

    *runtime.connection.write().await = Some(ConnectionState {
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

// ── Config CRUD via AppState ──

#[tokio::test]
async fn test_add_and_load_server_config() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    let config = echo_server_config("test-echo");
    state
        .config
        .add_config_for_instance(TEST_INSTANCE_ID, config)
        .unwrap();

    let loaded = state
        .config
        .load_configs_for_instance(TEST_INSTANCE_ID)
        .unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].name(), "test-echo");
}

#[tokio::test]
async fn test_add_remove_server_config() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    let config = echo_server_config("to-remove");
    state
        .config
        .add_config_for_instance(TEST_INSTANCE_ID, config)
        .unwrap();
    state
        .config
        .remove_config_for_instance(TEST_INSTANCE_ID, "to-remove")
        .unwrap();

    let loaded = state
        .config
        .load_configs_for_instance(TEST_INSTANCE_ID)
        .unwrap();
    assert!(loaded.is_empty());
}

#[tokio::test]
async fn test_update_server_config_replaces() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    let config1 = echo_server_config("updatable");
    state
        .config
        .add_config_for_instance(TEST_INSTANCE_ID, config1)
        .unwrap();

    // Add again with same name (should replace)
    let config2 = echo_server_config("updatable");
    state
        .config
        .add_config_for_instance(TEST_INSTANCE_ID, config2)
        .unwrap();

    let loaded = state
        .config
        .load_configs_for_instance(TEST_INSTANCE_ID)
        .unwrap();
    assert_eq!(loaded.len(), 1);
}

#[tokio::test]
async fn test_plugin_owned_mcp_server_reports_owner_and_blocks_user_lifecycle() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    state
        .config
        .add_managed_config_for_instance(
            TEST_INSTANCE_ID,
            ManagedMcpServer {
                config: echo_server_config("plugin-owned"),
                managed_by: McpServerManagedBy::Plugin {
                    marketplace: "tf-market".to_string(),
                    plugin: "desktop-tools".to_string(),
                    plugin_id: Some("plugin-1".to_string()),
                },
            },
        )
        .unwrap();

    let statuses = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].name, "plugin-owned");
    assert!(matches!(
        &statuses[0].managed_by,
        McpServerManagedBy::Plugin { .. }
    ));

    let update_err =
        mcp::update_mcp_server_core(&state, TEST_INSTANCE_ID, echo_server_config("plugin-owned"))
            .await
            .unwrap_err();
    let remove_err = mcp::remove_mcp_server_core(&state, TEST_INSTANCE_ID, "plugin-owned")
        .await
        .unwrap_err();
    let start_err = mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, "plugin-owned")
        .await
        .unwrap_err();
    let stop_err = mcp::stop_mcp_server_core(&state, TEST_INSTANCE_ID, "plugin-owned")
        .await
        .unwrap_err();

    for err in [update_err, remove_err, start_err, stop_err] {
        assert!(
            err.contains("Marketplace plugin"),
            "expected plugin lifecycle guard, got: {err}"
        );
    }
}

#[tokio::test]
async fn test_user_add_and_import_cannot_replace_plugin_owned_mcp_server() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    state
        .config
        .add_managed_config_for_instance(
            TEST_INSTANCE_ID,
            ManagedMcpServer {
                config: echo_server_config("plugin-owned"),
                managed_by: McpServerManagedBy::Plugin {
                    marketplace: "tf-market".to_string(),
                    plugin: "desktop-tools".to_string(),
                    plugin_id: Some("plugin-1".to_string()),
                },
            },
        )
        .unwrap();

    let add_err =
        mcp::add_mcp_server_core(&state, TEST_INSTANCE_ID, echo_server_config("plugin-owned"))
            .await
            .unwrap_err();
    assert!(add_err.contains("Marketplace plugin"));

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
    assert_eq!(cli_result.servers_imported, 0);
    assert_eq!(cli_result.servers_skipped, vec!["plugin-owned"]);

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
    assert_eq!(claude_result.servers_imported, 0);
    assert_eq!(claude_result.servers_skipped, vec!["plugin-owned"]);

    let managed = state
        .config
        .get_managed_config_for_instance(TEST_INSTANCE_ID, "plugin-owned")
        .unwrap();
    assert!(managed.is_plugin_owned());
}

#[tokio::test]
async fn test_start_all_and_stop_all_skip_plugin_owned_mcp_servers() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    state
        .config
        .add_managed_config_for_instance(
            TEST_INSTANCE_ID,
            ManagedMcpServer {
                config: echo_server_config("plugin-owned"),
                managed_by: McpServerManagedBy::Plugin {
                    marketplace: "tf-market".to_string(),
                    plugin: "desktop-tools".to_string(),
                    plugin_id: None,
                },
            },
        )
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
        .add_computer_instance(ComputerInstance {
            id: "second".to_string(),
            name: "Second".to_string(),
            ..ComputerInstance::new(TEST_INSTANCE_ID, "Computer A")
        })
        .unwrap();
    state
        .computer_registry
        .upsert_runtime(state.config.get_computer_instance("second").unwrap())
        .await;

    mcp::add_mcp_server_core(&state, "second", echo_server_config("second-only"))
        .await
        .unwrap();

    let default_configs = state
        .config
        .load_configs_for_instance(TEST_INSTANCE_ID)
        .unwrap();
    let second_configs = state.config.load_configs_for_instance("second").unwrap();
    let second_statuses = mcp::get_mcp_servers_core(&state, "second").await.unwrap();

    assert!(default_configs.is_empty());
    assert_eq!(second_configs.len(), 1);
    assert_eq!(second_configs[0].name(), "second-only");
    assert_eq!(second_statuses.len(), 1);
    assert_eq!(second_statuses[0].name, "second-only");
}

#[tokio::test]
async fn test_get_mcp_servers_uses_sdk_computer_status() {
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
    assert!(!statuses[0].running);
    assert_eq!(statuses[0].status_message, "pending");
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
    mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, "sdk-single")
        .await
        .unwrap();
    let started = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(started[0].name, "sdk-single");
    assert!(started[0].running);

    mcp::stop_mcp_server_core(&state, TEST_INSTANCE_ID, "sdk-single")
        .await
        .unwrap();
    let stopped = mcp::get_mcp_servers_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(stopped[0].name, "sdk-single");
    assert!(!stopped[0].running);
}

#[tokio::test]
async fn test_adding_server_while_running_preserves_active_sdk_server() {
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
    mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, "already-running")
        .await
        .unwrap();

    mcp::add_mcp_server_core(
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
    let added = statuses
        .iter()
        .find(|status| status.name == "newly-added")
        .expect("added server status should exist");

    assert!(active.running, "active server should remain running");
    assert!(
        !added.running,
        "new server should be registered but not auto-started"
    );
}

#[tokio::test]
async fn test_connected_computer_syncs_mcp_changes_to_robot_via_sdk() {
    require_node();
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    let (server_url, stats) = start_sync_capture_smcp_server().await;

    connect_runtime_to_mock_robot(&state, &server_url).await;
    wait_for_sync_event("mock robot never observed the SMCP join event", || {
        stats.join_events() == 1
    })
    .await;

    mcp::add_mcp_server_core(
        &state,
        TEST_INSTANCE_ID,
        everything_server_config("everything-sync"),
    )
    .await
    .unwrap();
    wait_for_sync_event("add should emit server:update_config to robot", || {
        stats.update_config_events() >= 1
    })
    .await;

    mcp::update_mcp_server_core(
        &state,
        TEST_INSTANCE_ID,
        everything_server_config_with_forbidden_tools("everything-sync", &["echo"]),
    )
    .await
    .unwrap();
    wait_for_sync_event("update should emit server:update_config to robot", || {
        stats.update_config_events() >= 2
    })
    .await;

    mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, "everything-sync")
        .await
        .unwrap();
    wait_for_sync_event("start should emit server:update_tool_list to robot", || {
        stats.update_tool_list_events() >= 1
    })
    .await;

    mcp::stop_mcp_server_core(&state, TEST_INSTANCE_ID, "everything-sync")
        .await
        .unwrap();
    wait_for_sync_event("stop should emit server:update_tool_list to robot", || {
        stats.update_tool_list_events() >= 2
    })
    .await;

    mcp::remove_mcp_server_core(&state, TEST_INSTANCE_ID, "everything-sync")
        .await
        .unwrap();
    wait_for_sync_event("remove should emit server:update_config to robot", || {
        stats.update_config_events() >= 3
    })
    .await;
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
        .synced_sdk_server_names()
        .await
        .contains("remove-me"));

    mcp::remove_mcp_server_core(&state, TEST_INSTANCE_ID, "remove-me")
        .await
        .unwrap();

    let configs = state
        .config
        .load_configs_for_instance(TEST_INSTANCE_ID)
        .unwrap();
    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();

    assert!(configs.is_empty());
    assert!(!runtime
        .synced_sdk_server_names()
        .await
        .contains("remove-me"));
    assert!(!runtime.sdk_mcp_server_names().await.contains("remove-me"));
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
    mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, "debug-tools")
        .await
        .unwrap();
    let tools = debug::get_available_tools_core(&state, TEST_INSTANCE_ID)
        .await
        .unwrap();

    let echo = tools.iter().find(|tool| tool.name == "echo").unwrap();
    assert_eq!(echo.server, "debug-tools");
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

    assert!(tools.iter().any(|tool| tool.name == "echo"));
    assert!(tools.iter().any(|tool| tool.name == "slow_echo"));
    assert!(tools.iter().all(|tool| tool.server == "unknown"));
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
    mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, "debug-exec")
        .await
        .unwrap();
    let first = debug::execute_tool_core(
        &state,
        TEST_INSTANCE_ID,
        "echo",
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
        "echo",
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
    mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, "debug-timeout")
        .await
        .unwrap();
    let response = debug::execute_tool_core(
        &state,
        TEST_INSTANCE_ID,
        "slow_echo",
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
}

#[tokio::test]
async fn test_config_io_import_export_are_instance_scoped() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;
    state
        .config
        .add_computer_instance(ComputerInstance {
            id: "second".to_string(),
            name: "Second".to_string(),
            ..ComputerInstance::new(TEST_INSTANCE_ID, "Computer A")
        })
        .unwrap();
    state
        .computer_registry
        .upsert_runtime(state.config.get_computer_instance("second").unwrap())
        .await;

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
    config_io::export_config_core(
        &state,
        export_path.to_string_lossy().to_string(),
        "second".into(),
        None,
    )
    .await
    .unwrap();

    let default_configs = state
        .config
        .load_configs_for_instance(TEST_INSTANCE_ID)
        .unwrap();
    let second_configs = state.config.load_configs_for_instance("second").unwrap();
    let second_runtime = state.computer_registry.runtime("second").await.unwrap();
    let second_sdk_servers = second_runtime.synced_sdk_server_names().await;
    let exported: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(export_path).unwrap()).unwrap();

    assert_eq!(import_result.servers_imported, 1);
    assert!(default_configs.is_empty());
    assert_eq!(second_configs.len(), 1);
    assert_eq!(second_configs[0].name(), "imported-second");
    assert!(second_sdk_servers.contains("imported-second"));
    assert_eq!(exported["servers"].as_array().unwrap().len(), 1);
    assert_eq!(exported["servers"][0]["name"], "imported-second");
}

#[tokio::test]
async fn test_cli_native_import_syncs_inputs_before_servers_with_placeholders() {
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
    assert_eq!(
        runtime.resolve_input_value("node-command").await.unwrap(),
        serde_json::json!("node")
    );
    assert!(runtime
        .synced_sdk_server_names()
        .await
        .contains("input-backed-server"));
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
        mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, "lifecycle-test"),
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

    mcp::stop_mcp_server_core(&state, TEST_INSTANCE_ID, "lifecycle-test")
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
        mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, "tool-list-test"),
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
    assert!(tools.iter().any(|tool| tool.name == "echo"));

    mcp::stop_mcp_server_core(&state, TEST_INSTANCE_ID, "tool-list-test")
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
    mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, "echo-call-test")
        .await
        .unwrap();

    let response = debug::execute_tool_core(
        &state,
        TEST_INSTANCE_ID,
        "echo",
        serde_json::json!({"message": "hello from test"}),
        Some(5.0),
    )
    .await
    .unwrap();

    assert!(response.success);
    assert!(response.result.is_some());
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
        mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, "ghost-server"),
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
        mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, "bad-server"),
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
    let config = echo_server_config("export-me");
    state
        .config
        .add_config_for_instance(TEST_INSTANCE_ID, config)
        .unwrap();

    // Export
    let configs = state
        .config
        .load_configs_for_instance(TEST_INSTANCE_ID)
        .unwrap();
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
            password: Some(true),
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
            smcp_computer::mcp_clients::model::MCPServerInput::PromptString(prompt)
                if prompt.description == "Secret API key"
                    && prompt.default.as_deref() == Some("default-key")
                    && prompt.password == Some(true)
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
    assert_eq!(
        runtime.resolve_input_value("api-key").await.unwrap(),
        serde_json::json!("runtime-key")
    );

    inputs::remove_input_value_core(&state, TEST_INSTANCE_ID, "api-key")
        .await
        .unwrap();
    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(
        runtime.resolve_input_value("api-key").await.unwrap(),
        serde_json::json!("default-key")
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
    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(
        runtime.resolve_input_value("api-key").await.unwrap(),
        serde_json::json!("default-key")
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
            password: Some(true),
        },
    )
    .await
    .unwrap();
    let runtime = state
        .computer_registry
        .runtime(TEST_INSTANCE_ID)
        .await
        .unwrap();
    assert_eq!(
        runtime.resolve_input_value("api-key").await.unwrap(),
        serde_json::json!("default-key")
    );
}

#[tokio::test]
async fn test_input_values_crud() {
    let tmp = tempfile::tempdir().unwrap();
    let state = create_mcp_test_app_state(tmp.path()).await;

    // Set values
    let mut values = std::collections::HashMap::new();
    values.insert("key1".to_string(), serde_json::json!("value1"));
    values.insert("key2".to_string(), serde_json::json!(42));
    state
        .config
        .save_input_values_for_instance(TEST_INSTANCE_ID, &values)
        .unwrap();

    // Read back
    let loaded = state
        .config
        .load_input_values_for_instance(TEST_INSTANCE_ID)
        .unwrap();
    assert_eq!(loaded["key1"], serde_json::json!("value1"));
    assert_eq!(loaded["key2"], serde_json::json!(42));

    // Clear
    state
        .config
        .save_input_values_for_instance(TEST_INSTANCE_ID, &std::collections::HashMap::new())
        .unwrap();
    let after = state
        .config
        .load_input_values_for_instance(TEST_INSTANCE_ID)
        .unwrap();
    assert!(after.is_empty());
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
        mcp::start_mcp_server_core(&state, TEST_INSTANCE_ID, "stderr-flood-test"),
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
                    "echo",
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

            let _ = mcp::stop_mcp_server_core(&state, TEST_INSTANCE_ID, "stderr-flood-test").await;
        }
        Ok(Err(e)) => panic!("start_mcp_server failed: {e}"),
        Err(_) => panic!(
            "start_mcp_server timed out after {STDERR_FLOOD_TIMEOUT:?} — \
             stderr pipe is likely blocked during initialization (Issue #19)"
        ),
    }
}
